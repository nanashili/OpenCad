use crate::trim::Curve;
use crate::{Arc3, Entity, GeometryError, Line3, Point3};
use std::f64::consts::TAU;

/// Original XY endpoint operations. Angular values are radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lengthen {
    Delta(f64),
    Percent(f64),
    Total(f64),
    DeltaAngle(f64),
    TotalAngle(f64),
    Dynamic(Point3),
}

impl Lengthen {
    pub(crate) fn validate(self) -> Result<(), GeometryError> {
        let valid = match self {
            Self::Delta(value) | Self::DeltaAngle(value) => value.is_finite(),
            Self::Percent(value) | Self::Total(value) => value.is_finite() && value > 0.0,
            Self::TotalAngle(value) => value.is_finite() && value > 0.0 && value < TAU,
            Self::Dynamic(_) => true,
        };
        if valid {
            Ok(())
        } else {
            Err(GeometryError::InvalidLengthenValue)
        }
    }

    fn length(self, current: f64) -> Result<f64, GeometryError> {
        let value = match self {
            Self::Delta(value) => current + value,
            Self::Percent(value) => current * (value / 100.0),
            Self::Total(value) => value,
            _ => return Err(GeometryError::UnsupportedLengthenAngle),
        };
        positive(value)
    }

    fn unchanged(self) -> bool {
        matches!(
            self,
            Self::Delta(0.0) | Self::DeltaAngle(0.0) | Self::Percent(100.0)
        )
    }
}

impl Entity {
    /// Change the endpoint nearest the XY pick, retaining the other endpoint,
    /// source direction, elevation and (for arcs) radius. Closed circles are
    /// unchanged. Dynamic points project along a line or around an arc's circle.
    /// Nonpositive lengths, zero/full-circle sweeps and endpoint ties reject.
    pub fn lengthen_xy(&self, pick: Point3, operation: Lengthen) -> Result<Self, GeometryError> {
        operation.validate()?;
        if matches!(self, Self::Circle(_)) {
            return Ok(self.clone());
        }
        let curve = Curve::from_entity(self).ok_or(GeometryError::UnsupportedLengthenEntity)?;
        curve.validate()?;
        let start = curve.nearer_start(pick)?;
        let result = match self {
            Self::Line(line) => {
                if matches!(operation, Lengthen::DeltaAngle(_) | Lengthen::TotalAngle(_)) {
                    return Err(GeometryError::UnsupportedLengthenAngle);
                }
                let current = line.length()?;
                let ux = (line.end().x() - line.start().x()) / current;
                let uy = (line.end().y() - line.start().y()) / current;
                let fixed = if start { line.end() } else { line.start() };
                let direction = if start { -1.0 } else { 1.0 };
                let length = if let Lengthen::Dynamic(point) = operation {
                    positive(
                        direction * ux.mul_add(point.x() - fixed.x(), uy * (point.y() - fixed.y())),
                    )?
                } else {
                    operation.length(current)?
                };
                if length == current {
                    return if operation.unchanged()
                        || matches!(operation, Lengthen::Total(_) | Lengthen::Dynamic(_))
                    {
                        Ok(self.clone())
                    } else {
                        Err(GeometryError::UnrepresentableLengthen)
                    };
                }
                let moved = Point3::new(
                    (direction * ux).mul_add(length, fixed.x()),
                    (direction * uy).mul_add(length, fixed.y()),
                    fixed.z(),
                )
                .map_err(|_| GeometryError::NonFiniteResult)?;
                Self::Line(if start {
                    Line3::new(moved, fixed)
                } else {
                    Line3::new(fixed, moved)
                })
            }
            Self::Arc(arc) => {
                let current = arc.sweep();
                let sweep = match operation {
                    Lengthen::DeltaAngle(value) => current + value,
                    Lengthen::TotalAngle(value) => value,
                    Lengthen::Percent(value) => current * (value / 100.0),
                    Lengthen::Dynamic(point) => {
                        let dx = finite(point.x() - arc.center().x())?;
                        let dy = finite(point.y() - arc.center().y())?;
                        if dx == 0.0 && dy == 0.0 {
                            return Err(GeometryError::AmbiguousCurvePick);
                        }
                        let angle = dy.atan2(dx);
                        if start {
                            (arc.end_angle() - angle).rem_euclid(TAU)
                        } else {
                            (angle - arc.start_angle()).rem_euclid(TAU)
                        }
                    }
                    _ => {
                        let length = arc.length()?;
                        let target = operation.length(length)?;
                        if target == length {
                            return if operation.unchanged()
                                || matches!(operation, Lengthen::Total(_))
                            {
                                Ok(self.clone())
                            } else {
                                Err(GeometryError::UnrepresentableLengthen)
                            };
                        }
                        target / arc.radius()
                    }
                };
                if !sweep.is_finite() {
                    return Err(GeometryError::NonFiniteResult);
                }
                if sweep <= 0.0 || sweep >= TAU {
                    return Err(GeometryError::InvalidLengthenValue);
                }
                if sweep == current {
                    return if operation.unchanged()
                        || matches!(operation, Lengthen::TotalAngle(_) | Lengthen::Dynamic(_))
                    {
                        Ok(self.clone())
                    } else {
                        Err(GeometryError::UnrepresentableLengthen)
                    };
                }
                Self::Arc(if start {
                    Arc3::new(
                        arc.center(),
                        arc.radius(),
                        arc.end_angle() - sweep,
                        arc.end_angle(),
                    )?
                } else {
                    Arc3::new(
                        arc.center(),
                        arc.radius(),
                        arc.start_angle(),
                        arc.start_angle() + sweep,
                    )?
                })
            }
            _ => return Err(GeometryError::UnsupportedLengthenEntity),
        };
        let result_curve =
            Curve::from_entity(&result).expect("lengthened lines/arcs keep their type");
        result_curve
            .validate()
            .map_err(|_| GeometryError::UnrepresentableLengthen)?;
        if result_curve.point(0.0)? == result_curve.point(1.0)?
            || result_curve.point(if start { 0.0 } else { 1.0 })?
                == curve.point(if start { 0.0 } else { 1.0 })?
        {
            return Err(GeometryError::UnrepresentableLengthen);
        }
        Ok(result)
    }
}

fn finite(value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GeometryError::NonFiniteResult)
    }
}
fn positive(value: f64) -> Result<f64, GeometryError> {
    let value = finite(value)?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err(GeometryError::InvalidLengthenValue)
    }
}
