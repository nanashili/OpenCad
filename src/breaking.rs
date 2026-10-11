use crate::trim::Curve;
use crate::{Entity, GeometryError, Point3};

// Original parameter/relative-distance policy, not a native CAD tolerance.
const EPS: f64 = 1e-10;

impl Entity {
    /// Remove the portion between two XY-projected picks. Open curves retain
    /// their direction; circles lose the CCW first-to-second portion. Equal
    /// interior picks split an open curve without a gap. Covering both ends
    /// removes an open curve completely. Pick Z does not change source Z.
    pub fn break_xy(&self, first: Point3, second: Point3) -> Result<Vec<Entity>, GeometryError> {
        let curve = Curve::from_entity(self).ok_or(GeometryError::UnsupportedBreakEntity)?;
        curve.validate()?;
        let a = nearest_parameter(curve, first)?;
        let b = nearest_parameter(curve, second)?;
        if curve.closed() {
            let gap = (b - a).rem_euclid(1.0);
            if gap <= EPS || 1.0 - gap <= EPS {
                return Err(GeometryError::InvalidBreakPoints);
            }
            return Ok(vec![fragment(curve, b, b + 1.0 - gap)?]);
        }
        let (a, b) = if (a - b).abs() <= EPS {
            if a <= EPS || a >= 1.0 - EPS {
                return Err(GeometryError::InvalidBreakPoints);
            }
            (a, a)
        } else {
            (a.min(b), a.max(b))
        };
        let mut parts = Vec::with_capacity(2);
        if a > EPS {
            parts.push(fragment(curve, 0.0, a)?);
        }
        if b < 1.0 - EPS {
            parts.push(fragment(curve, b, 1.0)?);
        }
        Ok(parts)
    }

    /// Split an open XY line or arc at the nearest point, without a gap.
    pub fn break_at_xy(&self, pick: Point3) -> Result<Vec<Entity>, GeometryError> {
        if !matches!(self, Self::Line(line) if line.start().z() == line.end().z())
            && !matches!(self, Self::Arc(_))
        {
            return Err(GeometryError::UnsupportedBreakAtPoint);
        }
        self.break_xy(pick, pick)
    }
}

fn nearest_parameter(curve: Curve, point: Point3) -> Result<f64, GeometryError> {
    let parameter = curve.parameter(point)?;
    if matches!(curve, Curve::Line(_)) {
        return Ok(parameter.clamp(0.0, 1.0));
    }
    if curve.closed() || parameter <= 1.0 + EPS {
        return Ok(parameter.clamp(0.0, 1.0));
    }
    // Outside an arc's angular span, the closest point is an endpoint.
    let distance = |end: f64| -> Result<f64, GeometryError> {
        let end = curve.point(end)?;
        let value = (point.x() - end.x()).hypot(point.y() - end.y());
        if value.is_finite() {
            Ok(value)
        } else {
            Err(GeometryError::NonFiniteResult)
        }
    };
    let start = distance(0.0)?;
    let end = distance(1.0)?;
    if (start - end).abs() <= EPS * start.max(end).max(1.0) {
        return Err(GeometryError::AmbiguousCurvePick);
    }
    Ok(if start < end { 0.0 } else { 1.0 })
}

fn fragment(curve: Curve, from: f64, to: f64) -> Result<Entity, GeometryError> {
    let part = curve.fragment(from, to)?;
    let part_curve = Curve::from_entity(&part).expect("curve fragments are lines or arcs");
    part_curve
        .validate()
        .map_err(|_| GeometryError::UnrepresentableBreak)?;
    if part_curve.point(0.0)? == part_curve.point(1.0)? {
        return Err(GeometryError::UnrepresentableBreak);
    }
    Ok(part)
}
