//! Plot and preview clipping for raster-backed references.
//!
//! RasterImage's historical projection helper has different pixel-vector
//! assumptions. This helper treats U and V as the complete placed extents.

use crate::{GeometryError, Point3, RasterTriangle};

const MAX_CLIP_VERTICES: usize = 4096;
const MAX_NORMALIZED_COORDINATE: f64 = 1.0e9;

pub(crate) fn clipped_triangles(
    insertion: Point3,
    u_vector: Point3,
    v_vector: Point3,
    clip_boundary: Option<&[Point3]>,
) -> Result<Vec<RasterTriangle>, GeometryError> {
    let basis = ImageBasis::new(u_vector, v_vector)?;
    let mut polygon = normalized_clip_polygon(insertion, &basis, clip_boundary)?;
    crate::reference::clip_uv_polygon(&mut polygon);
    if polygon.len() < 3 {
        return Ok(Vec::new());
    }
    validate_clipped_polygon(&polygon)?;

    crate::reference::triangulate_uv(&polygon)?
        .into_iter()
        .map(|uvs| {
            Ok(RasterTriangle {
                positions: [
                    world_point(insertion, u_vector, v_vector, uvs[0])?,
                    world_point(insertion, u_vector, v_vector, uvs[1])?,
                    world_point(insertion, u_vector, v_vector, uvs[2])?,
                ],
                uvs,
            })
        })
        .collect()
}

struct ImageBasis {
    u: Point3,
    v: Point3,
    u_squared: f64,
    v_squared: f64,
    uv: f64,
    determinant: f64,
    normal: Point3,
    normal_length: f64,
    plane_tolerance: f64,
}

impl ImageBasis {
    fn new(u: Point3, v: Point3) -> Result<Self, GeometryError> {
        let u_squared = dot(u, u)?;
        let v_squared = dot(v, v)?;
        if u_squared <= 0.0 || v_squared <= 0.0 {
            return Err(GeometryError::DegenerateLine);
        }

        let uv = dot(u, v)?;
        let determinant_scale = u_squared * v_squared;
        let determinant = determinant_scale - uv * uv;
        if !determinant.is_finite()
            || !determinant_scale.is_finite()
            || determinant <= f64::EPSILON * determinant_scale
        {
            return Err(GeometryError::DegenerateLine);
        }

        let normal = cross(u, v)?;
        let normal_length = dot(normal, normal)?.sqrt();
        let plane_tolerance = 1e-7 * (u_squared.sqrt() + v_squared.sqrt() + 1.0);
        if !normal_length.is_finite() || normal_length <= 0.0 || !plane_tolerance.is_finite() {
            return Err(GeometryError::DegenerateLine);
        }

        Ok(Self {
            u,
            v,
            u_squared,
            v_squared,
            uv,
            determinant,
            normal,
            normal_length,
            plane_tolerance,
        })
    }

    fn project(&self, origin: Point3, point: Point3) -> Result<[f64; 2], GeometryError> {
        let delta = subtract(point, origin)?;
        let distance_from_plane = dot(delta, self.normal)?.abs() / self.normal_length;
        if !distance_from_plane.is_finite() || distance_from_plane > self.plane_tolerance {
            return Err(GeometryError::NonFiniteParameter);
        }

        let along_u = dot(delta, self.u)?;
        let along_v = dot(delta, self.v)?;
        let coordinates = [
            (along_u * self.v_squared - along_v * self.uv) / self.determinant,
            (along_v * self.u_squared - along_u * self.uv) / self.determinant,
        ];
        if coordinates
            .iter()
            .any(|value| !value.is_finite() || value.abs() > MAX_NORMALIZED_COORDINATE)
        {
            return Err(GeometryError::NonFiniteParameter);
        }
        Ok(coordinates)
    }
}

fn normalized_clip_polygon(
    insertion: Point3,
    basis: &ImageBasis,
    clip_boundary: Option<&[Point3]>,
) -> Result<Vec<[f64; 2]>, GeometryError> {
    let Some(boundary) = clip_boundary else {
        return Ok(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    };
    if boundary.len() > MAX_CLIP_VERTICES {
        return Err(GeometryError::NonFiniteParameter);
    }
    boundary
        .iter()
        .map(|point| basis.project(insertion, *point))
        .collect()
}

fn validate_clipped_polygon(polygon: &[[f64; 2]]) -> Result<(), GeometryError> {
    if polygon.len() > MAX_CLIP_VERTICES
        || polygon
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || value.abs() > 1.0)
    {
        return Err(GeometryError::NonFiniteParameter);
    }
    Ok(())
}

fn world_point(
    insertion: Point3,
    u_vector: Point3,
    v_vector: Point3,
    uv: [f64; 2],
) -> Result<Point3, GeometryError> {
    Point3::new(
        insertion.x() + u_vector.x() * uv[0] + v_vector.x() * uv[1],
        insertion.y() + u_vector.y() * uv[0] + v_vector.y() * uv[1],
        insertion.z() + u_vector.z() * uv[0] + v_vector.z() * uv[1],
    )
}

fn subtract(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(a.x() - b.x(), a.y() - b.y(), a.z() - b.z())
}

fn dot(a: Point3, b: Point3) -> Result<f64, GeometryError> {
    let value = a.x() * b.x() + a.y() * b.y() + a.z() * b.z();
    value
        .is_finite()
        .then_some(value)
        .ok_or(GeometryError::NonFiniteResult)
}

fn cross(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(
        a.y() * b.z() - a.z() * b.y(),
        a.z() * b.x() - a.x() * b.z(),
        a.x() * b.y() - a.y() * b.x(),
    )
}
