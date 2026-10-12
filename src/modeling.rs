//! Bounded polygonal modeling helpers for sweep, loft, slicing, and helices.
use crate::{Face3, GeometryError, Mesh3, Point3};
use std::{error::Error, fmt};

const MAX_POINTS: usize = 65_536;
const MAX_RING: usize = 512;
const MAX_SECTION_WORK: usize = 4_000_000;
const EPS: f64 = 1.0e-9;

#[derive(Clone, Debug, PartialEq)]
pub enum ModelingError {
    InvalidInput(&'static str),
    NonFinite,
    TooManyPoints,
    Degenerate,
    Geometry(GeometryError),
}
impl fmt::Display for ModelingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(s) => f.write_str(s),
            Self::NonFinite => f.write_str("modeling input must be finite"),
            Self::TooManyPoints => f.write_str("modeling input exceeds the point limit"),
            Self::Degenerate => f.write_str("modeling input is degenerate"),
            Self::Geometry(e) => e.fmt(f),
        }
    }
}
impl Error for ModelingError {}
impl From<GeometryError> for ModelingError {
    fn from(e: GeometryError) -> Self {
        Self::Geometry(e)
    }
}

fn p(x: f64, y: f64, z: f64) -> Result<Point3, ModelingError> {
    Ok(Point3::new(x, y, z)?)
}
fn finite(v: Point3) -> bool {
    v.x().is_finite() && v.y().is_finite() && v.z().is_finite()
}
fn sub(a: Point3, b: Point3) -> [f64; 3] {
    [a.x() - b.x(), a.y() - b.y(), a.z() - b.z()]
}
fn add(a: Point3, v: [f64; 3]) -> Result<Point3, ModelingError> {
    p(a.x() + v[0], a.y() + v[1], a.z() + v[2])
}
fn mul(v: [f64; 3], s: f64) -> [f64; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}
fn unit(v: [f64; 3]) -> Result<[f64; 3], ModelingError> {
    let n = norm(v);
    if n <= EPS {
        Err(ModelingError::Degenerate)
    } else {
        Ok(mul(v, 1.0 / n))
    }
}
fn validate_points(points: &[Point3], min: usize) -> Result<(), ModelingError> {
    if points.len() < min {
        return Err(ModelingError::InvalidInput("too few points"));
    }
    if points.len() > MAX_POINTS || points.iter().any(|v| !finite(*v)) {
        return Err(if points.len() > MAX_POINTS {
            ModelingError::TooManyPoints
        } else {
            ModelingError::NonFinite
        });
    }
    Ok(())
}

fn frame(
    tangent: [f64; 3],
    previous: Option<[f64; 3]>,
) -> Result<([f64; 3], [f64; 3]), ModelingError> {
    let t = unit(tangent)?;
    let mut n = previous.unwrap_or_else(|| {
        let a = if t[0].abs() < 0.8 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        cross(a, t)
    });
    n = subv(n, mul(t, dot(n, t)));
    if norm(n) <= EPS {
        let a = if t[2].abs() < 0.8 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        n = cross(a, t);
    }
    let n = unit(n)?;
    Ok((n, unit(cross(t, n))?))
}
fn subv(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn sweep_profile(
    profile: &[Point3],
    path: &[Point3],
    closed_profile: bool,
    cap_ends: bool,
) -> Result<Mesh3, ModelingError> {
    validate_points(profile, if closed_profile { 3 } else { 2 })?;
    validate_points(path, 2)?;
    if closed_profile && profile.len() < 3 {
        return Err(ModelingError::Degenerate);
    };
    let segs = if closed_profile {
        profile.len()
    } else {
        profile.len() - 1
    };
    let ring_points = profile
        .len()
        .checked_mul(path.len())
        .ok_or(ModelingError::TooManyPoints)?;
    let side_faces = segs
        .checked_mul(path.len() - 1)
        .ok_or(ModelingError::TooManyPoints)?;
    if ring_points > MAX_POINTS || side_faces > MAX_POINTS {
        return Err(ModelingError::TooManyPoints);
    }
    let mut faces = Vec::new();
    let mut rings = Vec::with_capacity(path.len());
    let mut prev = None;
    for i in 0..path.len() {
        let tangent = if i == 0 {
            sub(path[1], path[0])
        } else if i + 1 == path.len() {
            sub(path[i], path[i - 1])
        } else {
            sub(path[i + 1], path[i - 1])
        };
        let (n, b) = frame(tangent, prev)?;
        let tangent = unit(tangent)?;
        prev = Some(n);
        let ring = profile
            .iter()
            .map(|q| {
                add(
                    path[i],
                    addv(mul(n, q.x()), addv(mul(b, q.y()), mul(tangent, q.z()))),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        rings.push(ring);
    }
    for r in 0..rings.len() - 1 {
        for j in 0..segs {
            let k = (j + 1) % profile.len();
            faces.push(Face3::quad(
                rings[r][j],
                rings[r][k],
                rings[r + 1][k],
                rings[r + 1][j],
            )?);
        }
    }
    if cap_ends && closed_profile {
        for (ring, reverse) in [(&rings[0], true), (&rings[rings.len() - 1], false)] {
            for tri in triangulate_ring(ring, reverse)? {
                faces.push(tri);
            }
        }
    }
    Mesh3::new(faces).map_err(Into::into)
}
fn addv(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn loft_profiles(
    profiles: &[Vec<Point3>],
    closed_profile: bool,
    cap_ends: bool,
) -> Result<Mesh3, ModelingError> {
    if profiles.len() < 2 {
        return Err(ModelingError::InvalidInput(
            "loft needs at least two profiles",
        ));
    }
    for ring in profiles {
        validate_points(ring, if closed_profile { 3 } else { 2 })?;
    }
    let count = profiles
        .iter()
        .map(Vec::len)
        .max()
        .ok_or(ModelingError::Degenerate)?;
    if count > MAX_RING {
        return Err(ModelingError::TooManyPoints);
    };
    let ring_points = count
        .checked_mul(profiles.len())
        .ok_or(ModelingError::TooManyPoints)?;
    let side_faces = (profiles.len() - 1)
        .checked_mul(if closed_profile { count } else { count - 1 })
        .ok_or(ModelingError::TooManyPoints)?;
    if ring_points > MAX_POINTS || side_faces > MAX_POINTS {
        return Err(ModelingError::TooManyPoints);
    }
    let rings = profiles
        .iter()
        .map(|r| resample(r, count, closed_profile))
        .collect::<Result<Vec<_>, _>>()?;
    let segs = if closed_profile { count } else { count - 1 };
    let mut faces = Vec::new();
    for i in 0..rings.len() - 1 {
        for j in 0..segs {
            let k = (j + 1) % count;
            faces.push(Face3::quad(
                rings[i][j],
                rings[i][k],
                rings[i + 1][k],
                rings[i + 1][j],
            )?);
        }
    }
    if cap_ends && closed_profile {
        for (r, rev) in [(&rings[0], true), (&rings[rings.len() - 1], false)] {
            for tri in triangulate_ring(r, rev)? {
                faces.push(tri);
            }
        }
    }
    Ok(Mesh3::new(faces)?)
}
fn resample(points: &[Point3], count: usize, closed: bool) -> Result<Vec<Point3>, ModelingError> {
    let mut lengths = Vec::new();
    let mut total = 0.0;
    for i in 0..(if closed {
        points.len()
    } else {
        points.len() - 1
    }) {
        let l = norm(sub(points[(i + 1) % points.len()], points[i]));
        if l <= EPS {
            return Err(ModelingError::Degenerate);
        }
        total += l;
        lengths.push(total);
    }
    let mut out = Vec::with_capacity(count);
    for j in 0..count {
        let d = total * j as f64 / (if closed { count } else { count - 1 }) as f64;
        let mut prev = 0.0;
        for i in 0..lengths.len() {
            if d <= lengths[i] + EPS {
                let f = (d - prev) / (lengths[i] - prev).max(EPS);
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                out.push(p(
                    a.x() + (b.x() - a.x()) * f,
                    a.y() + (b.y() - a.y()) * f,
                    a.z() + (b.z() - a.z()) * f,
                )?);
                break;
            }
            prev = lengths[i];
        }
    }
    Ok(out)
}

fn triangulate_ring(ring: &[Point3], reverse: bool) -> Result<Vec<Face3>, ModelingError> {
    let normal = polygon_normal(ring)?;
    triangulate_plane(ring, normal, reverse)
}

fn triangulate_plane(
    ring: &[Point3],
    desired_normal: [f64; 3],
    reverse: bool,
) -> Result<Vec<Face3>, ModelingError> {
    if ring.len() < 3 {
        return Err(ModelingError::Degenerate);
    }
    let dn = unit(desired_normal)?;
    let reference = if dn[0].abs() < 0.8 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = unit(cross(reference, dn))?;
    let v = unit(cross(dn, u))?;
    let coords: Vec<[f64; 2]> = ring
        .iter()
        .map(|point| {
            [
                dot([point.x(), point.y(), point.z()], u),
                dot([point.x(), point.y(), point.z()], v),
            ]
        })
        .collect();
    let mut ids: Vec<usize> = (0..ring.len()).collect();
    let area = signed_area_2d(&coords);
    if area.abs() <= EPS {
        return Err(ModelingError::Degenerate);
    }
    if area < 0.0 {
        ids.reverse();
    }
    let mut out = Vec::new();
    let mut guard = 0;
    while ids.len() > 2 && guard < ring.len() * ring.len() {
        guard += 1;
        let mut ear = None;
        for i in 0..ids.len() {
            let a = ids[(i + ids.len() - 1) % ids.len()];
            let b = ids[i];
            let c = ids[(i + 1) % ids.len()];
            if orient2d(coords[a], coords[b], coords[c]) <= EPS {
                continue;
            }
            if ids.iter().all(|&q| {
                q == a || q == b || q == c || !inside2d(coords[q], coords[a], coords[b], coords[c])
            }) {
                ear = Some((i, a, b, c));
                break;
            }
        }
        let Some((i, a, b, c)) = ear else {
            return Err(ModelingError::InvalidInput("non-simple cap profile"));
        };
        let (b, c) = if reverse { (c, b) } else { (b, c) };
        out.push(Face3::triangle(ring[a], ring[b], ring[c])?);
        ids.remove(i);
    }
    if ids.len() != 2 {
        return Err(ModelingError::InvalidInput("non-simple cap profile"));
    }
    Ok(out)
}

fn polygon_normal(ring: &[Point3]) -> Result<[f64; 3], ModelingError> {
    if ring.len() < 3 {
        return Err(ModelingError::Degenerate);
    }
    let mut normal = [0.0; 3];
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        normal = addv(normal, cross([a.x(), a.y(), a.z()], [b.x(), b.y(), b.z()]));
    }
    unit(normal)
}

fn signed_area_2d(ring: &[[f64; 2]]) -> f64 {
    ring.iter()
        .enumerate()
        .map(|(i, a)| {
            let b = ring[(i + 1) % ring.len()];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        * 0.5
}
fn orient2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn inside2d(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    orient2d(a, b, p) >= -EPS && orient2d(b, c, p) >= -EPS && orient2d(c, a, p) >= -EPS
}
fn triangulate_fan(points: &[Point3], reverse: bool) -> Result<Vec<Face3>, ModelingError> {
    if points.len() < 3 {
        return Err(ModelingError::Degenerate);
    }
    let mut out = Vec::with_capacity(points.len() - 2);
    for i in 1..points.len() - 1 {
        let (a, b, c) = if reverse {
            (points[0], points[i + 1], points[i])
        } else {
            (points[0], points[i], points[i + 1])
        };
        out.push(Face3::triangle(a, b, c)?);
    }
    Ok(out)
}

pub fn section_mesh(
    mesh: &Mesh3,
    origin: Point3,
    normal: Point3,
) -> Result<Vec<Vec<Point3>>, ModelingError> {
    if !finite(origin) || !finite(normal) {
        return Err(ModelingError::NonFinite);
    };
    let n = unit([normal.x(), normal.y(), normal.z()])?;
    let mut segs = Vec::new();
    if mesh.faces().len() > MAX_POINTS {
        return Err(ModelingError::TooManyPoints);
    }
    for face in mesh.faces() {
        let (vs, c) = face.vertices();
        let mut hits = Vec::new();
        for i in 0..c {
            let a = vs[i];
            let b = vs[(i + 1) % c];
            let da = dot(n, sub(a, origin));
            let db = dot(n, sub(b, origin));
            if da.abs() <= EPS {
                hits.push(a);
            }
            if da * db < -EPS * EPS {
                let t = da / (da - db);
                hits.push(add(a, mul(sub(b, a), t))?);
            }
        }
        hits.dedup_by(|a, b| distance(*a, *b) <= EPS);
        if hits.len() >= 2 {
            segs.push((hits[0], hits[1]));
        }
    }
    if segs.len().saturating_mul(segs.len()) > MAX_SECTION_WORK {
        return Err(ModelingError::TooManyPoints);
    }
    connect_segments(segs)
}
fn distance(a: Point3, b: Point3) -> f64 {
    norm(sub(a, b))
}
fn connect_segments(mut segs: Vec<(Point3, Point3)>) -> Result<Vec<Vec<Point3>>, ModelingError> {
    let mut out = Vec::new();
    while let Some((a, b)) = segs.pop() {
        let mut ring = vec![a, b];
        loop {
            let Some(last) = ring.last().copied() else {
                break;
            };
            let Some(i) = segs
                .iter()
                .position(|(x, y)| distance(*x, last) <= EPS || distance(*y, last) <= EPS)
            else {
                break;
            };
            let (x, y) = segs.swap_remove(i);
            let next = if distance(x, last) <= EPS { y } else { x };
            if distance(next, ring[0]) <= EPS {
                ring.push(ring[0]);
                break;
            }
            ring.push(next);
        }
        if ring.len() >= 2 {
            out.push(ring);
        }
    }
    Ok(out)
}

pub fn slice_mesh(
    mesh: &Mesh3,
    origin: Point3,
    normal: Point3,
    cap: bool,
) -> Result<(Option<Mesh3>, Option<Mesh3>), ModelingError> {
    if !finite(origin) || !finite(normal) {
        return Err(ModelingError::NonFinite);
    };
    let n = unit([normal.x(), normal.y(), normal.z()])?;
    let mut sides = [Vec::new(), Vec::new()];
    for face in mesh.faces() {
        let (vs, c) = face.vertices();
        let poly = vs[..c].to_vec();
        for (side, side_faces) in sides.iter_mut().enumerate() {
            let keep_positive = side == 0;
            let clipped = clip_polygon(&poly, origin, n, keep_positive)?;
            if clipped.len() >= 3 {
                for tri in triangulate_fan(&clipped, false)? {
                    side_faces.push(tri);
                }
            }
        }
    }
    if cap {
        let contours = section_mesh(mesh, origin, normal)?;
        let mut closed = Vec::new();
        for mut ring in contours {
            let Some(last) = ring.last().copied() else {
                continue;
            };
            if ring.len() < 4 || distance(ring[0], last) > EPS {
                return Err(ModelingError::InvalidInput(
                    "cannot cap an open section contour",
                ));
            }
            ring.pop();
            closed.push(ring);
        }
        if closed.len() > 1 {
            return Err(ModelingError::InvalidInput(
                "nested or disconnected section contours require hole-aware capping",
            ));
        }
        if let Some(ring) = closed.into_iter().next() {
            // The positive half gets a cap facing -normal; the negative half +normal.
            for tri in triangulate_plane(&ring, mul(n, -1.0), false)? {
                sides[0].push(tri);
            }
            for tri in triangulate_plane(&ring, n, false)? {
                sides[1].push(tri);
            }
        }
    }
    let [positive, negative] = sides;
    Ok((
        if positive.is_empty() {
            None
        } else {
            Some(Mesh3::new(positive)?)
        },
        if negative.is_empty() {
            None
        } else {
            Some(Mesh3::new(negative)?)
        },
    ))
}
fn clip_polygon(
    poly: &[Point3],
    origin: Point3,
    n: [f64; 3],
    positive: bool,
) -> Result<Vec<Point3>, ModelingError> {
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let da = dot(n, sub(a, origin)) * (if positive { 1.0 } else { -1.0 });
        let db = dot(n, sub(b, origin)) * (if positive { 1.0 } else { -1.0 });
        if da >= -EPS {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(add(a, mul(sub(b, a), t))?);
        }
    }
    out.dedup_by(|a, b| distance(*a, *b) <= EPS);
    Ok(out)
}

pub fn helix_points(
    center: Point3,
    base_radius: f64,
    top_radius: f64,
    height: f64,
    turns: f64,
    segments: usize,
) -> Result<Vec<Point3>, ModelingError> {
    if !finite(center)
        || ![base_radius, top_radius, height, turns]
            .iter()
            .all(|v| v.is_finite())
        || base_radius < 0.0
        || top_radius < 0.0
        || height.abs() <= EPS
        || turns.abs() <= EPS
    {
        return Err(ModelingError::InvalidInput("invalid helix parameters"));
    }
    if !(2..=MAX_POINTS).contains(&segments) {
        return Err(ModelingError::InvalidInput("helix segments out of bounds"));
    }
    let mut out = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let f = i as f64 / segments as f64;
        let r = base_radius + (top_radius - base_radius) * f;
        let a = std::f64::consts::TAU * turns * f;
        out.push(p(
            center.x() + r * a.cos(),
            center.y() + r * a.sin(),
            center.z() + height * f,
        )?);
    }
    Ok(out)
}
