#[derive(Clone, PartialEq)]
pub struct User {
    // name + lastname
    pub name: String
}

#[derive(PartialEq, Clone)]
pub struct Group {
    pub id: u64,
    pub missing: Vec<User>
}

#[derive(Clone)]
pub struct Grade {
    pub user: User,
    pub grade: u64,
    pub group: Group,
    pub time: u64,
}
