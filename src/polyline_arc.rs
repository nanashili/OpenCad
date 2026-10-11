use crate::{GeometryError, Point3, PolylineSegment};
use std::f64::consts::TAU;

impl PolylineSegment {
    /// Arc from `start` to `end` with a signed included angle in radians.
    /// Positive sweeps are counterclockwise; minor and major arcs are supported.
    pub fn arc_from_sweep(start: Point3, end: Point3, sweep: f64) -> Result<Self, GeometryError> {
        validate_sweep(sweep)?;
        if start.z() != end.z() {
            return Err(GeometryError::NonPlanarPolyline);
        }
        if start == end {
            return Err(GeometryError::DegenerateCurve);
        }
        let bulge = (sweep / 4.0).tan();
        if bulge == 0.0 {
            return Err(GeometryError::UnrepresentableBulge);
        }
        let segment = checked_segment(start, end, bulge)?;
        if segment.length()? == 0.0 {
            return Err(GeometryError::UnrepresentableBulge);
        }
        Ok(segment)
    }

    /// Arc from `start` about an explicit XY center by a signed sweep in radians.
    /// The endpoint is the starting radius rotated counterclockwise for positive sweeps.
    pub fn arc_about(start: Point3, center: Point3, sweep: f64) -> Result<Self, GeometryError> {
        Self::arc_about_with_endpoint(start, center, sweep).map(|(_, segment)| segment)
    }

    pub(crate) fn arc_about_with_endpoint(
        start: Point3,
        center: Point3,
        sweep: f64,
    ) -> Result<(Point3, Self), GeometryError> {
        validate_sweep(sweep)?;
        if start.z() != center.z() {
            return Err(GeometryError::NonPlanarPolyline);
        }
        if start == center {
            return Err(GeometryError::DegenerateCurve);
        }
        let end = crate::Transform::rotate_xy(center, sweep)?.point(start)?;
        Ok((end, Self::arc_from_sweep(start, end, sweep)?))
    }

    /// Signed bulge for this segment's traversal: positive is counterclockwise.
    pub fn bulge(self) -> f64 {
        match self {
            Self::Line(_) => 0.0,
            Self::Arc { arc, clockwise } => {
                (arc.sweep() / 4.0).tan() * if clockwise { -1.0 } else { 1.0 }
            }
        }
    }

    /// Unit tangent in traversal direction at the end of an XY segment.
    pub fn end_tangent_xy(self) -> Result<(f64, f64), GeometryError> {
        match self {
            Self::Line(line) => unit_xy(
                line.end().x() - line.start().x(),
                line.end().y() - line.start().y(),
            ),
            Self::Arc { arc, clockwise } => {
                if arc.radius() == 0.0 || arc.sweep() == 0.0 {
                    return Err(GeometryError::DegenerateCurve);
                }
                let angle = if clockwise {
                    arc.start_angle()
                } else {
                    arc.end_angle()
                };
                let (sin, cos) = angle.sin_cos();
                let sign = if clockwise { -1.0 } else { 1.0 };
                Ok((-sin * sign, cos * sign))
            }
        }
    }

    /// Arc from `start` to `end` with the given starting XY tangent direction.
    /// A forward collinear chord is represented by a line (zero curvature).
    /// A backward collinear chord has no finite circular solution and is rejected.
    /// Directions within 64 machine epsilons of collinear use the same policy.
    pub fn arc_from_tangent(
        start: Point3,
        end: Point3,
        tangent: (f64, f64),
    ) -> Result<Self, GeometryError> {
        if start.z() != end.z() {
            return Err(GeometryError::NonPlanarPolyline);
        }
        let (tx, ty) = unit_xy(tangent.0, tangent.1)?;
        let (cx, cy) = unit_xy(end.x() - start.x(), end.y() - start.y())?;
        let cross = tx * cy - ty * cx;
        let dot = tx * cx + ty * cy;
        if cross.abs() <= 64.0 * f64::EPSILON {
            return if dot < 0.0 {
                Err(GeometryError::UnrepresentableBulge)
            } else {
                checked_segment(start, end, 0.0)
            };
        }
        let sweep = 2.0 * cross.atan2(dot);
        checked_segment(start, end, (sweep / 4.0).tan())
    }

    /// Circular arc from `start`, through `through`, to `end` in their XY plane.
    /// Clockwise and major arcs retain the requested point order. Coincident
    /// points and near-collinear triples (64 machine epsilons) are rejected.
    pub fn arc_through(start: Point3, through: Point3, end: Point3) -> Result<Self, GeometryError> {
        if start.z() != through.z() || start.z() != end.z() {
            return Err(GeometryError::NonPlanarPolyline);
        }
        if start == through || through == end || start == end {
            return Err(GeometryError::DegenerateCurve);
        }
        let mut vx = through.x() - start.x();
        let mut vy = through.y() - start.y();
        let mut wx = end.x() - start.x();
        let mut wy = end.y() - start.y();
        let scale = vx.abs().max(vy.abs()).max(wx.abs()).max(wy.abs());
        if !scale.is_finite() || scale == 0.0 {
            return Err(GeometryError::UnrepresentableBulge);
        }
        vx /= scale;
        vy /= scale;
        wx /= scale;
        wy /= scale;
        let cross = vx * wy - vy * wx;
        if cross.abs() <= 64.0 * f64::EPSILON * vx.hypot(vy) * wx.hypot(wy) {
            return Err(GeometryError::DegenerateCurve);
        }
        // Circumcenter in translated, normalized coordinates. The scale cancels
        // when finding angles; the existing bulge interpreter validates geometry.
        let v2 = vx * vx + vy * vy;
        let w2 = wx * wx + wy * wy;
        let cx = (v2 * wy - w2 * vy) / (2.0 * cross);
        let cy = (vx * w2 - wx * v2) / (2.0 * cross);
        let a = (-cy).atan2(-cx);
        let b = (vy - cy).atan2(vx - cx);
        let c = (wy - cy).atan2(wx - cx);
        let ccw = (c - a).rem_euclid(TAU);
        let sweep = if (b - a).rem_euclid(TAU) < ccw {
            ccw
        } else {
            ccw - TAU
        };
        let bulge = (sweep / 4.0).tan();
        if bulge == 0.0 {
            return Err(GeometryError::UnrepresentableBulge);
        }
        checked_segment(start, end, bulge)
    }
}

fn validate_sweep(sweep: f64) -> Result<(), GeometryError> {
    if !sweep.is_finite() {
        Err(GeometryError::NonFiniteAngle)
    } else if sweep == 0.0 || sweep.abs() >= TAU {
        Err(GeometryError::InvalidArcSweep)
    } else {
        Ok(())
    }
}

fn unit_xy(x: f64, y: f64) -> Result<(f64, f64), GeometryError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(GeometryError::NonFiniteResult);
    }
    let scale = x.abs().max(y.abs());
    if scale == 0.0 {
        return Err(GeometryError::DegenerateCurve);
    }
    let (x, y) = (x / scale, y / scale);
    let length = x.hypot(y);
    Ok((x / length, y / length))
}

fn checked_segment(
    start: Point3,
    end: Point3,
    bulge: f64,
) -> Result<PolylineSegment, GeometryError> {
    if !bulge.is_finite() {
        return Err(GeometryError::UnrepresentableBulge);
    }
    let segment = crate::polyline::segment(start, end, bulge)?;
    segment.length()?;
    if let PolylineSegment::Arc { arc, .. } = segment {
        arc.extrema()?;
    }
    Ok(segment)
}
