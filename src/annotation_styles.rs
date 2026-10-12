//! Named annotation style values shared by text, dimensions, and multileaders.

use crate::GeometryError;

fn positive(value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(GeometryError::NonPositiveScale)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    name: String,
    height: f64,
    width_factor: f64,
    oblique: f64,
    font: String,
}
impl TextStyle {
    pub fn new(name: impl Into<String>) -> Result<Self, GeometryError> {
        Ok(Self {
            name: valid_name(name)?,
            height: 2.5,
            width_factor: 1.0,
            oblique: 0.0,
            font: "Standard".to_owned(),
        })
    }
    pub fn standard() -> Self {
        Self::new("Standard").expect("static style is valid")
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn height(&self) -> f64 {
        self.height
    }
    pub fn width_factor(&self) -> f64 {
        self.width_factor
    }
    pub fn oblique(&self) -> f64 {
        self.oblique
    }
    pub fn font(&self) -> &str {
        &self.font
    }
    pub fn set_height(&mut self, value: f64) -> Result<(), GeometryError> {
        self.height = positive(value)?;
        Ok(())
    }
    pub fn set_width_factor(&mut self, value: f64) -> Result<(), GeometryError> {
        self.width_factor = positive(value)?;
        Ok(())
    }
    pub fn set_oblique(&mut self, value: f64) -> Result<(), GeometryError> {
        if !value.is_finite() {
            return Err(GeometryError::NonFiniteAngle);
        }
        self.oblique = value;
        Ok(())
    }
    pub fn set_font(&mut self, value: impl Into<String>) -> Result<(), GeometryError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(GeometryError::NotEnoughVertices);
        }
        self.font = value;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DimensionStyle {
    name: String,
    text_height: f64,
    arrow_size: f64,
    overall_scale: f64,
    linear_scale: f64,
    precision: usize,
    gap: f64,
    extension_offset: f64,
}
impl DimensionStyle {
    pub fn new(name: impl Into<String>) -> Result<Self, GeometryError> {
        Ok(Self {
            name: valid_name(name)?,
            text_height: 2.5,
            arrow_size: 1.0,
            overall_scale: 1.0,
            linear_scale: 1.0,
            precision: 3,
            gap: 0.625,
            extension_offset: 1.25,
        })
    }
    pub fn standard() -> Self {
        Self::new("Standard").expect("static style is valid")
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn text_height(&self) -> f64 {
        self.text_height
    }
    pub fn arrow_size(&self) -> f64 {
        self.arrow_size
    }
    pub fn overall_scale(&self) -> f64 {
        self.overall_scale
    }
    pub fn linear_scale(&self) -> f64 {
        self.linear_scale
    }
    pub fn precision(&self) -> usize {
        self.precision
    }
    pub fn gap(&self) -> f64 {
        self.gap
    }
    pub fn extension_offset(&self) -> f64 {
        self.extension_offset
    }
    pub fn set_text_height(&mut self, value: f64) -> Result<(), GeometryError> {
        self.text_height = positive(value)?;
        Ok(())
    }
    pub fn set_arrow_size(&mut self, value: f64) -> Result<(), GeometryError> {
        self.arrow_size = positive(value)?;
        Ok(())
    }
    pub fn set_overall_scale(&mut self, value: f64) -> Result<(), GeometryError> {
        self.overall_scale = positive(value)?;
        Ok(())
    }
    pub fn set_linear_scale(&mut self, value: f64) -> Result<(), GeometryError> {
        self.linear_scale = positive(value)?;
        Ok(())
    }
    pub fn set_precision(&mut self, value: usize) -> Result<(), GeometryError> {
        if value > 12 {
            return Err(GeometryError::InvalidLengthenValue);
        }
        self.precision = value;
        Ok(())
    }
    pub fn set_gap(&mut self, value: f64) -> Result<(), GeometryError> {
        self.gap = positive(value)?;
        Ok(())
    }
    pub fn set_extension_offset(&mut self, value: f64) -> Result<(), GeometryError> {
        self.extension_offset = positive(value)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MLeaderStyle {
    name: String,
    text_height: f64,
    arrow_size: f64,
    landing_gap: f64,
    dogleg_length: f64,
    text_frame: bool,
    text_style: String,
}
impl MLeaderStyle {
    pub fn new(name: impl Into<String>) -> Result<Self, GeometryError> {
        Ok(Self {
            name: valid_name(name)?,
            text_height: 2.5,
            arrow_size: 1.0,
            landing_gap: 1.0,
            dogleg_length: 5.0,
            text_frame: false,
            text_style: "Standard".to_owned(),
        })
    }
    pub fn standard() -> Self {
        Self::new("Standard").expect("static style is valid")
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn text_height(&self) -> f64 {
        self.text_height
    }
    pub fn arrow_size(&self) -> f64 {
        self.arrow_size
    }
    pub fn landing_gap(&self) -> f64 {
        self.landing_gap
    }
    pub fn dogleg_length(&self) -> f64 {
        self.dogleg_length
    }
    pub fn text_frame(&self) -> bool {
        self.text_frame
    }
    pub fn text_style(&self) -> &str {
        &self.text_style
    }
    pub fn set_text_height(&mut self, v: f64) -> Result<(), GeometryError> {
        self.text_height = positive(v)?;
        Ok(())
    }
    pub fn set_arrow_size(&mut self, v: f64) -> Result<(), GeometryError> {
        self.arrow_size = positive(v)?;
        Ok(())
    }
    pub fn set_landing_gap(&mut self, v: f64) -> Result<(), GeometryError> {
        self.landing_gap = positive(v)?;
        Ok(())
    }
    pub fn set_dogleg_length(&mut self, v: f64) -> Result<(), GeometryError> {
        self.dogleg_length = positive(v)?;
        Ok(())
    }
    pub fn set_text_frame(&mut self, value: bool) {
        self.text_frame = value;
    }
    pub fn set_text_style(&mut self, value: impl Into<String>) -> Result<(), GeometryError> {
        self.text_style = valid_name(value)?;
        Ok(())
    }
}

fn valid_name(value: impl Into<String>) -> Result<String, GeometryError> {
    let value = value.into();
    if value.trim().is_empty() {
        return Err(GeometryError::NotEnoughVertices);
    }
    Ok(value)
}
