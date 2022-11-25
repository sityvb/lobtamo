use crate::grade_spy::user::{Grade, Group, User};
use crate::Subject;
use async_trait::async_trait;
use std::{error::Error, fmt::Display};
use tokio;

#[async_trait]
pub trait Source {
    async fn gpa_list(
        &self,
        start: &u64,
        end: &u64,
        subject_id: Option<&u64>,
    ) -> Result<Vec<f64>, Box<dyn Error>>;

    async fn relav_groups(&self) -> Result<Vec<Subject>, Box<dyn Error>>;

    fn class_size(&self) -> usize;

    fn time_end(&self) -> u64;
}

pub struct DummySource {
    class_students: Vec<User>,
    groups: Vec<Group>,
    grades: Vec<Grade>,
    time_end: u64
}

impl DummySource {
    pub fn new(groups: Vec<Group>, class_students: Vec<User>, time_end: u64) -> Self {
        Self {
            class_students: class_students,
            groups: groups,
            grades: Vec::new(),
            time_end: time_end
        }
    }

    pub fn add_grade(&mut self, user: User, grade: u64, group: Group, time: u64) {
        self.grades.push(Grade {
            user: user,
            grade: grade,
            group: group,
            time: time,
        })
    }
}

#[derive(Debug)]
struct InvalidSubject;
impl Error for InvalidSubject {}
impl Display for InvalidSubject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "subject does not exist")
    }
}

#[async_trait]
impl Source for DummySource {
    async fn relav_groups(&self) -> Result<Vec<Subject>, Box<dyn Error>> {
        let mut subjects = Vec::new();
        for group in &self.groups {
            subjects.push(Subject { name: group.id.to_string(), id: group.id });
        }
        Ok(subjects)
    }

    fn class_size(&self) -> usize {
        self.class_students.len()
    }

    fn time_end(&self) -> u64 {
        self.time_end.clone()
    }

    async fn gpa_list(
        &self,
        start: &u64,
        end: &u64,
        subject_id: Option<&u64>,
    ) -> Result<Vec<f64>, Box<dyn Error>> {
        let mut averages: Vec<f64> = Vec::new();
        match subject_id {
            Some(subject_id) => {
                for student in &self.class_students {
                    let mut sum = 0;
                    let mut count = 0;
                    for grade in &self.grades {
                        if grade.group.id != *subject_id {
                            continue;
                        }
                        if grade.user != *student {
                            continue;
                        }
                        if grade.time >= *start && grade.time <= *end {
                            sum += grade.grade;
                            count += 1;
                        }
                    }
                    if count == 0 {
                        continue;
                    }

                    let round = format!("{:.2}", sum as f64 / count as f64).parse::<f64>().unwrap();
                    averages.push(round);
                }
            }
            None => {
                for student in &self.class_students {
                    let mut sum = 0;
                    let mut count = 0;
                    for grade in &self.grades {
                        if grade.user != *student {
                            continue;
                        }
                        if grade.time >= *start && grade.time <= *end {
                            sum += grade.grade;
                            count += 1;
                        }
                    }
                    if count == 0 {
                        continue;
                    }
                    // round with format because the *100 / 100 doesnt really work
                    let round = format!("{:.2}", sum as f64 / count as f64).parse::<f64>().unwrap();
                    averages.push(round);
                }
            }
        }
        for float in &averages {
            if float.is_nan() {
                println!("AG");
            }
        }
        averages.sort_by(|a, b| a.partial_cmp(b).unwrap());
        averages.reverse();
        Ok(averages)
    }
}

#[tokio::test]
async fn dummy_source_basic() {
    let st_group = Group { id: 0, missing: Vec::new() };
    let st_user = User {
        name: String::from("Test"),
    };
    let mut source = DummySource::new(vec![st_group.clone()], vec![st_user.clone()], 100);
    source.add_grade(st_user.clone(), 9, st_group.clone(), 0);
    source.add_grade(st_user.clone(), 9, st_group.clone(), 1);
    source.add_grade(st_user.clone(), 8, st_group.clone(), 2);
    assert_eq!(source.gpa_list(&1, &2, None).await.unwrap(), [8.5])
}
