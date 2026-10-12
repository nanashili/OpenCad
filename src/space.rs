/// The native DXF ownership space of an entity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntitySpace {
    Model,
    Paper(String),
}

impl EntitySpace {
    pub fn matches_layout(&self, layout: &str) -> bool {
        match self {
            Self::Model => layout.eq_ignore_ascii_case("Model"),
            Self::Paper(name) => name.eq_ignore_ascii_case(layout),
        }
    }

    pub fn layout_name(&self) -> &str {
        match self {
            Self::Model => "Model",
            Self::Paper(name) => name,
        }
    }
}
