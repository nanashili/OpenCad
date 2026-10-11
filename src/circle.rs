use crate::{Arc3, GeometryError, Line3, Point3, Polyline2};

/// A finite circle in the world XY plane, at the center's Z elevation.
/// Radius updates follow the observed geometry contract: negative radii use
/// their magnitude, and zero remains representable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle3 {
    center: Point3,
    radius: f64,
}

impl Circle3 {
    pub fn new(center: Point3, radius: f64) -> Result<Self, GeometryError> {
        let mut circle = Self {
            center,
            radius: 0.0,
        };
        circle.set_radius(radius)?;
        Ok(circle)
    }

    pub fn center(self) -> Point3 {
        self.center
    }

    pub fn radius(self) -> f64 {
        self.radius
    }

    pub fn set_center(&mut self, center: Point3) {
        self.center = center;
    }

    pub fn set_radius(&mut self, radius: f64) -> Result<(), GeometryError> {
        if !radius.is_finite() {
            return Err(GeometryError::NonFiniteRadius);
        }
        self.radius = radius.abs();
        Ok(())
    }
}

/// Drawing entities supported by the editor and R12 document model.
#[derive(Clone, Debug, PartialEq)]
pub enum Entity {
    Line(Line3),
    Circle(Circle3),
    Arc(Arc3),
    Polyline(Polyline2),
}

impl Entity {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Line(_) => "LINE",
            Self::Circle(_) => "CIRCLE",
            Self::Arc(_) => "ARC",
            Self::Polyline(_) => "POLYLINE",
        }
    }

    pub fn translated(&self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        let point = |p: Point3| Point3::new(p.x() + dx, p.y() + dy, p.z() + dz);
        Ok(match self {
            Self::Line(line) => Self::Line(Line3::new(point(line.start())?, point(line.end())?)),
            Self::Circle(circle) => {
                Self::Circle(Circle3::new(point(circle.center())?, circle.radius())?)
            }
            Self::Arc(arc) => {
                let mut arc = *arc;
                arc.set_center(point(arc.center())?);
                Self::Arc(arc)
            }
            Self::Polyline(polyline) => {
                let mut result = polyline.clone();
                result.set_vertices(
                    polyline
                        .vertices()
                        .iter()
                        .copied()
                        .map(point)
                        .collect::<Result<_, _>>()?,
                )?;
                Self::Polyline(result)
            }
        })
    }

    pub(crate) fn coordinate(&self, code: u16) -> f64 {
        match self {
            Self::Line(line) => match code {
                10 => line.start().x(),
                20 => line.start().y(),
                30 => line.start().z(),
                11 => line.end().x(),
                21 => line.end().y(),
                31 => line.end().z(),
                _ => unreachable!("validated LINE coordinate field"),
            },
            Self::Circle(circle) => match code {
                10 => circle.center().x(),
                20 => circle.center().y(),
                30 => circle.center().z(),
                40 => circle.radius(),
                _ => unreachable!("validated CIRCLE coordinate field"),
            },
            Self::Arc(arc) => match code {
                10 => arc.center().x(),
                20 => arc.center().y(),
                30 => arc.center().z(),
                40 => arc.radius(),
                50 => arc.start_angle().to_degrees(),
                51 => arc.end_angle().to_degrees(),
                _ => unreachable!("validated ARC coordinate field"),
            },
            Self::Polyline(polyline) => match code {
                10 | 20 => 0.0,
                30 => polyline.elevation(),
                70 => f64::from(u8::from(polyline.is_closed())),
                _ => unreachable!("validated POLYLINE header field"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_updates_keep_xyz_and_radius_magnitude_without_accepting_nonfinite_input() {
        let center = Point3::new(1.0, 2.0, 3.0).unwrap();
        let mut circle = Circle3::new(center, -5.0).unwrap();
        assert_eq!(circle.center(), center);
        assert_eq!(circle.radius(), 5.0);
        circle.set_radius(0.0).unwrap();
        assert_eq!(circle.radius(), 0.0);
        let replacement = Point3::new(-4.0, 6.0, 8.0).unwrap();
        circle.set_center(replacement);
        assert_eq!(circle.center(), replacement);
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                circle.set_radius(invalid),
                Err(GeometryError::NonFiniteRadius)
            );
            assert_eq!(circle.radius(), 0.0);
        }
    }
}
