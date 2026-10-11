use crate::{GeometryError, Point3};
use std::f64::consts::{FRAC_PI_2, TAU};

/// A counterclockwise circular arc in the world XY plane.
/// Angles are radians, measured from +X, and stored in [0, 2π).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc3 {
    center: Point3,
    radius: f64,
    start: f64,
    end: f64,
}

impl Arc3 {
    pub fn new(center: Point3, radius: f64, start: f64, end: f64) -> Result<Self, GeometryError> {
        let mut arc = Self {
            center,
            radius: 0.0,
            start: 0.0,
            end: 0.0,
        };
        arc.set_radius(radius)?;
        arc.set_angles(start, end)?;
        Ok(arc)
    }

    pub fn center(self) -> Point3 {
        self.center
    }

    pub fn radius(self) -> f64 {
        self.radius
    }

    pub fn start_angle(self) -> f64 {
        self.start
    }

    pub fn end_angle(self) -> f64 {
        self.end
    }

    pub fn sweep(self) -> f64 {
        (self.end - self.start).rem_euclid(TAU)
    }

    pub fn set_center(&mut self, center: Point3) {
        self.center = center;
    }

    pub fn set_radius(&mut self, radius: f64) -> Result<(), GeometryError> {
        if !radius.is_finite() {
            return Err(GeometryError::NonFiniteRadius);
        }
        if radius < 0.0 {
            return Err(GeometryError::NegativeRadius);
        }
        self.radius = radius;
        Ok(())
    }

    pub fn set_angles(&mut self, start: f64, end: f64) -> Result<(), GeometryError> {
        if !start.is_finite() || !end.is_finite() {
            return Err(GeometryError::NonFiniteAngle);
        }
        let start = normalized_angle(start);
        let end = normalized_angle(end);
        self.start = start;
        self.end = end;
        Ok(())
    }

    pub fn length(self) -> Result<f64, GeometryError> {
        let length = self.radius * self.sweep();
        if length.is_finite() {
            Ok(length)
        } else {
            Err(GeometryError::NonFiniteResult)
        }
    }

    pub fn point_at_fraction(self, fraction: f64) -> Result<Point3, GeometryError> {
        if !fraction.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        self.point_at_angle(self.start + self.sweep() * fraction)
    }

    pub fn start_point(self) -> Result<Point3, GeometryError> {
        self.point_at_angle(self.start)
    }

    pub fn end_point(self) -> Result<Point3, GeometryError> {
        self.point_at_angle(self.end)
    }

    fn point_at_angle(self, angle: f64) -> Result<Point3, GeometryError> {
        if !angle.is_finite() {
            return Err(GeometryError::NonFiniteResult);
        }
        let (sin, cos) = angle.sin_cos();
        Point3::new(
            self.center.x() + self.radius * cos,
            self.center.y() + self.radius * sin,
            self.center.z(),
        )
        .map_err(|_| GeometryError::NonFiniteResult)
    }

    pub(crate) fn contains_angle(self, angle: f64) -> bool {
        angle.is_finite() && (angle - self.start).rem_euclid(TAU) <= self.sweep()
    }

    pub(crate) fn extrema(self) -> Result<Vec<Point3>, GeometryError> {
        let mut points = vec![self.start_point()?, self.end_point()?];
        for quadrant in 0..4 {
            let angle = f64::from(quadrant) * FRAC_PI_2;
            if self.contains_angle(angle) {
                points.push(self.point_at_angle(angle)?);
            }
        }
        Ok(points)
    }
}

fn normalized_angle(angle: f64) -> f64 {
    let value = angle.rem_euclid(TAU);
    if value == TAU { 0.0 } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn counterclockwise_arc_wraps_through_positive_x_at_its_xyz_elevation() {
        let center = Point3::new(1.0, 2.0, 3.0).unwrap();
        let arc = Arc3::new(center, 2.0, -FRAC_PI_2, 5.0 * FRAC_PI_2).unwrap();
        assert_eq!(arc.start_angle(), 3.0 * FRAC_PI_2);
        assert_eq!(arc.end_angle(), FRAC_PI_2);
        assert!((arc.sweep() - PI).abs() < 1e-12);
        let midpoint = arc.point_at_fraction(0.5).unwrap();
        assert!((midpoint.x() - 3.0).abs() < 1e-12);
        assert!((midpoint.y() - 2.0).abs() < 1e-12);
        assert_eq!(midpoint.z(), 3.0);
        assert!((arc.length().unwrap() - 2.0 * PI).abs() < 1e-12);
        assert!(arc.contains_angle(0.0));
        assert!(!arc.contains_angle(PI));
    }

    #[test]
    fn rejected_arc_mutations_leave_geometry_intact_and_overflow_is_reported() {
        let mut arc = Arc3::new(Point3::new(1.0, 2.0, 3.0).unwrap(), 4.0, 0.0, PI).unwrap();
        let original = arc;
        for radius in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(arc.set_radius(radius).is_err());
            assert_eq!(arc, original);
        }
        for (start, end) in [(f64::NAN, 1.0), (0.0, f64::INFINITY)] {
            assert!(arc.set_angles(start, end).is_err());
            assert_eq!(arc, original);
        }
        arc.set_angles(0.0, TAU).unwrap();
        assert_eq!(arc.length().unwrap(), 0.0);
        assert_eq!(arc.start_point(), arc.end_point());
        arc.set_angles(0.0, PI).unwrap();
        arc.set_radius(0.0).unwrap();
        assert_eq!(arc.start_point().unwrap(), arc.center());
        arc.set_radius(f64::MAX).unwrap();
        assert_eq!(arc.length(), Err(GeometryError::NonFiniteResult));
        arc.set_center(Point3::new(f64::MAX, 0.0, 0.0).unwrap());
        assert_eq!(arc.start_point(), Err(GeometryError::NonFiniteResult));
    }
}
