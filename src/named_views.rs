//! Validated named view and UCS records.

use crate::{Camera3, GeometryError, Ucs3};
use std::{error::Error, fmt};

const MAX_NAME: usize = 255;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamedViewError {
    InvalidName,
    DuplicateName,
    MissingName,
    Geometry(GeometryError),
}
impl fmt::Display for NamedViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("named view/UCS name is invalid"),
            Self::DuplicateName => f.write_str("a view/UCS with that name already exists"),
            Self::MissingName => f.write_str("named view/UCS was not found"),
            Self::Geometry(error) => error.fmt(f),
        }
    }
}
impl Error for NamedViewError {}
impl From<GeometryError> for NamedViewError {
    fn from(error: GeometryError) -> Self {
        Self::Geometry(error)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedView {
    name: String,
    camera: Camera3,
    center: [f64; 2],
    view_height: f64,
    view_width: f64,
    ucs: Option<Ucs3>,
    ucs_name: Option<String>,
}
impl NamedView {
    pub fn new(name: impl Into<String>, camera: Camera3) -> Result<Self, NamedViewError> {
        Self::with_viewport(name, camera, [0.0, 0.0], 1.0, 1.0)
    }
    pub fn with_viewport(
        name: impl Into<String>,
        camera: Camera3,
        center: [f64; 2],
        view_height: f64,
        view_width: f64,
    ) -> Result<Self, NamedViewError> {
        validate_viewport(center, view_height, view_width)?;
        Ok(Self {
            name: valid_name(name)?,
            camera,
            center,
            view_height,
            view_width,
            ucs: None,
            ucs_name: None,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn camera(&self) -> Camera3 {
        self.camera
    }
    pub fn center(&self) -> [f64; 2] {
        self.center
    }
    pub fn view_height(&self) -> f64 {
        self.view_height
    }
    pub fn view_width(&self) -> f64 {
        self.view_width
    }
    pub fn associated_ucs(&self) -> Option<Ucs3> {
        self.ucs
    }
    pub fn associated_ucs_name(&self) -> Option<&str> {
        self.ucs_name.as_deref()
    }
    pub fn set_camera(&mut self, camera: Camera3) {
        self.camera = camera;
    }
    pub fn set_viewport(
        &mut self,
        center: [f64; 2],
        view_height: f64,
        view_width: f64,
    ) -> Result<(), NamedViewError> {
        validate_viewport(center, view_height, view_width)?;
        self.center = center;
        self.view_height = view_height;
        self.view_width = view_width;
        Ok(())
    }
    pub fn set_associated_ucs(
        &mut self,
        ucs: Option<Ucs3>,
        name: Option<impl Into<String>>,
    ) -> Result<(), NamedViewError> {
        self.ucs = ucs;
        self.ucs_name = name.map(|value| valid_name(value)).transpose()?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedUcs {
    name: String,
    ucs: Ucs3,
}
impl NamedUcs {
    pub fn new(name: impl Into<String>, ucs: Ucs3) -> Result<Self, NamedViewError> {
        Ok(Self {
            name: valid_name(name)?,
            ucs,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn ucs(&self) -> Ucs3 {
        self.ucs
    }
    pub fn set_ucs(&mut self, ucs: Ucs3) {
        self.ucs = ucs;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedViewCatalog {
    views: Vec<NamedView>,
}
impl NamedViewCatalog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn views(&self) -> &[NamedView] {
        &self.views
    }
    pub fn get(&self, name: &str) -> Result<&NamedView, NamedViewError> {
        self.views
            .iter()
            .find(|view| same_name(view.name(), name))
            .ok_or(NamedViewError::MissingName)
    }
    pub fn get_mut(&mut self, name: &str) -> Result<&mut NamedView, NamedViewError> {
        self.views
            .iter_mut()
            .find(|view| same_name(view.name(), name))
            .ok_or(NamedViewError::MissingName)
    }
    pub fn add(&mut self, view: NamedView) -> Result<(), NamedViewError> {
        if self
            .views
            .iter()
            .any(|existing| same_name(existing.name(), view.name()))
        {
            return Err(NamedViewError::DuplicateName);
        }
        self.views.push(view);
        Ok(())
    }
    pub fn upsert(&mut self, view: NamedView) -> Result<(), NamedViewError> {
        if let Some(existing) = self
            .views
            .iter_mut()
            .find(|existing| same_name(existing.name(), view.name()))
        {
            *existing = view;
        } else {
            self.views.push(view);
        }
        Ok(())
    }
    pub fn remove(&mut self, name: &str) -> Result<NamedView, NamedViewError> {
        let index = self
            .views
            .iter()
            .position(|view| same_name(view.name(), name))
            .ok_or(NamedViewError::MissingName)?;
        Ok(self.views.remove(index))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedUcsCatalog {
    ucs: Vec<NamedUcs>,
}
impl NamedUcsCatalog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn ucs(&self) -> &[NamedUcs] {
        &self.ucs
    }
    pub fn get(&self, name: &str) -> Result<&NamedUcs, NamedViewError> {
        self.ucs
            .iter()
            .find(|value| same_name(value.name(), name))
            .ok_or(NamedViewError::MissingName)
    }
    pub fn get_mut(&mut self, name: &str) -> Result<&mut NamedUcs, NamedViewError> {
        self.ucs
            .iter_mut()
            .find(|value| same_name(value.name(), name))
            .ok_or(NamedViewError::MissingName)
    }
    pub fn add(&mut self, value: NamedUcs) -> Result<(), NamedViewError> {
        if self
            .ucs
            .iter()
            .any(|existing| same_name(existing.name(), value.name()))
        {
            return Err(NamedViewError::DuplicateName);
        }
        self.ucs.push(value);
        Ok(())
    }
    pub fn upsert(&mut self, value: NamedUcs) -> Result<(), NamedViewError> {
        if let Some(existing) = self
            .ucs
            .iter_mut()
            .find(|existing| same_name(existing.name(), value.name()))
        {
            *existing = value;
        } else {
            self.ucs.push(value);
        }
        Ok(())
    }
    pub fn remove(&mut self, name: &str) -> Result<NamedUcs, NamedViewError> {
        let index = self
            .ucs
            .iter()
            .position(|value| same_name(value.name(), name))
            .ok_or(NamedViewError::MissingName)?;
        Ok(self.ucs.remove(index))
    }
}

fn valid_name(name: impl Into<String>) -> Result<String, NamedViewError> {
    let name = name.into();
    if name.trim().is_empty() || name.len() > MAX_NAME {
        return Err(NamedViewError::InvalidName);
    }
    Ok(name)
}
fn validate_viewport(
    center: [f64; 2],
    view_height: f64,
    view_width: f64,
) -> Result<(), NamedViewError> {
    if !center.iter().all(|value| value.is_finite())
        || !view_height.is_finite()
        || !view_width.is_finite()
        || view_height <= 0.0
        || view_width <= 0.0
    {
        return Err(NamedViewError::Geometry(GeometryError::NonFiniteParameter));
    }
    Ok(())
}
fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}
