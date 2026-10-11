use crate::{Arc3, Entity, Layer, Line3, Point3};
use std::{error::Error, fmt};

/// An immutable simple R12 linetype. Values use drawing units before scaling.
#[derive(Clone, Debug, PartialEq)]
pub struct Linetype {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) flags: i16,
    pub(crate) pattern: Vec<f64>,
    pub(crate) length: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinetypeError {
    InvalidName,
    InvalidPattern,
    InvalidScale,
    DuplicateName(String),
    UnknownLinetype(String),
    UnknownLayer(String),
    TooManyLinetypes,
    TooDense,
    GeometryTooLarge,
}

impl fmt::Display for LinetypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => write!(
                f,
                "Use 1–31 ASCII letters, numbers, _, $, or - for a linetype name; BYLAYER and BYBLOCK are reserved."
            ),
            Self::InvalidPattern => write!(
                f,
                "A pattern needs 2–12 finite lengths, starting with a dash or dot, and a positive total length. Positive: dash; negative: gap; zero: dot."
            ),
            Self::InvalidScale => write!(f, "Linetype scale must be finite and greater than zero."),
            Self::DuplicateName(name) => write!(f, "Linetype {name} already exists."),
            Self::UnknownLinetype(name) => write!(f, "Unknown linetype {name}."),
            Self::UnknownLayer(name) => write!(f, "Unknown layer {name}."),
            Self::TooManyLinetypes => write!(f, "The R12 linetype table is full."),
            Self::TooDense => write!(
                f,
                "The linetype needs more than 4096 marks on one entity. Increase linetype scale before exporting."
            ),
            Self::GeometryTooLarge => write!(
                f,
                "The geometry or scaled linetype exceeds the finite range."
            ),
        }
    }
}
impl Error for LinetypeError {}

impl Linetype {
    pub fn new(name: &str, pattern: Vec<f64>) -> Result<Self, LinetypeError> {
        Layer::new(name).map_err(|_| LinetypeError::InvalidName)?;
        if name.eq_ignore_ascii_case("BYLAYER") || name.eq_ignore_ascii_case("BYBLOCK") {
            return Err(LinetypeError::InvalidName);
        }
        let length = pattern.iter().map(|value| value.abs()).sum();
        let result = Self {
            name: name.into(),
            description: String::new(),
            flags: 0,
            pattern,
            length,
        };
        result.validate()?;
        Ok(result)
    }

    pub(crate) fn continuous() -> Self {
        Self {
            name: "CONTINUOUS".into(),
            description: "Solid line".into(),
            flags: 0,
            pattern: Vec::new(),
            length: 0.0,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), LinetypeError> {
        let sum: f64 = self.pattern.iter().map(|value| value.abs()).sum();
        if self.pattern.is_empty() && self.length == 0.0 {
            return Ok(());
        }
        if !(2..=12).contains(&self.pattern.len())
            || self.pattern.iter().any(|value| !value.is_finite())
            || self.pattern[0] < 0.0
            || !sum.is_finite()
            || sum <= 0.0
            || !self.length.is_finite()
        {
            return Err(LinetypeError::InvalidPattern);
        }
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn pattern(&self) -> &[f64] {
        &self.pattern
    }
    pub fn pattern_length(&self) -> f64 {
        self.length
    }
    pub(crate) fn period(&self) -> f64 {
        self.pattern.iter().map(|value| value.abs()).sum()
    }

    // OpenCad's original A-fit policy: equally extend the first/last dash,
    // retaining the interior pattern. Leading dots and closed circles keep
    // their initial phase and clip the last repeat, without stretching dots.
    // Native endpoint fitting has not been established by the table contract.
    fn marks(&self, length: f64, scale: f64, closed: bool) -> Result<Vec<Mark>, LinetypeError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(LinetypeError::InvalidScale);
        }
        if !length.is_finite() || length < 0.0 {
            return Err(LinetypeError::GeometryTooLarge);
        }
        if self.pattern.is_empty() || length == 0.0 {
            return Ok(vec![Mark::Dash(0.0, length)]);
        }
        let period = self.period() * scale;
        if !period.is_finite() || period == 0.0 {
            return Err(LinetypeError::GeometryTooLarge);
        }
        let first = self.pattern[0] * scale;
        let periodic = closed || self.pattern[0] == 0.0;
        let cycles = if periodic {
            (length / period).ceil()
        } else {
            ((length - first) / period).floor()
        };
        if !periodic && cycles < 1.0 {
            return Ok(vec![Mark::Dash(0.0, length)]);
        }
        if cycles * self.pattern.len() as f64 > 4094.0 {
            return Err(LinetypeError::TooDense);
        }
        let mut marks = Vec::new();
        let mut cursor = 0.0;
        let mut append = |value: f64| -> Result<(), LinetypeError> {
            if cursor > length {
                return Ok(());
            }
            let next = cursor + value.abs();
            let end = next.min(length);
            if !end.is_finite() || (value != 0.0 && end == cursor && cursor < length) {
                return Err(LinetypeError::GeometryTooLarge);
            }
            if value == 0.0 {
                marks.push(Mark::Dot(cursor));
            } else if value > 0.0 && end > cursor {
                marks.push(Mark::Dash(cursor, end));
            }
            cursor = next;
            Ok(())
        };
        if periodic {
            for _ in 0..cycles as usize {
                for value in &self.pattern {
                    append(value * scale)?;
                }
            }
        } else {
            let end_dash = (length - cycles * period + first) / 2.0;
            append(end_dash)?;
            for cycle in 0..cycles as usize {
                for value in &self.pattern[1..] {
                    append(value * scale)?;
                }
                append(if cycle + 1 == cycles as usize {
                    end_dash
                } else {
                    first
                })?;
            }
        }
        Ok(marks)
    }

    pub(crate) fn parts(
        &self,
        entity: &Entity,
        scale: f64,
    ) -> Result<Vec<PatternPart>, LinetypeError> {
        self.oriented_parts(entity, scale, false)
    }

    fn oriented_parts(
        &self,
        entity: &Entity,
        scale: f64,
        reversed: bool,
    ) -> Result<Vec<PatternPart>, LinetypeError> {
        if self.pattern.is_empty() {
            return Ok(vec![PatternPart::Geometry(entity.clone())]);
        }
        if let Entity::Polyline(polyline) = entity {
            // The supported legacy flag restarts fitting at each segment.
            let mut parts = Vec::new();
            for (index, segment) in polyline.segments().enumerate() {
                let segment = segment.map_err(|_| LinetypeError::GeometryTooLarge)?;
                let width = polyline.widths()[index];
                if width.start() != 0.0 || width.end() != 0.0 {
                    let length = segment
                        .length()
                        .map_err(|_| LinetypeError::GeometryTooLarge)?;
                    if length == 0.0 {
                        continue;
                    }
                    let interpolate =
                        |fraction| width.start() * (1.0 - fraction) + width.end() * fraction;
                    for mark in self.marks(length, scale, false)? {
                        parts.push(match mark {
                            Mark::Dot(at) => PatternPart::Dot(
                                segment
                                    .point_at_fraction(at / length)
                                    .map_err(|_| LinetypeError::GeometryTooLarge)?,
                            ),
                            Mark::Dash(from, to) => {
                                let start = from / length;
                                let end = to / length;
                                let bulge = match segment {
                                    crate::PolylineSegment::Line(_) => 0.0,
                                    crate::PolylineSegment::Arc { arc, clockwise } => {
                                        let bulge = (arc.sweep() * (end - start) / 4.0).tan();
                                        if clockwise { -bulge } else { bulge }
                                    }
                                };
                                let mut dash = crate::Polyline2::with_bulges(
                                    vec![
                                        segment
                                            .point_at_fraction(start)
                                            .map_err(|_| LinetypeError::GeometryTooLarge)?,
                                        segment
                                            .point_at_fraction(end)
                                            .map_err(|_| LinetypeError::GeometryTooLarge)?,
                                    ],
                                    vec![bulge, 0.0],
                                    false,
                                )
                                .map_err(|_| LinetypeError::GeometryTooLarge)?;
                                dash.set_width(0, interpolate(start), interpolate(end))
                                    .map_err(|_| LinetypeError::GeometryTooLarge)?;
                                PatternPart::Geometry(Entity::Polyline(dash))
                            }
                        });
                    }
                    if parts.len() > 4096 {
                        return Err(LinetypeError::TooDense);
                    }
                    continue;
                }
                let reversed = matches!(
                    segment,
                    crate::PolylineSegment::Arc {
                        clockwise: true,
                        ..
                    }
                );
                parts.extend(self.oriented_parts(&segment.geometry(), scale, reversed)?);
                if parts.len() > 4096 {
                    return Err(LinetypeError::TooDense);
                }
            }
            return Ok(parts);
        }
        let (length, closed) = match entity {
            Entity::Line(line) => (
                line.length().map_err(|_| LinetypeError::GeometryTooLarge)?,
                false,
            ),
            Entity::Arc(arc) => (
                arc.length().map_err(|_| LinetypeError::GeometryTooLarge)?,
                false,
            ),
            Entity::Circle(circle) => (circle.radius() * std::f64::consts::TAU, true),
            Entity::Polyline(_) => unreachable!("polyline segments handled above"),
        };
        if length == 0.0 {
            return Ok(vec![PatternPart::Geometry(entity.clone())]);
        }
        self.marks(length, scale, closed)?
            .into_iter()
            .map(|mark| {
                let point = |distance: f64| {
                    let fraction = if reversed {
                        1.0 - distance / length
                    } else {
                        distance / length
                    };
                    match entity {
                        Entity::Line(line) => line.point_at_fraction(fraction),
                        Entity::Arc(arc) => arc.point_at_fraction(fraction),
                        Entity::Circle(circle) => {
                            let angle = std::f64::consts::TAU * (distance / length);
                            Point3::new(
                                circle.center().x() + circle.radius() * angle.cos(),
                                circle.center().y() + circle.radius() * angle.sin(),
                                circle.center().z(),
                            )
                        }
                        Entity::Polyline(_) => unreachable!("polyline segments handled above"),
                    }
                    .map_err(|_| LinetypeError::GeometryTooLarge)
                };
                Ok(match mark {
                    Mark::Dot(at) => PatternPart::Dot(point(at)?),
                    Mark::Dash(from, to) if from == 0.0 && to == length => {
                        PatternPart::Geometry(entity.clone())
                    }
                    Mark::Dash(from, to) => PatternPart::Geometry(match entity {
                        Entity::Line(_) => Entity::Line(Line3::new(point(from)?, point(to)?)),
                        Entity::Arc(arc) => {
                            let (from, to) = if reversed {
                                (length - to, length - from)
                            } else {
                                (from, to)
                            };
                            Entity::Arc(
                                Arc3::new(
                                    arc.center(),
                                    arc.radius(),
                                    arc.start_angle() + arc.sweep() * (from / length),
                                    arc.start_angle() + arc.sweep() * (to / length),
                                )
                                .map_err(|_| LinetypeError::GeometryTooLarge)?,
                            )
                        }
                        Entity::Circle(circle) => Entity::Arc(
                            Arc3::new(
                                circle.center(),
                                circle.radius(),
                                std::f64::consts::TAU * (from / length),
                                std::f64::consts::TAU * (to / length),
                            )
                            .map_err(|_| LinetypeError::GeometryTooLarge)?,
                        ),
                        Entity::Polyline(_) => unreachable!("polyline segments handled above"),
                    }),
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    Dash(f64, f64),
    Dot(f64),
}

#[derive(Clone, Debug)]
pub(crate) enum PatternPart {
    Geometry(Entity),
    Dot(Point3),
}

#[cfg(test)]
mod tests {
    use super::*;

    // Format/readback tests cannot catch incorrect mark placement or a repeat
    // that stops advancing. These are OpenCad's documented rendering policies.
    #[test]
    fn simple_marks_fit_endpoints_keep_dots_and_bound_repetition() {
        let dashed = Linetype::new("DASHED", vec![2.0, -1.0]).unwrap();
        assert_eq!(
            dashed.marks(8.0, 1.0, false).unwrap(),
            vec![
                Mark::Dash(0.0, 2.0),
                Mark::Dash(3.0, 5.0),
                Mark::Dash(6.0, 8.0)
            ]
        );
        assert_eq!(
            dashed.marks(7.0, 1.0, false).unwrap(),
            vec![Mark::Dash(0.0, 3.0), Mark::Dash(4.0, 7.0)]
        );
        assert_eq!(
            dashed.marks(0.5, 1.0, false).unwrap(),
            vec![Mark::Dash(0.0, 0.5)]
        );
        let dashdot = Linetype::new("DASHDOT", vec![2.0, -1.0, 0.0, -1.0]).unwrap();
        assert_eq!(
            dashdot.marks(20.0, 2.0, false).unwrap(),
            vec![
                Mark::Dash(0.0, 4.0),
                Mark::Dot(6.0),
                Mark::Dash(8.0, 12.0),
                Mark::Dot(14.0),
                Mark::Dash(16.0, 20.0)
            ]
        );
        assert_eq!(
            dashdot.marks(5.0, 1.0, true).unwrap(),
            vec![Mark::Dash(0.0, 2.0), Mark::Dot(3.0), Mark::Dash(4.0, 5.0)]
        );
        assert_eq!(
            dashdot.marks(1e9, 1e-9, false),
            Err(LinetypeError::TooDense)
        );
        assert_eq!(
            dashdot.marks(10.0, 0.0, false),
            Err(LinetypeError::InvalidScale)
        );
        for pattern in [
            vec![1.0],
            vec![0.0, 0.0],
            vec![-1.0, 1.0],
            vec![1.0, f64::NAN],
            vec![1.0; 13],
        ] {
            assert_eq!(
                Linetype::new("BAD", pattern),
                Err(LinetypeError::InvalidPattern)
            );
        }
        assert!(Linetype::new("DOT", vec![0.0, -0.5]).is_ok());
        let leading_dot = Linetype::new("DOTDASH", vec![0.0, -1.0, 1.0, -1.0]).unwrap();
        assert_eq!(
            leading_dot.marks(7.0, 1.0, false).unwrap(),
            vec![
                Mark::Dot(0.0),
                Mark::Dash(1.0, 2.0),
                Mark::Dot(3.0),
                Mark::Dash(4.0, 5.0),
                Mark::Dot(6.0)
            ]
        );
        assert_eq!(
            Linetype::new("DOT", vec![0.0, -1.0])
                .unwrap()
                .marks(0.5, 1.0, false)
                .unwrap(),
            vec![Mark::Dot(0.0)]
        );
        assert!(Linetype::new("SOLID_PATTERN", vec![1.0, 2.0]).is_ok());
        let circle =
            Entity::Circle(crate::Circle3::new(Point3::new(0.0, 0.0, 0.0).unwrap(), 0.1).unwrap());
        assert!(matches!(
            dashed.parts(&circle, 1.0).unwrap().as_slice(),
            [PatternPart::Geometry(Entity::Circle(_))]
        ));
        let large_circle = Entity::Circle(
            crate::Circle3::new(Point3::new(0.0, 0.0, 0.0).unwrap(), 1e307).unwrap(),
        );
        assert!(
            Linetype::new("LARGE", vec![5e307, -1e307])
                .unwrap()
                .parts(&large_circle, 1.0)
                .is_ok()
        );
    }
}
