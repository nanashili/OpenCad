//! Validated operations for tessellated closed meshes.
//!
//! Boolean operations use bounded polygon clipping over validated closed meshes. Inputs
//! are welded at a scale-aware tolerance before clipping so tessellation seams remain
//! topologically closed.

use crate::{Face3, Mesh3, Point3, Transform};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshBooleanOp {
    Union,
    Subtract,
    Intersect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshOperationError {
    EmptyMesh,
    NonFiniteCoordinate,
    DegenerateFace,
    OpenMesh,
    NonManifoldEdge,
    InconsistentOrientation,
    DegenerateMesh,
    TooManyFaces,
    TooComplex,
    InvalidSubdivision,
}

impl std::fmt::Display for MeshOperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl std::error::Error for MeshOperationError {}

const EPS: f64 = 1.0e-9;
const MAX_FACES: usize = 2_000_000;
const MAX_CSG_DEPTH: usize = 128;
const MAX_CSG_WORK: usize = 8_000_000;

pub fn validate_closed_mesh(mesh: &Mesh3) -> Result<(), MeshOperationError> {
    if mesh.faces().is_empty() {
        return Err(MeshOperationError::EmptyMesh);
    }
    let tolerance = mesh_tolerance(mesh)?;
    let mut edges: HashMap<EdgeKey, (usize, i8)> = HashMap::new();
    let mut volume = 0.0;
    for face in mesh.faces() {
        let (vertices, count) = face.vertices();
        let p = &vertices[..count];
        if p.iter().any(|v| !finite(*v)) {
            return Err(MeshOperationError::NonFiniteCoordinate);
        }
        if triangle_area(p[0], p[1], p[2]) <= EPS
            || count == 4 && triangle_area(p[0], p[2], p[3]) <= EPS
        {
            return Err(MeshOperationError::DegenerateFace);
        }
        volume += tetra_volume(p[0], p[1], p[2]);
        if count == 4 {
            volume += tetra_volume(p[0], p[2], p[3]);
        }
        for i in 0..count {
            let a = p[i];
            let b = p[(i + 1) % count];
            let key = EdgeKey::new(a, b, tolerance);
            let direction = if key.forward(a, b, tolerance) { 1 } else { -1 };
            let entry = edges.entry(key).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += direction;
        }
    }
    if edges.values().any(|(count, _)| *count != 2) {
        return Err(MeshOperationError::NonManifoldEdge);
    }
    if edges.values().any(|(_, orientation)| *orientation != 0) {
        return Err(MeshOperationError::InconsistentOrientation);
    }
    if volume.abs() <= EPS {
        return Err(MeshOperationError::DegenerateMesh);
    }
    Ok(())
}

pub fn mesh_area(mesh: &Mesh3) -> Result<f64, MeshOperationError> {
    validate_closed_mesh(mesh)?;
    Ok(mesh
        .faces()
        .iter()
        .map(|face| {
            let (v, n) = face.vertices();
            triangle_area(v[0], v[1], v[2])
                + if n == 4 {
                    triangle_area(v[0], v[2], v[3])
                } else {
                    0.0
                }
        })
        .sum())
}

pub fn mesh_volume(mesh: &Mesh3) -> Result<f64, MeshOperationError> {
    validate_closed_mesh(mesh)?;
    let signed = mesh
        .faces()
        .iter()
        .map(|face| {
            let (v, n) = face.vertices();
            tetra_volume(v[0], v[1], v[2])
                + if n == 4 {
                    tetra_volume(v[0], v[2], v[3])
                } else {
                    0.0
                }
        })
        .sum::<f64>();
    Ok(signed.abs())
}

pub fn boundary_edges(mesh: &Mesh3) -> Result<Vec<(Point3, Point3)>, MeshOperationError> {
    if mesh.faces().is_empty() {
        return Err(MeshOperationError::EmptyMesh);
    }
    let tolerance = mesh_tolerance(mesh)?;
    let mut edges: HashMap<EdgeKey, (Point3, Point3, usize)> = HashMap::new();
    for face in mesh.faces() {
        let (v, n) = face.vertices();
        for i in 0..n {
            let a = v[i];
            let b = v[(i + 1) % n];
            let entry = edges
                .entry(EdgeKey::new(a, b, tolerance))
                .or_insert((a, b, 0));
            entry.2 += 1;
        }
    }
    Ok(edges
        .into_values()
        .filter_map(|(a, b, count)| (count == 1).then_some((a, b)))
        .collect())
}

pub fn triangulate_mesh(mesh: &Mesh3) -> Result<Mesh3, MeshOperationError> {
    let mut faces = Vec::new();
    for face in mesh.faces() {
        let (v, n) = face.vertices();
        faces.push(
            Face3::triangle(v[0], v[1], v[2]).map_err(|_| MeshOperationError::DegenerateFace)?,
        );
        if n == 4 {
            faces.push(
                Face3::triangle(v[0], v[2], v[3])
                    .map_err(|_| MeshOperationError::DegenerateFace)?,
            );
        }
    }
    Mesh3::new(faces).map_err(|_| MeshOperationError::DegenerateMesh)
}

pub fn subdivide_mesh(mesh: &Mesh3, levels: usize) -> Result<Mesh3, MeshOperationError> {
    if levels > 8 {
        return Err(MeshOperationError::InvalidSubdivision);
    }
    let mut out = triangulate_mesh(mesh)?;
    for _ in 0..levels {
        let mut faces = Vec::with_capacity(out.faces().len() * 4);
        for face in out.faces() {
            let (v, _) = face.vertices();
            let a = v[0];
            let b = v[1];
            let c = v[2];
            let ab = midpoint(a, b)?;
            let bc = midpoint(b, c)?;
            let ca = midpoint(c, a)?;
            faces.extend(
                [
                    Face3::triangle(a, ab, ca),
                    Face3::triangle(ab, b, bc),
                    Face3::triangle(ca, bc, c),
                    Face3::triangle(ab, bc, ca),
                ]
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| MeshOperationError::DegenerateFace)?,
            );
            if faces.len() > MAX_FACES {
                return Err(MeshOperationError::TooManyFaces);
            }
        }
        out = Mesh3::new(faces).map_err(|_| MeshOperationError::DegenerateMesh)?;
    }
    Ok(out)
}

pub fn transform_mesh(mesh: &Mesh3, transform: Transform) -> Result<Mesh3, MeshOperationError> {
    let faces = mesh
        .faces()
        .iter()
        .map(|face| {
            let (mut v, n) = face.vertices();
            for p in v.iter_mut().take(n) {
                *p = transform
                    .point(*p)
                    .map_err(|_| MeshOperationError::NonFiniteCoordinate)?;
            }
            Face3::new(v, n as u8, face.invisible_edges())
                .map_err(|_| MeshOperationError::DegenerateFace)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Mesh3::new(faces).map_err(|_| MeshOperationError::DegenerateMesh)
}

pub fn scale_mesh(mesh: &Mesh3, scale: [f64; 3]) -> Result<Mesh3, MeshOperationError> {
    if scale.iter().any(|v| !v.is_finite() || *v == 0.0) {
        return Err(MeshOperationError::NonFiniteCoordinate);
    }
    let origin = Point3::new(0.0, 0.0, 0.0).map_err(|_| MeshOperationError::NonFiniteCoordinate)?;
    let t =
        Transform::scale(origin, scale[0]).map_err(|_| MeshOperationError::NonFiniteCoordinate)?;
    if scale[0] == scale[1] && scale[1] == scale[2] {
        return transform_mesh(mesh, t);
    }
    let faces = mesh
        .faces()
        .iter()
        .map(|face| {
            let (mut v, n) = face.vertices();
            for p in v.iter_mut().take(n) {
                *p = Point3::new(p.x() * scale[0], p.y() * scale[1], p.z() * scale[2])
                    .map_err(|_| MeshOperationError::NonFiniteCoordinate)?;
            }
            Face3::new(v, n as u8, face.invisible_edges())
                .map_err(|_| MeshOperationError::DegenerateFace)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Mesh3::new(faces).map_err(|_| MeshOperationError::DegenerateMesh)
}

fn mesh_tolerance(mesh: &Mesh3) -> Result<f64, MeshOperationError> {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for face in mesh.faces() {
        let (vertices, count) = face.vertices();
        for p in vertices.iter().take(count) {
            if !finite(*p) {
                return Err(MeshOperationError::NonFiniteCoordinate);
            }
            min[0] = min[0].min(p.x());
            min[1] = min[1].min(p.y());
            min[2] = min[2].min(p.z());
            max[0] = max[0].max(p.x());
            max[1] = max[1].max(p.y());
            max[2] = max[2].max(p.z());
        }
    }
    let diagonal =
        ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2)).sqrt();
    if !diagonal.is_finite() || diagonal <= 0.0 {
        return Err(MeshOperationError::DegenerateMesh);
    }
    Ok((diagonal * 1.0e-8).max(1.0e-10))
}

fn normalize_mesh(mesh: &Mesh3) -> Result<Mesh3, MeshOperationError> {
    if mesh.faces().is_empty() {
        return Err(MeshOperationError::EmptyMesh);
    }
    let tolerance = mesh_tolerance(mesh)?;
    let mut vertices = HashMap::<VertexKey, Point3>::new();
    let mut faces = Vec::with_capacity(mesh.faces().len());
    let mut signed_volume = 0.0;
    for face in mesh.faces() {
        let (raw, count) = face.vertices();
        let mut v = raw;
        for p in v.iter_mut().take(count) {
            let key = VertexKey::new(*p, tolerance);
            *p = *vertices.entry(key).or_insert(*p);
        }
        let area = triangle_area(v[0], v[1], v[2]);
        if count == 3 && area <= tolerance * tolerance {
            continue;
        }
        if area <= tolerance * tolerance
            || count == 4 && triangle_area(v[0], v[2], v[3]) <= tolerance * tolerance
        {
            return Err(MeshOperationError::DegenerateFace);
        }
        signed_volume += tetra_volume(v[0], v[1], v[2]);
        if count == 4 {
            signed_volume += tetra_volume(v[0], v[2], v[3]);
        }
        faces.push(
            Face3::new(v, count as u8, face.invisible_edges())
                .map_err(|_| MeshOperationError::DegenerateFace)?,
        );
    }
    if faces.is_empty() {
        return Err(MeshOperationError::DegenerateMesh);
    }
    // Imported mesh serializers sometimes reverse individual clockwise triangles.
    // Repair orientation from shared-edge adjacency before checking closure. This
    // preserves the actual surface while rejecting non-manifold edge incidence.
    let mut adjacency: HashMap<EdgeKey, Vec<(usize, bool)>> = HashMap::new();
    for (index, face) in faces.iter().enumerate() {
        let (vertices, count) = face.vertices();
        for edge in 0..count {
            let a = vertices[edge];
            let b = vertices[(edge + 1) % count];
            let key = EdgeKey::new(a, b, tolerance);
            let direction = key.forward(a, b, tolerance);
            let incident = adjacency.entry(key).or_default();
            incident.push((index, direction));
            if incident.len() > 2 {
                return Err(MeshOperationError::NonManifoldEdge);
            }
        }
    }
    let mut flipped = vec![None; faces.len()];
    for seed in 0..faces.len() {
        if flipped[seed].is_some() {
            continue;
        }
        flipped[seed] = Some(false);
        let mut stack = vec![seed];
        while let Some(index) = stack.pop() {
            let (vertices, count) = faces[index].vertices();
            let current_flip = flipped[index].unwrap_or(false);
            for edge in 0..count {
                let key = EdgeKey::new(vertices[edge], vertices[(edge + 1) % count], tolerance);
                let direction =
                    key.forward(vertices[edge], vertices[(edge + 1) % count], tolerance);
                let Some(neighbors) = adjacency.get(&key) else {
                    continue;
                };
                for &(neighbor, neighbor_direction) in neighbors {
                    if neighbor == index {
                        continue;
                    }
                    let desired = current_flip ^ (direction == neighbor_direction);
                    match flipped[neighbor] {
                        Some(existing) if existing != desired => {
                            return Err(MeshOperationError::InconsistentOrientation);
                        }
                        Some(_) => {}
                        None => {
                            flipped[neighbor] = Some(desired);
                            stack.push(neighbor);
                        }
                    }
                }
            }
        }
    }
    let mut repaired = Vec::with_capacity(faces.len());
    for (index, face) in faces.into_iter().enumerate() {
        let (mut vertices, count) = face.vertices();
        if flipped[index].unwrap_or(false) {
            vertices[..count].reverse();
        }
        repaired.push(
            Face3::new(vertices, count as u8, face.invisible_edges())
                .map_err(|_| MeshOperationError::DegenerateFace)?,
        );
    }
    faces = repaired;
    if signed_volume < 0.0 {
        for face in &mut faces {
            let (mut vertices, count) = face.vertices();
            vertices[..count].reverse();
            let invisible_edges = face.invisible_edges();
            *face = Face3::new(vertices, count as u8, invisible_edges)
                .map_err(|_| MeshOperationError::DegenerateFace)?;
        }
    }
    Mesh3::new(faces).map_err(|_| MeshOperationError::DegenerateMesh)
}

pub fn mesh_boolean(a: &Mesh3, b: &Mesh3, op: MeshBooleanOp) -> Result<Mesh3, MeshOperationError> {
    let a = normalize_mesh(a)?;
    let b = normalize_mesh(b)?;
    validate_closed_mesh(&a)?;
    validate_closed_mesh(&b)?;
    let mut budget = WorkBudget::default();
    let mut left = CsgNode::from_mesh(&a, &mut budget)?;
    let mut right = CsgNode::from_mesh(&b, &mut budget)?;
    match op {
        MeshBooleanOp::Union => {
            left.clip_to(&mut right, &mut budget)?;
            right.clip_to(&mut left, &mut budget)?;
            right.invert(&mut budget, 0)?;
            right.clip_to(&mut left, &mut budget)?;
            right.invert(&mut budget, 0)?;
            left.build(right.take_polygons(&mut budget)?, 0, &mut budget)?;
        }
        MeshBooleanOp::Subtract => {
            left.invert(&mut budget, 0)?;
            left.clip_to(&mut right, &mut budget)?;
            right.clip_to(&mut left, &mut budget)?;
            right.invert(&mut budget, 0)?;
            right.clip_to(&mut left, &mut budget)?;
            right.invert(&mut budget, 0)?;
            left.build(right.take_polygons(&mut budget)?, 0, &mut budget)?;
            left.invert(&mut budget, 0)?;
        }
        MeshBooleanOp::Intersect => {
            left.invert(&mut budget, 0)?;
            right.clip_to(&mut left, &mut budget)?;
            right.invert(&mut budget, 0)?;
            left.clip_to(&mut right, &mut budget)?;
            right.clip_to(&mut left, &mut budget)?;
            left.build(right.take_polygons(&mut budget)?, 0, &mut budget)?;
            left.invert(&mut budget, 0)?;
        }
    }
    let result = node_to_mesh(left, &mut budget)?;
    let normalized = normalize_mesh(&result)?;
    validate_closed_mesh(&normalized)?;
    Ok(normalized)
}

#[derive(Default)]
struct WorkBudget {
    used: usize,
}
impl WorkBudget {
    fn charge(&mut self, amount: usize) -> Result<(), MeshOperationError> {
        self.used = self
            .used
            .checked_add(amount)
            .ok_or(MeshOperationError::TooComplex)?;
        if self.used > MAX_CSG_WORK {
            Err(MeshOperationError::TooComplex)
        } else {
            Ok(())
        }
    }
    fn depth(depth: usize) -> Result<(), MeshOperationError> {
        (depth <= MAX_CSG_DEPTH)
            .then_some(())
            .ok_or(MeshOperationError::TooComplex)
    }
}

#[derive(Clone, Copy, Debug)]
struct CsgPlane {
    normal: Point3,
    w: f64,
}
#[derive(Clone, Debug)]
struct CsgPolygon {
    vertices: Vec<Point3>,
    plane: CsgPlane,
}
#[derive(Clone, Debug, Default)]
struct CsgNode {
    plane: Option<CsgPlane>,
    polygons: Vec<CsgPolygon>,
    front: Option<Box<CsgNode>>,
    back: Option<Box<CsgNode>>,
}

const COPLANAR: i8 = 0;
const FRONT: i8 = 1;
const BACK: i8 = 2;
const SPANNING: i8 = 3;

impl CsgNode {
    fn from_mesh(mesh: &Mesh3, budget: &mut WorkBudget) -> Result<Self, MeshOperationError> {
        let mut polygons = Vec::new();
        budget.charge(mesh.faces().len())?;
        for face in mesh.faces() {
            let (v, n) = face.vertices();
            let vertices = v[..n].to_vec();
            polygons.push(CsgPolygon {
                plane: make_plane(&vertices)?,
                vertices,
            });
        }
        let mut node = Self::default();
        node.build(polygons, 0, budget)?;
        Ok(node)
    }
    fn build(
        &mut self,
        polygons: Vec<CsgPolygon>,
        depth: usize,
        budget: &mut WorkBudget,
    ) -> Result<(), MeshOperationError> {
        if polygons.is_empty() {
            return Ok(());
        }
        WorkBudget::depth(depth)?;
        budget.charge(polygons.len())?;
        if self.plane.is_none() {
            self.plane = Some(polygons[0].plane);
        }
        let plane = self.plane.ok_or(MeshOperationError::DegenerateMesh)?;
        let mut front = Vec::new();
        let mut back = Vec::new();
        for polygon in polygons {
            split_polygon(plane, polygon, &mut self.polygons, &mut front, &mut back)?;
            if self.polygons.len() + front.len() + back.len() > MAX_FACES {
                return Err(MeshOperationError::TooManyFaces);
            }
        }
        if !front.is_empty() {
            self.front
                .get_or_insert_with(|| Box::new(Self::default()))
                .build(front, depth + 1, budget)?;
        }
        if !back.is_empty() {
            self.back
                .get_or_insert_with(|| Box::new(Self::default()))
                .build(back, depth + 1, budget)?;
        }
        Ok(())
    }
    fn all_polygons(&self, budget: &mut WorkBudget) -> Result<Vec<CsgPolygon>, MeshOperationError> {
        let mut result = Vec::new();
        self.collect_polygons(&mut result, 0, budget)?;
        Ok(result)
    }
    fn collect_polygons(
        &self,
        result: &mut Vec<CsgPolygon>,
        depth: usize,
        budget: &mut WorkBudget,
    ) -> Result<(), MeshOperationError> {
        WorkBudget::depth(depth)?;
        budget.charge(self.polygons.len())?;
        result.extend(self.polygons.iter().cloned());
        if let Some(front) = &self.front {
            front.collect_polygons(result, depth + 1, budget)?;
        }
        if let Some(back) = &self.back {
            back.collect_polygons(result, depth + 1, budget)?;
        }
        Ok(())
    }
    fn take_polygons(
        &mut self,
        budget: &mut WorkBudget,
    ) -> Result<Vec<CsgPolygon>, MeshOperationError> {
        self.all_polygons(budget)
    }
    fn clip_to(
        &mut self,
        other: &mut CsgNode,
        budget: &mut WorkBudget,
    ) -> Result<(), MeshOperationError> {
        let polygons = other.clip_polygons(self.all_polygons(budget)?, 0, budget)?;
        self.polygons = Vec::new();
        self.front = None;
        self.back = None;
        self.plane = None;
        self.build(polygons, 0, budget)
    }
    fn clip_polygons(
        &self,
        polygons: Vec<CsgPolygon>,
        depth: usize,
        budget: &mut WorkBudget,
    ) -> Result<Vec<CsgPolygon>, MeshOperationError> {
        WorkBudget::depth(depth)?;
        budget.charge(polygons.len())?;
        let Some(plane) = self.plane else {
            return Ok(polygons);
        };
        let mut front = Vec::new();
        let mut back = Vec::new();
        for polygon in polygons {
            let mut coplanar = Vec::new();
            split_polygon(plane, polygon, &mut coplanar, &mut front, &mut back)?;
            for polygon in coplanar {
                if dot(plane.normal, polygon.plane.normal) >= 0.0 {
                    front.push(polygon);
                } else {
                    back.push(polygon);
                }
            }
        }
        let mut result = if let Some(node) = &self.front {
            node.clip_polygons(front, depth + 1, budget)?
        } else {
            front
        };
        if let Some(node) = &self.back {
            result.extend(node.clip_polygons(back, depth + 1, budget)?);
        }
        Ok(result)
    }
    fn invert(&mut self, budget: &mut WorkBudget, depth: usize) -> Result<(), MeshOperationError> {
        WorkBudget::depth(depth)?;
        budget.charge(self.polygons.len().saturating_add(1))?;
        for polygon in &mut self.polygons {
            polygon.vertices.reverse();
            polygon.plane = invert_plane(polygon.plane);
        }
        if let Some(front) = &mut self.front {
            front.invert(budget, depth + 1)?;
        }
        if let Some(back) = &mut self.back {
            back.invert(budget, depth + 1)?;
        }
        std::mem::swap(&mut self.front, &mut self.back);
        self.plane = self.plane.map(invert_plane);
        Ok(())
    }
}

fn split_polygon(
    plane: CsgPlane,
    polygon: CsgPolygon,
    coplanar: &mut Vec<CsgPolygon>,
    front: &mut Vec<CsgPolygon>,
    back: &mut Vec<CsgPolygon>,
) -> Result<(), MeshOperationError> {
    let mut types = Vec::with_capacity(polygon.vertices.len());
    let mut has_front = false;
    let mut has_back = false;
    for p in &polygon.vertices {
        let distance = dot(plane.normal, *p) - plane.w;
        let kind = if distance > EPS {
            has_front = true;
            FRONT
        } else if distance < -EPS {
            has_back = true;
            BACK
        } else {
            COPLANAR
        };
        types.push(kind);
    }
    let kind = if has_front && has_back {
        SPANNING
    } else if has_front {
        FRONT
    } else if has_back {
        BACK
    } else {
        COPLANAR
    };
    if kind == COPLANAR {
        coplanar.push(polygon);
        return Ok(());
    }
    if kind == FRONT {
        front.push(polygon);
        return Ok(());
    }
    if kind == BACK {
        back.push(polygon);
        return Ok(());
    }
    let mut front_vertices = Vec::new();
    let mut back_vertices = Vec::new();
    for i in 0..polygon.vertices.len() {
        let j = (i + 1) % polygon.vertices.len();
        let a = polygon.vertices[i];
        let b = polygon.vertices[j];
        let ta = types[i];
        let tb = types[j];
        if ta != BACK {
            front_vertices.push(a);
        }
        if ta != FRONT {
            back_vertices.push(a);
        }
        if (ta | tb) == SPANNING {
            let direction = sub(b, a);
            let denom = dot(plane.normal, direction);
            if denom.abs() > EPS {
                let t = (plane.w - dot(plane.normal, a)) / denom;
                let p = add(a, mul(direction, t))?;
                front_vertices.push(p);
                back_vertices.push(p);
            }
        }
    }
    if front_vertices.len() >= 3 {
        front.push(CsgPolygon {
            plane: make_plane(&front_vertices)?,
            vertices: front_vertices,
        });
    }
    if back_vertices.len() >= 3 {
        back.push(CsgPolygon {
            plane: make_plane(&back_vertices)?,
            vertices: back_vertices,
        });
    }
    Ok(())
}

fn node_to_mesh(node: CsgNode, budget: &mut WorkBudget) -> Result<Mesh3, MeshOperationError> {
    let mut faces = Vec::new();
    for polygon in node.all_polygons(budget)? {
        if polygon.vertices.len() < 3 {
            continue;
        }
        for i in 1..polygon.vertices.len() - 1 {
            if triangle_area(
                polygon.vertices[0],
                polygon.vertices[i],
                polygon.vertices[i + 1],
            ) > EPS
            {
                faces.push(
                    Face3::triangle(
                        polygon.vertices[0],
                        polygon.vertices[i],
                        polygon.vertices[i + 1],
                    )
                    .map_err(|_| MeshOperationError::DegenerateFace)?,
                );
            }
        }
    }
    Mesh3::new(faces).map_err(|_| MeshOperationError::DegenerateMesh)
}
fn make_plane(v: &[Point3]) -> Result<CsgPlane, MeshOperationError> {
    if v.len() < 3 {
        return Err(MeshOperationError::DegenerateFace);
    }
    let n = cross(sub(v[1], v[0]), sub(v[2], v[0]));
    let l = (n.x() * n.x() + n.y() * n.y() + n.z() * n.z()).sqrt();
    if l <= EPS {
        return Err(MeshOperationError::DegenerateFace);
    }
    let normal = Point3::new(n.x() / l, n.y() / l, n.z() / l)
        .map_err(|_| MeshOperationError::NonFiniteCoordinate)?;
    Ok(CsgPlane {
        normal,
        w: dot(normal, v[0]),
    })
}
fn invert_plane(p: CsgPlane) -> CsgPlane {
    CsgPlane {
        normal: Point3::new(-p.normal.x(), -p.normal.y(), -p.normal.z()).expect("finite"),
        w: -p.w,
    }
}
fn dot(a: Point3, b: Point3) -> f64 {
    a.x() * b.x() + a.y() * b.y() + a.z() * b.z()
}
fn sub(a: Point3, b: Point3) -> Point3 {
    Point3::new(a.x() - b.x(), a.y() - b.y(), a.z() - b.z()).expect("finite")
}
fn add(a: Point3, b: Point3) -> Result<Point3, MeshOperationError> {
    Point3::new(a.x() + b.x(), a.y() + b.y(), a.z() + b.z())
        .map_err(|_| MeshOperationError::NonFiniteCoordinate)
}
fn mul(a: Point3, t: f64) -> Point3 {
    Point3::new(a.x() * t, a.y() * t, a.z() * t).expect("finite")
}
fn cross(a: Point3, b: Point3) -> Point3 {
    Point3::new(
        a.y() * b.z() - a.z() * b.y(),
        a.z() * b.x() - a.x() * b.z(),
        a.x() * b.y() - a.y() * b.x(),
    )
    .expect("finite")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct VertexKey([i64; 3]);
impl VertexKey {
    fn new(p: Point3, tolerance: f64) -> Self {
        Self([
            quantize(p.x(), tolerance),
            quantize(p.y(), tolerance),
            quantize(p.z(), tolerance),
        ])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct EdgeKey([i64; 6]);
impl EdgeKey {
    fn new(a: Point3, b: Point3, tolerance: f64) -> Self {
        let aa = VertexKey::new(a, tolerance).0;
        let bb = VertexKey::new(b, tolerance).0;
        if aa <= bb {
            Self([aa[0], aa[1], aa[2], bb[0], bb[1], bb[2]])
        } else {
            Self([bb[0], bb[1], bb[2], aa[0], aa[1], aa[2]])
        }
    }
    fn forward(self, a: Point3, b: Point3, tolerance: f64) -> bool {
        let aa = VertexKey::new(a, tolerance).0;
        let bb = VertexKey::new(b, tolerance).0;
        self.0[0] == aa[0]
            && self.0[1] == aa[1]
            && self.0[2] == aa[2]
            && self.0[3] == bb[0]
            && self.0[4] == bb[1]
            && self.0[5] == bb[2]
    }
}
fn quantize(value: f64, tolerance: f64) -> i64 {
    (value / tolerance).round() as i64
}
fn finite(p: Point3) -> bool {
    p.x().is_finite() && p.y().is_finite() && p.z().is_finite()
}
fn triangle_area(a: Point3, b: Point3, c: Point3) -> f64 {
    let x1 = b.x() - a.x();
    let y1 = b.y() - a.y();
    let z1 = b.z() - a.z();
    let x2 = c.x() - a.x();
    let y2 = c.y() - a.y();
    let z2 = c.z() - a.z();
    0.5 * ((y1 * z2 - z1 * y2)
        .hypot(z1 * x2 - x1 * z2)
        .hypot(x1 * y2 - y1 * x2))
}
fn tetra_volume(a: Point3, b: Point3, c: Point3) -> f64 {
    (a.x() * (b.y() * c.z() - b.z() * c.y()) - a.y() * (b.x() * c.z() - b.z() * c.x())
        + a.z() * (b.x() * c.y() - b.y() * c.x()))
        / 6.0
}
fn midpoint(a: Point3, b: Point3) -> Result<Point3, MeshOperationError> {
    Point3::new(
        (a.x() + b.x()) * 0.5,
        (a.y() + b.y()) * 0.5,
        (a.z() + b.z()) * 0.5,
    )
    .map_err(|_| MeshOperationError::NonFiniteCoordinate)
}
