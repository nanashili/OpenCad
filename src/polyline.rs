use crate::{Arc3, Entity, GeometryError, Line3, Point3};
use std::f64::consts::{PI, TAU};

/// An ordered line/arc polyline in a world-XY plane.
/// Coincident vertices remain representable, as they do for LINE geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline2 {
    vertices: Vec<Point3>,
    bulges: Vec<f64>,
    closed: bool,
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
        Ok(Self {
            vertices,
            bulges,
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
        *self = replacement;
        Ok(())
    }

    pub fn reverse(&mut self) {
        self.vertices.reverse();
        let last = self.bulges.len() - 1;
        self.bulges[..last].reverse();
        for bulge in &mut self.bulges {
            if *bulge != 0.0 {
                *bulge = -*bulge;
            }
        }
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

    /// Decompose the centerline into independent LINE/ARC entities in segment order.
    /// Closed polylines include the closing edge; coincident vertices yield a
    /// zero-length line. Standalone arcs store the same locus counterclockwise.
    pub fn explode(&self) -> Result<Vec<Entity>, GeometryError> {
        self.segments()
            .map(|segment| segment.map(PolylineSegment::geometry))
            .collect()
    }

    pub(crate) fn extrema(&self) -> Result<Vec<Point3>, GeometryError> {
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
