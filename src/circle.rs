use crate::{
    Arc3, Dimension, Ellipse3, Face3, GeometryError, Hatch, Insert, Leader, Line3, MText, Point3,
    PointEntity, Polyline2, RasterImage, Ray3, Spline3, Table, Text, XLine3,
};

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
    Point(PointEntity),
    Line(Line3),
    Circle(Circle3),
    Arc(Arc3),
    Polyline(Polyline2),
    Ellipse(Ellipse3),
    Spline(Spline3),
    Text(Text),
    MText(MText),
    Leader(Leader),
    Table(Table),
    Dimension(Dimension),
    Hatch(Hatch),
    Insert(Insert),
    Face3(Face3),
    XLine(XLine3),
    Ray(Ray3),
    Image(RasterImage),
}

impl Entity {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Point(_) => "POINT",
            Self::Line(_) => "LINE",
            Self::Circle(_) => "CIRCLE",
            Self::Arc(_) => "ARC",
            Self::Polyline(_) => "POLYLINE",
            Self::Ellipse(_) => "ELLIPSE",
            Self::Spline(_) => "SPLINE",
            Self::Text(_) => "TEXT",
            Self::MText(_) => "MTEXT",
            Self::Leader(_) => "LEADER",
            Self::Table(_) => "ACAD_TABLE",
            Self::Dimension(_) => "DIMENSION",
            Self::Hatch(_) => "HATCH",
            Self::Insert(_) => "INSERT",
            Self::Face3(_) => "3DFACE",
            Self::XLine(_) => "XLINE",
            Self::Ray(_) => "RAY",
            Self::Image(_) => "IMAGE",
        }
    }

    pub fn translated(&self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        let point = |p: Point3| Point3::new(p.x() + dx, p.y() + dy, p.z() + dz);
        Ok(match self {
            Self::Point(point_entity) => Self::Point(point_entity.translated(dx, dy, dz)?),
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
            Self::Ellipse(ellipse) => Self::Ellipse(ellipse.translated(dx, dy, dz)?),
            Self::Spline(spline) => Self::Spline(spline.translated(dx, dy, dz)?),
            Self::Text(text) => {
                Self::Text(text.transformed(crate::Transform::translation(dx, dy, dz))?)
            }
            Self::MText(text) => {
                Self::MText(text.transformed(crate::Transform::translation(dx, dy, dz))?)
            }
            Self::Leader(leader) => {
                Self::Leader(leader.transformed(crate::Transform::translation(dx, dy, dz))?)
            }
            Self::Table(table) => {
                Self::Table(table.transformed(crate::Transform::translation(dx, dy, dz))?)
            }
            Self::Dimension(dimension) => {
                Self::Dimension(dimension.transformed(crate::Transform::translation(dx, dy, dz))?)
            }
            Self::Hatch(hatch) => {
                Self::Hatch(hatch.transformed(crate::Transform::translation(dx, dy, dz))?)
            }
            Self::Insert(_) => return Err(GeometryError::UnsupportedPolylineConversion),
            Self::Face3(face) => {
                let (mut vertices, count) = face.vertices();
                for p in vertices.iter_mut().take(count) {
                    *p = point(*p)?;
                }
                Self::Face3(Face3::new(vertices, count as u8, face.invisible_edges())?)
            }
            Self::XLine(line) => Self::XLine(line.translated(dx, dy, dz)?),
            Self::Ray(ray) => Self::Ray(ray.translated(dx, dy, dz)?),
            Self::Image(image) => Self::Image(image.translated(dx, dy, dz)?),
        })
    }

    pub(crate) fn coordinate(&self, code: u16) -> f64 {
        match self {
            Self::Point(point_entity) => match code {
                10 => point_entity.position().x(),
                20 => point_entity.position().y(),
                30 => point_entity.position().z(),
                _ => unreachable!("validated POINT coordinate field"),
            },
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
            Self::Ellipse(ellipse) => match code {
                10 => ellipse.center().x(),
                20 => ellipse.center().y(),
                30 => ellipse.center().z(),
                11 => ellipse.major_axis().x(),
                21 => ellipse.major_axis().y(),
                31 => ellipse.major_axis().z(),
                40 => ellipse.ratio(),
                41 => ellipse.start_parameter(),
                42 => ellipse.end_parameter(),
                _ => unreachable!("validated ELLIPSE coordinate field"),
            },
            _ => 0.0,
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
