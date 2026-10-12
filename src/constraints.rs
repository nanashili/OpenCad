//! Persistent, validated 2D geometric constraints.
//!
//! Constraints deliberately operate on the public `Entity` values.  A document
//! owner can store `ConstraintSet` records beside its entities and call
//! `solve_entities` after any geometry edit, committing the candidate only when
//! solving succeeds.

use crate::{Circle3, Entity, GeometryError, Line3, Point3};
use std::{error::Error, fmt};

const EPS: f64 = 1.0e-9;
const MAX_CONSTRAINTS: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    EntityPoint(usize),
    LineStart(usize),
    LineEnd(usize),
    CircleCenter(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinePair {
    Lines(usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstraintKind {
    Horizontal(usize),
    Vertical(usize),
    Coincident(Anchor, Anchor),
    Parallel(LinePair),
    Perpendicular(LinePair),
    Concentric(usize, usize),
    EqualLength(LinePair),
    EqualRadius(usize, usize),
    Tangent { line: usize, circle: usize },
    Fix(Anchor),
    Distance { a: Anchor, b: Anchor },
    LineLength(usize),
    Radius(usize),
    Diameter(usize),
    Angle(LinePair),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintRecord {
    id: u64,
    kind: ConstraintKind,
    value: Option<f64>,
    enabled: bool,
    fixed: Option<Point3>,
}

impl ConstraintRecord {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn kind(&self) -> ConstraintKind {
        self.kind
    }
    pub fn value(&self) -> Option<f64> {
        self.value
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn fixed_point(&self) -> Option<Point3> {
        self.fixed
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintSet {
    records: Vec<ConstraintRecord>,
    next_id: u64,
}

impl Default for ConstraintSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstraintSet {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            next_id: 1,
        }
    }
    pub fn records(&self) -> &[ConstraintRecord] {
        &self.records
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn add(
        &mut self,
        kind: ConstraintKind,
        value: Option<f64>,
    ) -> Result<u64, ConstraintError> {
        if self.records.len() >= MAX_CONSTRAINTS {
            return Err(ConstraintError::TooMany);
        }
        if let Some(value) = value
            && !value.is_finite()
        {
            return Err(ConstraintError::NonFiniteValue);
        }
        validate_kind(kind, value)?;
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(ConstraintError::TooMany)?;
        self.records.push(ConstraintRecord {
            id,
            kind,
            value,
            enabled: true,
            fixed: None,
        });
        Ok(id)
    }

    pub fn add_fixed(&mut self, anchor: Anchor, point: Point3) -> Result<u64, ConstraintError> {
        if !finite_point(point) {
            return Err(ConstraintError::NonFiniteValue);
        }
        let id = self.add(ConstraintKind::Fix(anchor), None)?;
        self.records
            .last_mut()
            .ok_or(ConstraintError::Missing(id))?
            .fixed = Some(point);
        Ok(id)
    }

    pub fn remove(&mut self, id: u64) -> Result<ConstraintRecord, ConstraintError> {
        let index = self
            .records
            .iter()
            .position(|record| record.id == id)
            .ok_or(ConstraintError::Missing(id))?;
        Ok(self.records.remove(index))
    }

    pub fn set_enabled(&mut self, id: u64, enabled: bool) -> Result<bool, ConstraintError> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(ConstraintError::Missing(id))?;
        if record.enabled == enabled {
            return Ok(false);
        }
        record.enabled = enabled;
        Ok(true)
    }

    pub fn set_value(&mut self, id: u64, value: f64) -> Result<bool, ConstraintError> {
        if !value.is_finite() {
            return Err(ConstraintError::NonFiniteValue);
        }
        let record = self
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or(ConstraintError::Missing(id))?;
        if !matches!(
            record.kind,
            ConstraintKind::Distance { .. }
                | ConstraintKind::LineLength(_)
                | ConstraintKind::Radius(_)
                | ConstraintKind::Diameter(_)
                | ConstraintKind::Angle(_)
        ) {
            return Err(ConstraintError::InvalidConstraint(
                "only dimensional constraints have editable values",
            ));
        }
        validate_kind(record.kind, Some(value))?;
        if record.value == Some(value) {
            return Ok(false);
        }
        record.value = Some(value);
        Ok(true)
    }

    pub fn solve_entities(&self, entities: &mut [Entity]) -> Result<bool, ConstraintError> {
        let mut changed_any = false;
        for _ in 0..8 {
            let mut pass_changed = false;
            for record in self.records.iter().filter(|record| record.enabled) {
                pass_changed |= solve_one(record, entities)?;
            }
            changed_any |= pass_changed;
            if !pass_changed {
                validate_entities(entities)?;
                return Ok(changed_any);
            }
        }
        Err(ConstraintError::Overconstrained(
            "constraints did not converge",
        ))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstraintError {
    TooMany,
    Missing(u64),
    InvalidReference(usize),
    UnsupportedGeometry(&'static str),
    InvalidConstraint(&'static str),
    NonFiniteValue,
    Inconsistent(&'static str),
    Overconstrained(&'static str),
    Geometry(GeometryError),
}
impl fmt::Display for ConstraintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooMany => write!(f, "constraint limit exceeded"),
            Self::Missing(id) => write!(f, "constraint {id} does not exist"),
            Self::InvalidReference(index) => {
                write!(f, "constraint references missing entity {index}")
            }
            Self::UnsupportedGeometry(kind) => write!(f, "constraint requires {kind} geometry"),
            Self::InvalidConstraint(message) => f.write_str(message),
            Self::NonFiniteValue => f.write_str("constraint value must be finite"),
            Self::Inconsistent(message) => write!(f, "inconsistent constraint: {message}"),
            Self::Overconstrained(message) => write!(f, "overconstrained geometry: {message}"),
            Self::Geometry(error) => error.fmt(f),
        }
    }
}
impl Error for ConstraintError {}
impl From<GeometryError> for ConstraintError {
    fn from(error: GeometryError) -> Self {
        Self::Geometry(error)
    }
}

fn validate_kind(kind: ConstraintKind, value: Option<f64>) -> Result<(), ConstraintError> {
    let needs_value = matches!(
        kind,
        ConstraintKind::Distance { .. }
            | ConstraintKind::LineLength(_)
            | ConstraintKind::Radius(_)
            | ConstraintKind::Diameter(_)
            | ConstraintKind::Angle(_)
    );
    if needs_value && value.is_none() {
        return Err(ConstraintError::InvalidConstraint(
            "dimensional constraint requires a value",
        ));
    }
    if let Some(value) = value
        && matches!(
            kind,
            ConstraintKind::Distance { .. }
                | ConstraintKind::LineLength(_)
                | ConstraintKind::Radius(_)
                | ConstraintKind::Diameter(_)
        )
        && value < 0.0
    {
        return Err(ConstraintError::InvalidConstraint(
            "distance, length, and radius values must be nonnegative",
        ));
    }
    Ok(())
}

fn entity(entities: &[Entity], index: usize) -> Result<&Entity, ConstraintError> {
    entities
        .get(index)
        .ok_or(ConstraintError::InvalidReference(index))
}
fn entity_mut(entities: &mut [Entity], index: usize) -> Result<&mut Entity, ConstraintError> {
    entities
        .get_mut(index)
        .ok_or(ConstraintError::InvalidReference(index))
}
fn as_line(entity: &Entity) -> Result<Line3, ConstraintError> {
    match entity {
        Entity::Line(line) => Ok(*line),
        _ => Err(ConstraintError::UnsupportedGeometry("a line")),
    }
}
fn as_circle(entity: &Entity) -> Result<Circle3, ConstraintError> {
    match entity {
        Entity::Circle(circle) => Ok(*circle),
        _ => Err(ConstraintError::UnsupportedGeometry("a circle")),
    }
}
fn point_of(anchor: Anchor, entities: &[Entity]) -> Result<Point3, ConstraintError> {
    match anchor {
        Anchor::EntityPoint(index) => match entity(entities, index)? {
            Entity::Point(point) => Ok(point.position()),
            _ => Err(ConstraintError::UnsupportedGeometry("a point")),
        },
        Anchor::LineStart(index) => Ok(as_line(entity(entities, index)?)?.start()),
        Anchor::LineEnd(index) => Ok(as_line(entity(entities, index)?)?.end()),
        Anchor::CircleCenter(index) => Ok(as_circle(entity(entities, index)?)?.center()),
    }
}
fn set_point(
    anchor: Anchor,
    point: Point3,
    entities: &mut [Entity],
) -> Result<(), ConstraintError> {
    match anchor {
        Anchor::EntityPoint(index) => match entity_mut(entities, index)? {
            Entity::Point(item) => item.set_position(point),
            _ => return Err(ConstraintError::UnsupportedGeometry("a point")),
        },
        Anchor::LineStart(index) => match entity_mut(entities, index)? {
            Entity::Line(line) => line.set_start(point),
            _ => return Err(ConstraintError::UnsupportedGeometry("a line")),
        },
        Anchor::LineEnd(index) => match entity_mut(entities, index)? {
            Entity::Line(line) => line.set_end(point),
            _ => return Err(ConstraintError::UnsupportedGeometry("a line")),
        },
        Anchor::CircleCenter(index) => match entity_mut(entities, index)? {
            Entity::Circle(circle) => circle.set_center(point),
            _ => return Err(ConstraintError::UnsupportedGeometry("a circle")),
        },
    }
    Ok(())
}
fn finite_point(point: Point3) -> bool {
    point.x().is_finite() && point.y().is_finite() && point.z().is_finite()
}
fn distance(a: Point3, b: Point3) -> f64 {
    (a.x() - b.x()).hypot(a.y() - b.y()).hypot(a.z() - b.z())
}
fn solve_one(record: &ConstraintRecord, entities: &mut [Entity]) -> Result<bool, ConstraintError> {
    let mut changed = false;
    match record.kind {
        ConstraintKind::Horizontal(index) => {
            let line = as_line(entity(entities, index)?)?;
            if (line.start().y() - line.end().y()).abs() > EPS {
                let mut line = line;
                line.set_end(Point3::new(
                    line.end().x(),
                    line.start().y(),
                    line.end().z(),
                )?);
                *entity_mut(entities, index)? = Entity::Line(line);
                changed = true;
            }
        }
        ConstraintKind::Vertical(index) => {
            let line = as_line(entity(entities, index)?)?;
            if (line.start().x() - line.end().x()).abs() > EPS {
                let mut line = line;
                line.set_end(Point3::new(
                    line.start().x(),
                    line.end().y(),
                    line.end().z(),
                )?);
                *entity_mut(entities, index)? = Entity::Line(line);
                changed = true;
            }
        }
        ConstraintKind::Coincident(a, b) => {
            let target = point_of(a, entities)?;
            if distance(target, point_of(b, entities)?) > EPS {
                set_point(b, target, entities)?;
                changed = true;
            }
        }
        ConstraintKind::Concentric(a, b) => {
            let target = as_circle(entity(entities, a)?)?.center();
            if distance(target, as_circle(entity(entities, b)?)?.center()) > EPS {
                let mut circle = as_circle(entity(entities, b)?)?;
                circle.set_center(target);
                *entity_mut(entities, b)? = Entity::Circle(circle);
                changed = true;
            }
        }
        ConstraintKind::EqualRadius(a, b) => {
            let radius = as_circle(entity(entities, a)?)?.radius();
            let other = as_circle(entity(entities, b)?)?;
            if (radius - other.radius()).abs() > EPS {
                let mut circle = other;
                circle.set_radius(radius)?;
                *entity_mut(entities, b)? = Entity::Circle(circle);
                changed = true;
            }
        }
        ConstraintKind::EqualLength(LinePair::Lines(a, b)) => {
            let first = as_line(entity(entities, a)?)?;
            let second = as_line(entity(entities, b)?)?;
            changed |= set_line_length(b, second, first.length()?, entities)?;
        }
        ConstraintKind::LineLength(index) => {
            let line = as_line(entity(entities, index)?)?;
            changed |= set_line_length(
                index,
                line,
                record.value.ok_or(ConstraintError::NonFiniteValue)?,
                entities,
            )?;
        }
        ConstraintKind::Radius(index) => {
            let mut circle = as_circle(entity(entities, index)?)?;
            let value = record.value.ok_or(ConstraintError::NonFiniteValue)?;
            if (circle.radius() - value).abs() > EPS {
                circle.set_radius(value)?;
                *entity_mut(entities, index)? = Entity::Circle(circle);
                changed = true;
            }
        }
        ConstraintKind::Diameter(index) => {
            let mut circle = as_circle(entity(entities, index)?)?;
            let value = record.value.ok_or(ConstraintError::NonFiniteValue)? / 2.0;
            if value < 0.0 {
                return Err(ConstraintError::InvalidConstraint(
                    "diameter must be nonnegative",
                ));
            }
            if (circle.radius() - value).abs() > EPS {
                circle.set_radius(value)?;
                *entity_mut(entities, index)? = Entity::Circle(circle);
                changed = true;
            }
        }
        ConstraintKind::Distance { a, b } => {
            let target = record.value.ok_or(ConstraintError::NonFiniteValue)?;
            let first = point_of(a, entities)?;
            let second = point_of(b, entities)?;
            let actual = distance(first, second);
            if (actual - target).abs() > EPS {
                if actual <= EPS {
                    return Err(ConstraintError::Inconsistent(
                        "coincident anchors cannot satisfy positive distance",
                    ));
                }
                let scale = target / actual;
                set_point(
                    b,
                    Point3::new(
                        first.x() + (second.x() - first.x()) * scale,
                        first.y() + (second.y() - first.y()) * scale,
                        first.z() + (second.z() - first.z()) * scale,
                    )?,
                    entities,
                )?;
                changed = true;
            }
        }
        ConstraintKind::Fix(anchor) => {
            let fixed = record.fixed.ok_or(ConstraintError::InvalidConstraint(
                "fix constraint has no pinned point",
            ))?;
            if distance(fixed, point_of(anchor, entities)?) > EPS {
                set_point(anchor, fixed, entities)?;
                changed = true;
            }
        }
        ConstraintKind::Parallel(LinePair::Lines(a, b)) => {
            changed |= align_lines(a, b, false, entities)?;
        }
        ConstraintKind::Perpendicular(LinePair::Lines(a, b)) => {
            changed |= align_lines(a, b, true, entities)?;
        }
        ConstraintKind::Angle(LinePair::Lines(a, b)) => {
            let angle = record.value.ok_or(ConstraintError::NonFiniteValue)?;
            if !angle.is_finite() {
                return Err(ConstraintError::NonFiniteValue);
            }
            changed |= align_lines_angle(a, b, angle, entities)?;
        }
        ConstraintKind::Tangent { line, circle } => {
            changed |= make_tangent(line, circle, entities)?;
        }
    }
    Ok(changed)
}

fn set_line_length(
    index: usize,
    line: Line3,
    length: f64,
    entities: &mut [Entity],
) -> Result<bool, ConstraintError> {
    if !length.is_finite() || length < 0.0 {
        return Err(ConstraintError::InvalidConstraint(
            "line length must be finite and nonnegative",
        ));
    }
    let actual = line.length()?;
    if (actual - length).abs() <= EPS {
        return Ok(false);
    }
    if actual <= EPS {
        return Err(ConstraintError::Inconsistent(
            "zero-length line cannot receive a direction",
        ));
    }
    let scale = length / actual;
    let start = line.start();
    let end = Point3::new(
        start.x() + (line.end().x() - start.x()) * scale,
        start.y() + (line.end().y() - start.y()) * scale,
        start.z() + (line.end().z() - start.z()) * scale,
    )?;
    let mut replacement = line;
    replacement.set_end(end);
    *entity_mut(entities, index)? = Entity::Line(replacement);
    Ok(true)
}

fn align_lines(
    first: usize,
    second: usize,
    perpendicular: bool,
    entities: &mut [Entity],
) -> Result<bool, ConstraintError> {
    let reference = as_line(entity(entities, first)?)?;
    let target = as_line(entity(entities, second)?)?;
    let dx = reference.end().x() - reference.start().x();
    let dy = reference.end().y() - reference.start().y();
    let (mut ux, mut uy) = (dx, dy);
    if perpendicular {
        (ux, uy) = (-dy, dx);
    }
    let norm = ux.hypot(uy);
    let length = target.length()?;
    if norm <= EPS || length <= EPS {
        return Err(ConstraintError::Inconsistent(
            "degenerate line cannot define direction",
        ));
    }
    ux /= norm;
    uy /= norm;
    let start = target.start();
    let desired = Point3::new(
        start.x() + ux * length,
        start.y() + uy * length,
        target.end().z(),
    )?;
    if distance(desired, target.end()) <= EPS {
        return Ok(false);
    }
    let mut replacement = target;
    replacement.set_end(desired);
    *entity_mut(entities, second)? = Entity::Line(replacement);
    Ok(true)
}

fn align_lines_angle(
    first: usize,
    second: usize,
    angle: f64,
    entities: &mut [Entity],
) -> Result<bool, ConstraintError> {
    let reference = as_line(entity(entities, first)?)?;
    let target = as_line(entity(entities, second)?)?;
    let (dx, dy) = (
        reference.end().x() - reference.start().x(),
        reference.end().y() - reference.start().y(),
    );
    let base = dy.atan2(dx) + angle;
    let length = target.length()?;
    if length <= EPS {
        return Err(ConstraintError::Inconsistent(
            "degenerate line cannot define direction",
        ));
    }
    let start = target.start();
    let desired = Point3::new(
        start.x() + length * base.cos(),
        start.y() + length * base.sin(),
        target.end().z(),
    )?;
    if distance(desired, target.end()) <= EPS {
        return Ok(false);
    }
    let mut replacement = target;
    replacement.set_end(desired);
    *entity_mut(entities, second)? = Entity::Line(replacement);
    Ok(true)
}

fn make_tangent(
    line_index: usize,
    circle_index: usize,
    entities: &mut [Entity],
) -> Result<bool, ConstraintError> {
    let line = as_line(entity(entities, line_index)?)?;
    let circle = as_circle(entity(entities, circle_index)?)?;
    let dx = line.end().x() - line.start().x();
    let dy = line.end().y() - line.start().y();
    let norm = dx.hypot(dy);
    if norm <= EPS {
        return Err(ConstraintError::Inconsistent(
            "zero-length line cannot be tangent",
        ));
    }
    let signed = ((circle.center().x() - line.start().x()) * dy
        - (circle.center().y() - line.start().y()) * dx)
        / norm;
    let side = if signed < 0.0 { -1.0 } else { 1.0 };
    let projection = ((circle.center().x() - line.start().x()) * dx
        + (circle.center().y() - line.start().y()) * dy)
        / (norm * norm);
    let center = Point3::new(
        line.start().x() + projection * dx - side * dy / norm * circle.radius(),
        line.start().y() + projection * dy + side * dx / norm * circle.radius(),
        circle.center().z(),
    )?;
    if distance(center, circle.center()) <= EPS {
        return Ok(false);
    }
    let mut replacement = circle;
    replacement.set_center(center);
    *entity_mut(entities, circle_index)? = Entity::Circle(replacement);
    Ok(true)
}

fn validate_entities(entities: &[Entity]) -> Result<(), ConstraintError> {
    for entity in entities {
        match entity {
            Entity::Point(point) if !finite_point(point.position()) => {
                return Err(ConstraintError::NonFiniteValue);
            }
            Entity::Line(line) if !finite_point(line.start()) || !finite_point(line.end()) => {
                return Err(ConstraintError::NonFiniteValue);
            }
            Entity::Circle(circle)
                if !finite_point(circle.center()) || !circle.radius().is_finite() =>
            {
                return Err(ConstraintError::NonFiniteValue);
            }
            _ => {}
        }
    }
    Ok(())
}
