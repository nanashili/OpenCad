//! Public annotation geometry.
//!
//! These types intentionally keep annotation data separate from `Entity`: callers can
//! render or export the generated primitives without introducing recursive entity variants.

use crate::{Arc3, GeometryError, Line3, Point3, Transform};
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlignment {
    Bottom,
    Middle,
    Top,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnnotationBounds {
    pub min: Point3,
    pub max: Point3,
}

impl AnnotationBounds {
    pub fn from_points(points: &[Point3]) -> Result<Self, GeometryError> {
        let first = *points.first().ok_or(GeometryError::NotEnoughVertices)?;
        let mut min = first;
        let mut max = first;
        for p in &points[1..] {
            min = Point3::new(min.x().min(p.x()), min.y().min(p.y()), min.z().min(p.z()))?;
            max = Point3::new(max.x().max(p.x()), max.y().max(p.y()), max.z().max(p.z()))?;
        }
        Ok(Self { min, max })
    }
    pub fn include(&mut self, other: Self) -> Result<(), GeometryError> {
        *self = Self::from_points(&[self.min, self.max, other.min, other.max])?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    content: String,
    location: Point3,
    height: f64,
    rotation: f64,
    width_factor: f64,
    horizontal: HorizontalAlignment,
    vertical: VerticalAlignment,
}

impl Text {
    pub fn new(
        content: impl Into<String>,
        location: Point3,
        height: f64,
    ) -> Result<Self, GeometryError> {
        let mut value = Self {
            content: content.into(),
            location,
            height: 0.0,
            rotation: 0.0,
            width_factor: 1.0,
            horizontal: HorizontalAlignment::Left,
            vertical: VerticalAlignment::Bottom,
        };
        value.set_height(height)?;
        Ok(value)
    }
    pub fn content(&self) -> &str {
        &self.content
    }
    pub fn set_content(&mut self, value: impl Into<String>) {
        self.content = value.into();
    }
    pub fn location(&self) -> Point3 {
        self.location
    }
    pub fn height(&self) -> f64 {
        self.height
    }
    pub fn rotation(&self) -> f64 {
        self.rotation
    }
    pub fn width_factor(&self) -> f64 {
        self.width_factor
    }
    pub fn alignment(&self) -> (HorizontalAlignment, VerticalAlignment) {
        (self.horizontal, self.vertical)
    }
    pub fn set_height(&mut self, value: f64) -> Result<(), GeometryError> {
        if !value.is_finite() || value <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        self.height = value;
        Ok(())
    }
    pub fn set_rotation(&mut self, value: f64) -> Result<(), GeometryError> {
        if !value.is_finite() {
            return Err(GeometryError::NonFiniteAngle);
        }
        self.rotation = value.rem_euclid(TAU);
        Ok(())
    }
    pub fn set_width_factor(&mut self, value: f64) -> Result<(), GeometryError> {
        if !value.is_finite() || value <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        self.width_factor = value;
        Ok(())
    }
    pub fn set_alignment(&mut self, horizontal: HorizontalAlignment, vertical: VerticalAlignment) {
        self.horizontal = horizontal;
        self.vertical = vertical;
    }
    pub fn bounds(&self) -> Result<AnnotationBounds, GeometryError> {
        rotated_box(
            self.location,
            self.content.chars().count() as f64 * self.height * 0.6 * self.width_factor,
            self.height,
            self.rotation,
            self.horizontal,
            self.vertical,
        )
    }
    pub fn transformed(&self, transform: Transform) -> Result<Self, GeometryError> {
        let mut out = self.clone();
        out.location = transform.point(self.location)?;
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MText {
    text: Text,
    width: f64,
}
impl MText {
    pub fn new(
        content: impl Into<String>,
        location: Point3,
        height: f64,
        width: f64,
    ) -> Result<Self, GeometryError> {
        if !width.is_finite() || width <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        Ok(Self {
            text: Text::new(content, location, height)?,
            width,
        })
    }
    pub fn text(&self) -> &Text {
        &self.text
    }
    pub fn text_mut(&mut self) -> &mut Text {
        &mut self.text
    }
    pub fn set_content(&mut self, value: impl Into<String>) {
        self.text.set_content(value);
    }
    pub fn width(&self) -> f64 {
        self.width
    }
    pub fn set_width(&mut self, width: f64) -> Result<(), GeometryError> {
        if !width.is_finite() || width <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        self.width = width;
        Ok(())
    }
    pub fn bounds(&self) -> Result<AnnotationBounds, GeometryError> {
        let b = self.text.bounds()?;
        let p = self.text.location();
        rotated_box(
            p,
            self.width,
            b.max.y() - b.min.y(),
            self.text.rotation(),
            HorizontalAlignment::Left,
            VerticalAlignment::Bottom,
        )
    }
    pub fn transformed(&self, transform: Transform) -> Result<Self, GeometryError> {
        Ok(Self {
            text: self.text.transformed(transform)?,
            width: self.width,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Leader {
    vertices: Vec<Point3>,
    annotation: Option<String>,
}
impl Leader {
    pub fn new(vertices: Vec<Point3>) -> Result<Self, GeometryError> {
        if vertices.len() < 2 {
            return Err(GeometryError::NotEnoughVertices);
        }
        Ok(Self {
            vertices,
            annotation: None,
        })
    }
    pub fn vertices(&self) -> &[Point3] {
        &self.vertices
    }
    pub fn annotation(&self) -> Option<&str> {
        self.annotation.as_deref()
    }
    pub fn set_annotation(&mut self, value: Option<impl Into<String>>) {
        self.annotation = value.map(Into::into);
    }
    pub fn segments(&self) -> Vec<Line3> {
        self.vertices
            .windows(2)
            .map(|p| Line3::new(p[0], p[1]))
            .collect()
    }
    pub fn bounds(&self) -> Result<AnnotationBounds, GeometryError> {
        AnnotationBounds::from_points(&self.vertices)
    }
    pub fn transformed(&self, transform: Transform) -> Result<Self, GeometryError> {
        Ok(Self {
            vertices: self
                .vertices
                .iter()
                .copied()
                .map(|p| transform.point(p))
                .collect::<Result<_, _>>()?,
            annotation: self.annotation.clone(),
        })
    }
}
pub type MLeader = Leader;

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    anchor: Point3,
    row_heights: Vec<f64>,
    column_widths: Vec<f64>,
    cells: Vec<Vec<String>>,
}
impl Table {
    pub fn new(
        anchor: Point3,
        row_heights: Vec<f64>,
        column_widths: Vec<f64>,
        cells: Vec<Vec<String>>,
    ) -> Result<Self, GeometryError> {
        if row_heights.is_empty()
            || column_widths.is_empty()
            || cells.len() != row_heights.len()
            || cells.iter().any(|r| r.len() != column_widths.len())
        {
            return Err(GeometryError::NotEnoughVertices);
        }
        for x in row_heights.iter().chain(column_widths.iter()) {
            if !x.is_finite() || *x <= 0.0 {
                return Err(GeometryError::NonPositiveScale);
            }
        }
        Ok(Self {
            anchor,
            row_heights,
            column_widths,
            cells,
        })
    }
    pub fn anchor(&self) -> Point3 {
        self.anchor
    }
    pub fn row_heights(&self) -> &[f64] {
        &self.row_heights
    }
    pub fn column_widths(&self) -> &[f64] {
        &self.column_widths
    }
    pub fn cells(&self) -> &[Vec<String>] {
        &self.cells
    }
    pub fn set_cell(
        &mut self,
        row: usize,
        column: usize,
        value: impl Into<String>,
    ) -> Result<(), GeometryError> {
        let Some(cells) = self.cells.get_mut(row) else {
            return Err(GeometryError::NotEnoughVertices);
        };
        let Some(cell) = cells.get_mut(column) else {
            return Err(GeometryError::NotEnoughVertices);
        };
        *cell = value.into();
        Ok(())
    }
    pub fn bounds(&self) -> Result<AnnotationBounds, GeometryError> {
        let w: f64 = self.column_widths.iter().sum();
        let h: f64 = self.row_heights.iter().sum();
        AnnotationBounds::from_points(&[
            self.anchor,
            Point3::new(self.anchor.x() + w, self.anchor.y() + h, self.anchor.z())?,
        ])
    }
    pub fn grid_lines(&self) -> Result<Vec<Line3>, GeometryError> {
        let mut out = Vec::new();
        let b = self.bounds()?;
        let mut x = self.anchor.x();
        out.push(Line3::new(
            Point3::new(x, self.anchor.y(), self.anchor.z())?,
            Point3::new(x, b.max.y(), self.anchor.z())?,
        ));
        for w in &self.column_widths {
            x += *w;
            out.push(Line3::new(
                Point3::new(x, self.anchor.y(), self.anchor.z())?,
                Point3::new(x, b.max.y(), self.anchor.z())?,
            ));
        }
        let mut y = self.anchor.y();
        out.push(Line3::new(
            self.anchor,
            Point3::new(b.max.x(), y, self.anchor.z())?,
        ));
        for h in &self.row_heights {
            y += *h;
            out.push(Line3::new(
                Point3::new(self.anchor.x(), y, self.anchor.z())?,
                Point3::new(b.max.x(), y, self.anchor.z())?,
            ));
        }
        Ok(out)
    }
    pub fn transformed(&self, transform: Transform) -> Result<Self, GeometryError> {
        Ok(Self {
            anchor: transform.point(self.anchor)?,
            row_heights: self.row_heights.clone(),
            column_widths: self.column_widths.clone(),
            cells: self.cells.clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DimensionKind {
    Linear {
        first: Point3,
        second: Point3,
        offset: Point3,
    },
    Aligned {
        first: Point3,
        second: Point3,
        offset: Point3,
    },
    Radial {
        center: Point3,
        definition: Point3,
    },
    Diametric {
        first: Point3,
        second: Point3,
    },
    Angular {
        center: Point3,
        first: Point3,
        second: Point3,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct Dimension {
    kind: DimensionKind,
    text_override: Option<String>,
    text_height: f64,
    arrow_size: f64,
}
impl Dimension {
    pub fn new(kind: DimensionKind) -> Result<Self, GeometryError> {
        Ok(Self {
            kind,
            text_override: None,
            text_height: 2.5,
            arrow_size: 1.0,
        })
    }
    pub fn kind(&self) -> DimensionKind {
        self.kind
    }
    pub fn text_override(&self) -> Option<&str> {
        self.text_override.as_deref()
    }
    pub fn set_text_override(&mut self, value: Option<impl Into<String>>) {
        self.text_override = value.map(Into::into);
    }
    pub fn set_text_height(&mut self, v: f64) -> Result<(), GeometryError> {
        if !v.is_finite() || v <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        self.text_height = v;
        Ok(())
    }
    pub fn set_arrow_size(&mut self, v: f64) -> Result<(), GeometryError> {
        if !v.is_finite() || v <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        self.arrow_size = v;
        Ok(())
    }
    pub fn geometry(&self) -> Result<Vec<DimensionPrimitive>, GeometryError> {
        generate_dimension(self)
    }
    pub fn bounds(&self) -> Result<AnnotationBounds, GeometryError> {
        let pts = self
            .geometry()?
            .iter()
            .flat_map(|p| p.points())
            .collect::<Vec<_>>();
        AnnotationBounds::from_points(&pts)
    }
    pub fn transformed(&self, t: Transform) -> Result<Self, GeometryError> {
        Ok(Self {
            kind: transform_kind(self.kind, t)?,
            ..self.clone()
        })
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum DimensionPrimitive {
    Line(Line3),
    Arc(Arc3),
    Text(Text),
}
impl DimensionPrimitive {
    fn points(&self) -> Vec<Point3> {
        match self {
            Self::Line(l) => vec![l.start(), l.end()],
            Self::Arc(a) => a.extrema().unwrap_or_default(),
            Self::Text(t) => t.bounds().map(|b| vec![b.min, b.max]).unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HatchPattern {
    Solid,
    Lines { angle: f64, spacing: f64 },
}
#[derive(Clone, Debug, PartialEq)]
pub struct Hatch {
    boundaries: Vec<Vec<Point3>>,
    pattern: HatchPattern,
}
impl Hatch {
    pub fn new(boundaries: Vec<Vec<Point3>>, pattern: HatchPattern) -> Result<Self, GeometryError> {
        if boundaries.is_empty() || boundaries.iter().any(|b| b.len() < 3) {
            return Err(GeometryError::NotEnoughVertices);
        }
        if let HatchPattern::Lines { angle, spacing } = pattern {
            if !angle.is_finite() {
                return Err(GeometryError::NonFiniteAngle);
            }
            if !spacing.is_finite() || spacing <= 0.0 {
                return Err(GeometryError::NonPositiveOffset);
            }
        }
        Ok(Self {
            boundaries,
            pattern,
        })
    }
    pub fn boundaries(&self) -> &[Vec<Point3>] {
        &self.boundaries
    }
    pub fn pattern(&self) -> HatchPattern {
        self.pattern
    }
    pub fn set_pattern(&mut self, pattern: HatchPattern) -> Result<(), GeometryError> {
        if let HatchPattern::Lines { angle, spacing } = pattern {
            if !angle.is_finite() {
                return Err(GeometryError::NonFiniteAngle);
            }
            if !spacing.is_finite() || spacing <= 0.0 {
                return Err(GeometryError::NonPositiveOffset);
            }
        }
        self.pattern = pattern;
        Ok(())
    }
    pub fn set_boundaries(&mut self, boundaries: Vec<Vec<Point3>>) -> Result<(), GeometryError> {
        if boundaries.is_empty() || boundaries.iter().any(|b| b.len() < 3) {
            return Err(GeometryError::NotEnoughVertices);
        }
        self.boundaries = boundaries;
        Ok(())
    }
    pub fn bounds(&self) -> Result<AnnotationBounds, GeometryError> {
        AnnotationBounds::from_points(
            &self
                .boundaries
                .iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>(),
        )
    }
    pub fn transformed(&self, t: Transform) -> Result<Self, GeometryError> {
        Ok(Self {
            boundaries: self
                .boundaries
                .iter()
                .map(|b| b.iter().copied().map(|p| t.point(p)).collect())
                .collect::<Result<_, _>>()?,
            pattern: self.pattern,
        })
    }
    pub fn primitives(&self) -> Result<Vec<HatchPrimitive>, GeometryError> {
        let mut out = self
            .boundaries
            .iter()
            .map(|b| HatchPrimitive::Polygon(b.clone()))
            .collect::<Vec<_>>();
        if let HatchPattern::Lines { angle, spacing } = self.pattern {
            let b = self.bounds()?;
            let (s, c) = angle.sin_cos();
            let span = (b.max.x() - b.min.x()).hypot(b.max.y() - b.min.y()) + spacing;
            let n = (span / spacing).ceil() as usize + 1;
            for i in 0..n {
                let d = -(span / 2.0) + (i as f64) * spacing;
                let p = Point3::new(
                    (b.min.x() + b.max.x()) / 2.0 + c * d,
                    (b.min.y() + b.max.y()) / 2.0 + s * d,
                    b.min.z(),
                )?;
                let a = Point3::new(
                    p.x() - c * span / 2.0 + s * span / 2.0,
                    p.y() - s * span / 2.0 - c * span / 2.0,
                    p.z(),
                )?;
                let z = Point3::new(
                    p.x() + c * span / 2.0 - s * span / 2.0,
                    p.y() + s * span / 2.0 + c * span / 2.0,
                    p.z(),
                )?;
                out.push(HatchPrimitive::Line(Line3::new(a, z)));
            }
        }
        Ok(out)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum HatchPrimitive {
    Line(Line3),
    Polygon(Vec<Point3>),
}

fn rotated_box(
    origin: Point3,
    w: f64,
    h: f64,
    a: f64,
    ha: HorizontalAlignment,
    va: VerticalAlignment,
) -> Result<AnnotationBounds, GeometryError> {
    let x = match ha {
        HorizontalAlignment::Left => 0.0,
        HorizontalAlignment::Center => -w / 2.0,
        HorizontalAlignment::Right => -w,
    };
    let y = match va {
        VerticalAlignment::Bottom => 0.0,
        VerticalAlignment::Middle => -h / 2.0,
        VerticalAlignment::Top => -h,
    };
    let (s, c) = a.sin_cos();
    let ps = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
        .iter()
        .map(|(u, v)| {
            Point3::new(
                origin.x() + c * u - s * v,
                origin.y() + s * u + c * v,
                origin.z(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    AnnotationBounds::from_points(&ps)
}

fn transform_kind(k: DimensionKind, t: Transform) -> Result<DimensionKind, GeometryError> {
    let p = |x: Point3| t.point(x);
    Ok(match k {
        DimensionKind::Linear {
            first,
            second,
            offset,
        } => DimensionKind::Linear {
            first: p(first)?,
            second: p(second)?,
            offset: p(offset)?,
        },
        DimensionKind::Aligned {
            first,
            second,
            offset,
        } => DimensionKind::Aligned {
            first: p(first)?,
            second: p(second)?,
            offset: p(offset)?,
        },
        DimensionKind::Radial { center, definition } => DimensionKind::Radial {
            center: p(center)?,
            definition: p(definition)?,
        },
        DimensionKind::Diametric { first, second } => DimensionKind::Diametric {
            first: p(first)?,
            second: p(second)?,
        },
        DimensionKind::Angular {
            center,
            first,
            second,
        } => DimensionKind::Angular {
            center: p(center)?,
            first: p(first)?,
            second: p(second)?,
        },
    })
}
fn generate_dimension(d: &Dimension) -> Result<Vec<DimensionPrimitive>, GeometryError> {
    let mut o = Vec::new();
    match d.kind {
        DimensionKind::Linear {
            first,
            second,
            offset,
        }
        | DimensionKind::Aligned {
            first,
            second,
            offset,
        } => {
            o.push(DimensionPrimitive::Line(Line3::new(first, offset)));
            o.push(DimensionPrimitive::Line(Line3::new(second, offset)));
            o.push(DimensionPrimitive::Line(Line3::new(
                offset,
                Point3::new(
                    offset.x() + second.x() - first.x(),
                    offset.y() + second.y() - first.y(),
                    offset.z(),
                )?,
            )));
            let mid = Line3::new(first, second).point_at_fraction(0.5)?;
            o.push(DimensionPrimitive::Text(Text::new(
                d.text_override.clone().unwrap_or_else(|| {
                    format!(
                        "{:.3}",
                        first.x().hypot(first.y()) - second.x().hypot(second.y())
                    )
                }),
                mid,
                d.text_height,
            )?));
        }
        DimensionKind::Radial { center, definition }
        | DimensionKind::Diametric {
            first: center,
            second: definition,
        } => {
            o.push(DimensionPrimitive::Line(Line3::new(center, definition)));
            let r = Line3::new(center, definition).length()?;
            let a = (definition.y() - center.y()).atan2(definition.x() - center.x());
            if matches!(d.kind, DimensionKind::Radial { .. }) {
                o.push(DimensionPrimitive::Arc(Arc3::new(center, r, a, a + PI)?));
            }
        }
        DimensionKind::Angular {
            center,
            first,
            second,
        } => {
            let a = (first.y() - center.y()).atan2(first.x() - center.x());
            let b = (second.y() - center.y()).atan2(second.x() - center.x());
            let r = Line3::new(center, first).length()?;
            o.push(DimensionPrimitive::Arc(Arc3::new(center, r, a, b)?));
        }
    }
    Ok(o)
}
