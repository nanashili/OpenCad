//! Validated planar MLINE geometry and named MLINESTYLE data.

use crate::{GeometryError, Line3, Point3, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MLineJustification {
    Top,
    Zero,
    Bottom,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MLineElement {
    offset: f64,
    color: Option<u8>,
    linetype: Option<String>,
}

impl MLineElement {
    pub fn new(offset: f64) -> Result<Self, GeometryError> {
        if !offset.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        Ok(Self {
            offset,
            color: None,
            linetype: None,
        })
    }
    pub fn offset(&self) -> f64 {
        self.offset
    }
    pub fn color(&self) -> Option<u8> {
        self.color
    }
    pub fn linetype(&self) -> Option<&str> {
        self.linetype.as_deref()
    }
    pub fn set_color(&mut self, color: Option<u8>) {
        self.color = color;
    }
    pub fn set_linetype(&mut self, linetype: Option<String>) {
        self.linetype = linetype;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MLineStyle {
    name: String,
    elements: Vec<MLineElement>,
    start_cap: bool,
    end_cap: bool,
}

impl MLineStyle {
    pub fn new(name: impl Into<String>, offsets: Vec<f64>) -> Result<Self, GeometryError> {
        let name = name.into();
        if name.trim().is_empty() || offsets.is_empty() || offsets.len() > 16 {
            return Err(GeometryError::NotEnoughVertices);
        }
        let elements = offsets
            .into_iter()
            .map(MLineElement::new)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            name,
            elements,
            start_cap: false,
            end_cap: false,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn elements(&self) -> &[MLineElement] {
        &self.elements
    }
    pub fn set_elements(&mut self, elements: Vec<MLineElement>) -> Result<(), GeometryError> {
        if elements.is_empty() || elements.len() > 16 {
            return Err(GeometryError::NotEnoughVertices);
        }
        self.elements = elements;
        Ok(())
    }
    pub fn start_cap(&self) -> bool {
        self.start_cap
    }
    pub fn end_cap(&self) -> bool {
        self.end_cap
    }
    pub fn set_caps(&mut self, start: bool, end: bool) {
        self.start_cap = start;
        self.end_cap = end;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MLine2 {
    vertices: Vec<Point3>,
    closed: bool,
    scale: f64,
    justification: MLineJustification,
    style: MLineStyle,
}

impl MLine2 {
    pub fn new(vertices: Vec<Point3>, style: MLineStyle) -> Result<Self, GeometryError> {
        validate_path(&vertices)?;
        Ok(Self {
            vertices,
            closed: false,
            scale: 1.0,
            justification: MLineJustification::Zero,
            style,
        })
    }
    pub fn vertices(&self) -> &[Point3] {
        &self.vertices
    }
    pub fn style(&self) -> &MLineStyle {
        &self.style
    }
    pub fn style_mut(&mut self) -> &mut MLineStyle {
        &mut self.style
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn scale(&self) -> f64 {
        self.scale
    }
    pub fn justification(&self) -> MLineJustification {
        self.justification
    }
    pub fn set_closed(&mut self, closed: bool) -> Result<(), GeometryError> {
        if closed && self.vertices.len() < 3 {
            return Err(GeometryError::NotEnoughVertices);
        }
        self.closed = closed;
        Ok(())
    }
    pub fn set_scale(&mut self, scale: f64) -> Result<(), GeometryError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        self.scale = scale;
        Ok(())
    }
    pub fn set_justification(&mut self, justification: MLineJustification) {
        self.justification = justification;
    }
    pub fn set_vertices(&mut self, vertices: Vec<Point3>) -> Result<(), GeometryError> {
        validate_path(&vertices)?;
        if self.closed && vertices.len() < 3 {
            return Err(GeometryError::NotEnoughVertices);
        }
        self.vertices = vertices;
        Ok(())
    }
    pub fn translated(&self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        self.transformed(Transform::translation(dx, dy, dz))
    }
    pub fn transformed(&self, transform: Transform) -> Result<Self, GeometryError> {
        let vertices = self
            .vertices
            .iter()
            .copied()
            .map(|p| transform.point(p))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(vertices, self.style.clone()).map(|mut result| {
            result.closed = self.closed;
            result.scale = self.scale;
            result.justification = self.justification;
            result
        })
    }
    pub fn bounds(&self) -> Result<[f64; 4], GeometryError> {
        let paths = self.explode()?;
        let points = paths.iter().flatten().copied().collect::<Vec<_>>();
        let first = *points.first().ok_or(GeometryError::NotEnoughVertices)?;
        let mut b = [first.x(), first.y(), first.x(), first.y()];
        for p in points {
            b[0] = b[0].min(p.x());
            b[1] = b[1].min(p.y());
            b[2] = b[2].max(p.x());
            b[3] = b[3].max(p.y());
        }
        Ok(b)
    }
    /// Returns one open parallel path per style element. Closed paths repeat their first vertex.
    pub fn explode(&self) -> Result<Vec<Vec<Point3>>, GeometryError> {
        let mut result = Vec::with_capacity(self.style.elements.len());
        let adjustment = match self.justification {
            MLineJustification::Top => self
                .style
                .elements
                .iter()
                .map(|e| e.offset())
                .fold(f64::NEG_INFINITY, f64::max),
            MLineJustification::Bottom => self
                .style
                .elements
                .iter()
                .map(|e| e.offset())
                .fold(f64::INFINITY, f64::min),
            MLineJustification::Zero => 0.0,
        };
        for element in &self.style.elements {
            let distance = (element.offset() - adjustment) * self.scale;
            result.push(offset_path(&self.vertices, self.closed, distance)?);
        }
        Ok(result)
    }
    pub fn cap_lines(&self) -> Result<Vec<Line3>, GeometryError> {
        if !self.style.start_cap() && !self.style.end_cap() {
            return Ok(Vec::new());
        }
        let paths = self.explode()?;
        let mut result = Vec::new();
        if self.style.start_cap() {
            result.extend(cap_at(&paths, true)?);
        }
        if self.style.end_cap() {
            result.extend(cap_at(&paths, false)?);
        }
        Ok(result)
    }
}

fn validate_path(points: &[Point3]) -> Result<(), GeometryError> {
    if points.len() < 2 || points.len() > 100_000 {
        return Err(GeometryError::NotEnoughVertices);
    }
    let z = points[0].z();
    if points.iter().any(|p| {
        !p.x().is_finite() || !p.y().is_finite() || !p.z().is_finite() || (p.z() - z).abs() > 1.0e-8
    }) {
        return Err(GeometryError::NonPlanarPolyline);
    }
    if points
        .windows(2)
        .any(|pair| pair[0].x() == pair[1].x() && pair[0].y() == pair[1].y())
    {
        return Err(GeometryError::DegenerateLine);
    }
    Ok(())
}

fn offset_path(
    points: &[Point3],
    closed: bool,
    distance: f64,
) -> Result<Vec<Point3>, GeometryError> {
    let count = points.len();
    let mut output = Vec::with_capacity(count + usize::from(closed));
    for i in 0..count {
        if !closed && (i == 0 || i + 1 == count) {
            let next = if i == 0 { points[1] } else { points[count - 1] };
            let prev = if i == 0 { points[0] } else { points[count - 2] };
            let dx = next.x() - prev.x();
            let dy = next.y() - prev.y();
            let length = dx.hypot(dy);
            output.push(Point3::new(
                points[i].x() - dy / length * distance,
                points[i].y() + dx / length * distance,
                points[i].z(),
            )?);
            continue;
        }
        let prev = points[(i + count - 1) % count];
        let next = points[(i + 1) % count];
        let current = points[i];
        let (dx1, dy1) = (current.x() - prev.x(), current.y() - prev.y());
        let (dx2, dy2) = (next.x() - current.x(), next.y() - current.y());
        let l1 = dx1.hypot(dy1);
        let l2 = dx2.hypot(dy2);
        let nx = -dy1 / l1 - dy2 / l2;
        let ny = dx1 / l1 + dx2 / l2;
        let n = nx.hypot(ny);
        let (ox, oy) = if n <= f64::EPSILON {
            (-dy1 / l1, dx1 / l1)
        } else {
            (nx / n, ny / n)
        };
        let miter = (ox * (-dy1 / l1) + oy * (dx1 / l1)).abs().max(0.1);
        output.push(Point3::new(
            current.x() + ox * distance / miter,
            current.y() + oy * distance / miter,
            current.z(),
        )?);
    }
    if closed {
        output.push(output[0]);
    }
    Ok(output)
}

fn cap_at(paths: &[Vec<Point3>], start: bool) -> Result<Vec<Line3>, GeometryError> {
    let mut result = Vec::new();
    for pair in paths.windows(2) {
        let a = if start {
            pair[0][0]
        } else {
            *pair[0].last().ok_or(GeometryError::NotEnoughVertices)?
        };
        let b = if start {
            pair[1][0]
        } else {
            *pair[1].last().ok_or(GeometryError::NotEnoughVertices)?
        };
        result.push(Line3::new(a, b));
    }
    Ok(result)
}
