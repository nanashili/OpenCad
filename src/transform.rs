use crate::{Arc3, Circle3, Entity, GeometryError, Line3, Point3, Polyline2};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn sin_cos(angle: f64) -> (f64, f64) {
    // Exact axis-aligned turns keep zero coordinates and unchanged DXF fields
    // exact. Other angles retain the standard library's trigonometric result.
    match angle.rem_euclid(TAU) {
        0.0 => (0.0, 1.0),
        FRAC_PI_2 => (1.0, 0.0),
        PI => (0.0, -1.0),
        angle if angle == 3.0 * FRAC_PI_2 => (-1.0, 0.0),
        _ => angle.sin_cos(),
    }
}

/// A type-preserving transform for the editor's world XY geometry.
/// Rotations and mirrors leave Z unchanged; uniform scaling also scales Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    base: Point3,
    kind: TransformKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TransformKind {
    Rotate(f64),
    Scale(f64),
    Mirror(f64),
}

impl Transform {
    /// Rotate about the world Z axis through `base`, counterclockwise in radians.
    pub fn rotate_xy(base: Point3, angle: f64) -> Result<Self, GeometryError> {
        if !angle.is_finite() {
            return Err(GeometryError::NonFiniteAngle);
        }
        Ok(Self {
            base,
            kind: TransformKind::Rotate(angle.rem_euclid(TAU)),
        })
    }

    /// Scale XYZ uniformly about `base`. OpenCad requires a positive factor.
    pub fn scale(base: Point3, factor: f64) -> Result<Self, GeometryError> {
        if !factor.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        if factor <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        Ok(Self {
            base,
            kind: TransformKind::Scale(factor),
        })
    }

    /// Reflect across the vertical plane containing the two points' XY line.
    /// Points with identical XY coordinates do not define that plane.
    pub fn mirror_xy(first: Point3, second: Point3) -> Result<Self, GeometryError> {
        let dx = second.x() - first.x();
        let dy = second.y() - first.y();
        if !dx.is_finite() || !dy.is_finite() {
            return Err(GeometryError::NonFiniteResult);
        }
        if dx == 0.0 && dy == 0.0 {
            return Err(GeometryError::DegenerateLine);
        }
        Ok(Self {
            base: first,
            kind: TransformKind::Mirror(dy.atan2(dx)),
        })
    }

    fn is_identity(self) -> bool {
        matches!(
            self.kind,
            TransformKind::Rotate(0.0) | TransformKind::Scale(1.0)
        )
    }

    fn point(self, point: Point3) -> Result<Point3, GeometryError> {
        if self.is_identity() || point == self.base {
            return Ok(point);
        }
        let dx = point.x() - self.base.x();
        let dy = point.y() - self.base.y();
        if !dx.is_finite() || !dy.is_finite() {
            return Err(GeometryError::NonFiniteResult);
        }
        let (x, y, z) = match self.kind {
            TransformKind::Rotate(angle) => {
                let (sin, cos) = sin_cos(angle);
                (
                    cos.mul_add(dx, -sin * dy),
                    sin.mul_add(dx, cos * dy),
                    point.z(),
                )
            }
            TransformKind::Scale(factor) => (
                dx * factor,
                dy * factor,
                (point.z() - self.base.z()).mul_add(factor, self.base.z()),
            ),
            TransformKind::Mirror(angle) => {
                let (sin, cos) = sin_cos(2.0 * angle);
                (
                    cos.mul_add(dx, sin * dy),
                    sin.mul_add(dx, -cos * dy),
                    point.z(),
                )
            }
        };
        Point3::new(self.base.x() + x, self.base.y() + y, z)
            .map_err(|_| GeometryError::NonFiniteResult)
    }

    fn radius(self, radius: f64) -> Result<f64, GeometryError> {
        let radius = match self.kind {
            TransformKind::Scale(factor) => radius * factor,
            _ => radius,
        };
        if radius.is_finite() {
            Ok(radius)
        } else {
            Err(GeometryError::NonFiniteResult)
        }
    }

    fn arc_angles(self, arc: Arc3) -> (f64, f64) {
        match self.kind {
            TransformKind::Rotate(angle) => (arc.start_angle() + angle, arc.end_angle() + angle),
            TransformKind::Mirror(angle) => (
                2.0 * angle - arc.end_angle(),
                2.0 * angle - arc.start_angle(),
            ),
            TransformKind::Scale(_) => (arc.start_angle(), arc.end_angle()),
        }
    }
}

impl Entity {
    /// Apply a validated transform without mutating the source entity.
    pub fn transformed(&self, transform: Transform) -> Result<Self, GeometryError> {
        if transform.is_identity() {
            return Ok(self.clone());
        }
        Ok(match self {
            Self::Line(line) => Self::Line(Line3::new(
                transform.point(line.start())?,
                transform.point(line.end())?,
            )),
            Self::Circle(circle) => Self::Circle(Circle3::new(
                transform.point(circle.center())?,
                transform.radius(circle.radius())?,
            )?),
            Self::Arc(arc) => {
                let (start, end) = transform.arc_angles(*arc);
                Self::Arc(Arc3::new(
                    transform.point(arc.center())?,
                    transform.radius(arc.radius())?,
                    start,
                    end,
                )?)
            }
            Self::Polyline(polyline) => Self::Polyline(Polyline2::with_bulges(
                polyline
                    .vertices()
                    .iter()
                    .copied()
                    .map(|point| transform.point(point))
                    .collect::<Result<_, _>>()?,
                polyline
                    .bulges()
                    .iter()
                    .map(|bulge| {
                        if matches!(transform.kind, TransformKind::Mirror(_)) {
                            -*bulge
                        } else {
                            *bulge
                        }
                    })
                    .collect(),
                polyline.is_closed(),
            )?),
        })
    }
}
