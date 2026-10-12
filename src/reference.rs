//! Public external reference and raster image models.
//!
//! References keep their source path and loading state separate from drawing
//! entities.  Desktop integrations can attach them to an Entity and use the
//! manager to reload or detach sources without inventing a DWG importer.

use crate::{
    AnnotationBounds, BlockDefinition, DxfDocument, Entity, GeometryError, Insert, Point3,
    Transform,
};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct RasterImage {
    path: PathBuf,
    insertion: Point3,
    u_vector: Point3,
    v_vector: Point3,
    pixel_width: u32,
    pixel_height: u32,
    visible: bool,
    clip_boundary: Option<Vec<Point3>>,
    opacity: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterTriangle {
    pub positions: [Point3; 3],
    pub uvs: [[f64; 2]; 3],
}

impl RasterImage {
    pub fn new(
        path: impl Into<PathBuf>,
        insertion: Point3,
        u_vector: Point3,
        v_vector: Point3,
        pixel_width: u32,
        pixel_height: u32,
    ) -> Result<Self, GeometryError> {
        if pixel_width == 0 || pixel_height == 0 {
            return Err(GeometryError::NonPositiveScale);
        }
        if !finite_vector(u_vector) || !finite_vector(v_vector) {
            return Err(GeometryError::NonFiniteParameter);
        }
        if u_vector.x() == 0.0 && u_vector.y() == 0.0 && u_vector.z() == 0.0
            || v_vector.x() == 0.0 && v_vector.y() == 0.0 && v_vector.z() == 0.0
        {
            return Err(GeometryError::NonPositiveScale);
        }
        Ok(Self {
            path: path.into(),
            insertion,
            u_vector,
            v_vector,
            pixel_width,
            pixel_height,
            visible: true,
            clip_boundary: None,
            opacity: 1.0,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        self.path = path.into();
    }
    pub fn insertion(&self) -> Point3 {
        self.insertion
    }
    pub fn u_vector(&self) -> Point3 {
        self.u_vector
    }
    pub fn v_vector(&self) -> Point3 {
        self.v_vector
    }
    pub fn pixel_dimensions(&self) -> (u32, u32) {
        (self.pixel_width, self.pixel_height)
    }
    pub fn visible(&self) -> bool {
        self.visible
    }
    pub fn clip_boundary(&self) -> Option<&[Point3]> {
        self.clip_boundary.as_deref()
    }
    pub fn opacity(&self) -> f64 {
        self.opacity
    }
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
    pub fn set_opacity(&mut self, opacity: f64) -> Result<(), GeometryError> {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(GeometryError::NonFiniteParameter);
        }
        self.opacity = opacity;
        Ok(())
    }
    pub fn set_clip_boundary(
        &mut self,
        boundary: Option<Vec<Point3>>,
    ) -> Result<(), GeometryError> {
        if let Some(points) = &boundary {
            if points.len() < 3 {
                return Err(GeometryError::NotEnoughVertices);
            }
            if points.iter().any(|point| !finite_point(*point)) {
                return Err(GeometryError::NonFiniteCoordinate);
            }
        }
        self.clip_boundary = boundary;
        Ok(())
    }
    pub fn bounds(&self) -> Result<AnnotationBounds, GeometryError> {
        let points = if let Some(boundary) = &self.clip_boundary {
            boundary.clone()
        } else {
            vec![
                self.insertion,
                add(self.insertion, self.u_vector)?,
                add(self.insertion, self.v_vector)?,
                add(add(self.insertion, self.u_vector)?, self.v_vector)?,
            ]
        };
        AnnotationBounds::from_points(&points)
    }
    pub fn translated(&self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        let mut next = self.clone();
        next.insertion = Point3::new(
            self.insertion.x() + dx,
            self.insertion.y() + dy,
            self.insertion.z() + dz,
        )?;
        if let Some(boundary) = &self.clip_boundary {
            next.clip_boundary = Some(
                boundary
                    .iter()
                    .map(|p| Point3::new(p.x() + dx, p.y() + dy, p.z() + dz))
                    .collect::<Result<_, _>>()?,
            );
        }
        Ok(next)
    }
    pub fn transformed(&self, transform: Transform) -> Result<Self, GeometryError> {
        let mut next = self.clone();
        next.insertion = transform.point(self.insertion)?;
        next.u_vector = vector_difference(
            transform.point(add(self.insertion, self.u_vector)?)?,
            next.insertion,
        )?;
        next.v_vector = vector_difference(
            transform.point(add(self.insertion, self.v_vector)?)?,
            next.insertion,
        )?;
        if let Some(boundary) = &self.clip_boundary {
            next.clip_boundary = Some(
                boundary
                    .iter()
                    .copied()
                    .map(|p| transform.point(p))
                    .collect::<Result<_, _>>()?,
            );
        }
        Ok(next)
    }

    pub fn clipped_triangles(&self) -> Result<Vec<RasterTriangle>, GeometryError> {
        if !self.visible {
            return Ok(Vec::new());
        }
        let mut polygon = if let Some(boundary) = &self.clip_boundary {
            boundary
                .iter()
                .map(|point| self.project_uv(*point))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
        };
        if polygon.len() > 4096 {
            return Err(GeometryError::NonFiniteParameter);
        }
        clip_uv_polygon(&mut polygon);
        if polygon.len() < 3 {
            return Ok(Vec::new());
        }
        triangulate_uv(&polygon)?
            .into_iter()
            .map(|uvs| {
                Ok(RasterTriangle {
                    positions: [
                        self.world_from_uv(uvs[0])?,
                        self.world_from_uv(uvs[1])?,
                        self.world_from_uv(uvs[2])?,
                    ],
                    uvs,
                })
            })
            .collect()
    }
    fn project_uv(&self, point: Point3) -> Result<[f64; 2], GeometryError> {
        let d = vector_difference(point, self.insertion)?;
        let u = self.u_vector;
        let v = self.v_vector;
        let uu = dot3(u, u)?;
        let uv = dot3(u, v)?;
        let vv = dot3(v, v)?;
        let det = uu * vv - uv * uv;
        if !det.is_finite() || det.abs() <= f64::EPSILON {
            return Err(GeometryError::DegenerateLine);
        }
        if norm3(cross3(d, cross3(u, v)?)?)? > 1e-7 * (norm3(u)? + norm3(v)? + 1.0) {
            return Err(GeometryError::NonFiniteParameter);
        }
        Ok([
            (dot3(d, u)? * vv - dot3(d, v)? * uv) / det / self.pixel_width as f64,
            (dot3(d, v)? * uu - dot3(d, u)? * uv) / det / self.pixel_height as f64,
        ])
    }
    fn world_from_uv(&self, uv: [f64; 2]) -> Result<Point3, GeometryError> {
        Point3::new(
            self.insertion.x()
                + self.u_vector.x() * uv[0] * self.pixel_width as f64
                + self.v_vector.x() * uv[1] * self.pixel_height as f64,
            self.insertion.y()
                + self.u_vector.y() * uv[0] * self.pixel_width as f64
                + self.v_vector.y() * uv[1] * self.pixel_height as f64,
            self.insertion.z()
                + self.u_vector.z() * uv[0] * self.pixel_width as f64
                + self.v_vector.z() * uv[1] * self.pixel_height as f64,
        )
    }
}

pub(crate) fn clip_uv_polygon(polygon: &mut Vec<[f64; 2]>) {
    for (axis, lower) in [(0, true), (0, false), (1, true), (1, false)] {
        let input = std::mem::take(polygon);
        for (a, b) in input
            .iter()
            .copied()
            .zip(input.iter().copied().cycle().skip(1))
            .take(input.len())
        {
            let ain = if lower {
                a[axis] >= 0.0
            } else {
                a[axis] <= 1.0
            };
            let bin = if lower {
                b[axis] >= 0.0
            } else {
                b[axis] <= 1.0
            };
            if ain != bin {
                let edge = if lower { 0.0 } else { 1.0 };
                let t = (edge - a[axis]) / (b[axis] - a[axis]);
                let mut p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                p[axis] = edge;
                polygon.push(p);
            }
            if bin {
                polygon.push(b);
            }
        }
    }
}
pub(crate) fn triangulate_uv(p: &[[f64; 2]]) -> Result<Vec<[[f64; 2]; 3]>, GeometryError> {
    let mut ids: Vec<usize> = (0..p.len()).collect();
    let area: f64 = p
        .iter()
        .copied()
        .zip(p.iter().copied().cycle().skip(1))
        .take(p.len())
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum();
    if !area.is_finite() || area.abs() <= f64::EPSILON {
        return Err(GeometryError::DegenerateCurve);
    }
    if area < 0.0 {
        ids.reverse()
    }
    let mut out = Vec::new();
    let mut guard = 0;
    while ids.len() > 3 && guard < p.len() * p.len() {
        let mut found = None;
        for i in 0..ids.len() {
            let a = ids[(i + ids.len() - 1) % ids.len()];
            let b = ids[i];
            let c = ids[(i + 1) % ids.len()];
            if cross2(p[a], p[b], p[c]) <= f64::EPSILON {
                continue;
            }
            if ids
                .iter()
                .any(|j| *j != a && *j != b && *j != c && inside_triangle(p[*j], p[a], p[b], p[c]))
            {
                continue;
            }
            found = Some((i, [p[a], p[b], p[c]]));
            break;
        }
        let Some((i, t)) = found else {
            return Err(GeometryError::DegenerateCurve);
        };
        out.push(t);
        ids.remove(i);
        guard += 1
    }
    if ids.len() != 3 {
        return Err(GeometryError::DegenerateCurve);
    }
    out.push([p[ids[0]], p[ids[1]], p[ids[2]]]);
    Ok(out)
}
fn cross2(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn inside_triangle(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    cross2(a, b, p) >= 0.0 && cross2(b, c, p) >= 0.0 && cross2(c, a, p) >= 0.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalReferenceStatus {
    Loaded,
    Unloaded,
    Missing,
    Invalid,
}

#[derive(Clone, Debug)]
pub struct ExternalDxfReference {
    name: String,
    path: PathBuf,
    status: ExternalReferenceStatus,
    document: Option<DxfDocument>,
}
impl ExternalDxfReference {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn status(&self) -> ExternalReferenceStatus {
        self.status
    }
    pub fn document(&self) -> Option<&DxfDocument> {
        self.document.as_ref()
    }
    pub fn reload(&mut self) -> Result<(), String> {
        let source = fs::read_to_string(&self.path).map_err(|e| {
            self.status = ExternalReferenceStatus::Missing;
            e.to_string()
        })?;
        let document = DxfDocument::parse(&source).map_err(|e| {
            self.status = ExternalReferenceStatus::Invalid;
            e.to_string()
        })?;
        self.document = Some(document);
        self.status = ExternalReferenceStatus::Loaded;
        Ok(())
    }
    pub fn unload(&mut self) {
        self.document = None;
        self.status = ExternalReferenceStatus::Unloaded;
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReferenceManager {
    references: Vec<ExternalDxfReference>,
}
impl ReferenceManager {
    pub fn references(&self) -> &[ExternalDxfReference] {
        &self.references
    }
    pub fn attach(
        &mut self,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Result<(), String> {
        let name = name.into();
        if name.trim().is_empty()
            || self
                .references
                .iter()
                .any(|reference| reference.name.eq_ignore_ascii_case(&name))
        {
            return Err("Reference name is empty or already attached.".into());
        }
        let mut reference = ExternalDxfReference {
            name,
            path: path.into(),
            status: ExternalReferenceStatus::Unloaded,
            document: None,
        };
        reference.reload()?;
        self.references.push(reference);
        Ok(())
    }
    /// Restore a persisted reference without touching the filesystem. This is
    /// intentionally lazy so a drawing remains open when an xref is offline.
    pub fn attach_lazy(
        &mut self,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Result<(), String> {
        let name = name.into();
        if name.trim().is_empty()
            || self
                .references
                .iter()
                .any(|reference| reference.name.eq_ignore_ascii_case(&name))
        {
            return Err("Reference name is empty or already attached.".into());
        }
        self.references.push(ExternalDxfReference {
            name,
            path: path.into(),
            status: ExternalReferenceStatus::Unloaded,
            document: None,
        });
        Ok(())
    }
    pub fn get(&self, name: &str) -> Option<&ExternalDxfReference> {
        self.references
            .iter()
            .find(|reference| reference.name.eq_ignore_ascii_case(name))
    }
    pub fn reload(&mut self, name: &str) -> Result<(), String> {
        self.get_mut(name)?.reload()
    }
    pub fn unload(&mut self, name: &str) -> Result<(), String> {
        self.get_mut(name)?.unload();
        Ok(())
    }
    pub fn load(&mut self, name: &str) -> Result<(), String> {
        self.get_mut(name)?.reload()
    }
    pub fn bind(&mut self, name: &str) -> Result<DxfDocument, String> {
        let index = self
            .references
            .iter()
            .position(|reference| reference.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| "Reference is not attached.".to_owned())?;
        let reference = self.references.remove(index);
        reference
            .document
            .ok_or_else(|| "Reference must be loaded before binding.".to_owned())
    }
    /// Import the loaded reference into a drawing as an xref-marked BLOCK and
    /// a visible INSERT. The caller owns the drawing transaction boundary.
    pub fn attach_to_document(
        &mut self,
        name: &str,
        drawing: &mut DxfDocument,
        insertion: Point3,
    ) -> Result<(), String> {
        let reference = self
            .get(name)
            .ok_or_else(|| "Reference is not attached.".to_owned())?;
        let source = reference
            .document
            .as_ref()
            .ok_or_else(|| "Reference must be loaded before attaching.".to_owned())?;
        let mut definition = BlockDefinition::new(
            name,
            Point3::new(0.0, 0.0, 0.0).map_err(|e| e.to_string())?,
            source.entities().to_vec(),
        );
        definition.set_xref_source(Some(reference.path.to_string_lossy().into_owned()), false);
        if drawing.block(name).is_some() {
            drawing
                .replace_block_definition(definition)
                .map_err(|e| e.to_string())?;
        } else {
            drawing
                .add_block_definition(definition)
                .map_err(|e| e.to_string())?;
        }
        drawing
            .push_on_layer(Entity::Insert(Insert::new(name, insertion)), "0")
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    /// Refresh an already attached xref block and preserve its source path.
    pub fn reload_in_document(
        &mut self,
        name: &str,
        drawing: &mut DxfDocument,
    ) -> Result<(), String> {
        self.reload(name)?;
        let reference = self
            .get(name)
            .ok_or_else(|| "Reference is not attached.".to_owned())?;
        let source = reference
            .document
            .as_ref()
            .ok_or_else(|| "Reference must be loaded before attaching.".to_owned())?;
        let mut definition = drawing
            .block(name)
            .cloned()
            .ok_or_else(|| "Reference block is not present in the drawing.".to_owned())?;
        definition.set_entities(source.entities().to_vec());
        drawing
            .replace_block_definition(definition)
            .map_err(|e| e.to_string())
    }
    pub fn detach(&mut self, name: &str) -> Result<ExternalDxfReference, String> {
        let index = self
            .references
            .iter()
            .position(|reference| reference.name.eq_ignore_ascii_case(name))
            .ok_or("Reference is not attached.")?;
        Ok(self.references.remove(index))
    }
    fn get_mut(&mut self, name: &str) -> Result<&mut ExternalDxfReference, String> {
        self.references
            .iter_mut()
            .find(|reference| reference.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| "Reference is not attached.".into())
    }
}

fn finite_point(point: Point3) -> bool {
    point.x().is_finite() && point.y().is_finite() && point.z().is_finite()
}
fn finite_vector(point: Point3) -> bool {
    finite_point(point)
}
fn dot3(a: Point3, b: Point3) -> Result<f64, GeometryError> {
    let value = a.x() * b.x() + a.y() * b.y() + a.z() * b.z();
    value
        .is_finite()
        .then_some(value)
        .ok_or(GeometryError::NonFiniteResult)
}
fn cross3(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(
        a.y() * b.z() - a.z() * b.y(),
        a.z() * b.x() - a.x() * b.z(),
        a.x() * b.y() - a.y() * b.x(),
    )
}
fn norm3(value: Point3) -> Result<f64, GeometryError> {
    let squared = dot3(value, value)?;
    squared
        .sqrt()
        .is_finite()
        .then_some(squared.sqrt())
        .ok_or(GeometryError::NonFiniteResult)
}
fn add(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(a.x() + b.x(), a.y() + b.y(), a.z() + b.z())
}
fn vector_difference(a: Point3, b: Point3) -> Result<Point3, GeometryError> {
    Point3::new(a.x() - b.x(), a.y() - b.y(), a.z() - b.z())
}
