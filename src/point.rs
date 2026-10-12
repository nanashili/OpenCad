use crate::{GeometryError, Point3};

/// A point entity in model space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointEntity {
    position: Point3,
}

impl PointEntity {
    pub fn new(position: Point3) -> Self {
        Self { position }
    }

    pub fn position(self) -> Point3 {
        self.position
    }

    pub fn set_position(&mut self, position: Point3) {
        self.position = position;
    }

    pub fn translated(self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        Ok(Self::new(Point3::new(
            self.position.x() + dx,
            self.position.y() + dy,
            self.position.z() + dz,
        )?))
    }
}
