//! Lightweight tessellated 3D faces and mesh primitives.
//!
//! These types deliberately describe polygonal surfaces rather than analytic
//! boundary representations.  They are suitable for preview, export, and
//! construction tools while remaining small and dependency free.

use crate::{GeometryError, Point3};
use std::f64::consts::TAU;

const MAX_SEGMENTS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face3 {
    vertices: [Point3; 4],
    count: u8,
    invisible_edges: [bool; 4],
}

impl Face3 {
    pub fn triangle(a: Point3, b: Point3, c: Point3) -> Result<Self, GeometryError> {
        Self::new([a, b, c, c], 3, [false; 4])
    }

    pub fn quad(a: Point3, b: Point3, c: Point3, d: Point3) -> Result<Self, GeometryError> {
        Self::new([a, b, c, d], 4, [false; 4])
    }

    pub fn new(
        vertices: [Point3; 4],
        count: u8,
        invisible_edges: [bool; 4],
    ) -> Result<Self, GeometryError> {
        if !(count == 3 || count == 4) {
            return Err(GeometryError::DegenerateCurve);
        }
        if vertices[..usize::from(count)]
            .iter()
            .any(|p| !finite_point(*p))
        {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        Ok(Self {
            vertices,
            count,
            invisible_edges,
        })
    }

    pub fn vertices(self) -> ([Point3; 4], usize) {
        (self.vertices, usize::from(self.count))
    }
    pub fn vertex(self, index: usize) -> Option<Point3> {
        (index < usize::from(self.count)).then_some(self.vertices[index])
    }
    pub fn vertex_count(self) -> usize {
        usize::from(self.count)
    }
    pub fn invisible_edges(self) -> [bool; 4] {
        self.invisible_edges
    }
    pub fn with_invisible_edges(mut self, edges: [bool; 4]) -> Self {
        self.invisible_edges = edges;
        self
    }

    pub fn transformed(
        self,
        origin: Point3,
        axis: Point3,
        angle: f64,
    ) -> Result<Self, GeometryError> {
        let mut vertices = self.vertices;
        for p in vertices.iter_mut().take(usize::from(self.count)) {
            *p = rotate_point(*p, origin, axis, angle)?;
        }
        Self::new(vertices, self.count, self.invisible_edges)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh3 {
    faces: Vec<Face3>,
}

impl Mesh3 {
    pub fn new(faces: Vec<Face3>) -> Result<Self, GeometryError> {
        Ok(Self { faces })
    }
    pub fn faces(&self) -> &[Face3] {
        &self.faces
    }
    pub fn into_faces(self) -> Vec<Face3> {
        self.faces
    }
    pub fn push(&mut self, face: Face3) {
        self.faces.push(face);
    }
    pub fn translated(&self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        let mut faces = Vec::with_capacity(self.faces.len());
        for face in &self.faces {
            let (mut points, count) = face.vertices();
            for p in points.iter_mut().take(count) {
                *p = Point3::new(p.x() + dx, p.y() + dy, p.z() + dz)?;
            }
            faces.push(Face3::new(points, count as u8, face.invisible_edges())?);
        }
        Self::new(faces)
    }
    pub fn rotated(&self, origin: Point3, axis: Point3, angle: f64) -> Result<Self, GeometryError> {
        self.faces
            .iter()
            .copied()
            .map(|f| f.transformed(origin, axis, angle))
            .collect::<Result<Vec<_>, _>>()
            .and_then(Self::new)
    }

    pub fn box_mesh(min: Point3, max: Point3) -> Result<Self, GeometryError> {
        validate_ordered_bounds(min, max)?;
        let p = [
            Point3::new(min.x(), min.y(), min.z())?,
            Point3::new(max.x(), min.y(), min.z())?,
            Point3::new(max.x(), max.y(), min.z())?,
            Point3::new(min.x(), max.y(), min.z())?,
            Point3::new(min.x(), min.y(), max.z())?,
            Point3::new(max.x(), min.y(), max.z())?,
            Point3::new(max.x(), max.y(), max.z())?,
            Point3::new(min.x(), max.y(), max.z())?,
        ];
        Self::new(vec![
            Face3::quad(p[0], p[3], p[2], p[1])?,
            Face3::quad(p[4], p[5], p[6], p[7])?,
            Face3::quad(p[0], p[1], p[5], p[4])?,
            Face3::quad(p[1], p[2], p[6], p[5])?,
            Face3::quad(p[2], p[3], p[7], p[6])?,
            Face3::quad(p[3], p[0], p[4], p[7])?,
        ])
    }

    pub fn wedge(width: f64, depth: f64, height: f64) -> Result<Self, GeometryError> {
        positive_dimensions(&[width, depth, height])?;
        let p = [
            Point3::new(0.0, 0.0, 0.0)?,
            Point3::new(width, 0.0, 0.0)?,
            Point3::new(width, depth, 0.0)?,
            Point3::new(0.0, depth, 0.0)?,
            Point3::new(0.0, 0.0, height)?,
            Point3::new(width, 0.0, height)?,
        ];
        Self::new(vec![
            Face3::quad(p[0], p[3], p[2], p[1])?,
            Face3::quad(p[0], p[1], p[5], p[4])?,
            Face3::triangle(p[1], p[2], p[5])?,
            Face3::triangle(p[2], p[3], p[4])?,
            Face3::quad(p[4], p[5], p[2], p[3])?,
        ])
    }

    pub fn pyramid(width: f64, depth: f64, height: f64) -> Result<Self, GeometryError> {
        positive_dimensions(&[width, depth, height])?;
        let a = Point3::new(0.0, 0.0, 0.0)?;
        let b = Point3::new(width, 0.0, 0.0)?;
        let c = Point3::new(width, depth, 0.0)?;
        let d = Point3::new(0.0, depth, 0.0)?;
        let t = Point3::new(width / 2.0, depth / 2.0, height)?;
        Self::new(vec![
            Face3::quad(a, d, c, b)?,
            Face3::triangle(a, b, t)?,
            Face3::triangle(b, c, t)?,
            Face3::triangle(c, d, t)?,
            Face3::triangle(d, a, t)?,
        ])
    }

    pub fn sphere(
        center: Point3,
        radius: f64,
        segments: usize,
        rings: usize,
    ) -> Result<Self, GeometryError> {
        positive_dimensions(&[radius])?;
        validate_resolution(segments)?;
        validate_resolution(rings)?;
        let mut faces = Vec::with_capacity(segments * rings * 2);
        let s = segments as f64;
        let r = rings as f64;
        for j in 0..rings {
            let a0 = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * j as f64 / r;
            let a1 = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * (j + 1) as f64 / r;
            for i in 0..segments {
                let t0 = TAU * i as f64 / s;
                let t1 = TAU * (i + 1) as f64 / s;
                let q = |a: f64, t: f64| {
                    Point3::new(
                        center.x() + radius * a.cos() * t.cos(),
                        center.y() + radius * a.cos() * t.sin(),
                        center.z() + radius * a.sin(),
                    )
                };
                let p0 = q(a0, t0)?;
                let p1 = q(a0, t1)?;
                let p2 = q(a1, t1)?;
                let p3 = q(a1, t0)?;
                faces.push(Face3::quad(p0, p1, p2, p3)?);
            }
        }
        Self::new(faces)
    }

    pub fn cylinder(
        center: Point3,
        radius: f64,
        height: f64,
        segments: usize,
    ) -> Result<Self, GeometryError> {
        Self::frustum(center, radius, radius, height, segments)
    }
    pub fn cone(
        center: Point3,
        radius: f64,
        height: f64,
        segments: usize,
    ) -> Result<Self, GeometryError> {
        Self::frustum(center, radius, 0.0, height, segments)
    }
    pub fn frustum(
        center: Point3,
        bottom: f64,
        top: f64,
        height: f64,
        segments: usize,
    ) -> Result<Self, GeometryError> {
        positive_dimensions(&[bottom, height])?;
        if !top.is_finite() || top < 0.0 {
            return Err(GeometryError::DegenerateCurve);
        }
        validate_resolution(segments)?;
        let mut faces = Vec::with_capacity(segments + 2);
        let s = segments as f64;
        for i in 0..segments {
            let a = TAU * i as f64 / s;
            let b = TAU * (i + 1) as f64 / s;
            let q = |r: f64, z: f64, t: f64| {
                Point3::new(
                    center.x() + r * t.cos(),
                    center.y() + r * t.sin(),
                    center.z() + z,
                )
            };
            faces.push(Face3::quad(
                q(bottom, 0.0, a)?,
                q(bottom, 0.0, b)?,
                q(top, height, b)?,
                q(top, height, a)?,
            )?);
        }
        let cb = Point3::new(center.x(), center.y(), center.z())?;
        let ct = Point3::new(center.x(), center.y(), center.z() + height)?;
        for i in 1..segments - 1 {
            let a = TAU * i as f64 / s;
            let b = TAU * (i + 1) as f64 / s;
            faces.push(Face3::triangle(
                cb,
                Point3::new(
                    center.x() + bottom * b.cos(),
                    center.y() + bottom * b.sin(),
                    center.z(),
                )?,
                Point3::new(
                    center.x() + bottom * a.cos(),
                    center.y() + bottom * a.sin(),
                    center.z(),
                )?,
            )?);
            if top > 0.0 {
                faces.push(Face3::triangle(
                    ct,
                    Point3::new(
                        center.x() + top * a.cos(),
                        center.y() + top * a.sin(),
                        center.z() + height,
                    )?,
                    Point3::new(
                        center.x() + top * b.cos(),
                        center.y() + top * b.sin(),
                        center.z() + height,
                    )?,
                )?);
            }
        }
        Self::new(faces)
    }

    pub fn torus(
        center: Point3,
        major: f64,
        minor: f64,
        major_segments: usize,
        minor_segments: usize,
    ) -> Result<Self, GeometryError> {
        positive_dimensions(&[major, minor])?;
        validate_resolution(major_segments)?;
        validate_resolution(minor_segments)?;
        let mut faces = Vec::with_capacity(major_segments * minor_segments);
        let (a, b) = (major_segments as f64, minor_segments as f64);
        for i in 0..major_segments {
            for j in 0..minor_segments {
                let q = |u: f64, v: f64| {
                    Point3::new(
                        center.x() + (major + minor * v.cos()) * u.cos(),
                        center.y() + (major + minor * v.cos()) * u.sin(),
                        center.z() + minor * v.sin(),
                    )
                };
                let u0 = TAU * i as f64 / a;
                let u1 = TAU * (i + 1) as f64 / a;
                let v0 = TAU * j as f64 / b;
                let v1 = TAU * (j + 1) as f64 / b;
                faces.push(Face3::quad(q(u0, v0)?, q(u1, v0)?, q(u1, v1)?, q(u0, v1)?)?);
            }
        }
        Self::new(faces)
    }

    pub fn extrude_polygon(points: &[Point3], height: f64) -> Result<Self, GeometryError> {
        if points.len() < 3 || !height.is_finite() || height == 0.0 {
            return Err(GeometryError::DegenerateCurve);
        }
        let top = points
            .iter()
            .map(|p| Point3::new(p.x(), p.y(), p.z() + height))
            .collect::<Result<Vec<_>, _>>()?;
        let mut faces = Vec::with_capacity(points.len() + 2);
        for i in 1..points.len() - 1 {
            faces.push(Face3::triangle(points[0], points[i + 1], points[i])?);
            faces.push(Face3::triangle(top[0], top[i], top[i + 1])?);
        }
        for i in 0..points.len() {
            let j = (i + 1) % points.len();
            faces.push(Face3::quad(points[i], points[j], top[j], top[i])?);
        }
        Self::new(faces)
    }

    pub fn revolve_profile(
        profile: &[Point3],
        axis_origin: Point3,
        axis: Point3,
        segments: usize,
    ) -> Result<Self, GeometryError> {
        Self::revolve_profile_angle(profile, axis_origin, axis, segments, TAU)
    }

    /// Revolve a profile through a signed angle in radians.
    pub fn revolve_profile_angle(
        profile: &[Point3],
        axis_origin: Point3,
        axis: Point3,
        segments: usize,
        angle: f64,
    ) -> Result<Self, GeometryError> {
        if profile.len() < 2 || !angle.is_finite() || angle == 0.0 {
            return Err(GeometryError::DegenerateCurve);
        }
        validate_resolution(segments)?;
        let mut faces = Vec::with_capacity((profile.len() - 1) * segments);
        for i in 0..segments {
            let a = angle * i as f64 / segments as f64;
            let b = angle * (i + 1) as f64 / segments as f64;
            for pair in profile.windows(2) {
                faces.push(Face3::quad(
                    rotate_point(pair[0], axis_origin, axis, a)?,
                    rotate_point(pair[1], axis_origin, axis, a)?,
                    rotate_point(pair[1], axis_origin, axis, b)?,
                    rotate_point(pair[0], axis_origin, axis, b)?,
                )?);
            }
        }
        Self::new(faces)
    }
}

pub fn rotate_point(
    point: Point3,
    origin: Point3,
    axis: Point3,
    angle: f64,
) -> Result<Point3, GeometryError> {
    if !angle.is_finite() {
        return Err(GeometryError::NonFiniteParameter);
    }
    let n = axis.x().hypot(axis.y()).hypot(axis.z());
    if !n.is_finite() || n == 0.0 {
        return Err(GeometryError::DegenerateCurve);
    }
    let (x, y, z) = (axis.x() / n, axis.y() / n, axis.z() / n);
    let (s, c) = angle.sin_cos();
    let (px, py, pz) = (
        point.x() - origin.x(),
        point.y() - origin.y(),
        point.z() - origin.z(),
    );
    let dot = x * px + y * py + z * pz;
    Point3::new(
        origin.x() + (px * c + (y * pz - z * py) * s + x * dot * (1.0 - c)),
        origin.y() + (py * c + (z * px - x * pz) * s + y * dot * (1.0 - c)),
        origin.z() + (pz * c + (x * py - y * px) * s + z * dot * (1.0 - c)),
    )
    .map_err(|_| GeometryError::NonFiniteResult)
}

fn finite_point(p: Point3) -> bool {
    p.x().is_finite() && p.y().is_finite() && p.z().is_finite()
}
fn positive_dimensions(values: &[f64]) -> Result<(), GeometryError> {
    if values.iter().all(|v| v.is_finite() && *v > 0.0) {
        Ok(())
    } else {
        Err(GeometryError::DegenerateCurve)
    }
}
fn validate_resolution(value: usize) -> Result<(), GeometryError> {
    if (3..=MAX_SEGMENTS).contains(&value) {
        Ok(())
    } else {
        Err(GeometryError::DegenerateCurve)
    }
}
fn validate_ordered_bounds(min: Point3, max: Point3) -> Result<(), GeometryError> {
    if finite_point(min)
        && finite_point(max)
        && max.x() > min.x()
        && max.y() > min.y()
        && max.z() > min.z()
    {
        Ok(())
    } else {
        Err(GeometryError::DegenerateCurve)
    }
}
