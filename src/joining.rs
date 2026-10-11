use crate::trim::Curve;
use crate::{Arc3, Circle3, Entity, GeometryError, Line3, Point3};
use std::f64::consts::TAU;

// Original geometric comparison policy, independent of native CAD tolerances.
const EPS: f64 = 1e-10;

#[derive(Clone, Copy)]
enum JoinCurve {
    Line {
        line: Line3,
        length: f64,
        ux: f64,
        uy: f64,
    },
    Arc(Arc3),
}

impl JoinCurve {
    fn new(entity: &Entity) -> Result<Self, GeometryError> {
        if !matches!(entity, Entity::Line(_) | Entity::Arc(_)) {
            return Err(GeometryError::UnsupportedJoinEntity);
        }
        let curve = Curve::from_entity(entity).ok_or(GeometryError::UnsupportedJoinEntity)?;
        curve.validate()?;
        if curve.point(0.0)? == curve.point(1.0)? {
            return Err(GeometryError::UnrepresentableJoin);
        }
        match entity {
            Entity::Line(line) => {
                let length = line.length()?;
                Ok(Self::Line {
                    line: *line,
                    length,
                    ux: (line.end().x() - line.start().x()) / length,
                    uy: (line.end().y() - line.start().y()) / length,
                })
            }
            Entity::Arc(arc) => {
                arc.length()?;
                Ok(Self::Arc(*arc))
            }
            _ => unreachable!("validated open curve type"),
        }
    }

    fn accepts(self, other: &Entity) -> Result<bool, GeometryError> {
        let compatible = match (self, other) {
            (
                Self::Line {
                    line,
                    length,
                    ux,
                    uy,
                },
                Entity::Line(candidate),
            ) => {
                if candidate.start().z() != line.start().z()
                    || candidate.end().z() != line.start().z()
                {
                    return Ok(false);
                }
                Self::new(other)?;
                let tolerance = EPS * length.max(candidate.length()?).max(1.0);
                for point in [candidate.start(), candidate.end()] {
                    let dx = finite(point.x() - line.start().x())?;
                    let dy = finite(point.y() - line.start().y())?;
                    let error = finite(ux.mul_add(dy, -uy * dx))?;
                    let rounding = 8.0 * f64::EPSILON * dx.abs().max(dy.abs());
                    if error.abs() > tolerance + rounding {
                        return Ok(false);
                    }
                }
                true
            }
            (Self::Arc(arc), Entity::Arc(candidate)) => {
                if candidate.center().z() != arc.center().z() {
                    return Ok(false);
                }
                let tolerance = EPS * arc.radius().max(candidate.radius()).max(1.0);
                let dx = finite(candidate.center().x() - arc.center().x())?;
                let dy = finite(candidate.center().y() - arc.center().y())?;
                if finite(dx.hypot(dy))? > tolerance
                    || (candidate.radius() - arc.radius()).abs() > tolerance
                {
                    return Ok(false);
                }
                Self::new(other)?;
                true
            }
            _ => false,
        };
        Ok(compatible)
    }
}

impl Entity {
    pub(crate) fn join_compatible_xy(&self, other: &Self) -> Result<bool, GeometryError> {
        JoinCurve::new(self)?.accepts(other)
    }

    /// Join compatible open curves to this source. Collinear lines span all
    /// endpoints in the source direction; same-circle arcs lengthen CCW from
    /// the source start, including gaps. Elevation must match exactly. This
    /// strict geometry API rejects incompatible inputs; the editor filters
    /// them. Full-turn arc results require an explicit `close_arc_xy` instead.
    pub fn join_xy(&self, others: &[Self]) -> Result<Self, GeometryError> {
        let source = JoinCurve::new(self)?;
        for other in others {
            if !source.accepts(other)? {
                return Err(GeometryError::IncompatibleJoinEntities);
            }
        }
        let result = match source {
            JoinCurve::Line {
                line,
                length,
                ux,
                uy,
            } => {
                let mut low: f64 = 0.0;
                let mut high = length;
                for other in others {
                    let Entity::Line(candidate) = other else {
                        unreachable!("validated line")
                    };
                    for point in [candidate.start(), candidate.end()] {
                        let dx = finite(point.x() - line.start().x())?;
                        let dy = finite(point.y() - line.start().y())?;
                        let distance = if point == line.start() {
                            0.0
                        } else if point == line.end() {
                            length
                        } else {
                            finite(ux.mul_add(dx, uy * dy))?
                        };
                        low = low.min(distance);
                        high = high.max(distance);
                    }
                }
                let point = |distance| {
                    Point3::new(
                        ux.mul_add(distance, line.start().x()),
                        uy.mul_add(distance, line.start().y()),
                        line.start().z(),
                    )
                    .map_err(|_| GeometryError::NonFiniteResult)
                };
                Self::Line(Line3::new(
                    if low == 0.0 {
                        line.start()
                    } else {
                        point(low)?
                    },
                    if high == length {
                        line.end()
                    } else {
                        point(high)?
                    },
                ))
            }
            JoinCurve::Arc(arc) => {
                let mut sweep = arc.sweep();
                for other in others {
                    let Entity::Arc(candidate) = other else {
                        unreachable!("validated arc")
                    };
                    let mut offset = (candidate.start_angle() - arc.start_angle()).rem_euclid(TAU);
                    if TAU - offset <= EPS {
                        offset = 0.0;
                    }
                    let end = offset + candidate.sweep();
                    if end >= TAU - EPS {
                        return Err(GeometryError::JoinRequiresFullTurn);
                    }
                    sweep = sweep.max(end);
                }
                if sweep == arc.sweep() {
                    self.clone()
                } else {
                    Self::Arc(Arc3::new(
                        arc.center(),
                        arc.radius(),
                        arc.start_angle(),
                        arc.start_angle() + sweep,
                    )?)
                }
            }
        };
        JoinCurve::new(&result)?;
        Ok(result)
    }

    /// Explicitly close a nondegenerate XY arc, retaining its center/radius/Z.
    pub fn close_arc_xy(&self) -> Result<Self, GeometryError> {
        if !matches!(self, Self::Arc(_)) {
            return Err(GeometryError::UnsupportedJoinClose);
        }
        let JoinCurve::Arc(arc) = JoinCurve::new(self)? else {
            unreachable!("validated arc type")
        };
        for coordinate in [arc.center().x(), arc.center().y()] {
            finite(coordinate - arc.radius())?;
            finite(coordinate + arc.radius())?;
        }
        Ok(Self::Circle(Circle3::new(arc.center(), arc.radius())?))
    }
}

fn finite(value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GeometryError::NonFiniteResult)
    }
}
