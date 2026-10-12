//! Infinite construction geometry used by XLINE and RAY tools.

use crate::{GeometryError, Point3, Transform};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XLine3 {
    origin: Point3,
    direction: Point3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray3 {
    origin: Point3,
    direction: Point3,
}

impl XLine3 {
    pub fn new(origin: Point3, direction: Point3) -> Result<Self, GeometryError> {
        Ok(Self {
            origin,
            direction: normalize(direction)?,
        })
    }
    pub fn from_points(origin: Point3, through: Point3) -> Result<Self, GeometryError> {
        Self::new(origin, difference(through, origin)?)
    }
    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn direction(self) -> Point3 {
        self.direction
    }
    pub fn point_at(self, parameter: f64) -> Result<Point3, GeometryError> {
        if !parameter.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        add(self.origin, scale(self.direction, parameter)?)
    }
    pub fn parameter_of(self, point: Point3) -> Result<f64, GeometryError> {
        finite_result(dot(difference(point, self.origin)?, self.direction))
    }
    pub fn closest_parameter(self, point: Point3) -> Result<f64, GeometryError> {
        self.parameter_of(point)
    }
    pub fn closest_point(self, point: Point3) -> Result<Point3, GeometryError> {
        self.point_at(self.closest_parameter(point)?)
    }
    pub fn transformed(self, transform: Transform) -> Result<Self, GeometryError> {
        let origin = transform.point(self.origin)?;
        let endpoint = transform.point(add(self.origin, self.direction)?)?;
        Self::from_points(origin, endpoint)
    }
    /// Returns the two signed parameters where this line enters and exits an axis-aligned box.
    pub fn clip_to_bounds(
        self,
        min: Point3,
        max: Point3,
    ) -> Result<Option<(f64, f64)>, GeometryError> {
        clip(self.origin, self.direction, min, max, false)
    }
    pub fn translated(self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        Self::new(add(self.origin, Point3::new(dx, dy, dz)?)?, self.direction)
    }
}

impl Ray3 {
    pub fn new(origin: Point3, direction: Point3) -> Result<Self, GeometryError> {
        Ok(Self {
            origin,
            direction: normalize(direction)?,
        })
    }
    pub fn from_points(origin: Point3, through: Point3) -> Result<Self, GeometryError> {
        Self::new(origin, difference(through, origin)?)
    }
    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn direction(self) -> Point3 {
        self.direction
    }
    pub fn point_at(self, parameter: f64) -> Result<Point3, GeometryError> {
        if !parameter.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        if parameter < 0.0 {
            return Err(GeometryError::NonFiniteParameter);
        }
        add(self.origin, scale(self.direction, parameter)?)
    }
    pub fn parameter_of(self, point: Point3) -> Result<f64, GeometryError> {
        Ok(finite_result(dot(difference(point, self.origin)?, self.direction))?.max(0.0))
    }
    pub fn closest_parameter(self, point: Point3) -> Result<f64, GeometryError> {
        self.parameter_of(point)
    }
    pub fn closest_point(self, point: Point3) -> Result<Point3, GeometryError> {
        self.point_at(self.closest_parameter(point)?)
    }
    pub fn transformed(self, transform: Transform) -> Result<Self, GeometryError> {
        let origin = transform.point(self.origin)?;
        let endpoint = transform.point(add(self.origin, self.direction)?)?;
        Self::from_points(origin, endpoint)
    }
    pub fn clip_to_bounds(
        self,
        min: Point3,
        max: Point3,
    ) -> Result<Option<(f64, f64)>, GeometryError> {
        clip(self.origin, self.direction, min, max, true)
    }
    pub fn translated(self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        Self::new(add(self.origin, Point3::new(dx, dy, dz)?)?, self.direction)
    }
}

fn clip(
    origin: Point3,
    direction: Point3,
    min: Point3,
    max: Point3,
    ray: bool,
) -> Result<Option<(f64, f64)>, GeometryError> {
    if min.x() > max.x() || min.y() > max.y() || min.z() > max.z() {
        return Err(GeometryError::DegenerateCurve);
    }
    let mut lo = if ray { 0.0 } else { f64::NEG_INFINITY };
    let mut hi = f64::INFINITY;
    for (o, d, a, b) in [
        (origin.x(), direction.x(), min.x(), max.x()),
        (origin.y(), direction.y(), min.y(), max.y()),
        (origin.z(), direction.z(), min.z(), max.z()),
    ] {
        if d.abs() <= f64::EPSILON {
            if o < a || o > b {
                return Ok(None);
            }
            continue;
        }
        let mut p = (a - o) / d;
        let mut q = (b - o) / d;
        if p > q {
            std::mem::swap(&mut p, &mut q);
        }
        lo = lo.max(p);
        hi = hi.min(q);
        if lo > hi {
            return Ok(None);
        }
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Err(GeometryError::NonFiniteResult);
    }
    Ok(Some((lo, hi)))
}
fn difference(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(a.x() - b.x(), a.y() - b.y(), a.z() - b.z())
        .map_err(|_| GeometryError::NonFiniteResult)
}
fn add(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(a.x() + b.x(), a.y() + b.y(), a.z() + b.z())
        .map_err(|_| GeometryError::NonFiniteResult)
}
fn scale(a: Point3, s: f64) -> Result<Point3, GeometryError> {
    Point3::new(a.x() * s, a.y() * s, a.z() * s).map_err(|_| GeometryError::NonFiniteResult)
}
fn dot(a: Point3, b: Point3) -> f64 {
    a.x() * b.x() + a.y() * b.y() + a.z() * b.z()
}
fn finite_result(value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GeometryError::NonFiniteResult)
    }
}
fn normalize(v: Point3) -> Result<Point3, GeometryError> {
    let n = v.x().hypot(v.y()).hypot(v.z());
    if !n.is_finite() {
        return Err(GeometryError::NonFiniteResult);
    }
    if n <= f64::EPSILON {
        return Err(GeometryError::DegenerateLine);
    }
    scale(v, 1.0 / n)
}
