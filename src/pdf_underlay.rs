//! External PDF underlay references and page-space geometry.
//!
//! The model stores a source path and page placement. It does not rasterize or embed PDF data;
//! renderers may resolve the path and page independently.

use crate::{AnnotationBounds, GeometryError, Point3, Transform};
use std::f64::consts::TAU;
use std::path::{Path, PathBuf};

const MAX_CLIP_POINTS: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct PdfUnderlay {
    path: PathBuf,
    page: u32,
    page_size: [f64; 2],
    page_size_known: bool,
    insertion: Point3,
    scale: [f64; 2],
    rotation: f64,
    clip: Option<Vec<[f64; 2]>>,
    pending_page_clip: Option<Vec<[f64; 2]>>,
    clip_enabled: bool,
    display_enabled: bool,
    frame_mode: u8,
    fade: f64,
    contrast: f64,
    monochrome: bool,
}

impl PdfUnderlay {
    pub fn new(
        path: impl Into<PathBuf>,
        page: u32,
        page_size: [f64; 2],
        insertion: Point3,
    ) -> Result<Self, GeometryError> {
        let path = path.into();
        validate_path(&path)?;
        validate_page(page, page_size)?;
        Ok(Self {
            path,
            page,
            page_size,
            page_size_known: true,
            insertion,
            scale: [1.0, 1.0],
            rotation: 0.0,
            clip: None,
            pending_page_clip: None,
            clip_enabled: true,
            display_enabled: true,
            frame_mode: 1,
            fade: 0.0,
            contrast: 100.0,
            monochrome: false,
        })
    }
    /// Creates a native reference whose page dimensions must be resolved from the source.
    /// Geometry methods return `GeometryError::UnresolvedReference` until `set_page_size` runs.
    pub fn unresolved(
        path: impl Into<PathBuf>,
        page: u32,
        insertion: Point3,
    ) -> Result<Self, GeometryError> {
        let path = path.into();
        validate_path(&path)?;
        if page == 0 {
            return Err(GeometryError::NonPositiveScale);
        }
        Ok(Self {
            path,
            page,
            page_size: [0.0, 0.0],
            page_size_known: false,
            insertion,
            scale: [1.0, 1.0],
            rotation: 0.0,
            clip: None,
            pending_page_clip: None,
            clip_enabled: true,
            display_enabled: true,
            frame_mode: 1,
            fade: 0.0,
            contrast: 100.0,
            monochrome: false,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn set_path(&mut self, path: impl Into<PathBuf>) -> Result<(), GeometryError> {
        let path = path.into();
        validate_path(&path)?;
        self.path = path;
        Ok(())
    }
    pub fn page(&self) -> u32 {
        self.page
    }
    pub fn set_page(&mut self, page: u32) -> Result<(), GeometryError> {
        if page == 0 {
            return Err(GeometryError::NonPositiveScale);
        }
        self.page = page;
        Ok(())
    }
    pub fn page_size(&self) -> [f64; 2] {
        self.page_size
    }
    pub fn page_size_known(&self) -> bool {
        self.page_size_known
    }
    pub fn resolved_page_size(&self) -> Result<[f64; 2], GeometryError> {
        if !self.page_size_known {
            return Err(GeometryError::UnresolvedReference);
        }
        Ok(self.page_size)
    }
    pub fn set_page_size(&mut self, size: [f64; 2]) -> Result<(), GeometryError> {
        validate_page_dimensions(size)?;
        let resolved_clip = self.normalized_pending_clip(size)?;
        self.page_size = size;
        self.page_size_known = true;
        if let Some(clip) = resolved_clip {
            self.clip = Some(clip);
        }
        Ok(())
    }
    pub fn insertion(&self) -> Point3 {
        self.insertion
    }
    pub fn set_insertion(&mut self, point: Point3) {
        self.insertion = point;
    }
    pub fn scale(&self) -> [f64; 2] {
        self.scale
    }
    pub fn set_scale(&mut self, scale: [f64; 2]) -> Result<(), GeometryError> {
        validate_scale(scale)?;
        self.scale = scale;
        Ok(())
    }
    pub fn rotation(&self) -> f64 {
        self.rotation
    }
    pub fn set_rotation(&mut self, rotation: f64) -> Result<(), GeometryError> {
        if !rotation.is_finite() {
            return Err(GeometryError::NonFiniteAngle);
        }
        self.rotation = rotation.rem_euclid(TAU);
        Ok(())
    }
    pub fn clip_enabled(&self) -> bool {
        self.clip_enabled
    }
    pub fn set_clip_enabled(&mut self, enabled: bool) {
        self.clip_enabled = enabled;
    }
    pub fn display_enabled(&self) -> bool {
        self.display_enabled
    }
    pub fn set_display_enabled(&mut self, enabled: bool) {
        self.display_enabled = enabled;
    }
    pub fn frame_mode(&self) -> u8 {
        self.frame_mode
    }
    pub fn set_frame_mode(&mut self, mode: u8) -> Result<(), GeometryError> {
        if mode > 2 {
            return Err(GeometryError::NonFiniteParameter);
        }
        self.frame_mode = mode;
        Ok(())
    }
    pub fn fade(&self) -> f64 {
        self.fade
    }
    pub fn set_fade(&mut self, value: f64) -> Result<(), GeometryError> {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err(GeometryError::NonFiniteParameter);
        }
        self.fade = value;
        Ok(())
    }
    pub fn contrast(&self) -> f64 {
        self.contrast
    }
    pub fn set_contrast(&mut self, value: f64) -> Result<(), GeometryError> {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err(GeometryError::NonFiniteParameter);
        }
        self.contrast = value;
        Ok(())
    }
    pub fn monochrome(&self) -> bool {
        self.monochrome
    }
    pub fn set_monochrome(&mut self, enabled: bool) {
        self.monochrome = enabled;
    }
    pub fn clip(&self) -> Option<&[[f64; 2]]> {
        self.clip.as_deref()
    }
    pub fn pending_page_clip(&self) -> Option<&[[f64; 2]]> {
        self.pending_page_clip.as_deref()
    }
    pub fn set_page_clip(&mut self, clip: Option<Vec<[f64; 2]>>) -> Result<(), GeometryError> {
        if let Some(points) = &clip {
            validate_page_clip(points)?;
        }
        let resolved_clip = if self.page_size_known {
            self.normalized_page_clip(clip.as_deref())?
        } else {
            None
        };
        self.pending_page_clip = clip;
        if self.page_size_known {
            self.clip = resolved_clip;
        }
        Ok(())
    }
    pub fn set_clip(&mut self, clip: Option<Vec<[f64; 2]>>) -> Result<(), GeometryError> {
        if let Some(points) = &clip {
            validate_clip(points)?;
        }
        self.clip = clip;
        self.pending_page_clip = None;
        Ok(())
    }

    fn normalized_pending_clip(
        &self,
        size: [f64; 2],
    ) -> Result<Option<Vec<[f64; 2]>>, GeometryError> {
        self.normalized_page_clip_for_size(self.pending_page_clip.as_deref(), size)
    }
    fn normalized_page_clip(
        &self,
        points: Option<&[[f64; 2]]>,
    ) -> Result<Option<Vec<[f64; 2]>>, GeometryError> {
        self.normalized_page_clip_for_size(points, self.page_size)
    }
    fn normalized_page_clip_for_size(
        &self,
        points: Option<&[[f64; 2]]>,
        size: [f64; 2],
    ) -> Result<Option<Vec<[f64; 2]>>, GeometryError> {
        let Some(points) = points else {
            return Ok(None);
        };
        let [width, height] = size;
        let normalized = points
            .iter()
            .map(|[x, y]| [x / width, y / height])
            .collect::<Vec<_>>();
        validate_clip(&normalized)?;
        Ok(Some(normalized))
    }

    /// Returns the four page corners in world coordinates, counterclockwise from the page origin.
    pub fn page_corners_world(&self) -> Result<[Point3; 4], GeometryError> {
        let [w, h] = self.resolved_page_size()?;
        Ok([
            self.map_local([0.0, 0.0])?,
            self.map_local([w, 0.0])?,
            self.map_local([w, h])?,
            self.map_local([0.0, h])?,
        ])
    }
    /// Returns the active clip polygon in world coordinates, or the complete page rectangle.
    pub fn clipped_polygon(&self) -> Result<Vec<Point3>, GeometryError> {
        let [page_width, page_height] = self.resolved_page_size()?;
        if self.clip_enabled
            && let Some(clip) = &self.clip
        {
            return clip
                .iter()
                .map(|p| self.map_local([p[0] * page_width, p[1] * page_height]))
                .collect();
        }
        Ok(self.page_corners_world()?.to_vec())
    }
    pub fn bounds(&self) -> Result<AnnotationBounds, GeometryError> {
        AnnotationBounds::from_points(&self.clipped_polygon()?)
    }
    /// Maps a world point to normalized page coordinates (before clipping).
    pub fn inverse_local(&self, world: Point3) -> Result<[f64; 2], GeometryError> {
        let [page_width, page_height] = self.resolved_page_size()?;
        let dx = world.x() - self.insertion.x();
        let dy = world.y() - self.insertion.y();
        let (sin, cos) = self.rotation.sin_cos();
        let x = (cos * dx + sin * dy) / self.scale[0];
        let y = (-sin * dx + cos * dy) / self.scale[1];
        let u = x / page_width;
        let v = y / page_height;
        if u.is_finite() && v.is_finite() {
            Ok([u, v])
        } else {
            Err(GeometryError::NonFiniteResult)
        }
    }
    pub fn translated(&self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        let mut out = self.clone();
        out.insertion = Point3::new(
            self.insertion.x() + dx,
            self.insertion.y() + dy,
            self.insertion.z() + dz,
        )?;
        Ok(out)
    }
    pub fn transformed(&self, transform: Transform) -> Result<Self, GeometryError> {
        let [page_width, page_height] = self.resolved_page_size()?;
        let mut out = self.clone();
        let origin = transform.point(self.insertion)?;
        let x = transform.point(self.map_local([page_width, 0.0])?)?;
        let y = transform.point(self.map_local([0.0, page_height])?)?;
        out.insertion = origin;
        out.set_basis(x, y)?;
        Ok(out)
    }
    fn map_local(&self, local: [f64; 2]) -> Result<Point3, GeometryError> {
        let x = local[0] * self.scale[0];
        let y = local[1] * self.scale[1];
        let (sin, cos) = self.rotation.sin_cos();
        Point3::new(
            self.insertion.x() + cos.mul_add(x, -sin * y),
            self.insertion.y() + sin.mul_add(x, cos * y),
            self.insertion.z(),
        )
    }
    fn set_basis(&mut self, x: Point3, y: Point3) -> Result<(), GeometryError> {
        let [page_width, page_height] = self.resolved_page_size()?;
        let ux = x.x() - self.insertion.x();
        let uy = x.y() - self.insertion.y();
        let vx = y.x() - self.insertion.x();
        let vy = y.y() - self.insertion.y();
        let sx = (ux * ux + uy * uy).sqrt() / page_width;
        let sy = (vx * vx + vy * vy).sqrt() / page_height;
        let dot = ux * vx + uy * vy;
        let norm = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        if !sx.is_finite()
            || !sy.is_finite()
            || sx == 0.0
            || sy == 0.0
            || !dot.is_finite()
            || dot.abs() > norm * 1.0e-7
        {
            return Err(GeometryError::NonPositiveScale);
        }
        self.scale = [sx.copysign(ux * vy - uy * vx), sy];
        self.rotation = uy.atan2(ux).rem_euclid(TAU);
        Ok(())
    }
}
fn validate_path(path: &Path) -> Result<(), GeometryError> {
    let text = path.to_string_lossy();
    if text.trim().is_empty() || text.contains('\0') || text.contains('\r') || text.contains('\n') {
        Err(GeometryError::NotEnoughVertices)
    } else {
        Ok(())
    }
}
fn validate_page(page: u32, size: [f64; 2]) -> Result<(), GeometryError> {
    if page == 0 {
        Err(GeometryError::NonPositiveScale)
    } else {
        validate_page_dimensions(size)
    }
}
fn validate_page_dimensions(size: [f64; 2]) -> Result<(), GeometryError> {
    if size.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        Err(GeometryError::NonPositiveScale)
    } else {
        Ok(())
    }
}
fn validate_scale(scale: [f64; 2]) -> Result<(), GeometryError> {
    if scale.iter().any(|v| !v.is_finite() || *v == 0.0) {
        Err(GeometryError::NonPositiveScale)
    } else {
        Ok(())
    }
}
fn validate_clip(points: &[[f64; 2]]) -> Result<(), GeometryError> {
    if points.len() < 3
        || points.len() > MAX_CLIP_POINTS
        || points
            .iter()
            .any(|p| p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)))
    {
        return Err(GeometryError::NonFiniteCoordinate);
    }
    let area: f64 = points
        .windows(2)
        .map(|p| p[0][0] * p[1][1] - p[1][0] * p[0][1])
        .sum::<f64>()
        + points.last().unwrap()[0] * points[0][1]
        - points[0][0] * points.last().unwrap()[1];
    if area.abs() <= f64::EPSILON {
        Err(GeometryError::DegenerateCurve)
    } else {
        Ok(())
    }
}
fn validate_page_clip(points: &[[f64; 2]]) -> Result<(), GeometryError> {
    if points.len() < 3
        || points.len() > MAX_CLIP_POINTS
        || points
            .iter()
            .any(|p| p.iter().any(|v| !v.is_finite() || *v < 0.0))
    {
        return Err(GeometryError::NonFiniteCoordinate);
    }
    let area: f64 = points
        .windows(2)
        .map(|p| p[0][0] * p[1][1] - p[1][0] * p[0][1])
        .sum::<f64>()
        + points.last().unwrap()[0] * points[0][1]
        - points[0][0] * points.last().unwrap()[1];
    if area.abs() <= f64::EPSILON {
        Err(GeometryError::DegenerateCurve)
    } else {
        Ok(())
    }
}
