use chrono;
use chrono::format;
use reqwest;
use scraper::{Html, Selector};
use serde::Deserialize;
use serde_json;
use std::collections::HashMap;
use std::{error::Error, fmt};
use tokio;
use uuid;

#[derive(Deserialize)]
struct MobileWebAuthResponseResult {
    #[serde(rename = "tokenCode")]
    token: String,
}

#[derive(Deserialize)]
struct MobileAuthResponseResult {
    #[serde(rename = "authToken")]
    token: String,
    #[serde(rename = "firstName")]
    first_name: String,
    #[serde(rename = "lastName")]
    last_name: String,
    #[serde(rename = "personId")]
    person_id: i64,
}

#[derive(Deserialize)]
struct MobileAuthResponse {
    #[serde(rename = "ErrorCode")]
    error_code: i64,
    #[serde(rename = "Result")]
    result: MobileAuthResponseResult,
}

#[derive(Deserialize)]
struct MobileWebAuthResponse {
    #[serde(rename = "ErrorCode")]
    error_code: i64,
    #[serde(rename = "Result")]
    result: MobileWebAuthResponseResult,
}

#[derive(Deserialize)]
struct RoleResponse {
    roles: Vec<Role>,
}

#[derive(Deserialize, Clone)]
struct Role {
    id: String,
}

#[derive(Debug)]
pub struct WebChangeError;

impl Error for WebChangeError {}

impl fmt::Display for WebChangeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "The website seems to have changed in an unexpected way")
    }
}

#[derive(Debug)]
pub struct Client {
    req_client: reqwest::Client,
    username: String,
    password: String,
    guid: String,
    ms3_token: String,
    web_token: String,
    role_id: String,
}

mod event_parse {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct Response {
        #[serde(rename = "isSuccess")]
        pub success: bool,
        pub days: Vec<Day>,
    }

    #[derive(Deserialize)]
    pub struct Day {
        date: String,
        #[serde(default)]
        pub events: Vec<Event>,
    }

    #[derive(Deserialize)]
    pub struct Event {
        #[serde(rename = "type")]
        pub event_type: String,
        #[serde(rename = "eventTitle")]
        pub event_title: EventTitle,
        #[serde(rename = "schoolSubjectId")]
        pub subject_id: u64,
    }

    #[derive(Deserialize)]
    pub struct EventTitle {
        #[serde(rename = "content")]
        pub title: String,
    }
}

#[derive(PartialEq)]
pub struct Subject {
    name: String,
    // mokymoLygiuPeriodoId
    id: u64,
}

impl From<&event_parse::Event> for Subject {
    fn from(i: &event_parse::Event) -> Subject {
        // FIXME, it is assumed that the response is only for one day,
        //
        Subject {
            name: i.event_title.title.clone(),
            id: i.subject_id,
        }
    }
}

impl Client {
    pub async fn new(username: String, password: String, guid: String) -> Result<Client, Box<dyn Error + Send + Sync>> {
        let req_client = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .expect("No TLS backend");

        let mut mobile_login_map = HashMap::new();
        mobile_login_map.insert("username", username.clone());
        mobile_login_map.insert("password", password.clone());
        // probably doesn't matter, but let's try to mimic the official client
        mobile_login_map.insert("typePhoneSystem", String::from("Android"));
        mobile_login_map.insert(
            "dateTime",
            format!("{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
        );
        // A GUUID is required, and I am not sure from where it comes from yet, when I log in on my phone from
        // somewhere it gets this GUID. How can it know what is a valid ID before I even log in?
        // Even after clearing app data, the GUID is always the same. Possible identifiable information?
        mobile_login_map.insert("guid", guid.clone());
        let mobile_auth_response: MobileAuthResponse = req_client
            .post("https://dienynas.tamo.lt/MobileServiceV3/AuthenticateV2")
            .json(&mobile_login_map)
            .send()
            .await?
            .json()
            .await
            .expect("json fail");

        let web_token_auth_response: MobileWebAuthResponse = req_client
            .get("https://dienynas.tamo.lt/MobileServiceV3/GetWebToken")
            .query(&[
                ("authToken", mobile_auth_response.result.token.clone()),
                (
                    "menuUrl",
                    String::from("https://dienynas.tamo.lt/goto/bendrauk"),
                ),
            ])
            .send()
            .await?
            .json()
            .await
            .expect("json_fail");

        let role_response: RoleResponse = req_client
            .get("https://api.tamo.lt/core/app/roles")
            .bearer_auth(&mobile_auth_response.result.token)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?
            .json()
            .await
            .expect("json_fail");

        let client = Client {
            req_client: req_client,
            username: username,
            password: password,
            guid: guid,
            ms3_token: mobile_auth_response.result.token,
            web_token: web_token_auth_response.result.token,
            role_id: role_response.roles[0].id.clone(),
        };

        if mobile_auth_response.error_code == 0 && web_token_auth_response.error_code == 0 {
            Ok(client)
        } else {
            Err(Box::new(WebChangeError))
        }
    }

    pub async fn subjects(&self) -> Result<Vec<Subject>, Box<dyn Error + Send + Sync>> {
        // FIXME take into account that there might be tri-weekly lessons
        // Use the mokymosi planas thing from the website?
        let mut encountered_days_with_subjects = 0;
        let mut date = chrono::offset::Local::now().date_naive();
        date = date.succ();
        let mut subjects: Vec<Subject> = Vec::new();
        while encountered_days_with_subjects < 14 {
            let response: event_parse::Response = self
                .req_client
                .get("https://api.tamo.lt/core/app/calendar/events")
                .query(&[("date", &date.format("%Y-%m-%d").to_string())])
                .bearer_auth(&self.ms3_token)
                .header("x-selected-role", &self.role_id)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await?
                .json()
                .await
                .expect("json fail");
            // for some reason it returns multiple days when we pass a single one
            date = date.succ().succ().succ().succ();
            for day in &response.days {
                let mut has_lesson = false;
                for event in &day.events {
                    if event.event_type == "schedule" {
                        let subject = Subject::from(event);
                        // There might be duplicates with this source, but we simply
                        // want a list for the ids usually
                        if !subjects.contains(&subject) {
                            subjects.push(subject);
                        }
                        has_lesson = true;
                    }
                }
                if has_lesson {
                    encountered_days_with_subjects += 1;
                }
            }
        }
        Ok(subjects)
    }

    pub async fn gpa_list(
        &self,
        start: chrono::naive::NaiveDate,
        end: chrono::naive::NaiveDate,
        subject: Option<Subject>,
    ) -> Result<Vec<f64>, Box<dyn Error>> {
        let mut subject_repr_as_string = String::from("");
        if let Some(subject) = subject {
            subject_repr_as_string = subject.id.to_string();
        } else {
            subject_repr_as_string = String::from("0");
        }
        let mut params = HashMap::new();
        // FIXME fetch this from calendar as gradeLevelPeriodID
        params.insert("mokymoLygiuPeriodoId", 64098.to_string());
        params.insert(
            "Intervalas",
            start.format("%Y%m%d").to_string() + "_" + &end.format("%Y%m%d").to_string(),
        );
        // Programėlė šitą taip pat daro
        // params.insert("IsvestiPazymiai", String::from("true"));
        let fragment = Html::parse_fragment(
            &self
                .req_client
                .post("https://dienynas.tamo.lt/Analytics/VidurkiuSarasas")
                .query(&[
                    ("state", "OnlyData"),
                    ("IstaiguDalykaiId", &subject_repr_as_string),
                ])
                .form(&params)
                .send()
                .await?
                .text()
                .await?,
        );
        let selector =
            Selector::parse(".row > div[style*=\"line-height:50px;font-size:13px\"]:last-child")
                .unwrap();
        let mut grades = Vec::new();
        for element in fragment.select(&selector) {
            for text in element.text() {
                if let Ok(grade) = text.parse::<f64>() {
                    grades.push(grade);
                }
            }
        }
        Ok(grades)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn login_works() {
        if let Ok(username) = std::env::var("USERNAME") {
            if let Ok(password) = std::env::var("PASSWORD") {
                if let Ok(guid) = std::env::var("GUID") {
                    let client =
                        Client::new(username, password, guid).await;
                    assert!(client.is_ok());
                } else {
                    eprintln!("Set guid env");
                    assert!(false);
                }
            } else {
                eprintln!("Set password env");
                assert!(false);
            }
        } else {
            eprintln!("Set username env");
            assert!(false)
        }
    }
}
