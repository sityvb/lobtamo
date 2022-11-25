use crate::grade_spy::source::{DummySource, Source};
use crate::grade_spy::user::{Grade, Group, User};
use crate::Subject;
use itertools::Itertools;
use petgraph::{
    self,
    dot::{Config, Dot},
    Graph,
    stable_graph::StableGraph
};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use std::cmp::{Eq, Ord, PartialEq, PartialOrd};
use std::collections::{HashMap, VecDeque};
use std::error::Error;

#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq)]
pub struct PartialGrade {
    day: u64,
    group_id: u64,
    grade: u64,
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Ord, Eq)]
struct GradeEvent {
    // -1 for the orig event
    day: i64,
    grades: Vec<PartialGrade>,
}

#[test]
fn test_grads_for_avr() {
    let mut valids = grads_for_avr(&8.0, 2, 0);
    valids.sort();
    assert_eq!(
        valids,
        vec![
            (0, vec![8]),
            (0, vec![7, 9]),
            (0, vec![6, 10]),
            (0, vec![8, 8])
        ]
        .into_iter()
        .sorted()
        .collect::<Vec<(u64, Vec<u64>)>>()
    );
}

/// What grades can be used to get a particular grade when limited by a number?
/// Example: if the max is a doublegrade, 8 can be 8, or it can be 7 and 9, or 6 10.
/// Assumes a 1-10 grade system.
/// Last argument is an additional number that can be associated with these possibilities
fn grads_for_avr(avr: &f64, max: usize, dat: u64) -> Vec<(u64, Vec<u64>)> {
    // 0 is no grade for the purpose of this
    let mut valids: Vec<(u64, Vec<u64>)> = Vec::new();
    for combo in (0..11).combinations_with_replacement(max) {
        let count = combo.iter().filter(|&n| *n != 0).count();
        let mut sum = 0;
        for grade in &combo {
            sum += grade;
        }
        let combo_avr = format!("{:.2}", sum as f64 / count as f64)
            .parse::<f64>()
            .unwrap();
        if combo_avr == *avr {
            valids.push((dat.clone(), combo.into_iter().filter(|&n| n != 0).collect()));
        }
    }
    valids.sort();
    valids
}

fn get_ends(graph: &StableGraph<GradeEvent, i32, petgraph::Directed>) -> Vec<petgraph::graph::NodeIndex> {
    graph
        .node_indices()
        .filter(|&x| {
            graph
                .edges_directed(x, petgraph::Direction::Outgoing)
                .next()
                .is_some()
                == false
        })
        .collect()
}

fn get_root(graph: &StableGraph<GradeEvent, i32, petgraph::Directed>) -> petgraph::graph::NodeIndex {
    graph
        .node_indices()
        .filter(|&x| {
            graph
                .edges_directed(x, petgraph::Direction::Incoming)
                .next()
                .is_some()
                == false
        })
        .next()
        .unwrap()
}

#[derive(PartialEq)]
struct ContentCount {
    group_id: u64,
    sum: u64,
    count: u64,
}

fn get_gravs(
    graph: &StableGraph<GradeEvent, i32, petgraph::Directed>,
    root: &petgraph::graph::NodeIndex,
    end: &petgraph::graph::NodeIndex,
) -> Vec<ContentCount> {
    let path = petgraph::algo::astar(&graph, *root, |n| n == *end, |e| *e.weight(), |_| 0)
        .unwrap()
        .1;

    let mut counts = Vec::new();
    let mut subject_counts: HashMap<u64, Vec<u64>> = HashMap::new();
    for node in path {
        let event = graph.node_weight(node).unwrap();
        for grade in &event.grades {
            if subject_counts.contains_key(&grade.group_id) == false {
                subject_counts.insert(grade.group_id.clone(), vec![]);
            }
            subject_counts
                .get_mut(&grade.group_id)
                .unwrap()
                .push(grade.grade);
        }
    }
    for (k, x) in &subject_counts {
        counts.push(ContentCount {
            group_id: *k,
            sum: x.iter().sum(),
            count: x.len() as u64,
        })
    }
    counts
}

async fn event_valid_for_path(
    graph: &StableGraph<GradeEvent, i32, petgraph::Directed>,
    source: &dyn Source,
    event: &GradeEvent,
    root: &petgraph::graph::NodeIndex,
    node: &petgraph::graph::NodeIndex,
) -> Result<bool, Box<dyn Error>> {
    let counts = get_gravs(&graph, &root, &node);
    for count in counts.iter() {
        let new_grades: Vec<u64> = event
            .grades
            .iter()
            .filter(|&x| x.group_id == count.group_id)
            .map(|x| x.grade)
            .collect();
        let full_sum = count.sum + new_grades.iter().sum::<u64>();
        let full_count = count.count + new_grades.len() as u64;
        let avr = format!("{:.2}", full_sum as f64 / full_count as f64)
            .parse::<f64>()
            .unwrap();
        let gpas = source
            .gpa_list(&0, &(event.day as u64), Some(&count.group_id))
            .await?;
        // a completely empty one is valid if not everyone in the class got a grade that day
        match gpas.contains(&avr) {
            false => {
                if gpas.len() < source.class_size() {
                    if full_count == 0 {
                        continue;
                    }
                }
                break;
            }
            true => {
                continue;
            }
        }
    }

    let event_sum = event.grades.iter().map(|x| x.grade).sum::<u64>();
    let event_count = event.grades.iter().map(|x| x.grade).count() as u64;
    let full_count = event_count + counts.iter().map(|x| x.count).sum::<u64>();
    let full_sum = event_sum + counts.iter().map(|x| x.sum).sum::<u64>();
    let avr = format!("{:.2}", full_sum as f64 / full_count as f64)
        .parse::<f64>()
        .unwrap();
    let gpas = source.gpa_list(&0, &(event.day as u64), None).await?;
    if gpas.len() < source.class_size() {
        if full_count == 0 {
            return Ok(true);
        }
    } else if gpas.contains(&avr) {
        return Ok(true);
    }
    return Ok(false);
}

async fn should_skip(source: &dyn Source, it_day: &u64) -> Result<bool, Box<dyn Error>> {
    let grade_list = source.gpa_list(&it_day, &it_day, None).await?;
    Ok(grade_list.len() == 0)
}

async fn gather(source: &dyn Source, it: &u64) -> Result<HashMap<u64, Vec<f64>>, Box<dyn Error>> {
    let mut subject_grade_map: HashMap<u64, Vec<f64>> = HashMap::new();
    // on purpose for ratelimiting
    for relav_subject in source.relav_groups().await? {
        let gpas = source.gpa_list(&it, &it, Some(&relav_subject.id)).await?;
        if gpas.len() > 0 {
            subject_grade_map.insert(relav_subject.id, gpas);
        }
    }
    Ok(subject_grade_map)
}

fn purge_daggling(it_day: &u64, graph: &mut StableGraph<GradeEvent, i32, petgraph::Directed>) -> usize {
    let mut removed = 0;
    for end in get_ends(graph) {
        let event = graph.node_weight(end).unwrap();
        if event.day < *it_day as i64 {
            graph.remove_node(end);
            removed += 1;
        }
    }
    removed
}

async fn addition(
    source: &dyn Source,
    it_day: &u64,
    mut tree: Vec<StableGraph<GradeEvent, i32, petgraph::Directed>>,
    subject_grade_map: HashMap<u64, Vec<f64>>,
) -> Result<Vec<StableGraph<GradeEvent, i32, petgraph::Directed>>, Box<dyn Error>> {
    let mut subs_possibs: Vec<(u64, Vec<u64>)> = Vec::new();
    for subject in subject_grade_map.keys() {
        let gpas = subject_grade_map
            .get(subject)
            .expect("gathered group doesn't exist in source");
        let mut sub_possibilities = Vec::new();
        for i in gpas {
            sub_possibilities.extend(grads_for_avr(i, 1, subject.clone()));
        }
        if gpas.len() < source.class_size() {
            sub_possibilities.push((subject.clone(), vec![]));
        }
        subs_possibs.extend(sub_possibilities);
    }
    subs_possibs.sort();
    subs_possibs.dedup();
    let different_subjects = subject_grade_map.keys().len();
    let mut multi_grade_possibilities = Vec::new();

    // the average of the combo must exist in that day's gpas'
    let day_gpas = source.gpa_list(&it_day, &it_day, None).await?;

    let mut total = 0;
    for combo in subs_possibs.into_iter().combinations(different_subjects) {
        total += 1;
        // An duplicate combination is of two possibilities of the same subject
        let mut seens = Vec::new();
        let mut duplicate = false;
        for possib in &combo {
            if seens.contains(&possib.0) {
                duplicate = true;
                break;
            }
            seens.push(possib.0.clone());
        }
        if duplicate == true {
            continue;
        }
        let count: usize = combo.iter().map(|x| x.1.len()).sum();
        let sum: u64 = combo.iter().map(|x| x.1.iter().sum::<u64>()).sum();
        let combo_avr = format!("{:.2}", sum as f64 / count as f64)
            .parse::<f64>()
            .unwrap();
        match day_gpas.contains(&combo_avr) {
            false => {
                if day_gpas.len() < source.class_size() {
                    if count == 0 {
                        multi_grade_possibilities.push(combo);
                    }
                }
            }
            true => {
                multi_grade_possibilities.push(combo);
            }
        }
    }

    // maybe already added, but if not all people have a gpa in that day, some people didn't get anything
    if &source.gpa_list(&it_day, &it_day, None).await?.len() < &source.class_size() {
        multi_grade_possibilities.push(vec![]);
    }

    multi_grade_possibilities.sort();
    multi_grade_possibilities.dedup();

    for graph in &mut tree {
        if graph.node_count() == 0 {
            let node = graph.add_node(GradeEvent {
                day: -1,
                grades: vec![],
            });
        }
        let mut partial_grade_events: Vec<Vec<PartialGrade>> = Vec::new();
        for possibility in &multi_grade_possibilities {
            let mut grades = Vec::new();
            for subject in possibility {
                for grade in &subject.1 {
                    grades.push(PartialGrade {
                        grade: grade.clone(),
                        group_id: subject.0.clone(),
                        day: it_day.clone(),
                    })
                }
            }
            partial_grade_events.push(grades);
        }
        let end_nodes = get_ends(graph);
        let root = get_root(graph);

        // TODO, if there already is an added grade event, where the sum and count of all of its
        // subject is the same, the paths should merge.
        let mut count = 0;
        for partial_grade_event in partial_grade_events {
            let grade_event = GradeEvent {
                day: it_day.clone() as i64,
                grades: partial_grade_event,
            };
            for end_node in &end_nodes {
                if event_valid_for_path(graph, source, &grade_event, &root, end_node).await? == true
                {
                    let node = graph.add_node(grade_event.clone());
                    graph.add_edge(*end_node, node, 0);
                    count += 1;
                }
            }
        }
        let purged = purge_daggling(it_day, graph);
        println!("{}", &count);
    }

    Ok(tree)
}

fn iter_possibs(
    graphs: &Vec<StableGraph<GradeEvent, i32, petgraph::Directed>>,
) -> impl Iterator<Item = Vec<Vec<GradeEvent>>> {
    /*
    let mut graphs_with_their_ways = vec![];
    for graph in graphs {
        println!("thisstart");
        let mut graph_ways: Vec<Vec<GradeEvent>> = vec![];
        let root = get_root(graph);
        let end_nodes = get_ends(graph);
        for end in end_nodes {
            let paths = petgraph::algo::all_simple_paths::<Vec<_>, _>(graph, root, end, 0, None)
                .collect::<Vec<_>>();
            for path in paths {
                graph_ways.push(path.into_iter().map(|x| graph.node_weight(x).unwrap().clone()).collect::<Vec<GradeEvent>>());
            }
        }
        graph_ways.sort();
        graph_ways.dedup();
        println!("{}", &graph_ways.len());
        graphs_with_their_ways.push(graph_ways.into_iter());
    }
    graphs_with_their_ways.into_iter().multi_cartesian_product()
    */
    let graph = &graphs[0];
    let mut graph_ways: Vec<Vec<GradeEvent>> = vec![];
    let root = get_root(graph);
    let end_nodes = get_ends(graph);
    for end in end_nodes {
        let paths = petgraph::algo::all_simple_paths::<Vec<_>, _>(graph, root, end, 0, None)
            .collect::<Vec<_>>();
        for path in paths {
            graph_ways.push(
                path.into_iter()
                    .map(|x| graph.node_weight(x).unwrap().clone())
                    .collect::<Vec<GradeEvent>>(),
            );
        }
    }
    graph_ways.sort();
    graph_ways.dedup();
    graph_ways
        .into_iter()
        .combinations_with_replacement(graphs.len())
}

fn gpa_list_for_scenario(
    scenario: &Vec<Vec<GradeEvent>>,
    start: &u64,
    end: &u64,
    subject: Option<&u64>,
) -> Vec<f64> {
    let mut averages: Vec<f64> = Vec::new();
    for user in scenario {
        let mut sum = 0;
        let mut count = 0;
        'event: for event in user {
            for grade in &event.grades {
                if let Some(subject) = subject {
                    if grade.group_id != *subject {
                        continue 'event;
                    }
                }
                if grade.day >= *start && grade.day <= *end {
                    sum += grade.grade;
                    count += 1;
                }
            }
        }
        if count == 0 {
            continue;
        }
        let round = format!("{:.2}", sum as f64 / count as f64)
            .parse::<f64>()
            .unwrap();
        averages.push(round);
    }
    averages.sort_by(|a, b| a.partial_cmp(b).unwrap());
    averages.reverse();
    averages
}

async fn gpas_valid_for_source(
    source: &dyn Source,
    scenario: &Vec<Vec<GradeEvent>>,
    start: &u64,
    end: &u64,
    subject: Option<&u64>,
) -> Result<bool, Box<dyn Error>> {
    let scenario_list = gpa_list_for_scenario(scenario, start, end, subject);
    let real_list = source.gpa_list(start, end, subject).await?;
    return Ok(real_list == scenario_list);
}

async fn verify_simple(
    source: &dyn Source,
    scenario: &Vec<Vec<GradeEvent>>,
    day: &u64,
) -> Result<bool, Box<dyn Error>> {
    if gpas_valid_for_source(source, scenario, &0, day, None).await? == false {
        return Ok(false);
    }
    for group in source.relav_groups().await? {
        if gpas_valid_for_source(source, scenario, &0, day, Some(&group.id)).await? == false {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn distinguish(source: &dyn Source) -> Result<bool, Box<dyn Error>> {
    let mut it_day = 0;
    let mut graphs: Vec<petgraph::stable_graph::StableGraph<GradeEvent, i32, petgraph::Directed>> =
        vec![petgraph::stable_graph::StableGraph::new(); source.class_size()];

    while it_day <= source.time_end() {
        if should_skip(source, &it_day).await? {
            it_day += 1;
            continue;
        }
        let grade_map = gather(source, &it_day).await?;
        graphs = addition(source, &it_day, graphs, grade_map).await?;
        it_day += 1;
    }

    println!("{}", &iter_possibs(&graphs).count());
    let mut possible_scenarios = vec![];
    println!("started");
    for scenario in iter_possibs(&graphs) {
        if verify_simple(source, &scenario, &source.time_end()).await? == true {
            println!("GOT IT");
            println!("{:?}", &scenario);
            possible_scenarios.push(scenario.clone());
        }
    }
    possible_scenarios.sort();
    possible_scenarios.dedup();
    println!("{}", possible_scenarios.len());

    Ok(true)
}

#[tokio::test]
async fn test_single_grade_random_distinguish() {
    let class_size = 8;
    let time_end = 3;
    let mut groups = Vec::new();
    let mut users = Vec::new();

    for i in 0..10 {
        let group = Group {
            id: i,
            missing: Vec::new(),
        };
        groups.push(group);
    }

    for i in 0..class_size {
        let user = User {
            name: i.to_string(),
        };
        users.push(user);
    }

    let mut source = DummySource::new(groups.clone(), users.clone(), time_end.clone());

    let mut rng = StdRng::seed_from_u64(class_size.clone());
    for day in 0..time_end {
        let group_getter_count = rng.gen_range(0..=3);
        let group_getters: Vec<Group> = groups
            .clone()
            .choose_multiple(&mut rng, group_getter_count)
            .cloned()
            .collect();
        for group in group_getters {
            //let grade_getter_count = rng.gen_range(0..=class_size).try_into().unwrap();
            let grade_getter_count = class_size.clone() as usize;
            let grade_getters: Vec<User> = users
                .clone()
                .choose_multiple(&mut rng, grade_getter_count)
                .cloned()
                .collect();
            for user in grade_getters {
                source.add_grade(user, rng.gen_range(1..=10), group.clone(), day.clone());
            }
        }
    }

    let x = 1;

    distinguish(&source).await.unwrap();
}
