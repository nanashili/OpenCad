//! Dependency-free OBJ and STL mesh interchange.
//!
//! The interchange layer intentionally handles mesh geometry only.  OBJ materials,
//! UVs, smoothing groups, and STL attributes are ignored; malformed or unbounded input
//! is rejected instead of being silently discarded.

use crate::{Face3, Mesh3, Point3};
use std::fmt::Write as _;

const MAX_VERTICES: usize = 1_000_000;
const MAX_FACES: usize = 2_000_000;
const MAX_POLYGON_VERTICES: usize = 1_024;
const EPS: f64 = 1.0e-10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterchangeError {
    EmptyInput,
    InvalidUtf8,
    InvalidSyntax { line: usize },
    InvalidIndex { line: usize },
    NonFiniteCoordinate { line: usize },
    NonPlanarFace { line: usize },
    DegenerateFace { line: usize },
    TooManyVertices,
    TooManyFaces,
    InvalidBinaryLength,
    EmptyMesh,
}

impl std::fmt::Display for InterchangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for InterchangeError {}

pub fn read_obj(input: &str) -> Result<Vec<Mesh3>, InterchangeError> {
    if input.trim().is_empty() {
        return Err(InterchangeError::EmptyInput);
    }
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let mut meshes = Vec::new();
    for (line_index, source) in input.lines().enumerate() {
        let line = line_index + 1;
        let mut fields = source.split_whitespace();
        let Some(kind) = fields.next() else { continue };
        match kind {
            "v" => {
                let values = fields
                    .take(3)
                    .map(|value| value.parse::<f64>().ok())
                    .collect::<Option<Vec<_>>>();
                let Some(values) = values else {
                    return Err(InterchangeError::InvalidSyntax { line });
                };
                if values.len() != 3 || values.iter().any(|v| !v.is_finite()) {
                    return Err(InterchangeError::NonFiniteCoordinate { line });
                }
                if vertices.len() == MAX_VERTICES {
                    return Err(InterchangeError::TooManyVertices);
                }
                vertices.push(
                    Point3::new(values[0], values[1], values[2])
                        .map_err(|_| InterchangeError::NonFiniteCoordinate { line })?,
                );
            }
            "f" => {
                let indices = fields
                    .map(|field| parse_obj_index(field, vertices.len(), line))
                    .collect::<Result<Vec<_>, _>>()?;
                if indices.len() < 3 {
                    return Err(InterchangeError::DegenerateFace { line });
                }
                let triangles = triangulate_polygon(&indices, &vertices, line)?;
                if faces.len().saturating_add(triangles.len()) > MAX_FACES {
                    return Err(InterchangeError::TooManyFaces);
                }
                faces.extend(
                    triangles
                        .into_iter()
                        .map(|[a, b, c]| {
                            Face3::triangle(vertices[a], vertices[b], vertices[c])
                                .map_err(|_| InterchangeError::DegenerateFace { line })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
            "o" | "g" if !faces.is_empty() => {
                meshes.push(
                    Mesh3::new(std::mem::take(&mut faces))
                        .map_err(|_| InterchangeError::DegenerateFace { line })?,
                );
            }
            _ => {}
        }
    }
    if !faces.is_empty() {
        meshes.push(Mesh3::new(faces).map_err(|_| InterchangeError::EmptyMesh)?);
    }
    if meshes.is_empty() {
        return Err(InterchangeError::EmptyMesh);
    }
    Ok(meshes)
}

pub fn write_obj(meshes: &[Mesh3]) -> Result<String, InterchangeError> {
    if meshes.is_empty() {
        return Err(InterchangeError::EmptyMesh);
    }
    let mut output = String::new();
    let mut offset = 0usize;
    for (mesh_index, mesh) in meshes.iter().enumerate() {
        if mesh.faces().is_empty() {
            return Err(InterchangeError::EmptyMesh);
        }
        writeln!(output, "o mesh_{}", mesh_index + 1).unwrap();
        for face in mesh.faces() {
            let (vertices, count) = face.vertices();
            for point in vertices.iter().take(count) {
                if !finite(*point) {
                    return Err(InterchangeError::NonFiniteCoordinate { line: 0 });
                }
                writeln!(
                    output,
                    "v {:.17} {:.17} {:.17}",
                    point.x(),
                    point.y(),
                    point.z()
                )
                .unwrap();
            }
        }
        let mut cursor = offset;
        for face in mesh.faces() {
            let count = face.vertex_count();
            write!(output, "f").unwrap();
            for index in 0..count {
                write!(output, " {}", cursor + index + 1).unwrap();
            }
            output.push('\n');
            cursor += count;
        }
        offset = cursor;
    }
    Ok(output)
}

pub fn read_stl(input: &[u8]) -> Result<Mesh3, InterchangeError> {
    if input.is_empty() {
        return Err(InterchangeError::EmptyInput);
    }
    if input.len() >= 84 {
        let count = u32::from_le_bytes(input[80..84].try_into().unwrap()) as usize;
        if count <= MAX_FACES && 84usize.saturating_add(count.saturating_mul(50)) == input.len() {
            return read_binary_stl(input, count);
        }
    }
    let text = std::str::from_utf8(input).map_err(|_| InterchangeError::InvalidUtf8)?;
    read_ascii_stl(text)
}

pub fn write_stl_binary(meshes: &[Mesh3]) -> Result<Vec<u8>, InterchangeError> {
    let triangles = mesh_triangles(meshes)?;
    let mut output = vec![0u8; 84 + triangles.len() * 50];
    output[..20].copy_from_slice(b"OpenCad mesh export\0");
    output[80..84].copy_from_slice(&(triangles.len() as u32).to_le_bytes());
    for (index, (a, b, c)) in triangles.into_iter().enumerate() {
        let normal = normal(a, b, c).ok_or(InterchangeError::DegenerateFace { line: index + 1 })?;
        let start = 84 + index * 50;
        for (slot, value) in [
            normal.0,
            normal.1,
            normal.2,
            a.x(),
            a.y(),
            a.z(),
            b.x(),
            b.y(),
            b.z(),
            c.x(),
            c.y(),
            c.z(),
        ]
        .into_iter()
        .enumerate()
        {
            let value = value as f32;
            if !value.is_finite() {
                return Err(InterchangeError::NonFiniteCoordinate { line: index + 1 });
            }
            output[start + slot * 4..start + slot * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(output)
}

pub fn write_stl_ascii(meshes: &[Mesh3]) -> Result<String, InterchangeError> {
    let triangles = mesh_triangles(meshes)?;
    let mut output = String::from("solid OpenCad\n");
    for (a, b, c) in triangles {
        let n = normal(a, b, c).ok_or(InterchangeError::DegenerateFace { line: 0 })?;
        writeln!(output, "  facet normal {:.17} {:.17} {:.17}", n.0, n.1, n.2).unwrap();
        output.push_str("    outer loop\n");
        for p in [a, b, c] {
            writeln!(
                output,
                "      vertex {:.17} {:.17} {:.17}",
                p.x(),
                p.y(),
                p.z()
            )
            .unwrap();
        }
        output.push_str("    endloop\n  endfacet\n");
    }
    output.push_str("endsolid OpenCad\n");
    Ok(output)
}

fn read_binary_stl(input: &[u8], count: usize) -> Result<Mesh3, InterchangeError> {
    if count == 0 {
        return Err(InterchangeError::EmptyMesh);
    }
    let mut faces = Vec::with_capacity(count);
    for index in 0..count {
        let start = 84 + index * 50 + 12;
        let mut points = [Point3::new(0.0, 0.0, 0.0).unwrap(); 3];
        for (point_index, point) in points.iter_mut().enumerate() {
            let point_start = start + point_index * 12;
            let values = [
                read_f32(input, point_start),
                read_f32(input, point_start + 4),
                read_f32(input, point_start + 8),
            ];
            if values.iter().any(|v| !v.is_finite()) {
                return Err(InterchangeError::NonFiniteCoordinate { line: index + 1 });
            }
            *point = Point3::new(values[0], values[1], values[2])
                .map_err(|_| InterchangeError::NonFiniteCoordinate { line: index + 1 })?;
        }
        faces.push(
            Face3::triangle(points[0], points[1], points[2])
                .map_err(|_| InterchangeError::DegenerateFace { line: index + 1 })?,
        );
    }
    Mesh3::new(faces).map_err(|_| InterchangeError::EmptyMesh)
}

fn read_ascii_stl(input: &str) -> Result<Mesh3, InterchangeError> {
    let mut lines = input.lines().enumerate().peekable();
    let mut faces = Vec::new();
    let Some((header_line, header)) = lines.next() else {
        return Err(InterchangeError::EmptyMesh);
    };
    if !header.trim_start().starts_with("solid") {
        return Err(InterchangeError::InvalidSyntax {
            line: header_line + 1,
        });
    }
    let mut ended = false;
    while let Some((line_index, source)) = lines.next() {
        let line = line_index + 1;
        let trimmed = source.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut fields = trimmed.split_whitespace();
        if fields.next() != Some("facet") || fields.next() != Some("normal") {
            if trimmed.split_whitespace().next() == Some("endsolid") {
                ended = true;
                if lines.any(|(_, trailing)| !trailing.trim().is_empty()) {
                    return Err(InterchangeError::InvalidSyntax { line });
                }
                break;
            }
            return Err(InterchangeError::InvalidSyntax { line });
        }
        let normal_values = fields
            .map(|value| value.parse::<f64>().ok())
            .collect::<Option<Vec<_>>>();
        if normal_values
            .as_ref()
            .is_none_or(|values| values.len() != 3 || values.iter().any(|value| !value.is_finite()))
        {
            return Err(InterchangeError::InvalidSyntax { line });
        }
        let Some((outer_line_index, outer)) = lines.next() else {
            return Err(InterchangeError::InvalidSyntax { line });
        };
        if outer.trim() != "outer loop" {
            return Err(InterchangeError::InvalidSyntax {
                line: outer_line_index + 1,
            });
        }
        let mut points = Vec::with_capacity(3);
        for _ in 0..3 {
            let Some((vertex_line_index, vertex_line)) = lines.next() else {
                return Err(InterchangeError::InvalidSyntax { line });
            };
            points.push(parse_ascii_vertex(vertex_line, vertex_line_index + 1)?);
        }
        let Some((end_loop_index, end_loop)) = lines.next() else {
            return Err(InterchangeError::InvalidSyntax { line });
        };
        if end_loop.trim() != "endloop" {
            return Err(InterchangeError::InvalidSyntax {
                line: end_loop_index + 1,
            });
        }
        let Some((end_facet_index, end_facet)) = lines.next() else {
            return Err(InterchangeError::InvalidSyntax { line });
        };
        if end_facet.trim() != "endfacet" {
            return Err(InterchangeError::InvalidSyntax {
                line: end_facet_index + 1,
            });
        }
        faces.push(
            Face3::triangle(points[0], points[1], points[2])
                .map_err(|_| InterchangeError::DegenerateFace { line })?,
        );
        if faces.len() > MAX_FACES {
            return Err(InterchangeError::TooManyFaces);
        }
    }
    if !ended || faces.is_empty() {
        return Err(InterchangeError::EmptyMesh);
    }
    Mesh3::new(faces).map_err(|_| InterchangeError::EmptyMesh)
}

fn parse_ascii_vertex(source: &str, line: usize) -> Result<Point3, InterchangeError> {
    let mut fields = source.split_whitespace();
    if fields.next() != Some("vertex") {
        return Err(InterchangeError::InvalidSyntax { line });
    }
    let values = fields
        .map(|v| v.parse::<f64>().ok())
        .collect::<Option<Vec<_>>>();
    let Some(values) = values else {
        return Err(InterchangeError::InvalidSyntax { line });
    };
    if values.len() != 3 || values.iter().any(|v| !v.is_finite()) {
        return Err(InterchangeError::NonFiniteCoordinate { line });
    }
    Point3::new(values[0], values[1], values[2])
        .map_err(|_| InterchangeError::NonFiniteCoordinate { line })
}

fn mesh_triangles(meshes: &[Mesh3]) -> Result<Vec<(Point3, Point3, Point3)>, InterchangeError> {
    let mut triangles = Vec::new();
    for mesh in meshes {
        for face in mesh.faces() {
            let (v, n) = face.vertices();
            if n == 3 {
                triangles.push((v[0], v[1], v[2]));
            } else {
                triangles.extend([(v[0], v[1], v[2]), (v[0], v[2], v[3])]);
            }
            if triangles.len() > MAX_FACES {
                return Err(InterchangeError::TooManyFaces);
            }
        }
    }
    if triangles.is_empty() {
        return Err(InterchangeError::EmptyMesh);
    }
    if triangles
        .iter()
        .any(|(a, b, c)| !finite(*a) || !finite(*b) || !finite(*c))
    {
        return Err(InterchangeError::NonFiniteCoordinate { line: 0 });
    }
    Ok(triangles)
}

fn parse_obj_index(
    field: &str,
    vertex_count: usize,
    line: usize,
) -> Result<usize, InterchangeError> {
    let raw = field
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<isize>()
        .map_err(|_| InterchangeError::InvalidIndex { line })?;
    if raw == 0 {
        return Err(InterchangeError::InvalidIndex { line });
    }
    let index = if raw < 0 {
        vertex_count as isize + raw
    } else {
        raw - 1
    };
    if index < 0 || index as usize >= vertex_count {
        return Err(InterchangeError::InvalidIndex { line });
    }
    Ok(index as usize)
}

fn triangulate_polygon(
    indices: &[usize],
    vertices: &[Point3],
    line: usize,
) -> Result<Vec<[usize; 3]>, InterchangeError> {
    if indices.len() > MAX_POLYGON_VERTICES {
        return Err(InterchangeError::TooManyFaces);
    }
    if indices.len() == 3 {
        return Ok(vec![[indices[0], indices[1], indices[2]]]);
    }
    let normal = polygon_normal(indices, vertices);
    let length = (normal.0 * normal.0 + normal.1 * normal.1 + normal.2 * normal.2).sqrt();
    if !length.is_finite() || length <= EPS {
        return Err(InterchangeError::DegenerateFace { line });
    }
    let origin = vertices[indices[0]];
    let tolerance = EPS * length;
    if indices.iter().skip(1).any(|index| {
        ((vertices[*index].x() - origin.x()) * normal.0
            + (vertices[*index].y() - origin.y()) * normal.1
            + (vertices[*index].z() - origin.z()) * normal.2)
            .abs()
            > tolerance
    }) {
        return Err(InterchangeError::NonPlanarFace { line });
    }
    let axis = if normal.0.abs() >= normal.1.abs() && normal.0.abs() >= normal.2.abs() {
        0
    } else if normal.1.abs() >= normal.2.abs() {
        1
    } else {
        2
    };
    let mut remaining = indices.to_vec();
    let mut triangles = Vec::with_capacity(indices.len() - 2);
    let winding = projected_area(&remaining, vertices, axis);
    while remaining.len() > 3 {
        let mut ear = None;
        for i in 0..remaining.len() {
            let a = remaining[(i + remaining.len() - 1) % remaining.len()];
            let b = remaining[i];
            let c = remaining[(i + 1) % remaining.len()];
            if orient2(vertices[a], vertices[b], vertices[c], axis) * winding <= EPS {
                continue;
            }
            if remaining.iter().enumerate().any(|(j, p)| {
                j != i
                    && *p != a
                    && *p != b
                    && *p != c
                    && point_in_triangle(vertices[*p], vertices[a], vertices[b], vertices[c], axis)
            }) {
                continue;
            }
            ear = Some((i, [a, b, c]));
            break;
        }
        let Some((index, triangle)) = ear else {
            return Err(InterchangeError::DegenerateFace { line });
        };
        triangles.push(triangle);
        remaining.remove(index);
    }
    triangles.push([remaining[0], remaining[1], remaining[2]]);
    Ok(triangles)
}

fn polygon_normal(indices: &[usize], vertices: &[Point3]) -> (f64, f64, f64) {
    let mut n = (0.0, 0.0, 0.0);
    for i in 0..indices.len() {
        let a = vertices[indices[i]];
        let b = vertices[indices[(i + 1) % indices.len()]];
        n.0 += (a.y() - b.y()) * (a.z() + b.z());
        n.1 += (a.z() - b.z()) * (a.x() + b.x());
        n.2 += (a.x() - b.x()) * (a.y() + b.y());
    }
    n
}
fn projected_area(indices: &[usize], vertices: &[Point3], axis: usize) -> f64 {
    indices
        .iter()
        .enumerate()
        .map(|(i, index)| {
            let (x1, y1) = project(vertices[*index], axis);
            let (x2, y2) = project(vertices[indices[(i + 1) % indices.len()]], axis);
            x1 * y2 - y1 * x2
        })
        .sum::<f64>()
        * 0.5
}
fn orient2(a: Point3, b: Point3, c: Point3, axis: usize) -> f64 {
    let (ax, ay) = project(a, axis);
    let (bx, by) = project(b, axis);
    let (cx, cy) = project(c, axis);
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}
fn project(p: Point3, axis: usize) -> (f64, f64) {
    match axis {
        0 => (p.y(), p.z()),
        1 => (p.x(), p.z()),
        _ => (p.x(), p.y()),
    }
}
fn point_in_triangle(p: Point3, a: Point3, b: Point3, c: Point3, axis: usize) -> bool {
    let x = orient2(p, a, b, axis);
    let y = orient2(p, b, c, axis);
    let z = orient2(p, c, a, axis);
    (x >= -EPS && y >= -EPS && z >= -EPS) || (x <= EPS && y <= EPS && z <= EPS)
}
fn normal(a: Point3, b: Point3, c: Point3) -> Option<(f64, f64, f64)> {
    let u = (b.x() - a.x(), b.y() - a.y(), b.z() - a.z());
    let v = (c.x() - a.x(), c.y() - a.y(), c.z() - a.z());
    let n = (
        u.1 * v.2 - u.2 * v.1,
        u.2 * v.0 - u.0 * v.2,
        u.0 * v.1 - u.1 * v.0,
    );
    if !n.0.is_finite() || !n.1.is_finite() || !n.2.is_finite() {
        return None;
    }
    let l = n.0.hypot(n.1).hypot(n.2);
    (l.is_finite() && l > EPS).then_some((n.0 / l, n.1 / l, n.2 / l))
}
fn finite(p: Point3) -> bool {
    p.x().is_finite() && p.y().is_finite() && p.z().is_finite()
}
fn read_f32(input: &[u8], offset: usize) -> f64 {
    f32::from_le_bytes(input[offset..offset + 4].try_into().unwrap()) as f64
}
