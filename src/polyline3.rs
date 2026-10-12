//! Ordered, finite 3D vertex paths for spatial polyline workflows.

use crate::{GeometryError, Line3, Point3};

const MAX_VERTICES: usize = 65_536;

/// A linear 3D path.  Unlike `Polyline2`, this type deliberately permits
/// non-planar vertices and has no bulge or width state.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline3 {
    vertices: Vec<Point3>,
    closed: bool,
}

impl Polyline3 {
    pub fn new(vertices: Vec<Point3>, closed: bool) -> Result<Self, GeometryError> {
        let minimum = if closed { 3 } else { 2 };
        if vertices.len() < minimum {
            return Err(GeometryError::NotEnoughVertices);
        }
        if vertices.len() > MAX_VERTICES {
            return Err(GeometryError::InvalidVertexIndex);
        }
        if vertices
            .iter()
            .any(|point| !point.x().is_finite() || !point.y().is_finite() || !point.z().is_finite())
        {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        let segments = if closed {
            vertices.len()
        } else {
            vertices.len() - 1
        };
        for index in 0..segments {
            if vertices[index] == vertices[(index + 1) % vertices.len()] {
                return Err(GeometryError::DegenerateLine);
            }
        }
        Ok(Self { vertices, closed })
    }

    pub fn vertices(&self) -> &[Point3] {
        &self.vertices
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn set_closed(&mut self, closed: bool) -> Result<(), GeometryError> {
        let minimum = if closed { 3 } else { 2 };
        if self.vertices.len() < minimum {
            return Err(GeometryError::NotEnoughVertices);
        }
        let segments = if closed {
            self.vertices.len()
        } else {
            self.vertices.len() - 1
        };
        for index in 0..segments {
            if self.vertices[index] == self.vertices[(index + 1) % self.vertices.len()] {
                return Err(GeometryError::DegenerateLine);
            }
        }
        self.closed = closed;
        Ok(())
    }

    /// Returns one straight segment for each path edge.
    pub fn segments(&self) -> impl Iterator<Item = Line3> + '_ {
        let count = self.vertices.len() - usize::from(!self.closed);
        (0..count).map(|index| {
            Line3::new(
                self.vertices[index],
                self.vertices[(index + 1) % self.vertices.len()],
            )
        })
    }

    pub fn explode(&self) -> Vec<Line3> {
        self.segments().collect()
    }

    pub fn length(&self) -> Result<f64, GeometryError> {
        self.segments().try_fold(0.0, |total, segment| {
            let length = total + segment.length()?;
            if length.is_finite() {
                Ok(length)
            } else {
                Err(GeometryError::NonFiniteResult)
            }
        })
    }

    pub fn translated(&self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        if !dx.is_finite() || !dy.is_finite() || !dz.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        let vertices = self
            .vertices
            .iter()
            .map(|point| Point3::new(point.x() + dx, point.y() + dy, point.z() + dz))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(vertices, self.closed)
    }
}
