use crate::{Arc3, Circle3, Entity, GeometryError, Point3};

/// An XY offset at a positive distance toward a point, or through that point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offset {
    point: Point3,
    distance: Option<f64>,
}

impl Offset {
    pub fn distance(distance: f64, side: Point3) -> Result<Self, GeometryError> {
        if !distance.is_finite() {
            return Err(GeometryError::NonFiniteDistance);
        }
        if distance <= 0.0 {
            return Err(GeometryError::NonPositiveOffset);
        }
        Ok(Self {
            point: side,
            distance: Some(distance),
        })
    }

    pub fn through(point: Point3) -> Self {
        Self {
            point,
            distance: None,
        }
    }

    fn radius(self, center: Point3, radius: f64) -> Result<f64, GeometryError> {
        let radial = (self.point.x() - center.x()).hypot(self.point.y() - center.y());
        if !radial.is_finite() {
            return Err(GeometryError::NonFiniteResult);
        }
        let result = if let Some(distance) = self.distance {
            if radial == radius {
                return Err(GeometryError::UndefinedOffsetSide);
            }
            if radial > radius {
                radius + distance
            } else {
                radius - distance
            }
        } else {
            radial
        };
        if !result.is_finite() {
            return Err(GeometryError::NonFiniteResult);
        }
        if result <= 0.0 {
            return Err(GeometryError::OffsetCollapse);
        }
        if result == radius {
            return Err(GeometryError::UnrepresentableOffset);
        }
        Ok(result)
    }
}

impl Entity {
    /// Offset an XY LINE, CIRCLE or ARC, retaining Z and arc angles.
    /// POLYLINE joins and non-XY lines are not implemented.
    pub fn offset_xy(&self, offset: Offset) -> Result<Self, GeometryError> {
        match self {
            Self::Line(line) => {
                if line.start().z() != line.end().z() {
                    return Err(GeometryError::UnsupportedOffsetEntity);
                }
                let dx = line.end().x() - line.start().x();
                let dy = line.end().y() - line.start().y();
                let length = dx.hypot(dy);
                if !length.is_finite() {
                    return Err(GeometryError::NonFiniteResult);
                }
                if length == 0.0 {
                    return Err(GeometryError::DegenerateLine);
                }
                let (nx, ny) = (-dy / length, dx / length);
                let side = nx.mul_add(
                    offset.point.x() - line.start().x(),
                    ny * (offset.point.y() - line.start().y()),
                );
                if !side.is_finite() {
                    return Err(GeometryError::NonFiniteResult);
                }
                if side == 0.0 {
                    return Err(GeometryError::UndefinedOffsetSide);
                }
                let distance = offset.distance.map_or(side, |value| value.copysign(side));
                let result = self.translated(nx * distance, ny * distance, 0.0)?;
                if result == *self {
                    return Err(GeometryError::UnrepresentableOffset);
                }
                Ok(result)
            }
            Self::Circle(circle) => Ok(Self::Circle(Circle3::new(
                circle.center(),
                offset.radius(circle.center(), circle.radius())?,
            )?)),
            Self::Arc(arc) => Ok(Self::Arc(Arc3::new(
                arc.center(),
                offset.radius(arc.center(), arc.radius())?,
                arc.start_angle(),
                arc.end_angle(),
            )?)),
            Self::Polyline(_) => Err(GeometryError::UnsupportedOffsetEntity),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OffsetLayer {
    #[default]
    Source,
    Current,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OffsetOptions {
    pub erase_source: bool,
    pub layer: OffsetLayer,
}
