//! Coordinate systems and dependency-free orthographic 3D viewing math.

use crate::{GeometryError, Point3};
use std::f64::consts::{FRAC_PI_2, PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ucs3 {
    origin: Point3,
    x: Point3,
    y: Point3,
    z: Point3,
}

impl Ucs3 {
    pub fn world() -> Self {
        Self {
            origin: Point3::default(),
            x: Point3::new(1.0, 0.0, 0.0).expect("finite"),
            y: Point3::new(0.0, 1.0, 0.0).expect("finite"),
            z: Point3::new(0.0, 0.0, 1.0).expect("finite"),
        }
    }
    pub fn from_axes(
        origin: Point3,
        x_axis: Point3,
        y_axis: Point3,
    ) -> Result<Self, GeometryError> {
        let x = normalize(x_axis)?;
        let projection = dot(y_axis, x);
        if !projection.is_finite() {
            return Err(GeometryError::NonFiniteResult);
        }
        let y0 = sub(y_axis, mul(x, projection)?)?;
        let y = normalize(y0)?;
        let z = normalize(cross(x, y)?)?;
        Ok(Self { origin, x, y, z })
    }
    pub fn from_points(
        origin: Point3,
        x_point: Point3,
        y_point: Point3,
    ) -> Result<Self, GeometryError> {
        Self::from_axes(origin, sub(x_point, origin)?, sub(y_point, origin)?)
    }
    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn x_axis(self) -> Point3 {
        self.x
    }
    pub fn y_axis(self) -> Point3 {
        self.y
    }
    pub fn z_axis(self) -> Point3 {
        self.z
    }
    pub fn local_to_world(self, local: Point3) -> Result<Point3, GeometryError> {
        add(
            self.origin,
            add(
                mul(self.x, local.x())?,
                add(mul(self.y, local.y())?, mul(self.z, local.z())?)?,
            )?,
        )
    }
    pub fn world_to_local(self, world: Point3) -> Result<Point3, GeometryError> {
        let p = sub(world, self.origin)?;
        Point3::new(dot(p, self.x), dot(p, self.y), dot(p, self.z))
            .map_err(|_| GeometryError::NonFiniteResult)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewOrientation {
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
    SouthWest,
    SouthEast,
    NorthEast,
    NorthWest,
}

impl ViewOrientation {
    pub fn ucs(self) -> Ucs3 {
        let v = |x, y, z| Point3::new(x, y, z).expect("finite");
        match self {
            Self::Top => {
                Ucs3::from_axes(Point3::default(), v(1., 0., 0.), v(0., 1., 0.)).expect("basis")
            }
            Self::Bottom => {
                Ucs3::from_axes(Point3::default(), v(1., 0., 0.), v(0., -1., 0.)).expect("basis")
            }
            Self::Front => {
                Ucs3::from_axes(Point3::default(), v(1., 0., 0.), v(0., 0., 1.)).expect("basis")
            }
            Self::Back => {
                Ucs3::from_axes(Point3::default(), v(-1., 0., 0.), v(0., 0., 1.)).expect("basis")
            }
            Self::Left => {
                Ucs3::from_axes(Point3::default(), v(0., 1., 0.), v(0., 0., 1.)).expect("basis")
            }
            Self::Right => {
                Ucs3::from_axes(Point3::default(), v(0., -1., 0.), v(0., 0., 1.)).expect("basis")
            }
            Self::SouthWest => {
                Ucs3::from_axes(Point3::default(), v(1., -1., 0.), v(0., 0., 1.)).expect("basis")
            }
            Self::SouthEast => {
                Ucs3::from_axes(Point3::default(), v(1., 1., 0.), v(0., 0., 1.)).expect("basis")
            }
            Self::NorthEast => {
                Ucs3::from_axes(Point3::default(), v(-1., 1., 0.), v(0., 0., 1.)).expect("basis")
            }
            Self::NorthWest => {
                Ucs3::from_axes(Point3::default(), v(-1., -1., 0.), v(0., 0., 1.)).expect("basis")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera3 {
    target: Point3,
    distance: f64,
    orientation: ViewOrientation,
    yaw: f64,
    pitch: f64,
}

impl Camera3 {
    pub fn new(
        target: Point3,
        distance: f64,
        orientation: ViewOrientation,
    ) -> Result<Self, GeometryError> {
        if !distance.is_finite() || distance <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        Ok(Self {
            target,
            distance,
            orientation,
            yaw: 0.0,
            pitch: 0.0,
        })
    }
    pub fn target(self) -> Point3 {
        self.target
    }
    pub fn distance(self) -> f64 {
        self.distance
    }
    pub fn orientation(self) -> ViewOrientation {
        self.orientation
    }
    pub fn set_target(&mut self, target: Point3) {
        self.target = target;
    }
    pub fn set_distance(&mut self, distance: f64) -> Result<(), GeometryError> {
        if !distance.is_finite() || distance <= 0.0 {
            return Err(GeometryError::NonPositiveScale);
        }
        self.distance = distance;
        Ok(())
    }
    pub fn set_orientation(&mut self, orientation: ViewOrientation) {
        self.orientation = orientation;
        self.yaw = 0.0;
        self.pitch = 0.0;
    }
    pub fn orbit(&mut self, yaw_delta: f64, pitch_delta: f64) -> Result<(), GeometryError> {
        if !yaw_delta.is_finite() || !pitch_delta.is_finite() {
            return Err(GeometryError::NonFiniteAngle);
        }
        self.yaw = (self.yaw + yaw_delta).rem_euclid(2.0 * PI);
        self.pitch = (self.pitch + pitch_delta).clamp(-FRAC_PI_2 + 1e-9, FRAC_PI_2 - 1e-9);
        Ok(())
    }
    pub fn project(self, point: Point3) -> Result<Point3, GeometryError> {
        let (r, u, f) = self.basis()?;
        let p = sub(point, self.target)?;
        Point3::new(dot(p, r), dot(p, u), dot(p, f)).map_err(|_| GeometryError::NonFiniteResult)
    }
    pub fn inverse_ray(self, view_x: f64, view_y: f64) -> Result<(Point3, Point3), GeometryError> {
        if !view_x.is_finite() || !view_y.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        let (r, u, f) = self.basis()?;
        let origin = add(
            self.target,
            add(
                mul(r, view_x)?,
                add(mul(u, view_y)?, mul(f, self.distance)?)?,
            )?,
        )?;
        Ok((origin, mul(f, -1.0)?))
    }
    pub fn intersect_ucs(
        self,
        view_x: f64,
        view_y: f64,
        ucs: Ucs3,
    ) -> Result<Point3, GeometryError> {
        let (origin, direction) = self.inverse_ray(view_x, view_y)?;
        let denominator = dot(direction, ucs.z_axis());
        if denominator.abs() < 1e-12 {
            return Err(GeometryError::NoBoundaryIntersection);
        }
        let t = dot(sub(ucs.origin(), origin)?, ucs.z_axis()) / denominator;
        if !t.is_finite() {
            return Err(GeometryError::NonFiniteResult);
        }
        add(origin, mul(direction, t)?)
    }
    fn basis(self) -> Result<(Point3, Point3, Point3), GeometryError> {
        let base = self.orientation.ucs();
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let r0 = base.x_axis();
        let u0 = base.y_axis();
        let f0 = base.z_axis();
        let r = add(mul(r0, cy)?, mul(f0, sy)?)?;
        let f = add(
            mul(mul(r0, -sy)?, cp)?,
            add(mul(u0, sp)?, mul(f0, cy * cp)?)?,
        )?;
        let u = normalize(cross(f, r)?)?;
        Ok((normalize(r)?, u, normalize(f)?))
    }
}

fn add(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(a.x() + b.x(), a.y() + b.y(), a.z() + b.z())
        .map_err(|_| GeometryError::NonFiniteResult)
}
fn sub(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(a.x() - b.x(), a.y() - b.y(), a.z() - b.z())
        .map_err(|_| GeometryError::NonFiniteResult)
}
fn mul(a: Point3, s: f64) -> Result<Point3, GeometryError> {
    Point3::new(a.x() * s, a.y() * s, a.z() * s).map_err(|_| GeometryError::NonFiniteResult)
}
fn dot(a: Point3, b: Point3) -> f64 {
    a.x() * b.x() + a.y() * b.y() + a.z() * b.z()
}
fn cross(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(
        a.y() * b.z() - a.z() * b.y(),
        a.z() * b.x() - a.x() * b.z(),
        a.x() * b.y() - a.y() * b.x(),
    )
    .map_err(|_| GeometryError::NonFiniteResult)
}
fn normalize(v: Point3) -> Result<Point3, GeometryError> {
    let n = v.x().hypot(v.y()).hypot(v.z());
    if !n.is_finite() {
        return Err(GeometryError::NonFiniteResult);
    }
    if n <= 1e-12 {
        return Err(GeometryError::DegenerateLine);
    }
    mul(v, 1.0 / n)
}
