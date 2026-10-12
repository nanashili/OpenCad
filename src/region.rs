//! Planar polygon regions and robust boolean operations.
//!
//! Curved source boundaries are tessellated at construction time. Boolean
//! operations use geo's validated polygon clipping implementation and results
//! retain disconnected exteriors and interior holes.

use crate::{
    Circle3, Ellipse3, Entity, GeometryError, Hatch, HatchPattern, Point3, Polyline2,
    PolylineSegment,
};
use geo::algorithm::bool_ops::BooleanOps;
use geo::{Coord, LineString, MultiPolygon, Polygon};
use std::f64::consts::TAU;

const EPS: f64 = 1.0e-9;
const MAX_BOUNDARY_POINTS: usize = 16_384;

#[derive(Clone, Debug, PartialEq)]
pub struct RegionLoop {
    points: Vec<Point3>,
    hole: bool,
}

impl RegionLoop {
    pub fn new(points: Vec<Point3>, hole: bool) -> Result<Self, GeometryError> {
        validate_loop(&points)?;
        Ok(Self { points, hole })
    }
    pub fn points(&self) -> &[Point3] {
        &self.points
    }
    pub fn is_hole(&self) -> bool {
        self.hole
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region2 {
    loops: Vec<RegionLoop>,
    elevation: f64,
}

impl Region2 {
    pub fn from_boundaries(boundaries: Vec<Vec<Point3>>) -> Result<Self, GeometryError> {
        if boundaries.is_empty() {
            return Err(GeometryError::NotEnoughVertices);
        }
        let mut loops = boundaries
            .into_iter()
            .map(|points| RegionLoop::new(points, false))
            .collect::<Result<Vec<_>, _>>()?;
        classify_loops(&mut loops);
        let elevation = loops[0].points[0].z();
        Ok(Self { loops, elevation })
    }

    pub fn from_loops(loops: Vec<RegionLoop>) -> Result<Self, GeometryError> {
        if loops.is_empty() {
            return Ok(Self {
                loops,
                elevation: 0.0,
            });
        }
        let mut loops = loops;
        let elevation = loops[0].points[0].z();
        for loop_ in &loops {
            validate_loop(&loop_.points)?;
            if loop_.points.iter().any(|p| (p.z() - elevation).abs() > EPS) {
                return Err(GeometryError::NonPlanarPolyline);
            }
        }
        normalize_orientation(&mut loops);
        Ok(Self { loops, elevation })
    }

    pub fn empty(elevation: f64) -> Result<Self, GeometryError> {
        if !elevation.is_finite() {
            return Err(GeometryError::NonFiniteCoordinate);
        }
        Ok(Self {
            loops: Vec::new(),
            elevation,
        })
    }

    pub fn from_entity(entity: &Entity) -> Result<Self, GeometryError> {
        match entity {
            Entity::Polyline(polyline) if polyline.is_closed() => {
                Self::from_boundaries(vec![tessellate_polyline(polyline)?])
            }
            Entity::Circle(circle) => Self::from_boundaries(vec![sample_circle(*circle)?]),
            Entity::Ellipse(ellipse) => Self::from_boundaries(vec![sample_ellipse(*ellipse)?]),
            Entity::Hatch(hatch) => Self::from_boundaries(hatch.boundaries().to_vec()),
            Entity::Face3(face) => {
                let (vertices, count) = face.vertices();
                Self::from_boundaries(vec![vertices[..count].to_vec()])
            }
            Entity::Polyline(_) => Err(GeometryError::NotEnoughSegmentsToClose),
            _ => Err(GeometryError::UnsupportedPolylineConversion),
        }
    }

    pub fn loops(&self) -> &[RegionLoop] {
        &self.loops
    }
    pub fn elevation(&self) -> f64 {
        self.elevation
    }
    pub fn is_empty(&self) -> bool {
        self.loops.is_empty()
    }

    pub fn to_hatch(&self) -> Result<Hatch, GeometryError> {
        if self.loops.is_empty() {
            return Err(GeometryError::NotEnoughVertices);
        }
        Hatch::new(
            self.loops
                .iter()
                .map(|loop_| loop_.points.clone())
                .collect(),
            HatchPattern::Solid,
        )
    }

    pub fn union(&self, other: &Self) -> Result<Self, GeometryError> {
        self.boolean(other, BooleanOp::Union)
    }
    pub fn subtract(&self, other: &Self) -> Result<Self, GeometryError> {
        self.boolean(other, BooleanOp::Subtract)
    }
    pub fn intersect(&self, other: &Self) -> Result<Self, GeometryError> {
        self.boolean(other, BooleanOp::Intersect)
    }

    fn boolean(&self, other: &Self, op: BooleanOp) -> Result<Self, GeometryError> {
        if (self.elevation - other.elevation).abs() > EPS {
            return Err(GeometryError::NonPlanarPolyline);
        }
        let first = self.to_geo()?;
        let second = other.to_geo()?;
        let result = match op {
            BooleanOp::Union => first.union(&second),
            BooleanOp::Subtract => first.difference(&second),
            BooleanOp::Intersect => first.intersection(&second),
        };
        Self::from_geo(result, self.elevation)
    }

    fn to_geo(&self) -> Result<MultiPolygon<f64>, GeometryError> {
        let mut polygons = Vec::new();
        for (index, loop_) in self
            .loops
            .iter()
            .enumerate()
            .filter(|(_, loop_)| !loop_.hole)
        {
            let holes = self
                .loops
                .iter()
                .enumerate()
                .filter(|(hole_index, hole)| {
                    *hole_index != index
                        && hole.hole
                        && point_in_polygon(hole.points[0], &loop_.points)
                })
                .map(|(_, hole)| line_string(&hole.points))
                .collect();
            polygons.push(Polygon::new(line_string(&loop_.points), holes));
        }
        Ok(MultiPolygon(polygons))
    }

    fn from_geo(multi: MultiPolygon<f64>, elevation: f64) -> Result<Self, GeometryError> {
        let mut loops = Vec::new();
        for polygon in multi.0 {
            let exterior = ring_points(polygon.exterior(), elevation)?;
            loops.push(RegionLoop::new(exterior, false)?);
            for interior in polygon.interiors() {
                loops.push(RegionLoop::new(ring_points(interior, elevation)?, true)?);
            }
        }
        Self::from_loops(loops)
    }
}

#[derive(Clone, Copy)]
enum BooleanOp {
    Union,
    Subtract,
    Intersect,
}

fn validate_loop(points: &[Point3]) -> Result<(), GeometryError> {
    if points.len() < 3 || points.len() > MAX_BOUNDARY_POINTS {
        return Err(GeometryError::NotEnoughVertices);
    }
    let z = points[0].z();
    if points.iter().any(|p| (p.z() - z).abs() > EPS) {
        return Err(GeometryError::NonPlanarPolyline);
    }
    if signed_area(points).abs() <= EPS {
        return Err(GeometryError::DegenerateCurve);
    }
    for first in 0..points.len() {
        for second in (first + 1)..points.len() {
            if second == first + 1 || (first == 0 && second + 1 == points.len()) {
                continue;
            }
            if segments_intersect(
                points[first],
                points[(first + 1) % points.len()],
                points[second],
                points[(second + 1) % points.len()],
            ) {
                return Err(GeometryError::DegenerateCurve);
            }
        }
    }
    Ok(())
}

fn segments_intersect(a: Point3, b: Point3, c: Point3, d: Point3) -> bool {
    let cross = |p: Point3, q: Point3, r: Point3| {
        (q.x() - p.x()) * (r.y() - p.y()) - (q.y() - p.y()) * (r.x() - p.x())
    };
    let ab_c = cross(a, b, c);
    let ab_d = cross(a, b, d);
    let cd_a = cross(c, d, a);
    let cd_b = cross(c, d, b);
    if ab_c.abs() <= EPS && ab_d.abs() <= EPS && cd_a.abs() <= EPS && cd_b.abs() <= EPS {
        return ranges_overlap(a.x(), b.x(), c.x(), d.x())
            && ranges_overlap(a.y(), b.y(), c.y(), d.y());
    }
    ((ab_c > EPS && ab_d < -EPS)
        || (ab_c < -EPS && ab_d > EPS)
        || ab_c.abs() <= EPS
        || ab_d.abs() <= EPS)
        && ((cd_a > EPS && cd_b < -EPS)
            || (cd_a < -EPS && cd_b > EPS)
            || cd_a.abs() <= EPS
            || cd_b.abs() <= EPS)
}
fn ranges_overlap(a: f64, b: f64, c: f64, d: f64) -> bool {
    a.min(b) <= c.max(d) + EPS && c.min(d) <= a.max(b) + EPS
}

fn normalize_orientation(loops: &mut [RegionLoop]) {
    for loop_ in loops {
        let positive = signed_area(&loop_.points) > 0.0;
        if positive == loop_.hole {
            loop_.points.reverse();
        }
    }
}
fn classify_loops(loops: &mut [RegionLoop]) {
    for i in 0..loops.len() {
        let point = loops[i].points[0];
        loops[i].hole = (0..loops.len())
            .filter(|&j| j != i && point_in_polygon(point, &loops[j].points))
            .count()
            % 2
            == 1;
    }
    normalize_orientation(loops);
}
fn signed_area(points: &[Point3]) -> f64 {
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = points[(i + 1) % points.len()];
            p.x() * q.y() - q.x() * p.y()
        })
        .sum::<f64>()
        * 0.5
}
fn point_in_polygon(point: Point3, polygon: &[Point3]) -> bool {
    let mut inside = false;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        if ((a.y() > point.y()) != (b.y() > point.y()))
            && point.x() < (b.x() - a.x()) * (point.y() - a.y()) / (b.y() - a.y()) + a.x()
        {
            inside = !inside;
        }
    }
    inside
}
fn line_string(points: &[Point3]) -> LineString<f64> {
    let mut coords = points
        .iter()
        .map(|p| Coord { x: p.x(), y: p.y() })
        .collect::<Vec<_>>();
    if coords.first() != coords.last() {
        coords.push(coords[0]);
    }
    LineString::new(coords)
}
fn ring_points(ring: &LineString<f64>, elevation: f64) -> Result<Vec<Point3>, GeometryError> {
    let mut points = ring
        .0
        .iter()
        .map(|coord| Point3::new(coord.x, coord.y, elevation))
        .collect::<Result<Vec<_>, _>>()?;
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    Ok(points)
}
fn tessellate_polyline(polyline: &Polyline2) -> Result<Vec<Point3>, GeometryError> {
    let mut points = Vec::new();
    for segment in polyline.segments() {
        let segment = segment?;
        let steps = match segment {
            PolylineSegment::Line(_) => 1,
            PolylineSegment::Arc { arc, .. } => {
                ((arc.sweep().abs() / (TAU / 96.0)).ceil() as usize).clamp(1, 96)
            }
        };
        for i in 0..steps {
            points.push(segment.point_at_fraction(i as f64 / steps as f64)?);
        }
    }
    Ok(points)
}
fn sample_circle(circle: Circle3) -> Result<Vec<Point3>, GeometryError> {
    (0..96)
        .map(|i| {
            Point3::new(
                circle.center().x() + circle.radius() * (TAU * i as f64 / 96.0).cos(),
                circle.center().y() + circle.radius() * (TAU * i as f64 / 96.0).sin(),
                circle.center().z(),
            )
        })
        .collect()
}
fn sample_ellipse(ellipse: Ellipse3) -> Result<Vec<Point3>, GeometryError> {
    let steps = if ellipse.is_full() { 128 } else { 64 };
    (0..steps)
        .map(|i| ellipse.point_at_fraction(i as f64 / steps as f64))
        .collect()
}
