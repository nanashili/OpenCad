//! Named groups of drawing entities.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityGroup {
    name: String,
    description: String,
    selectable: bool,
    members: Vec<usize>,
}

impl EntityGroup {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        selectable: bool,
        members: Vec<usize>,
    ) -> Result<Self, &'static str> {
        let name = name.into();
        if name.trim().is_empty() || members.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("group name is empty or membership is not strictly ordered");
        }
        Ok(Self {
            name,
            description: description.into(),
            selectable,
            members,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn selectable(&self) -> bool {
        self.selectable
    }
    pub fn members(&self) -> &[usize] {
        &self.members
    }
    pub(crate) fn set_name(&mut self, name: String) {
        self.name = name;
    }
    pub(crate) fn set_members(&mut self, members: Vec<usize>) {
        self.members = members;
    }
}
