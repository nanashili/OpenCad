use crate::{Arc3, Entity, GeometryError, Line3, Point3};
use std::f64::consts::TAU;

// Original fractional/angular comparison policy; not a native CAD tolerance.
const EPS: f64 = 1e-10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoundaryMode {
    /// Use the finite boundary curve, including its endpoints.
    #[default]
    NoExtend,
    /// Use the boundary's supporting line or full circle.
    Extend,
}

#[derive(Clone, Copy)]
enum Curve {
    Line(Line3),
    Round {
        center: Point3,
        radius: f64,
        start: f64,
        sweep: f64,
        closed: bool,
    },
}

impl Curve {
    fn from_entity(entity: &Entity) -> Option<Self> {
        match entity {
            Entity::Line(line) if line.start().z() == line.end().z() => Some(Self::Line(*line)),
            Entity::Circle(circle) => Some(Self::Round {
                center: circle.center(),
                radius: circle.radius(),
                start: 0.0,
                sweep: TAU,
                closed: true,
            }),
            Entity::Arc(arc) => Some(Self::Round {
                center: arc.center(),
                radius: arc.radius(),
                start: arc.start_angle(),
                sweep: arc.sweep(),
                closed: false,
            }),
            _ => None,
        }
    }

    fn validate(self) -> Result<(), GeometryError> {
        match self {
            Self::Line(line) => {
                if line.length()? == 0.0 {
                    return Err(GeometryError::DegenerateCurve);
                }
            }
            Self::Round { radius, sweep, .. } if radius == 0.0 || sweep == 0.0 => {
                return Err(GeometryError::DegenerateCurve);
            }
            _ => {}
        }
        Ok(())
    }

    fn elevation(self) -> f64 {
        match self {
            Self::Line(line) => line.start().z(),
            Self::Round { center, .. } => center.z(),
        }
    }

    fn closed(self) -> bool {
        matches!(self, Self::Round { closed: true, .. })
    }

    fn parameter(self, point: Point3) -> Result<f64, GeometryError> {
        let value = match self {
            Self::Line(line) => {
                let length = line.length()?;
                let dx = (line.end().x() - line.start().x()) / length;
                let dy = (line.end().y() - line.start().y()) / length;
                dx.mul_add(
                    point.x() - line.start().x(),
                    dy * (point.y() - line.start().y()),
                ) / length
            }
            Self::Round {
                center,
                start,
                sweep,
                ..
            } => {
                let dx = finite(point.x() - center.x())?;
                let dy = finite(point.y() - center.y())?;
                if dx == 0.0 && dy == 0.0 {
                    return Err(GeometryError::AmbiguousCurvePick);
                }
                let delta = (dy.atan2(dx) - start).rem_euclid(TAU);
                if TAU - delta <= EPS {
                    0.0
                } else {
                    delta / sweep
                }
            }
        };
        finite(value)
    }

    fn point(self, parameter: f64) -> Result<Point3, GeometryError> {
        match self {
            Self::Line(line) => line.point_at_fraction(parameter),
            Self::Round {
                center,
                radius,
                start,
                sweep,
                ..
            } => {
                let (sin, cos) = (start + sweep * parameter).sin_cos();
                result_point(
                    center.x() + radius * cos,
                    center.y() + radius * sin,
                    center.z(),
                )
            }
        }
    }

    fn fragment(self, from: f64, to: f64) -> Result<Entity, GeometryError> {
        match self {
            Self::Line(_) => Ok(Entity::Line(Line3::new(self.point(from)?, self.point(to)?))),
            Self::Round {
                center,
                radius,
                start,
                sweep,
                ..
            } => Ok(Entity::Arc(Arc3::new(
                center,
                radius,
                start + sweep * from,
                start + sweep * to,
            )?)),
        }
    }

    fn cuts(self, boundaries: &[Entity], mode: BoundaryMode) -> Result<Vec<f64>, GeometryError> {
        let mut cuts = Vec::new();
        for boundary in boundaries {
            self.boundary_cuts(boundary, mode, &mut cuts)?;
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() <= EPS);
        Ok(cuts)
    }

    fn boundary_cuts(
        self,
        boundary: &Entity,
        mode: BoundaryMode,
        cuts: &mut Vec<f64>,
    ) -> Result<(), GeometryError> {
        if let Entity::Polyline(polyline) = boundary {
            for segment in polyline.segments() {
                self.boundary_cuts(&segment?.geometry(), mode, cuts)?;
            }
            return Ok(());
        }
        let Some(boundary) = Self::from_entity(boundary) else {
            return Ok(());
        };
        if boundary.elevation() != self.elevation() {
            return Ok(());
        }
        match boundary.validate() {
            Err(GeometryError::DegenerateCurve) => return Ok(()),
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        for point in intersections(self, boundary)? {
            let parameter = boundary.parameter(point)?;
            if mode == BoundaryMode::Extend
                || boundary.closed()
                || (-EPS..=1.0 + EPS).contains(&parameter)
            {
                cuts.push(self.parameter(point)?);
            }
        }
        Ok(())
    }
}

impl Entity {
    /// Remove the picked portion between adjacent boundary intersections.
    /// Supports XY LINE/CIRCLE/ARC at the same elevation as each cutting edge.
    /// Tangent contacts count as intersections; a circle requires two distinct
    /// cuts. Coincident supports are ignored and picks on a cut are rejected.
    pub fn trim_xy(
        &self,
        boundaries: &[Self],
        pick: Point3,
        mode: BoundaryMode,
    ) -> Result<Vec<Self>, GeometryError> {
        let curve = Curve::from_entity(self).ok_or(GeometryError::UnsupportedTrimEntity)?;
        curve.validate()?;
        let mut cuts = curve.cuts(boundaries, mode)?;
        let parameter = curve.parameter(pick)?;
        if curve.closed() {
            if cuts.len() < 2 {
                return Err(GeometryError::NoBoundaryIntersection);
            }
            if cuts.iter().any(|cut| {
                let delta = (cut - parameter).abs();
                delta <= EPS || (1.0 - delta).abs() <= EPS
            }) {
                return Err(GeometryError::AmbiguousCurvePick);
            }
            let left = cuts
                .iter()
                .copied()
                .rfind(|cut| *cut < parameter)
                .unwrap_or(cuts[cuts.len() - 1] - 1.0);
            let right = cuts
                .iter()
                .copied()
                .find(|cut| *cut > parameter)
                .unwrap_or(cuts[0] + 1.0);
            return Ok(vec![curve.fragment(right, left + 1.0)?]);
        }
        cuts.retain(|cut| *cut > EPS && *cut < 1.0 - EPS);
        if cuts.is_empty() {
            return Err(GeometryError::NoBoundaryIntersection);
        }
        if !(-EPS..=1.0 + EPS).contains(&parameter)
            || cuts.iter().any(|cut| (cut - parameter).abs() <= EPS)
        {
            return Err(GeometryError::AmbiguousCurvePick);
        }
        let parameter = parameter.clamp(0.0, 1.0);
        let left = cuts
            .iter()
            .copied()
            .rfind(|cut| *cut < parameter)
            .unwrap_or(0.0);
        let right = cuts
            .iter()
            .copied()
            .find(|cut| *cut > parameter)
            .unwrap_or(1.0);
        let mut result = Vec::new();
        if left > EPS {
            result.push(curve.fragment(0.0, left)?);
        }
        if right < 1.0 - EPS {
            result.push(curve.fragment(right, 1.0)?);
        }
        Ok(result)
    }

    /// Extend the endpoint nearest the XY pick to the first boundary on its
    /// natural path. An equal-distance endpoint pick is rejected.
    pub fn extend_xy(
        &self,
        boundaries: &[Self],
        pick: Point3,
        mode: BoundaryMode,
    ) -> Result<Self, GeometryError> {
        let curve = Curve::from_entity(self).ok_or(GeometryError::UnsupportedExtendEntity)?;
        curve.validate()?;
        if curve.closed() {
            return Err(GeometryError::UnsupportedExtendEntity);
        }
        let distance =
            |endpoint: Point3| finite((pick.x() - endpoint.x()).hypot(pick.y() - endpoint.y()));
        let first = distance(curve.point(0.0)?)?;
        let last = distance(curve.point(1.0)?)?;
        if (first - last).abs() <= EPS * first.max(last).max(1.0) {
            return Err(GeometryError::AmbiguousCurvePick);
        }
        let start = first < last;
        let mut candidates = curve.cuts(boundaries, mode)?;
        match curve {
            Curve::Line(_) => {
                candidates.retain(|value| {
                    if start {
                        *value < -EPS
                    } else {
                        *value > 1.0 + EPS
                    }
                });
                let parameter = if start {
                    candidates.last()
                } else {
                    candidates.first()
                }
                .copied()
                .ok_or(GeometryError::NoBoundaryIntersection)?;
                if start {
                    curve.fragment(parameter, 1.0)
                } else {
                    curve.fragment(0.0, parameter)
                }
            }
            Curve::Round { sweep, .. } => {
                candidates.retain(|value| *value > 1.0 + EPS);
                let parameter = if start {
                    candidates.last()
                } else {
                    candidates.first()
                }
                .copied()
                .ok_or(GeometryError::NoBoundaryIntersection)?;
                if start {
                    curve.fragment(parameter - TAU / sweep, 1.0)
                } else {
                    curve.fragment(0.0, parameter)
                }
            }
        }
    }
}

fn intersections(first: Curve, second: Curve) -> Result<Vec<Point3>, GeometryError> {
    match (first, second) {
        (Curve::Line(a), Curve::Line(b)) => {
            Ok(crate::line::xy_intersection(a, b)?.into_iter().collect())
        }
        (Curve::Line(line), Curve::Round { center, radius, .. })
        | (Curve::Round { center, radius, .. }, Curve::Line(line)) => {
            let length = line.length()?;
            let (ux, uy) = (
                (line.end().x() - line.start().x()) / length,
                (line.end().y() - line.start().y()) / length,
            );
            let (fx, fy) = (
                finite(center.x() - line.start().x())?,
                finite(center.y() - line.start().y())?,
            );
            let along = finite(ux.mul_add(fx, uy * fy))?;
            let perpendicular = finite(ux.mul_add(fy, -uy * fx))?;
            if perpendicular.abs() > radius {
                return Ok(Vec::new());
            }
            let ratio = perpendicular / radius;
            let half = finite(radius * ((1.0 - ratio) * (1.0 + ratio)).max(0.0).sqrt())?;
            let mut points = vec![line.point_at_fraction(finite((along - half) / length)?)?];
            if half != 0.0 {
                points.push(line.point_at_fraction(finite((along + half) / length)?)?);
            }
            Ok(points)
        }
        (
            Curve::Round {
                center: a,
                radius: ar,
                ..
            },
            Curve::Round {
                center: b,
                radius: br,
                ..
            },
        ) => {
            let (dx, dy) = (finite(b.x() - a.x())?, finite(b.y() - a.y())?);
            let distance = finite(dx.hypot(dy))?;
            if distance == 0.0 {
                return Ok(Vec::new());
            }
            let scale = distance.max(ar).max(br);
            let (d, r, s) = (distance / scale, ar / scale, br / scale);
            if d > r + s || d < (r - s).abs() {
                return Ok(Vec::new());
            }
            let along = finite((r.mul_add(r, -s * s) + d * d) / (2.0 * d))?;
            let height = finite(r.mul_add(r, -along * along).max(0.0).sqrt() * scale)?;
            let along = finite(along * scale)?;
            let (ux, uy) = (dx / distance, dy / distance);
            let (x, y) = (
                finite(ux.mul_add(along, a.x()))?,
                finite(uy.mul_add(along, a.y()))?,
            );
            let mut points = vec![result_point(x - uy * height, y + ux * height, a.z())?];
            if height != 0.0 {
                points.push(result_point(x + uy * height, y - ux * height, a.z())?);
            }
            Ok(points)
        }
    }
}

fn finite(value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GeometryError::NonFiniteResult)
    }
}

fn result_point(x: f64, y: f64, z: f64) -> Result<Point3, GeometryError> {
    Point3::new(x, y, z).map_err(|_| GeometryError::NonFiniteResult)
}
