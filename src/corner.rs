//! Original analytic construction for corners between straight XY lines.
//! The angular cutoff and pick/precision policies are OpenCad choices.

use crate::{Arc3, Entity, GeometryError, Line3, Point3};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerParameter {
    FilletRadius,
    ChamferFirst,
    ChamferSecond,
}

impl CornerParameter {
    pub(crate) const ALL: [Self; 3] = [Self::FilletRadius, Self::ChamferFirst, Self::ChamferSecond];

    pub fn name(self) -> &'static str {
        match self {
            Self::FilletRadius => "FILLETRAD",
            Self::ChamferFirst => "CHAMFERA",
            Self::ChamferSecond => "CHAMFERB",
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Fillet(f64),
    Chamfer(f64, f64),
}

/// Validated dimensions for a fillet or a two-distance chamfer.
#[derive(Clone, Copy, Debug)]
pub struct Corner(Mode);

#[derive(Clone, Debug, PartialEq)]
pub struct LineCorner {
    first: Line3,
    second: Line3,
    connector: Option<Entity>,
}

impl LineCorner {
    pub fn first(&self) -> Line3 {
        self.first
    }

    pub fn second(&self) -> Line3 {
        self.second
    }

    pub fn connector(&self) -> Option<&Entity> {
        self.connector.as_ref()
    }
}

impl Corner {
    pub fn fillet(radius: f64) -> Result<Self, GeometryError> {
        Ok(Self(Mode::Fillet(dimension(radius)?)))
    }

    pub fn chamfer(first: f64, second: f64) -> Result<Self, GeometryError> {
        Ok(Self(Mode::Chamfer(dimension(first)?, dimension(second)?)))
    }

    /// Each pick chooses the ray from the supporting-line intersection to keep.
    /// The original endpoint order and common Z elevation are preserved.
    pub fn between_lines(
        self,
        first: Line3,
        first_pick: Point3,
        second: Line3,
        second_pick: Point3,
    ) -> Result<LineCorner, GeometryError> {
        if first.start().z() != first.end().z()
            || second.start().z() != second.end().z()
            || first.start().z() != second.start().z()
        {
            return Err(GeometryError::UnsupportedCornerEntities);
        }
        let a = direction(first)?;
        let b = direction(second)?;
        // A dimensionless cutoff, independent of line length. This is not a
        // claim about native geometric tolerance.
        if cross(a, b).abs() <= 1.0e-12 {
            return Err(GeometryError::ParallelCorner);
        }
        let intersection =
            crate::line::xy_intersection(first, second)?.ok_or(GeometryError::ParallelCorner)?;
        let (u, forward_a) = picked_ray(a, intersection, first_pick)?;
        let (v, forward_b) = picked_ray(b, intersection, second_pick)?;
        let turn = cross(u, v);
        let angle = turn.abs().atan2(dot(u, v));
        let (distance_a, distance_b) = match self.0 {
            Mode::Fillet(radius) => {
                let (sin, cos) = (angle * 0.5).sin_cos();
                let distance = finite(radius * (cos / sin))?;
                (distance, distance)
            }
            Mode::Chamfer(a, b) => (a, b),
        };
        let tangent_a = along(intersection, u, distance_a)?;
        let tangent_b = along(intersection, v, distance_b)?;
        let first = retained(first, intersection, u, forward_a, distance_a, tangent_a)?;
        let second = retained(second, intersection, v, forward_b, distance_b, tangent_b)?;
        let connector = match self.0 {
            Mode::Fillet(0.0) | Mode::Chamfer(0.0, 0.0) => None,
            Mode::Chamfer(_, _) => {
                if tangent_a == tangent_b {
                    return Err(GeometryError::UnrepresentableCorner);
                }
                Some(Entity::Line(Line3::new(tangent_a, tangent_b)))
            }
            Mode::Fillet(radius) => {
                if tangent_a == intersection || tangent_b == intersection {
                    return Err(GeometryError::UnrepresentableCorner);
                }
                let normal = if turn > 0.0 {
                    [-u[1], u[0]]
                } else {
                    [u[1], -u[0]]
                };
                let center = along(tangent_a, normal, radius)?;
                if center == tangent_a || center == tangent_b {
                    return Err(GeometryError::UnrepresentableCorner);
                }
                let angle_a = (tangent_a.y() - center.y()).atan2(tangent_a.x() - center.x());
                let angle_b = (tangent_b.y() - center.y()).atan2(tangent_b.x() - center.x());
                let (start, end) = if turn > 0.0 {
                    (angle_b, angle_a)
                } else {
                    (angle_a, angle_b)
                };
                let arc = Arc3::new(center, radius, start, end)?;
                if arc.sweep() == 0.0 || arc.sweep() > PI + 1.0e-12 {
                    return Err(GeometryError::UnrepresentableCorner);
                }
                arc.start_point()?;
                arc.end_point()?;
                Some(Entity::Arc(arc))
            }
        };
        Ok(LineCorner {
            first,
            second,
            connector,
        })
    }
}

pub(crate) fn dimension(value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(GeometryError::InvalidCornerDimension)
    }
}

fn direction(line: Line3) -> Result<[f64; 2], GeometryError> {
    let x = finite(line.end().x() - line.start().x())?;
    let y = finite(line.end().y() - line.start().y())?;
    let length = finite(x.hypot(y))?;
    if length == 0.0 {
        return Err(GeometryError::DegenerateCurve);
    }
    Ok([x / length, y / length])
}

fn picked_ray(
    direction: [f64; 2],
    intersection: Point3,
    pick: Point3,
) -> Result<([f64; 2], bool), GeometryError> {
    let distance = projection(intersection, pick, direction)?;
    if distance == 0.0 {
        return Err(GeometryError::AmbiguousCornerPick);
    }
    let forward = distance > 0.0;
    Ok((
        if forward {
            direction
        } else {
            [-direction[0], -direction[1]]
        },
        forward,
    ))
}

fn retained(
    line: Line3,
    intersection: Point3,
    ray: [f64; 2],
    forward: bool,
    distance: f64,
    tangent: Point3,
) -> Result<Line3, GeometryError> {
    let far = if forward { line.end() } else { line.start() };
    if projection(intersection, far, ray)? <= distance {
        return Err(GeometryError::CornerOutsideSources);
    }
    if tangent == far {
        return Err(GeometryError::UnrepresentableCorner);
    }
    Ok(if forward {
        Line3::new(tangent, far)
    } else {
        Line3::new(far, tangent)
    })
}

fn projection(origin: Point3, point: Point3, direction: [f64; 2]) -> Result<f64, GeometryError> {
    let x = finite(point.x() - origin.x())?;
    let y = finite(point.y() - origin.y())?;
    finite(x * direction[0] + y * direction[1])
}

fn along(origin: Point3, direction: [f64; 2], distance: f64) -> Result<Point3, GeometryError> {
    Point3::new(
        direction[0].mul_add(distance, origin.x()),
        direction[1].mul_add(distance, origin.y()),
        origin.z(),
    )
    .map_err(|_| GeometryError::NonFiniteResult)
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn finite(value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GeometryError::NonFiniteResult)
    }
}
