use std::{error::Error, fmt};

/// Explicit entity properties. BYLAYER is the default for color and linetype.
/// Geometry and layer visibility are independent of these overrides.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityStyle {
    pub(crate) color: i16,
    pub(crate) linetype: String,
    pub(crate) scale: f64,
}

impl Default for EntityStyle {
    fn default() -> Self {
        Self {
            color: 256,
            linetype: "BYLAYER".into(),
            scale: 1.0,
        }
    }
}

impl EntityStyle {
    /// 0 is BYBLOCK, 256 is BYLAYER, and 1..=255 are indexed colors.
    pub fn color_index(&self) -> i16 {
        self.color
    }
    pub fn linetype(&self) -> &str {
        &self.linetype
    }
    /// Effective entity value before the global multiplier. Imported finite
    /// nonpositive group 48 uses default 1, retaining its original source bytes.
    pub fn linetype_scale(&self) -> f64 {
        self.scale
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityStyleError {
    NoSelection,
    UnknownEntity(usize),
    InvalidColor,
    InvalidScale,
    UnknownLinetype(String),
}

impl fmt::Display for EntityStyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSelection => write!(f, "Select entities before applying a style."),
            Self::UnknownEntity(index) => write!(f, "Unknown entity index {index}."),
            Self::InvalidColor => write!(
                f,
                "Entity color must be BYLAYER, BYBLOCK, or an index from 1 to 255."
            ),
            Self::InvalidScale => {
                write!(f, "Entity linetype scale must be finite and nonnegative.")
            }
            Self::UnknownLinetype(name) => write!(f, "Unknown linetype {name}."),
        }
    }
}
impl Error for EntityStyleError {}
