//! Validated rational B-splines in three dimensions.

use crate::{GeometryError, Point3};

#[derive(Clone, Debug, PartialEq)]
pub struct Spline3 {
    degree: usize,
    knots: Vec<f64>,
    control_points: Vec<Point3>,
    weights: Vec<f64>,
    closed: bool,
}

impl Spline3 {
    /// Creates a B-spline. Knots use the standard `n + degree + 2` layout,
    /// where `n + 1` is the number of control points.
    pub fn new(
        degree: usize,
        knots: Vec<f64>,
        control_points: Vec<Point3>,
        weights: Vec<f64>,
        closed: bool,
    ) -> Result<Self, GeometryError> {
        if control_points.is_empty() || degree == 0 || degree >= control_points.len() {
            return Err(GeometryError::DegenerateCurve);
        }
        if knots.len() != control_points.len() + degree + 1 || weights.len() != control_points.len()
        {
            return Err(GeometryError::DegenerateCurve);
        }
        if knots.iter().any(|k| !k.is_finite())
            || weights.iter().any(|w| !w.is_finite() || *w <= 0.0)
        {
            return Err(GeometryError::NonFiniteParameter);
        }
        if knots.windows(2).any(|pair| pair[1] < pair[0]) {
            return Err(GeometryError::DegenerateCurve);
        }
        let low = knots[degree];
        let high = knots[control_points.len()];
        if high <= low {
            return Err(GeometryError::DegenerateCurve);
        }
        Ok(Self {
            degree,
            knots,
            control_points,
            weights,
            closed,
        })
    }

    pub fn unweighted(
        degree: usize,
        knots: Vec<f64>,
        control_points: Vec<Point3>,
        closed: bool,
    ) -> Result<Self, GeometryError> {
        Self::new(
            degree,
            knots,
            control_points.clone(),
            vec![1.0; control_points.len()],
            closed,
        )
    }

    pub fn degree(&self) -> usize {
        self.degree
    }
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }
    pub fn control_points(&self) -> &[Point3] {
        &self.control_points
    }
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    pub fn parameter_range(&self) -> (f64, f64) {
        (
            self.knots[self.degree],
            self.knots[self.control_points.len()],
        )
    }

    /// Evaluates the rational curve with the de Boor algorithm.
    pub fn point_at(&self, parameter: f64) -> Result<Point3, GeometryError> {
        if !parameter.is_finite() {
            return Err(GeometryError::NonFiniteParameter);
        }
        let (low, high) = self.parameter_range();
        let u = parameter.clamp(low, high);
        let span = if u == high {
            self.control_points.len() - 1
        } else {
            self.knots[self.degree..self.control_points.len() + 1]
                .partition_point(|k| *k <= u)
                .saturating_sub(1)
                .max(self.degree)
        };
        let mut work = Vec::with_capacity(self.degree + 1);
        for j in 0..=self.degree {
            let index = span - self.degree + j;
            let p = self.control_points[index];
            let w = self.weights[index];
            work.push([p.x() * w, p.y() * w, p.z() * w, w]);
        }
        for level in 1..=self.degree {
            for j in (level..=self.degree).rev() {
                let i = span - self.degree + j;
                let denominator = self.knots[i + self.degree + 1 - level] - self.knots[i];
                let alpha = if denominator == 0.0 {
                    0.0
                } else {
                    (u - self.knots[i]) / denominator
                };
                let previous = work[j - 1];
                let mut current = work[j];
                for (component, slot) in current.iter_mut().enumerate() {
                    *slot = (1.0 - alpha) * previous[component] + alpha * *slot;
                }
                work[j] = current;
            }
        }
        let result = work[self.degree];
        if !result[3].is_finite() || result[3] <= 0.0 {
            return Err(GeometryError::NonFiniteResult);
        }
        Point3::new(
            result[0] / result[3],
            result[1] / result[3],
            result[2] / result[3],
        )
        .map_err(|_| GeometryError::NonFiniteResult)
    }

    pub fn tessellate(&self, segments: usize) -> Result<Vec<Point3>, GeometryError> {
        if segments == 0 {
            return Err(GeometryError::DegenerateCurve);
        }
        let (low, high) = self.parameter_range();
        (0..=segments)
            .map(|i| self.point_at(low + (high - low) * (i as f64 / segments as f64)))
            .collect()
    }

    pub fn translated(&self, dx: f64, dy: f64, dz: f64) -> Result<Self, GeometryError> {
        let points = self
            .control_points
            .iter()
            .map(|p| Point3::new(p.x() + dx, p.y() + dy, p.z() + dz))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(
            self.degree,
            self.knots.clone(),
            points,
            self.weights.clone(),
            self.closed,
        )
    }
}
