use crate::{Arc3, Entity, GeometryError, Line3, Point3};
use std::f64::consts::{PI, TAU};

/// An ordered line/arc polyline in a world-XY plane.
/// Coincident vertices remain representable, as they do for LINE geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline2 {
    vertices: Vec<Point3>,
    bulges: Vec<f64>,
    widths: Vec<PolylineWidth>,
    closed: bool,
}

/// Effective starting and ending widths of one outgoing segment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PolylineWidth {
    start: f64,
    end: f64,
}

impl PolylineWidth {
    pub fn new(start: f64, end: f64) -> Result<Self, GeometryError> {
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end < 0.0 {
            return Err(GeometryError::InvalidPolylineWidth);
        }
        Ok(Self { start, end })
    }

    pub fn start(self) -> f64 {
        self.start
    }
    pub fn end(self) -> f64 {
        self.end
    }

    fn reversed(self) -> Self {
        Self {
            start: self.end,
            end: self.start,
        }
    }
}

/// A segment follows vertex order; Arc3 itself always stores a CCW locus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PolylineSegment {
    Line(Line3),
    Arc { arc: Arc3, clockwise: bool },
}

impl PolylineSegment {
    pub fn length(self) -> Result<f64, GeometryError> {
        match self {
            Self::Line(line) => line.length(),
            Self::Arc { arc, .. } => arc.length(),
        }
    }

    pub fn point_at_fraction(self, fraction: f64) -> Result<Point3, GeometryError> {
        match self {
            Self::Line(line) => line.point_at_fraction(fraction),
            Self::Arc { arc, clockwise } => {
                arc.point_at_fraction(if clockwise { 1.0 - fraction } else { fraction })
            }
        }
    }

    pub(crate) fn geometry(self) -> Entity {
        match self {
            Self::Line(line) => Entity::Line(line),
            Self::Arc { arc, .. } => Entity::Arc(arc),
        }
    }
}

impl Polyline2 {
    pub fn new(vertices: Vec<Point3>, closed: bool) -> Result<Self, GeometryError> {
        let elevation = vertices
            .first()
            .filter(|_| vertices.len() >= 2)
            .ok_or(GeometryError::NotEnoughVertices)?
            .z();
        if vertices.iter().any(|point| point.z() != elevation) {
            return Err(GeometryError::NonPlanarPolyline);
        }
        let bulges = vec![0.0; vertices.len()];
        let widths = vec![PolylineWidth::default(); vertices.len()];
        Ok(Self {
            vertices,
            bulges,
            widths,
            closed,
        })
    }

    /// Each bulge belongs to its vertex's outgoing segment. The final value
    /// is retained but unused while the polyline is open.
    pub fn with_bulges(
        vertices: Vec<Point3>,
        bulges: Vec<f64>,
        closed: bool,
    ) -> Result<Self, GeometryError> {
        let mut polyline = Self::new(vertices, closed)?;
        polyline.set_bulges(bulges)?;
        Ok(polyline)
    }

    pub fn vertices(&self) -> &[Point3] {
        &self.vertices
    }

    pub fn bulges(&self) -> &[f64] {
        &self.bulges
    }

    /// Like bulges, the last open value is dormant and is retained.
    /// DXF import resolves omitted values against the parent defaults.
    pub fn widths(&self) -> &[PolylineWidth] {
        &self.widths
    }

    pub fn set_widths(&mut self, widths: Vec<PolylineWidth>) -> Result<(), GeometryError> {
        if widths.len() != self.vertices.len() {
            return Err(GeometryError::InvalidWidthCount);
        }
        self.widths = widths;
        Ok(())
    }

    pub fn set_width(&mut self, index: usize, start: f64, end: f64) -> Result<(), GeometryError> {
        let width = PolylineWidth::new(start, end)?;
        *self
            .widths
            .get_mut(index)
            .ok_or(GeometryError::InvalidVertexIndex)? = width;
        Ok(())
    }

    pub fn set_uniform_width(&mut self, width: f64) -> Result<(), GeometryError> {
        let width = PolylineWidth::new(width, width)?;
        self.widths.fill(width);
        Ok(())
    }

    pub fn set_bulges(&mut self, bulges: Vec<f64>) -> Result<(), GeometryError> {
        if bulges.len() != self.vertices.len() {
            return Err(GeometryError::InvalidBulgeCount);
        }
        if bulges.iter().any(|bulge| !bulge.is_finite()) {
            return Err(GeometryError::NonFiniteBulge);
        }
        self.bulges = bulges;
        Ok(())
    }

    pub fn set_bulge(&mut self, index: usize, bulge: f64) -> Result<(), GeometryError> {
        if !bulge.is_finite() {
            return Err(GeometryError::NonFiniteBulge);
        }
        let target = self
            .bulges
            .get_mut(index)
            .ok_or(GeometryError::InvalidVertexIndex)?;
        *target = bulge;
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn set_closed(&mut self, closed: bool) {
        self.closed = closed;
    }

    pub fn elevation(&self) -> f64 {
        self.vertices[0].z()
    }

    pub fn set_vertices(&mut self, vertices: Vec<Point3>) -> Result<(), GeometryError> {
        let mut replacement = Self::new(vertices, self.closed)?;
        let count = replacement.bulges.len().min(self.bulges.len());
        replacement.bulges[..count].copy_from_slice(&self.bulges[..count]);
        replacement.widths[..count].copy_from_slice(&self.widths[..count]);
        *self = replacement;
        Ok(())
    }

    pub fn reverse(&mut self) {
        self.reverse_with_widths(true);
    }

    /// Reverse vertex order. When requested, also reverse width order and ends
    /// to preserve the geometric taper. Otherwise width pairs keep their indices.
    pub fn reverse_with_widths(&mut self, reverse_widths: bool) {
        self.vertices.reverse();
        let last = self.bulges.len() - 1;
        self.bulges[..last].reverse();
        if reverse_widths {
            self.widths[..last].reverse();
            for width in &mut self.widths {
                *width = width.reversed();
            }
        }
        for bulge in &mut self.bulges {
            if *bulge != 0.0 {
                *bulge = -*bulge;
            }
        }
    }

    /// Remove the stored-order range between two vertices, retaining both ends.
    /// An open polyline splits or truncates; at least one vertex must be interior.
    /// A closed polyline retains the complementary path through its closing edge
    /// as one open polyline. Equal closed markers open the whole loop at that
    /// vertex. Retained outgoing bulges, including dormant final values, survive.
    pub fn break_vertices(&self, first: usize, second: usize) -> Result<Vec<Self>, GeometryError> {
        let count = self.vertices.len();
        if first >= count || second >= count {
            return Err(GeometryError::InvalidVertexIndex);
        }
        let lower = first.min(second);
        let upper = first.max(second);
        if !self.closed
            && ((lower == 0 && upper == count - 1)
                || (lower == upper && (lower == 0 || lower == count - 1)))
        {
            return Err(GeometryError::InvalidPolylineBreak);
        }
        self.length()?;
        self.extrema()?;
        let fragment = |indices: Vec<usize>| -> Result<Self, GeometryError> {
            let mut part = Self::with_bulges(
                indices.iter().map(|&index| self.vertices[index]).collect(),
                indices.iter().map(|&index| self.bulges[index]).collect(),
                false,
            )?;
            part.set_widths(indices.iter().map(|&index| self.widths[index]).collect())?;
            part.length()?;
            part.extrema()?;
            Ok(part)
        };
        if self.closed {
            return Ok(vec![fragment((upper..count).chain(0..=lower).collect())?]);
        }
        let mut parts = Vec::with_capacity(2);
        if lower > 0 {
            parts.push(fragment((0..=lower).collect())?);
        }
        if upper < count - 1 {
            parts.push(fragment((upper..count).collect())?);
        }
        Ok(parts)
    }

    pub fn set_elevation(&mut self, elevation: f64) -> Result<(), GeometryError> {
        if !elevation.is_finite() {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        for point in &mut self.vertices {
            *point = Point3::new(point.x(), point.y(), elevation)
                .expect("existing XY and validated elevation are finite");
        }
        Ok(())
    }

    pub fn segments(&self) -> impl Iterator<Item = Result<PolylineSegment, GeometryError>> + '_ {
        let count = self.vertices.len() - usize::from(!self.closed);
        (0..count).map(|index| {
            segment(
                self.vertices[index],
                self.vertices[(index + 1) % self.vertices.len()],
                self.bulges[index],
            )
        })
    }

    pub fn has_active_width(&self) -> bool {
        self.widths
            .iter()
            .take(self.vertices.len() - usize::from(!self.closed))
            .any(|width| width.start != 0.0 || width.end != 0.0)
    }

    /// Original display policy: linear taper along distance, flat segment caps,
    /// and arc strips sampled at no more than PI/64 radians per interval.
    /// Inner arc offsets stop at the center when half-width exceeds the radius.
    pub fn width_quads_xy(&self) -> Result<Vec<[Point3; 4]>, GeometryError> {
        let mut quads = Vec::new();
        for (index, segment) in self.segments().enumerate() {
            let width = self.widths[index];
            if width.start == 0.0 && width.end == 0.0 {
                continue;
            }
            let segment = segment?;
            let (steps, normal) = match segment {
                PolylineSegment::Line(line) => {
                    let length = line.length()?;
                    if length == 0.0 {
                        continue;
                    }
                    (
                        1,
                        Some((
                            -(line.end().y() - line.start().y()) / length,
                            (line.end().x() - line.start().x()) / length,
                        )),
                    )
                }
                PolylineSegment::Arc { arc, .. } => (
                    (arc.sweep() / (PI / 64.0)).ceil().clamp(1.0, 256.0) as usize,
                    None,
                ),
            };
            let edge = |fraction: f64| -> Result<[Point3; 2], GeometryError> {
                let point = segment.point_at_fraction(fraction)?;
                let half = (width.start / 2.0) * (1.0 - fraction) + (width.end / 2.0) * fraction;
                let (nx, ny, left, right) = if let Some((nx, ny)) = normal {
                    (nx, ny, half, half)
                } else if let PolylineSegment::Arc { arc, .. } = segment {
                    let angle = arc.start_angle()
                        + arc.sweep()
                            * match segment {
                                PolylineSegment::Arc {
                                    clockwise: true, ..
                                } => 1.0 - fraction,
                                _ => fraction,
                            };
                    // Radial outer/inner edges are independent of traversal direction.
                    (angle.cos(), angle.sin(), half, half.min(arc.radius()))
                } else {
                    unreachable!("arc normal")
                };
                Ok([
                    Point3::new(point.x() + nx * left, point.y() + ny * left, point.z())
                        .map_err(|_| GeometryError::NonFiniteResult)?,
                    Point3::new(point.x() - nx * right, point.y() - ny * right, point.z())
                        .map_err(|_| GeometryError::NonFiniteResult)?,
                ])
            };
            let mut previous = edge(0.0)?;
            for step in 1..=steps {
                let next = edge(step as f64 / steps as f64)?;
                quads.push([previous[0], next[0], next[1], previous[1]]);
                previous = next;
            }
        }
        Ok(quads)
    }

    /// Decompose the centerline into independent LINE/ARC entities in segment order.
    /// Closed polylines include the closing edge; coincident vertices yield a
    /// zero-length line. Standalone arcs store the same locus counterclockwise.
    pub fn explode(&self) -> Result<Vec<Entity>, GeometryError> {
        self.segments()
            .map(|segment| segment.map(PolylineSegment::geometry))
            .collect()
    }

    /// Attach one open XY line, arc or polyline at a touching endpoint.
    /// Source coordinates win at the junction. Matching prefers appending,
    /// then prepending; the candidate may be reversed. Closed/off-plane and
    /// unsupported candidates are ignored. The output remains explicitly open.
    pub fn join_endpoints_xy(&self, other: &Entity) -> Result<Option<Self>, GeometryError> {
        if self.closed {
            return Err(GeometryError::ClosedPolylineJoin);
        }
        self.length()?;
        self.extrema()?;
        let elevation = match other {
            Entity::Line(line) if line.start().z() == line.end().z() => line.start().z(),
            Entity::Arc(arc) => arc.center().z(),
            Entity::Polyline(polyline) if !polyline.is_closed() => polyline.elevation(),
            _ => return Ok(None),
        };
        if elevation != self.elevation() {
            return Ok(None);
        }
        let mut candidate = other.to_polyline_xy()?;
        let first = self.vertices[0];
        let last = self.vertices[self.vertices.len() - 1];
        let candidate_first = candidate.vertices[0];
        let candidate_last = candidate.vertices[candidate.vertices.len() - 1];
        let append = if touching_xy(last, candidate_first) {
            true
        } else if touching_xy(last, candidate_last) {
            candidate.reverse();
            true
        } else if touching_xy(first, candidate_last) {
            false
        } else if touching_xy(first, candidate_first) {
            candidate.reverse();
            false
        } else {
            return Ok(None);
        };
        let (vertices, bulges) = if append {
            (
                self.vertices
                    .iter()
                    .chain(&candidate.vertices[1..])
                    .copied()
                    .collect(),
                self.bulges[..self.bulges.len() - 1]
                    .iter()
                    .chain(&candidate.bulges)
                    .copied()
                    .collect(),
            )
        } else {
            (
                candidate.vertices[..candidate.vertices.len() - 1]
                    .iter()
                    .chain(&self.vertices)
                    .copied()
                    .collect(),
                candidate.bulges[..candidate.bulges.len() - 1]
                    .iter()
                    .chain(&self.bulges)
                    .copied()
                    .collect(),
            )
        };
        let widths = if append {
            self.widths[..self.widths.len() - 1]
                .iter()
                .chain(&candidate.widths)
                .copied()
                .collect()
        } else {
            candidate.widths[..candidate.widths.len() - 1]
                .iter()
                .chain(&self.widths)
                .copied()
                .collect()
        };
        let mut result = Self::with_bulges(vertices, bulges, false)?;
        result.set_widths(widths)?;
        result.length()?;
        result.extrema()?;
        Ok(Some(result))
    }

    pub(crate) fn extrema(&self) -> Result<Vec<Point3>, GeometryError> {
        let mut points = self.centerline_extrema()?;
        if self.has_active_width() {
            points.extend(self.width_quads_xy()?.into_iter().flatten());
        }
        Ok(points)
    }

    pub(crate) fn centerline_extrema(&self) -> Result<Vec<Point3>, GeometryError> {
        let mut points = self.vertices.clone();
        for segment in self.segments() {
            if let PolylineSegment::Arc { arc, .. } = segment? {
                points.extend(arc.extrema()?);
            }
        }
        Ok(points)
    }

    pub fn length(&self) -> Result<f64, GeometryError> {
        self.segments().try_fold(0.0, |total, segment| {
            let total = total + segment?.length()?;
            if total.is_finite() {
                Ok(total)
            } else {
                Err(GeometryError::NonFiniteResult)
            }
        })
    }
}

fn touching_xy(a: Point3, b: Point3) -> bool {
    // OpenCad's numerical allowance, not a user-specified fuzz distance.
    let magnitude = a
        .x()
        .abs()
        .max(a.y().abs())
        .max(b.x().abs())
        .max(b.y().abs());
    let allowance = 1e-10 + 8.0 * f64::EPSILON * magnitude;
    (a.x() - b.x()).hypot(a.y() - b.y()) <= allowance
}

impl Entity {
    /// Convert an XY line or arc to a single-segment, zero-width polyline.
    /// Existing polylines retain their vertices, bulges and closing flag.
    pub fn to_polyline_xy(&self) -> Result<Polyline2, GeometryError> {
        let polyline = match self {
            Self::Polyline(polyline) => polyline.clone(),
            Self::Line(line) => Polyline2::new(vec![line.start(), line.end()], false)?,
            Self::Arc(arc) => {
                if arc.radius() == 0.0 || arc.sweep() == 0.0 {
                    return Err(GeometryError::DegenerateCurve);
                }
                let start = arc.start_point()?;
                let end = arc.end_point()?;
                if start == end {
                    return Err(GeometryError::UnrepresentableBulge);
                }
                Polyline2::with_bulges(
                    vec![start, end],
                    vec![(arc.sweep() / 4.0).tan(), 0.0],
                    false,
                )?
            }
            _ => return Err(GeometryError::UnsupportedPolylineConversion),
        };
        polyline.length()?;
        polyline.extrema()?;
        Ok(polyline)
    }
}

fn segment(start: Point3, end: Point3, bulge: f64) -> Result<PolylineSegment, GeometryError> {
    let line = Line3::new(start, end);
    if bulge == 0.0 || start == end {
        // Coincident endpoints do not determine a unique circular locus.
        // Keep the stored bulge and use the existing zero-length line policy.
        return Ok(PolylineSegment::Line(line));
    }
    let dx = end.x() - start.x();
    let dy = end.y() - start.y();
    let chord = line.length()?;
    let quarter = chord / 4.0;
    // Algebra from b = tan(theta/4). Splitting the terms avoids b*b overflow.
    let radius = quarter * bulge.abs() + quarter / bulge.abs();
    let offset = quarter / bulge - quarter * bulge;
    let center = Point3::new(
        start.x() / 2.0 + end.x() / 2.0 - (dy / chord) * offset,
        start.y() / 2.0 + end.y() / 2.0 + (dx / chord) * offset,
        start.z(),
    )
    .map_err(|_| GeometryError::UnrepresentableBulge)?;
    let a = (start.y() - center.y()).atan2(start.x() - center.x());
    let b = (end.y() - center.y()).atan2(end.x() - center.x());
    let clockwise = bulge < 0.0;
    let arc = Arc3::new(
        center,
        radius,
        if clockwise { b } else { a },
        if clockwise { a } else { b },
    )
    .map_err(|_| GeometryError::UnrepresentableBulge)?;
    let sweep = arc.sweep();
    let expected = 4.0 * bulge.abs().atan();
    if sweep == 0.0
        || sweep >= TAU
        || (expected > PI && sweep <= PI)
        || (expected < PI && sweep > PI)
    {
        return Err(GeometryError::UnrepresentableBulge);
    }
    Ok(PolylineSegment::Arc { arc, clockwise })
}
