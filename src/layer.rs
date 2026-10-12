use std::{error::Error, fmt};

/// Drawing organization retained independently of entity geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub(crate) name: String,
    pub(crate) flags: i16,
    pub(crate) color: i16,
    pub(crate) linetype: String,
}

impl Layer {
    /// New R12 names use the portable ASCII subset. Imported names are retained.
    pub fn new(name: &str) -> Result<Self, LayerError> {
        if name.is_empty()
            || name.len() > 31
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_$-".contains(&c))
        {
            return Err(LayerError::InvalidName);
        }
        Ok(Self {
            name: name.to_owned(),
            flags: 0,
            color: 7,
            linetype: "CONTINUOUS".into(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn flags(&self) -> i16 {
        self.flags
    }
    pub fn color_index(&self) -> i16 {
        self.color
    }
    pub fn linetype(&self) -> &str {
        &self.linetype
    }
    pub fn is_off(&self) -> bool {
        self.color < 0
    }
    pub fn is_frozen(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn is_locked(&self) -> bool {
        self.flags & 4 != 0
    }
    pub fn is_visible(&self) -> bool {
        !self.is_off() && !self.is_frozen()
    }

    pub(crate) fn rgb(&self, on_white: bool) -> [u8; 3] {
        crate::color::layer_rgb(self.color, on_white)
    }

    pub(crate) fn set_visible(&mut self, visible: bool) -> Result<(), LayerError> {
        if !visible && self.color == 0 {
            return Err(LayerError::CannotHideColorZero(self.name.clone()));
        }
        if visible {
            self.color = self.color.abs();
            self.flags &= !1;
        } else {
            self.color = -self.color.abs();
        }
        Ok(())
    }

    pub(crate) fn set_locked(&mut self, locked: bool) {
        if locked {
            self.flags |= 4;
        } else {
            self.flags &= !4;
        }
    }

    /// Explicit edits use ordinary ACI colors and preserve the layer's off state.
    pub(crate) fn set_color_index(&mut self, index: i16) -> Result<(), LayerError> {
        if !(1..=255).contains(&index) {
            return Err(LayerError::InvalidColorIndex(index));
        }
        self.color = if self.is_off() { -index } else { index };
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerError {
    InvalidName,
    DuplicateName(String),
    UnknownLayer(String),
    MissingEntity(usize),
    HiddenLayer(String),
    LockedLayer(String),
    TooManyLayers,
    CannotHideColorZero(String),
    InvalidColorIndex(i16),
    Constraint(String),
}

impl fmt::Display for LayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => write!(
                f,
                "use 1–31 ASCII letters, digits, _, $, or - for a new layer name"
            ),
            Self::DuplicateName(name) => {
                write!(f, "layer {name} already exists (names ignore letter case)")
            }
            Self::UnknownLayer(name) => write!(f, "layer {name} does not exist"),
            Self::MissingEntity(index) => write!(f, "entity {} does not exist", index + 1),
            Self::HiddenLayer(name) => write!(f, "show layer {name} before drawing on it"),
            Self::LockedLayer(name) => write!(f, "unlock layer {name} before editing it"),
            Self::TooManyLayers => write!(f, "the R12 layer table is full"),
            Self::CannotHideColorZero(name) => write!(
                f,
                "layer {name} has color 0, which cannot encode an off state in R12"
            ),
            Self::InvalidColorIndex(index) => {
                write!(f, "layer color {index} must be an ACI index from 1 to 255")
            }
            Self::Constraint(message) => write!(f, "constraint edit failed: {message}"),
        }
    }
}
impl Error for LayerError {}
