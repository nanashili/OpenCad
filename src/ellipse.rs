//! Validated planar ellipses and elliptic arcs.
//!
//! The major axis is stored as a world-space vector from the center.  The
//! minor axis is the counter-clockwise XY perpendicular, scaled by `ratio`.
//! Parameters are radians measured from the major axis; equal start and end
//! parameters represent a complete ellipse.

use crate::{GeometryError, Point3};
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipse3 {
    center: Point3,
    major_axis: Point3,
    ratio: f64,
    start: f64,
    end: f64,
}

impl Ellipse3 {
    pub fn new(
        center: Point3,
        major_axis: Point3,
        ratio: f64,
        start: f64,
        end: f64,
    ) -> Result<Self, GeometryError> {
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err(GeometryError::DegenerateCurve);
        }
        if !start.is_finite() || !end.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        let length = major_axis.x().hypot(major_axis.y());
        if !length.is_finite() || length == 0.0 {
            return Err(GeometryError::DegenerateCurve);
        }
        let sweep = end - start;
        if !sweep.is_finite() || (sweep != 0.0 && sweep.abs() > TAU) {
            return Err(GeometryError::InvalidArcSweep);
        }
        Ok(Self {
            center,
            major_axis,
            ratio,
            start,
            end,
        })
    }

    pub fn full(center: Point3, major_axis: Point3, ratio: f64) -> Result<Self, GeometryError> {
        Self::new(center, major_axis, ratio, 0.0, 0.0)
    }

    pub fn center(self) -> Point3 {
        self.center
    }
    pub fn major_axis(self) -> Point3 {
        self.major_axis
    }
    pub fn ratio(self) -> f64 {
        self.ratio
    }
    pub fn start_parameter(self) -> f64 {
        self.start
    }
    pub fn end_parameter(self) -> f64 {
        self.end
    }
    pub fn is_full(self) -> bool {
        self.start == self.end
    }
    pub fn sweep(self) -> f64 {
        if self.is_full() {
            TAU
        } else {
            self.end - self.start
        }
    }
    pub fn major_radius(self) -> f64 {
        self.major_axis.x().hypot(self.major_axis.y())
    }
    pub fn minor_radius(self) -> f64 {
        self.major_radius() * self.ratio
    }

    pub fn set_parameters(&mut self, start: f64, end: f64) -> Result<(), GeometryError> {
        *self = Self::new(self.center, self.major_axis, self.ratio, start, end)?;
        Ok(())
    }

    pub fn point_at_parameter(self, parameter: f64) -> Result<Point3, GeometryError> {
        if !parameter.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        let (s, c) = parameter.sin_cos();
        let length = self.major_radius();
        let ux = self.major_axis.x() / length;
        let uy = self.major_axis.y() / length;
        let minor = length * self.ratio;
        Point3::new(
            self.center.x() + self.major_axis.x() * c - uy * minor * s,
            self.center.y() + self.major_axis.y() * c + ux * minor * s,
            self.center.z(),
        )
        .map_err(|_| GeometryError::NonFiniteResult)
    }

    pub fn point_at_fraction(self, fraction: f64) -> Result<Point3, GeometryError> {
        if !fraction.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        self.point_at_parameter(self.start + self.sweep() * fraction)
    }

    pub fn length(self) -> Result<f64, GeometryError> {
        let a = self.major_radius();
        let b = self.minor_radius();
        // Ramanujan's second approximation is stable and useful for display and DXF tools.
        let h = ((a - b) / (a + b)).powi(2);
        let full =
            std::f64::consts::PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()));
        let result = if self.is_full() {
            full
        } else {
            full * self.sweep().abs() / TAU
        };
        if result.is_finite() {
            Ok(result)
        } else {
            Err(GeometryError::NonFiniteResult)
        }
    }

    pub fn translated(self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        let center = Point3::new(
            self.center.x() + dx,
            self.center.y() + dy,
            self.center.z() + dz,
        )?;
        Self::new(center, self.major_axis, self.ratio, self.start, self.end)
    }

    pub fn tessellate_for_svg(self) -> Vec<Point3> {
        (0..=64)
            .filter_map(|i| self.point_at_fraction(i as f64 / 64.0).ok())
            .collect()
    }
}
