//! Ordered 3D line endpoints from the sanitized native behavior specification.
//! Entity parameter bounds are distance-based; normalized interpolation uses fractions.
//! Evaluation, reversal, and bounds remain original mathematical contracts.

use crate::{GeometryError, Point2};
use std::ops::RangeInclusive;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point3 {
    x: f64,
    y: f64,
    z: f64,
}

impl Point3 {
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, GeometryError> {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        Ok(Self { x, y, z })
    }

    pub fn x(self) -> f64 {
        self.x
    }

    pub fn y(self) -> f64 {
        self.y
    }

    pub fn z(self) -> f64 {
        self.z
    }
}

impl From<Point2> for Point3 {
    fn from(point: Point2) -> Self {
        Self {
            x: point.x(),
            y: point.y(),
            z: 0.0,
        }
    }
}

/// A finite, ordered pair of world-space endpoints. Coincident endpoints are valid.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Line3 {
    start: Point3,
    end: Point3,
}

impl Line3 {
    pub fn new(start: Point3, end: Point3) -> Self {
        Self { start, end }
    }

    pub fn start(self) -> Point3 {
        self.start
    }

    pub fn end(self) -> Point3 {
        self.end
    }

    pub fn set_start(&mut self, point: Point3) {
        self.start = point;
    }

    pub fn set_end(&mut self, point: Point3) {
        self.end = point;
    }

    pub fn length(self) -> Result<f64, GeometryError> {
        finite_result(
            (self.end.x - self.start.x)
                .hypot(self.end.y - self.start.y)
                .hypot(self.end.z - self.start.z),
        )
    }

    /// Entity parameter properties observed in AutoCAD: zero through line length.
    pub fn parameter_range(self) -> Result<RangeInclusive<f64>, GeometryError> {
        Ok(0.0..=self.length()?)
    }

    /// Fractions 0 and 1 select the endpoints; other finite values extrapolate.
    /// A fraction is separate from the entity's distance-based parameter range.
    pub fn point_at_fraction(self, fraction: f64) -> Result<Point3, GeometryError> {
        if !fraction.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        if fraction == 0.0 || self.start == self.end {
            return Ok(self.start);
        }
        if fraction == 1.0 {
            return Ok(self.end);
        }
        let interpolate = |start: f64, end: f64| {
            if (0.0..=1.0).contains(&fraction) {
                // Weighted interpolation avoids overflowing end - start.
                (1.0 - fraction) * start + fraction * end
            } else {
                fraction.mul_add(end - start, start)
            }
        };
        Point3::new(
            interpolate(self.start.x, self.end.x),
            interpolate(self.start.y, self.end.y),
            interpolate(self.start.z, self.end.z),
        )
        .map_err(|_| GeometryError::NonFiniteResult)
    }

    pub fn distance_at_fraction(self, fraction: f64) -> Result<f64, GeometryError> {
        if !fraction.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        finite_result(fraction * self.length()?)
    }

    pub fn fraction_at_distance(self, distance: f64) -> Result<f64, GeometryError> {
        if !distance.is_finite() {
            return Err(GeometryError::NonFiniteDistance);
        }
        let length = self.length()?;
        if length == 0.0 {
            return Err(GeometryError::DegenerateLine);
        }
        finite_result(distance / length)
    }

    pub fn point_at_distance(self, distance: f64) -> Result<Point3, GeometryError> {
        self.point_at_fraction(self.fraction_at_distance(distance)?)
    }

    pub fn reverse(&mut self) {
        std::mem::swap(&mut self.start, &mut self.end);
    }

    pub fn bounds(self) -> Bounds3 {
        Bounds3 {
            min: Point3 {
                x: self.start.x.min(self.end.x),
                y: self.start.y.min(self.end.y),
                z: self.start.z.min(self.end.z),
            },
            max: Point3 {
                x: self.start.x.max(self.end.x),
                y: self.start.y.max(self.end.y),
                z: self.start.z.max(self.end.z),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds3 {
    min: Point3,
    max: Point3,
}

impl Bounds3 {
    pub fn min(self) -> Point3 {
        self.min
    }

    pub fn max(self) -> Point3 {
        self.max
    }
}

fn finite_result(value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GeometryError::NonFiniteResult)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_and_updates_preserve_endpoint_order_and_coincidence() {
        let origin = Point3::default();
        assert_eq!(Line3::default(), Line3::new(origin, origin));
        let a = Point3::new(1.0, 2.0, 3.0).unwrap();
        let b = Point3::new(-5.0, 8.0, -13.0).unwrap();
        let mut line = Line3::new(a, b);
        assert_eq!((line.start(), line.end()), (a, b));
        line.set_start(b);
        assert_eq!((line.start(), line.end()), (b, b));
        line.set_end(a);
        assert_eq!((line.start(), line.end()), (b, a));
        assert_eq!(
            Point3::new(0.0, 0.0, f64::NAN),
            Err(GeometryError::NonFiniteCoordinate)
        );
    }

    #[test]
    fn spatial_evaluation_reversal_and_bounds_match_independent_fixtures() {
        let a = Point3::new(1.0, 6.0, 3.0).unwrap();
        let b = Point3::new(4.0, 2.0, 15.0).unwrap();
        let mut line = Line3::new(a, b);
        assert_eq!(line.length(), Ok(13.0));
        assert_eq!(line.distance_at_fraction(0.5), Ok(6.5));
        assert_eq!(line.fraction_at_distance(6.5), Ok(0.5));
        assert_eq!(
            line.point_at_distance(6.5),
            Ok(Point3::new(2.5, 4.0, 9.0).unwrap())
        );
        let quarter = Point3::new(1.75, 5.0, 6.0).unwrap();
        assert_eq!(line.point_at_fraction(0.25), Ok(quarter));
        line.reverse();
        assert_eq!((line.start(), line.end()), (b, a));
        assert_eq!(line.point_at_fraction(0.75), Ok(quarter));
        assert_eq!(line.length(), Ok(13.0));
        assert_eq!(line.bounds().min(), Point3::new(1.0, 2.0, 3.0).unwrap());
        assert_eq!(line.bounds().max(), Point3::new(4.0, 6.0, 15.0).unwrap());
    }

    #[test]
    fn entity_parameter_range_matches_source_app_properties() {
        // AutoCAD 2027 getpropertyvalue fixtures: runtime-20261010c/properties.txt.
        let line = Line3::new(
            Point3::new(1.0, 2.0, 3.0).unwrap(),
            Point3::new(4.0, 6.0, 15.0).unwrap(),
        );
        assert_eq!(line.parameter_range(), Ok(0.0..=13.0));
        let point = Point3::new(-7.0, 8.0, 0.5).unwrap();
        assert_eq!(Line3::new(point, point).parameter_range(), Ok(0.0..=0.0));
    }

    #[test]
    fn degenerate_and_large_lines_report_numeric_boundaries() {
        let point = Point3::new(7.0, -1.0, 2.0).unwrap();
        let line = Line3::new(point, point);
        assert_eq!(line.length(), Ok(0.0));
        assert_eq!(line.point_at_fraction(-3.0), Ok(point));
        assert_eq!(
            line.fraction_at_distance(0.0),
            Err(GeometryError::DegenerateLine)
        );
        assert_eq!(
            line.point_at_distance(1.0),
            Err(GeometryError::DegenerateLine)
        );
        assert_eq!(
            line.point_at_fraction(f64::NAN),
            Err(GeometryError::NonFiniteParameter)
        );
        assert_eq!(
            line.fraction_at_distance(f64::INFINITY),
            Err(GeometryError::NonFiniteDistance)
        );

        let large = Line3::new(
            Point3::new(-f64::MAX, 0.0, 0.0).unwrap(),
            Point3::new(f64::MAX, 0.0, 0.0).unwrap(),
        );
        assert_eq!(large.point_at_fraction(0.5), Ok(Point3::default()));
        assert_eq!(large.length(), Err(GeometryError::NonFiniteResult));
        assert_eq!(large.parameter_range(), Err(GeometryError::NonFiniteResult));
        assert_eq!(
            large.point_at_fraction(2.0),
            Err(GeometryError::NonFiniteResult)
        );
    }
}
