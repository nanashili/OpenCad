//! Paper-space layout and plotting models.

use crate::{Entity, GeometryError};
use std::{collections::BTreeMap, error::Error, fmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlotArea {
    Extents,
    Window([f64; 4]),
    Layout,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotTransform {
    scale: f64,
    translation: [f64; 2],
    clip_rect: [f64; 4],
}

impl PlotTransform {
    pub fn scale(self) -> f64 {
        self.scale
    }

    pub fn translation(self) -> [f64; 2] {
        self.translation
    }

    pub fn clip_rect(self) -> [f64; 4] {
        self.clip_rect
    }

    pub fn map(self, point: [f64; 2]) -> [f64; 2] {
        [
            point[0] * self.scale + self.translation[0],
            point[1] * self.scale + self.translation[1],
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    paper: [f64; 2],
    margins: [f64; 4],
    orientation: Orientation,
    scale: f64,
    plot_area: PlotArea,
    fit_to_paper: bool,
    plot_offset: [f64; 2],
    centered: bool,
}
impl PageSetup {
    pub fn new(width: f64, height: f64) -> Result<Self, GeometryError> {
        let mut p = Self {
            paper: [0.0; 2],
            margins: [0.0; 4],
            orientation: Orientation::Portrait,
            scale: 1.0,
            plot_area: PlotArea::Extents,
            fit_to_paper: false,
            plot_offset: [0.0; 2],
            centered: false,
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
    pub fn plot_area(self) -> PlotArea {
        self.plot_area
    }
    pub fn fit_to_paper(self) -> bool {
        self.fit_to_paper
    }
    pub fn plot_offset(self) -> [f64; 2] {
        self.plot_offset
    }
    pub fn centered(self) -> bool {
        self.centered
    }
    pub fn set_paper(&mut self, w: f64, h: f64) -> Result<(), GeometryError> {
        if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        };
        let dimensions = match self.orientation {
            Orientation::Portrait => [w, h],
            Orientation::Landscape => [h, w],
        };
        if !margins_fit(self.margins, dimensions) {
            return Err(GeometryError::NonPositiveScale);
        }
        self.paper = [w, h];
        Ok(())
    }
    pub fn set_margins(&mut self, m: [f64; 4]) -> Result<(), GeometryError> {
        if !margins_fit(m, self.dimensions()) {
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
        if margins_fit(self.margins, [w, h]) {
            self.orientation = o;
        }
    }
    pub fn set_scale(&mut self, s: f64) -> Result<(), GeometryError> {
        if !s.is_finite() || s <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        };
        self.scale = s;
        self.fit_to_paper = false;
        Ok(())
    }
    pub fn set_plot_area(&mut self, area: PlotArea) -> Result<(), GeometryError> {
        let area = match area {
            PlotArea::Window(bounds) => PlotArea::Window(normalized_bounds(bounds, false)?),
            area => area,
        };
        if matches!(area, PlotArea::Layout) {
            self.centered = false;
        }
        self.plot_area = area;
        Ok(())
    }
    pub fn set_fit_to_paper(&mut self, fit: bool) {
        self.fit_to_paper = fit;
    }
    pub fn set_plot_offset(&mut self, offset: [f64; 2]) -> Result<(), GeometryError> {
        if offset.iter().any(|value| !value.is_finite()) {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        self.plot_offset = offset;
        self.centered = false;
        Ok(())
    }
    pub fn set_centered(&mut self, centered: bool) -> Result<(), GeometryError> {
        if centered && matches!(self.plot_area, PlotArea::Layout) {
            return Err(GeometryError::NonFiniteParameter);
        }
        self.centered = centered;
        Ok(())
    }
    pub fn dimensions(self) -> [f64; 2] {
        match self.orientation {
            Orientation::Portrait => self.paper,
            Orientation::Landscape => [self.paper[1], self.paper[0]],
        }
    }

    fn validate(self) -> Result<(), GeometryError> {
        let dimensions = self.dimensions();
        if dimensions
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
            || !margins_fit(self.margins, dimensions)
            || !self.scale.is_finite()
            || self.scale <= 0.0
        {
            return Err(GeometryError::NonPositiveScale);
        }
        if self.plot_offset.iter().any(|value| !value.is_finite()) {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        if let PlotArea::Window(bounds) = self.plot_area {
            normalized_bounds(bounds, false)?;
        }
        if self.centered && matches!(self.plot_area, PlotArea::Layout) {
            return Err(GeometryError::NonFiniteParameter);
        }
        Ok(())
    }

    pub fn plot_transform(self, source_bounds: [f64; 4]) -> Result<PlotTransform, GeometryError> {
        self.validate()?;
        let printable = printable_rect(self.dimensions(), self.margins);
        let printable_width = printable[2] - printable[0];
        let printable_height = printable[3] - printable[1];
        let (scale, source) =
            self.plot_source_and_scale(source_bounds, printable_width, printable_height)?;
        let translation = self.plot_translation(source, scale, printable)?;
        if !scale.is_finite() || scale <= 0.0 || translation.iter().any(|value| !value.is_finite())
        {
            return Err(GeometryError::NonFiniteResult);
        }

        let mapped_source = map_bounds(source, scale, translation)?;
        let clip_rect = match self.plot_area {
            PlotArea::Window(_) => intersect_rect(printable, mapped_source),
            _ => printable,
        };
        if clip_rect.iter().any(|value| !value.is_finite()) {
            return Err(GeometryError::NonFiniteResult);
        }
        Ok(PlotTransform {
            scale,
            translation,
            clip_rect,
        })
    }

    fn plot_source_and_scale(
        self,
        source_bounds: [f64; 4],
        printable_width: f64,
        printable_height: f64,
    ) -> Result<(f64, [f64; 4]), GeometryError> {
        match self.plot_area {
            PlotArea::Layout => Ok((1.0, [0.0; 4])),
            PlotArea::Extents => {
                let source = normalized_bounds(source_bounds, true)?;
                let scale = self.plot_scale(source, printable_width, printable_height)?;
                Ok((scale, source))
            }
            PlotArea::Window(source) => {
                let source = normalized_bounds(source, false)?;
                let scale = self.plot_scale(source, printable_width, printable_height)?;
                Ok((scale, source))
            }
        }
    }

    fn plot_translation(
        self,
        source: [f64; 4],
        scale: f64,
        printable: [f64; 4],
    ) -> Result<[f64; 2], GeometryError> {
        if matches!(self.plot_area, PlotArea::Layout) {
            return Ok([
                printable[0] + self.plot_offset[0],
                printable[1] + self.plot_offset[1],
            ]);
        }

        let printable_width = printable[2] - printable[0];
        let printable_height = printable[3] - printable[1];
        let source_center = [
            source[0] * 0.5 + source[2] * 0.5,
            source[1] * 0.5 + source[3] * 0.5,
        ];
        let printable_center = [
            printable[0] + printable_width * 0.5,
            printable[1] + printable_height * 0.5,
        ];
        let anchor = if self.centered {
            printable_center
        } else {
            [printable[0], printable[1]]
        };
        let source_anchor = if self.centered {
            source_center
        } else {
            [source[0], source[1]]
        };
        let offset = if self.centered {
            [0.0, 0.0]
        } else {
            self.plot_offset
        };
        let translation = [
            anchor[0] - source_anchor[0] * scale + offset[0],
            anchor[1] - source_anchor[1] * scale + offset[1],
        ];
        if translation.iter().all(|value| value.is_finite()) {
            Ok(translation)
        } else {
            Err(GeometryError::NonFiniteResult)
        }
    }

    fn plot_scale(
        self,
        source: [f64; 4],
        printable_width: f64,
        printable_height: f64,
    ) -> Result<f64, GeometryError> {
        if !self.fit_to_paper {
            return Ok(self.scale);
        }
        let source_width = source[2] - source[0];
        let source_height = source[3] - source[1];
        let fit_width = if source_width > 0.0 {
            printable_width / source_width
        } else {
            f64::INFINITY
        };
        let fit_height = if source_height > 0.0 {
            printable_height / source_height
        } else {
            f64::INFINITY
        };
        let scale = if source_width == 0.0 && source_height == 0.0 {
            1.0
        } else {
            fit_width.min(fit_height)
        };
        if scale.is_finite() && scale > 0.0 {
            Ok(scale)
        } else {
            Err(GeometryError::NonFiniteResult)
        }
    }
}

fn normalized_bounds(bounds: [f64; 4], allow_zero_span: bool) -> Result<[f64; 4], GeometryError> {
    if bounds.iter().any(|value| !value.is_finite()) {
        return Err(GeometryError::NonFiniteCoordinate);
    }
    let normalized = [
        bounds[0].min(bounds[2]),
        bounds[1].min(bounds[3]),
        bounds[0].max(bounds[2]),
        bounds[1].max(bounds[3]),
    ];
    let width = normalized[2] - normalized[0];
    let height = normalized[3] - normalized[1];
    if !width.is_finite() || !height.is_finite() {
        return Err(GeometryError::NonFiniteResult);
    }
    if (!allow_zero_span && (width <= 0.0 || height <= 0.0))
        || (allow_zero_span && (width < 0.0 || height < 0.0))
    {
        return Err(GeometryError::DegenerateCurve);
    }
    Ok(normalized)
}

fn printable_rect(dimensions: [f64; 2], margins: [f64; 4]) -> [f64; 4] {
    [
        margins[0],
        margins[1],
        dimensions[0] - margins[2],
        dimensions[1] - margins[3],
    ]
}

fn map_bounds(
    bounds: [f64; 4],
    scale: f64,
    translation: [f64; 2],
) -> Result<[f64; 4], GeometryError> {
    let mapped = [
        bounds[0] * scale + translation[0],
        bounds[1] * scale + translation[1],
        bounds[2] * scale + translation[0],
        bounds[3] * scale + translation[1],
    ];
    if mapped.iter().all(|value| value.is_finite()) {
        Ok(mapped)
    } else {
        Err(GeometryError::NonFiniteResult)
    }
}

fn intersect_rect(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    let x_min = left[0].max(right[0]);
    let y_min = left[1].max(right[1]);
    let x_max = left[2].min(right[2]).max(x_min);
    let y_max = left[3].min(right[3]).max(y_min);
    [x_min, y_min, x_max, y_max]
}

const MAX_PAGE_SETUP_NAME: usize = 255;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageSetupError {
    InvalidName,
    DuplicateName,
    MissingName,
    Geometry(GeometryError),
}

impl fmt::Display for PageSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("page setup name is invalid"),
            Self::DuplicateName => f.write_str("a page setup with that name already exists"),
            Self::MissingName => f.write_str("named page setup was not found"),
            Self::Geometry(error) => error.fmt(f),
        }
    }
}

impl Error for PageSetupError {}

impl From<GeometryError> for PageSetupError {
    fn from(error: GeometryError) -> Self {
        Self::Geometry(error)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedPageSetup {
    name: String,
    setup: PageSetup,
}

impl NamedPageSetup {
    pub fn new(name: impl Into<String>, setup: PageSetup) -> Result<Self, PageSetupError> {
        setup.validate()?;
        Ok(Self {
            name: valid_page_setup_name(name)?,
            setup,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn setup(&self) -> PageSetup {
        self.setup
    }

    pub fn set_setup(&mut self, setup: PageSetup) -> Result<(), PageSetupError> {
        setup.validate()?;
        self.setup = setup;
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NamedPageSetupCatalog {
    entries: Vec<NamedPageSetup>,
}

impl NamedPageSetupCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[NamedPageSetup] {
        &self.entries
    }

    pub fn get(&self, name: &str) -> Result<&NamedPageSetup, PageSetupError> {
        let name = valid_page_setup_name(name)?;
        self.entries
            .iter()
            .find(|entry| same_page_setup_name(entry.name(), &name))
            .ok_or(PageSetupError::MissingName)
    }

    pub fn get_mut(&mut self, name: &str) -> Result<&mut NamedPageSetup, PageSetupError> {
        let name = valid_page_setup_name(name)?;
        self.entries
            .iter_mut()
            .find(|entry| same_page_setup_name(entry.name(), &name))
            .ok_or(PageSetupError::MissingName)
    }

    pub fn add(&mut self, entry: NamedPageSetup) -> Result<(), PageSetupError> {
        entry.setup.validate()?;
        let name = valid_page_setup_name(entry.name())?;
        if self
            .entries
            .iter()
            .any(|existing| same_page_setup_name(existing.name(), &name))
        {
            return Err(PageSetupError::DuplicateName);
        }
        self.entries.push(entry);
        Ok(())
    }

    pub fn insert_or_replace(&mut self, entry: NamedPageSetup) -> Result<(), PageSetupError> {
        entry.setup.validate()?;
        let name = valid_page_setup_name(entry.name())?;
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|existing| same_page_setup_name(existing.name(), &name))
        {
            existing.set_setup(entry.setup)?;
        } else {
            self.entries.push(entry);
        }
        Ok(())
    }

    pub fn rename(&mut self, old_name: &str, new_name: &str) -> Result<(), PageSetupError> {
        let old_name = valid_page_setup_name(old_name)?;
        let new_name = valid_page_setup_name(new_name)?;
        let index = self
            .entries
            .iter()
            .position(|entry| same_page_setup_name(entry.name(), &old_name))
            .ok_or(PageSetupError::MissingName)?;
        if self
            .entries
            .iter()
            .enumerate()
            .any(|(other, entry)| other != index && same_page_setup_name(entry.name(), &new_name))
        {
            return Err(PageSetupError::DuplicateName);
        }
        self.entries[index].name = new_name;
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<NamedPageSetup, PageSetupError> {
        let name = valid_page_setup_name(name)?;
        let index = self
            .entries
            .iter()
            .position(|entry| same_page_setup_name(entry.name(), &name))
            .ok_or(PageSetupError::MissingName)?;
        Ok(self.entries.remove(index))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    world: [f64; 4],
    paper: [f64; 4],
    scale: f64,
    clip_boundary: Option<Vec<crate::Point3>>,
    clip_enabled: bool,
    display: bool,
    display_locked: bool,
    frozen_layers: Vec<String>,
    frozen_layer_handles: Vec<String>,
    imported_display_status: Option<i32>,
    imported_displayed: Option<bool>,
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
            display: true,
            display_locked: false,
            frozen_layers: Vec::new(),
            frozen_layer_handles: Vec::new(),
            imported_display_status: None,
            imported_displayed: None,
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
    pub fn is_displayed(&self) -> bool {
        self.display
    }
    pub fn set_displayed(&mut self, displayed: bool) {
        self.display = displayed;
    }
    pub fn is_display_locked(&self) -> bool {
        self.display_locked
    }
    pub fn set_display_locked(&mut self, locked: bool) {
        self.display_locked = locked;
    }
    pub fn frozen_layers(&self) -> &[String] {
        &self.frozen_layers
    }
    pub fn freeze_layer(&mut self, name: impl Into<String>) -> bool {
        let name = name.into();
        if self
            .frozen_layers
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&name))
        {
            return false;
        }
        self.frozen_layers.push(name);
        true
    }
    pub fn thaw_layer(&mut self, name: &str) -> bool {
        let Some(index) = self
            .frozen_layers
            .iter()
            .position(|existing| existing.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        self.frozen_layers.remove(index);
        true
    }
    pub(crate) fn frozen_layer_handles(&self) -> &[String] {
        &self.frozen_layer_handles
    }
    pub(crate) fn set_imported_frozen_layer_handles(&mut self, handles: Vec<String>) {
        self.frozen_layer_handles = handles;
    }
    pub(crate) fn set_imported_display_status(&mut self, status: Option<i32>, displayed: bool) {
        self.imported_display_status = status;
        self.imported_displayed = Some(displayed);
    }
    pub(crate) fn display_status_changed(&self) -> bool {
        self.imported_displayed
            .is_some_and(|value| value != self.display)
    }
    pub(crate) fn imported_display_status(&self) -> Option<i32> {
        self.imported_display_status
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
    pub fn set_view(&mut self, world: [f64; 4], scale: f64) -> Result<(), GeometryError> {
        if world.iter().any(|value| !value.is_finite())
            || world[2] <= world[0]
            || world[3] <= world[1]
        {
            return Err(GeometryError::DegenerateCurve);
        }
        self.set_scale(scale)?;
        self.world = world;
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
    applied_page_setup: Option<String>,
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
            applied_page_setup: None,
            viewports: Vec::new(),
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn setup(&self) -> PageSetup {
        self.setup
    }
    pub fn applied_page_setup(&self) -> Option<&str> {
        self.applied_page_setup.as_deref()
    }
    pub fn viewports(&self) -> &[Viewport] {
        &self.viewports
    }
    pub fn viewports_mut(&mut self) -> &mut [Viewport] {
        &mut self.viewports
    }
    pub fn set_setup(&mut self, s: PageSetup) {
        self.setup = s;
        self.applied_page_setup = None;
    }
    fn apply_page_setup(&mut self, name: String, setup: PageSetup) {
        self.setup = setup;
        self.applied_page_setup = Some(name);
    }
    pub fn add_viewport(&mut self, v: Viewport) {
        self.viewports.push(v)
    }
}

#[derive(Clone, Debug)]
pub struct LayoutCollection {
    layouts: BTreeMap<String, Layout>,
    active: String,
    page_setups: NamedPageSetupCatalog,
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
            page_setups: NamedPageSetupCatalog::default(),
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

    pub fn get_mut(&mut self, n: &str) -> Option<&mut Layout> {
        self.layouts.get_mut(n)
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.layouts.keys().map(String::as_str)
    }
    pub fn page_setups(&self) -> &NamedPageSetupCatalog {
        &self.page_setups
    }
    pub fn page_setups_mut(&mut self) -> &mut NamedPageSetupCatalog {
        &mut self.page_setups
    }
    pub fn apply_page_setup(
        &mut self,
        layout_name: &str,
        setup_name: &str,
    ) -> Result<(), PageSetupError> {
        let layout_name = valid_page_setup_name(layout_name)?;
        let setup_name = valid_page_setup_name(setup_name)?;
        let setup = self.page_setups.get(&setup_name)?.clone();
        let layout = self
            .layouts
            .iter_mut()
            .find(|(name, _)| same_page_setup_name(name, &layout_name))
            .map(|(_, layout)| layout)
            .ok_or(PageSetupError::MissingName)?;
        layout.apply_page_setup(setup.name, setup.setup);
        Ok(())
    }
    pub(crate) fn restore_page_setup_snapshot(
        &mut self,
        layout_name: &str,
        setup: PageSetup,
        applied_name: Option<&str>,
    ) -> Result<(), PageSetupError> {
        let layout_name = valid_page_setup_name(layout_name)?;
        setup.validate()?;
        let applied_name = applied_name
            .map(valid_page_setup_name)
            .transpose()?
            .map(|name| self.page_setups.get(&name).map(|entry| entry.name.clone()))
            .transpose()?;
        let layout = self
            .layouts
            .iter_mut()
            .find(|(name, _)| same_page_setup_name(name, &layout_name))
            .map(|(_, layout)| layout)
            .ok_or(PageSetupError::MissingName)?;
        layout.setup = setup;
        layout.applied_page_setup = applied_name;
        Ok(())
    }
    pub fn rename_page_setup(
        &mut self,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), PageSetupError> {
        let old_name = valid_page_setup_name(old_name)?;
        let new_name = valid_page_setup_name(new_name)?;
        self.page_setups.rename(&old_name, &new_name)?;
        for layout in self.layouts.values_mut() {
            if layout
                .applied_page_setup
                .as_deref()
                .is_some_and(|name| same_page_setup_name(name, &old_name))
            {
                layout.applied_page_setup = Some(new_name.clone());
            }
        }
        Ok(())
    }
    pub fn remove_page_setup(&mut self, name: &str) -> Result<NamedPageSetup, PageSetupError> {
        let name = valid_page_setup_name(name)?;
        let removed = self.page_setups.remove(&name)?;
        for layout in self.layouts.values_mut() {
            if layout
                .applied_page_setup
                .as_deref()
                .is_some_and(|applied| same_page_setup_name(applied, &name))
            {
                layout.applied_page_setup = None;
            }
        }
        Ok(removed)
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

fn valid_page_setup_name(name: impl Into<String>) -> Result<String, PageSetupError> {
    let name = name.into();
    if name.trim().is_empty() || name.len() > MAX_PAGE_SETUP_NAME {
        return Err(PageSetupError::InvalidName);
    }
    Ok(name)
}

fn same_page_setup_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn margins_fit(margins: [f64; 4], dimensions: [f64; 2]) -> bool {
    margins
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
        && margins[0] + margins[2] < dimensions[0]
        && margins[1] + margins[3] < dimensions[1]
}

pub fn world_bounds(entities: &[Entity]) -> Result<[f64; 4], GeometryError> {
    crate::svg::entity_bounds(entities).map_err(|_| GeometryError::NonFiniteResult)
}
