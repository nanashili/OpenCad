//! Paper-space layout and plotting models.

use crate::{Entity, GeometryError};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    paper: [f64; 2],
    margins: [f64; 4],
    orientation: Orientation,
    scale: f64,
}
impl PageSetup {
    pub fn new(width: f64, height: f64) -> Result<Self, GeometryError> {
        let mut p = Self {
            paper: [0.0; 2],
            margins: [0.0; 4],
            orientation: Orientation::Portrait,
            scale: 1.0,
        };
        p.set_paper(width, height)?;
        Ok(p)
    }
    pub fn paper(self) -> [f64; 2] {
        self.paper
    }
    pub fn margins(self) -> [f64; 4] {
        self.margins
    }
    pub fn orientation(self) -> Orientation {
        self.orientation
    }
    pub fn scale(self) -> f64 {
        self.scale
    }
    pub fn set_paper(&mut self, w: f64, h: f64) -> Result<(), GeometryError> {
        if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        };
        if self.margins[0] + self.margins[2] >= w || self.margins[1] + self.margins[3] >= h {
            return Err(GeometryError::NonPositiveScale);
        }
        self.paper = [w, h];
        Ok(())
    }
    pub fn set_margins(&mut self, m: [f64; 4]) -> Result<(), GeometryError> {
        if m.iter().any(|v| !v.is_finite() || *v < 0.0)
            || m[0] + m[2] >= self.paper[0]
            || m[1] + m[3] >= self.paper[1]
        {
            return Err(GeometryError::NonPositiveScale);
        };
        self.margins = m;
        Ok(())
    }
    pub fn set_orientation(&mut self, o: Orientation) {
        let [w, h] = match o {
            Orientation::Portrait => self.paper,
            Orientation::Landscape => [self.paper[1], self.paper[0]],
        };
        if self.margins[0] + self.margins[2] < w && self.margins[1] + self.margins[3] < h {
            self.orientation = o;
        }
    }
    pub fn set_scale(&mut self, s: f64) -> Result<(), GeometryError> {
        if !s.is_finite() || s <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        };
        self.scale = s;
        Ok(())
    }
    pub fn dimensions(self) -> [f64; 2] {
        match self.orientation {
            Orientation::Portrait => self.paper,
            Orientation::Landscape => [self.paper[1], self.paper[0]],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    world: [f64; 4],
    paper: [f64; 4],
    scale: f64,
    clip_boundary: Option<Vec<crate::Point3>>,
    clip_enabled: bool,
}
impl Viewport {
    pub fn new(world: [f64; 4], paper: [f64; 4], scale: f64) -> Result<Self, GeometryError> {
        if world.iter().any(|v| !v.is_finite())
            || paper.iter().any(|v| !v.is_finite())
            || world[2] <= world[0]
            || world[3] <= world[1]
            || paper[2] <= paper[0]
            || paper[3] <= paper[1]
        {
            return Err(GeometryError::DegenerateCurve);
        };
        let mut v = Self {
            world,
            paper,
            scale: 1.0,
            clip_boundary: None,
            clip_enabled: false,
        };
        v.set_scale(scale)?;
        Ok(v)
    }
    pub fn world(&self) -> [f64; 4] {
        self.world
    }
    pub fn paper(&self) -> [f64; 4] {
        self.paper
    }
    pub fn scale(&self) -> f64 {
        self.scale
    }
    pub fn clip_boundary(&self) -> Option<&[crate::Point3]> {
        self.clip_boundary.as_deref()
    }
    pub fn clip_enabled(&self) -> bool {
        self.clip_enabled
    }
    pub fn set_clip_enabled(&mut self, enabled: bool) -> Result<(), GeometryError> {
        if enabled && self.clip_boundary.is_none() {
            return Err(GeometryError::NotEnoughVertices);
        }
        self.clip_enabled = enabled;
        Ok(())
    }
    pub fn set_clip_boundary(&mut self, boundary: Vec<crate::Point3>) -> Result<(), GeometryError> {
        validate_clip_boundary(&boundary)?;
        self.clip_boundary = Some(boundary);
        self.clip_enabled = true;
        Ok(())
    }
    pub fn clear_clip_boundary(&mut self) {
        self.clip_boundary = None;
        self.clip_enabled = false;
    }
    pub fn set_scale(&mut self, s: f64) -> Result<(), GeometryError> {
        if !s.is_finite() || s <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        };
        self.scale = s;
        Ok(())
    }
    pub fn map(&self, p: [f64; 2]) -> [f64; 2] {
        [
            self.paper[0] + (p[0] - self.world[0]) * self.scale,
            self.paper[1] + (p[1] - self.world[1]) * self.scale,
        ]
    }
}

fn validate_clip_boundary(points: &[crate::Point3]) -> Result<(), GeometryError> {
    if points.len() < 3 || points.len() > 256 {
        return Err(GeometryError::NotEnoughVertices);
    }
    if points
        .iter()
        .any(|p| !p.x().is_finite() || !p.y().is_finite())
    {
        return Err(GeometryError::NonFiniteCoordinate);
    }
    let area = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.x() * b.y() - b.x() * a.y())
        .sum::<f64>();
    if area.abs() <= f64::EPSILON {
        return Err(GeometryError::DegenerateCurve);
    }
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        for j in (i + 1)..points.len() {
            if j == i + 1 || (i == 0 && j + 1 == points.len()) {
                continue;
            }
            let c = points[j];
            let d = points[(j + 1) % points.len()];
            if segments_intersect(a, b, c, d) {
                return Err(GeometryError::DegenerateCurve);
            }
        }
    }
    Ok(())
}

fn segments_intersect(
    a: crate::Point3,
    b: crate::Point3,
    c: crate::Point3,
    d: crate::Point3,
) -> bool {
    fn orient(a: crate::Point3, b: crate::Point3, c: crate::Point3) -> f64 {
        (b.x() - a.x()) * (c.y() - a.y()) - (b.y() - a.y()) * (c.x() - a.x())
    }
    let eps = 1.0e-10;
    let ab_c = orient(a, b, c);
    let ab_d = orient(a, b, d);
    let cd_a = orient(c, d, a);
    let cd_b = orient(c, d, b);
    let proper = (ab_c > eps && ab_d < -eps || ab_c < -eps && ab_d > eps)
        && (cd_a > eps && cd_b < -eps || cd_a < -eps && cd_b > eps);
    let on_segment = |p: crate::Point3, a: crate::Point3, b: crate::Point3| {
        p.x() >= a.x().min(b.x()) - eps
            && p.x() <= a.x().max(b.x()) + eps
            && p.y() >= a.y().min(b.y()) - eps
            && p.y() <= a.y().max(b.y()) + eps
    };
    proper
        || (ab_c.abs() <= eps && on_segment(c, a, b))
        || (ab_d.abs() <= eps && on_segment(d, a, b))
        || (cd_a.abs() <= eps && on_segment(a, c, d))
        || (cd_b.abs() <= eps && on_segment(b, c, d))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    name: String,
    setup: PageSetup,
    viewports: Vec<Viewport>,
}
impl Layout {
    pub fn new(name: impl Into<String>, setup: PageSetup) -> Result<Self, GeometryError> {
        let n = name.into();
        if n.trim().is_empty() {
            return Err(GeometryError::NonFiniteParameter);
        };
        Ok(Self {
            name: n,
            setup,
            viewports: Vec::new(),
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn setup(&self) -> PageSetup {
        self.setup
    }
    pub fn viewports(&self) -> &[Viewport] {
        &self.viewports
    }
    pub fn viewports_mut(&mut self) -> &mut [Viewport] {
        &mut self.viewports
    }
    pub fn set_setup(&mut self, s: PageSetup) {
        self.setup = s
    }
    pub fn add_viewport(&mut self, v: Viewport) {
        self.viewports.push(v)
    }
}

#[derive(Clone, Debug)]
pub struct LayoutCollection {
    layouts: BTreeMap<String, Layout>,
    active: String,
}
impl Default for LayoutCollection {
    fn default() -> Self {
        let setup = PageSetup::new(210.0, 297.0).expect("finite defaults");
        let layout = Layout::new("Model", setup).expect("valid default");
        let mut layouts = BTreeMap::new();
        layouts.insert("Model".into(), layout);
        Self {
            layouts,
            active: "Model".into(),
        }
    }
}
impl LayoutCollection {
    pub fn active(&self) -> &Layout {
        self.layouts
            .get(&self.active)
            .expect("active layout exists")
    }
    pub fn active_mut(&mut self) -> &mut Layout {
        self.layouts
            .get_mut(&self.active)
            .expect("active layout exists")
    }
    pub fn get(&self, n: &str) -> Option<&Layout> {
        self.layouts.get(n)
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.layouts.keys().map(String::as_str)
    }
    pub fn create(
        &mut self,
        name: impl Into<String>,
        setup: PageSetup,
    ) -> Result<(), GeometryError> {
        let n = name.into();
        if n.trim().is_empty() || self.layouts.contains_key(&n) {
            return Err(GeometryError::UnsupportedPolylineConversion);
        };
        self.layouts.insert(n.clone(), Layout::new(n, setup)?);
        Ok(())
    }
    pub fn set_active(&mut self, name: &str) -> Result<(), GeometryError> {
        if self.layouts.contains_key(name) {
            self.active = name.into();
            Ok(())
        } else {
            Err(GeometryError::UnsupportedPolylineConversion)
        }
    }
    pub fn copy(&mut self, source: &str, target: &str) -> Result<(), GeometryError> {
        if target.trim().is_empty() {
            return Err(GeometryError::NonFiniteParameter);
        }
        let mut l = self
            .get(source)
            .cloned()
            .ok_or(GeometryError::UnsupportedPolylineConversion)?;
        if self.layouts.contains_key(target) {
            return Err(GeometryError::UnsupportedPolylineConversion);
        };
        l.name = target.into();
        self.layouts.insert(target.into(), l);
        Ok(())
    }
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), GeometryError> {
        if old == "Model"
            || new.trim().is_empty()
            || !self.layouts.contains_key(old)
            || self.layouts.contains_key(new)
        {
            return Err(GeometryError::UnsupportedPolylineConversion);
        };
        let mut l = self.layouts.remove(old).expect("checked");
        l.name = new.into();
        self.layouts.insert(new.into(), l);
        if self.active == old {
            self.active = new.into()
        };
        Ok(())
    }
    pub fn delete(&mut self, name: &str) -> Result<(), GeometryError> {
        if name == "Model" || self.layouts.len() <= 1 {
            return Err(GeometryError::UnsupportedPolylineConversion);
        };
        if self.layouts.remove(name).is_none() {
            return Err(GeometryError::UnsupportedPolylineConversion);
        };
        if self.active == name {
            self.active = "Model".into()
        };
        Ok(())
    }
}

pub fn world_bounds(entities: &[Entity]) -> Result<[f64; 4], GeometryError> {
    crate::svg::entity_bounds(entities).map_err(|_| GeometryError::NonFiniteResult)
}
