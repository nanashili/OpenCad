//! Parameterized block definitions and deterministic INSERT evaluation.
//!
//! Dynamic data is kept beside, rather than inside, [`BlockDefinition`].  A
//! definition therefore remains the raw reusable geometry while an instance
//! supplies its parameter overrides at evaluation time.

use crate::{BlockDefinition, Entity, GeometryError, Insert, Point3, Transform};
use std::collections::HashMap;
use std::fmt;

const MAX_PARAMETERS: usize = 256;
const MAX_ACTIONS: usize = 512;
const MAX_VISIBILITY_STATES: usize = 256;
const MAX_ARRAY_ITEMS: usize = 10_000;

#[derive(Clone, Debug, PartialEq)]
pub enum DynamicParameterType {
    Distance { min: f64, max: f64 },
    Angle { min: f64, max: f64 },
    Point { min: [f64; 3], max: [f64; 3] },
    XY { min: [f64; 2], max: [f64; 2] },
    Flip,
    Visibility { states: Vec<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DynamicValue {
    Number(f64),
    Point(Point3),
    XY { x: f64, y: f64 },
    Flip(bool),
    Visibility(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DynamicParameter {
    name: String,
    parameter_type: DynamicParameterType,
    default: DynamicValue,
}

impl DynamicParameter {
    pub fn new(
        name: impl Into<String>,
        parameter_type: DynamicParameterType,
        default: DynamicValue,
    ) -> Result<Self, DynamicBlockError> {
        let parameter = Self {
            name: name.into(),
            parameter_type,
            default,
        };
        parameter.validate_definition()?;
        Ok(parameter)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn parameter_type(&self) -> &DynamicParameterType {
        &self.parameter_type
    }
    pub fn default(&self) -> &DynamicValue {
        &self.default
    }

    fn validate_definition(&self) -> Result<(), DynamicBlockError> {
        if self.name.trim().is_empty() || self.name.len() > 128 {
            return Err(DynamicBlockError::InvalidParameter("parameter name".into()));
        }
        validate_parameter_type(&self.parameter_type)?;
        validate_value(&self.parameter_type, &self.default)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DynamicAction {
    Move {
        parameter: String,
        origin: Point3,
        direction: Point3,
    },
    Scale {
        parameter: String,
        center: Point3,
    },
    Rotate {
        parameter: String,
        center: Point3,
    },
    Flip {
        parameter: String,
        first: Point3,
        second: Point3,
    },
    Stretch {
        parameter: String,
        min: Point3,
        max: Point3,
        direction: Point3,
    },
    Array {
        parameter: Option<String>,
        rows: usize,
        columns: usize,
        row_spacing: f64,
        column_spacing: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DynamicVisibilityState {
    name: String,
    entity_ids: Vec<u64>,
}

impl DynamicVisibilityState {
    pub fn new(name: impl Into<String>, entity_ids: Vec<u64>) -> Result<Self, DynamicBlockError> {
        let name = name.into();
        if name.trim().is_empty() || name.len() > 128 {
            return Err(DynamicBlockError::InvalidVisibility("state name".into()));
        }
        if entity_ids.contains(&0)
            || entity_ids
                .iter()
                .enumerate()
                .any(|(index, id)| entity_ids[..index].contains(id))
        {
            return Err(DynamicBlockError::InvalidVisibility(
                "indices must be sorted and unique".into(),
            ));
        }
        Ok(Self { name, entity_ids })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn entity_ids(&self) -> &[u64] {
        &self.entity_ids
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynamicBlockDefinition {
    parameters: Vec<DynamicParameter>,
    actions: Vec<DynamicAction>,
    action_targets: Vec<Vec<u64>>,
    visibility_states: Vec<DynamicVisibilityState>,
}

impl DynamicBlockDefinition {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn parameters(&self) -> &[DynamicParameter] {
        &self.parameters
    }
    pub fn actions(&self) -> &[DynamicAction] {
        &self.actions
    }
    pub fn action_targets(&self, action_index: usize) -> Option<&[u64]> {
        self.action_targets.get(action_index).map(Vec::as_slice)
    }
    pub fn visibility_states(&self) -> &[DynamicVisibilityState] {
        &self.visibility_states
    }

    pub fn add_parameter(&mut self, parameter: DynamicParameter) -> Result<(), DynamicBlockError> {
        if self.parameters.len() >= MAX_PARAMETERS {
            return Err(DynamicBlockError::TooComplex);
        }
        if self
            .parameters
            .iter()
            .any(|item| item.name == parameter.name)
        {
            return Err(DynamicBlockError::InvalidParameter(
                "duplicate parameter".into(),
            ));
        }
        parameter.validate_definition()?;
        self.parameters.push(parameter);
        Ok(())
    }

    pub fn add_action(&mut self, action: DynamicAction) -> Result<(), DynamicBlockError> {
        if self.actions.len() >= MAX_ACTIONS {
            return Err(DynamicBlockError::TooComplex);
        }
        self.actions.push(action);
        self.action_targets.push(Vec::new());
        Ok(())
    }

    pub fn set_action_targets(
        &mut self,
        action_index: usize,
        entity_ids: Vec<u64>,
    ) -> Result<(), DynamicBlockError> {
        if action_index >= self.actions.len()
            || entity_ids.contains(&0)
            || entity_ids
                .iter()
                .enumerate()
                .any(|(index, id)| entity_ids[..index].contains(id))
        {
            return Err(DynamicBlockError::InvalidAction("target entity IDs".into()));
        }
        self.action_targets[action_index] = entity_ids;
        Ok(())
    }

    pub fn parameter(&self, name: &str) -> Option<&DynamicParameter> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == name)
    }

    pub fn validate_value(
        &self,
        name: &str,
        value: &DynamicValue,
    ) -> Result<(), DynamicBlockError> {
        let parameter = self
            .parameter(name)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))?;
        validate_value(&parameter.parameter_type, value)
    }

    pub fn resolved_value(
        &self,
        name: &str,
        overrides: &DynamicBlockOverrides,
    ) -> Result<DynamicValue, DynamicBlockError> {
        self.parameter_value(name, overrides)
    }

    pub fn validate_overrides(
        &self,
        overrides: &DynamicBlockOverrides,
    ) -> Result<(), DynamicBlockError> {
        for (name, value) in &overrides.values {
            self.validate_value(name, value)?;
        }
        if let Some(name) = overrides.visibility.as_deref() {
            if !self
                .visibility_states
                .iter()
                .any(|state| state.name == name)
            {
                return Err(DynamicBlockError::UnknownVisibility(name.into()));
            }
            if let Some(parameter) = self.parameters.iter().find(|parameter| {
                matches!(
                    parameter.parameter_type,
                    DynamicParameterType::Visibility { .. }
                )
            }) && let Some(DynamicValue::Visibility(value)) =
                overrides.values.get(&parameter.name)
                && value != name
            {
                return Err(DynamicBlockError::InvalidValue(
                    "conflicting visibility overrides".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn prune_targets(&mut self, valid_entity_ids: &[u64]) -> Result<(), DynamicBlockError> {
        let mut actions = Vec::with_capacity(self.actions.len());
        let mut action_targets = Vec::with_capacity(self.action_targets.len());
        for (action, targets) in self.actions.drain(..).zip(self.action_targets.drain(..)) {
            if targets.is_empty() {
                actions.push(action);
                action_targets.push(targets);
                continue;
            }
            let retained: Vec<u64> = targets
                .into_iter()
                .filter(|id| valid_entity_ids.contains(id))
                .collect();
            if !retained.is_empty() {
                actions.push(action);
                action_targets.push(retained);
            }
        }
        self.actions = actions;
        self.action_targets = action_targets;
        for state in &mut self.visibility_states {
            state.entity_ids.retain(|id| valid_entity_ids.contains(id));
        }
        Ok(())
    }

    pub fn add_visibility_state(
        &mut self,
        state: DynamicVisibilityState,
    ) -> Result<(), DynamicBlockError> {
        if self.visibility_states.len() >= MAX_VISIBILITY_STATES {
            return Err(DynamicBlockError::TooComplex);
        }
        if self
            .visibility_states
            .iter()
            .any(|item| item.name == state.name)
        {
            return Err(DynamicBlockError::InvalidVisibility(
                "duplicate state".into(),
            ));
        }
        self.visibility_states.push(state);
        Ok(())
    }

    pub fn replace_visibility_state(
        &mut self,
        name: &str,
        state: DynamicVisibilityState,
    ) -> Result<(), DynamicBlockError> {
        let Some(existing) = self
            .visibility_states
            .iter_mut()
            .find(|item| item.name == name)
        else {
            return self.add_visibility_state(state);
        };
        *existing = state;
        Ok(())
    }

    pub fn validate(&self, entity_ids: &[u64]) -> Result<(), DynamicBlockError> {
        for parameter in &self.parameters {
            parameter.validate_definition()?;
            if let DynamicParameterType::Visibility { states } = &parameter.parameter_type
                && states.iter().any(|name| {
                    !self
                        .visibility_states
                        .iter()
                        .any(|state| state.name == *name)
                })
            {
                return Err(DynamicBlockError::InvalidVisibility(
                    "parameter state is not defined".into(),
                ));
            }
        }
        for state in &self.visibility_states {
            if state.entity_ids.iter().any(|id| !entity_ids.contains(id)) {
                return Err(DynamicBlockError::InvalidVisibility(
                    "entity ID out of range".into(),
                ));
            }
        }
        for targets in &self.action_targets {
            if targets.iter().any(|id| !entity_ids.contains(id)) {
                return Err(DynamicBlockError::InvalidAction(
                    "target entity ID out of range".into(),
                ));
            }
        }
        for action in &self.actions {
            self.validate_action(action)?;
        }
        Ok(())
    }

    fn validate_action(&self, action: &DynamicAction) -> Result<(), DynamicBlockError> {
        match action {
            DynamicAction::Move {
                parameter,
                origin,
                direction,
            } => {
                let parameter_type = self.parameter_type(parameter)?;
                if !matches!(
                    parameter_type,
                    DynamicParameterType::Distance { .. }
                        | DynamicParameterType::Point { .. }
                        | DynamicParameterType::XY { .. }
                ) {
                    return Err(DynamicBlockError::InvalidAction(
                        "move requires distance or point".into(),
                    ));
                }
                if !finite_point(*origin)
                    || !finite_point(*direction)
                    || (direction.x() == 0.0 && direction.y() == 0.0)
                {
                    return Err(DynamicBlockError::InvalidAction("move direction".into()));
                }
            }
            DynamicAction::Scale { parameter, center } => {
                if !matches!(
                    self.parameter_type(parameter)?,
                    DynamicParameterType::Distance { .. }
                ) || !finite_point(*center)
                {
                    return Err(DynamicBlockError::InvalidAction(
                        "scale requires distance and finite center".into(),
                    ));
                }
            }
            DynamicAction::Rotate { parameter, center } => {
                if !matches!(
                    self.parameter_type(parameter)?,
                    DynamicParameterType::Angle { .. }
                ) || !finite_point(*center)
                {
                    return Err(DynamicBlockError::InvalidAction(
                        "rotate requires angle and finite center".into(),
                    ));
                }
            }
            DynamicAction::Flip {
                parameter,
                first,
                second,
            } => {
                if !matches!(self.parameter_type(parameter)?, DynamicParameterType::Flip)
                    || !finite_point(*first)
                    || !finite_point(*second)
                    || *first == *second
                {
                    return Err(DynamicBlockError::InvalidAction(
                        "flip requires a valid axis".into(),
                    ));
                }
            }
            DynamicAction::Stretch {
                parameter,
                min,
                max,
                direction,
            } => {
                if !matches!(
                    self.parameter_type(parameter)?,
                    DynamicParameterType::Distance { .. }
                        | DynamicParameterType::Point { .. }
                        | DynamicParameterType::XY { .. }
                ) || !finite_point(*min)
                    || !finite_point(*max)
                    || !finite_point(*direction)
                    || min.x() > max.x()
                    || min.y() > max.y()
                {
                    return Err(DynamicBlockError::InvalidAction("stretch window".into()));
                }
            }
            DynamicAction::Array {
                parameter,
                rows,
                columns,
                row_spacing,
                column_spacing,
            } => {
                if let Some(parameter) = parameter
                    && !matches!(
                        self.parameter_type(parameter)?,
                        DynamicParameterType::Distance { .. } | DynamicParameterType::XY { .. }
                    )
                {
                    return Err(DynamicBlockError::InvalidAction(
                        "array parameter type".into(),
                    ));
                }
                if *rows == 0
                    || *columns == 0
                    || rows.saturating_mul(*columns) > MAX_ARRAY_ITEMS
                    || !row_spacing.is_finite()
                    || !column_spacing.is_finite()
                {
                    return Err(DynamicBlockError::InvalidAction("array dimensions".into()));
                }
            }
        }
        Ok(())
    }

    fn parameter_type(&self, name: &str) -> Result<&DynamicParameterType, DynamicBlockError> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .map(|parameter| &parameter.parameter_type)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))
    }

    fn parameter_value(
        &self,
        name: &str,
        overrides: &DynamicBlockOverrides,
    ) -> Result<DynamicValue, DynamicBlockError> {
        let parameter = self
            .parameters
            .iter()
            .find(|item| item.name == name)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))?;
        let value = overrides
            .values
            .get(name)
            .cloned()
            .unwrap_or_else(|| parameter.default.clone());
        validate_value(&parameter.parameter_type, &value)?;
        Ok(value)
    }

    fn parameter_values(
        &self,
        name: &str,
        overrides: &DynamicBlockOverrides,
    ) -> Result<(DynamicValue, DynamicValue), DynamicBlockError> {
        let parameter = self
            .parameters
            .iter()
            .find(|item| item.name == name)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))?;
        let value = overrides
            .values
            .get(name)
            .cloned()
            .unwrap_or_else(|| parameter.default.clone());
        validate_value(&parameter.parameter_type, &value)?;
        Ok((value, parameter.default.clone()))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DynamicBlockOverrides {
    values: HashMap<String, DynamicValue>,
    visibility: Option<String>,
}

impl DynamicBlockOverrides {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn values(&self) -> &HashMap<String, DynamicValue> {
        &self.values
    }
    pub fn visibility(&self) -> Option<&str> {
        self.visibility.as_deref()
    }
    pub fn set(&mut self, name: impl Into<String>, value: DynamicValue) {
        self.values.insert(name.into(), value);
    }
    pub fn set_visibility(&mut self, name: impl Into<String>) {
        self.visibility = Some(name.into());
    }
    pub fn reset(&mut self, name: &str) {
        self.values.remove(name);
    }
    pub fn reset_visibility(&mut self) {
        self.visibility = None;
    }
    pub fn reset_all(&mut self) {
        self.values.clear();
        self.visibility = None;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DynamicBlockError {
    InvalidParameter(String),
    InvalidValue(String),
    UnknownParameter(String),
    InvalidAction(String),
    InvalidVisibility(String),
    UnknownVisibility(String),
    UnsupportedStretchEntity(&'static str),
    TooComplex,
    Geometry(GeometryError),
}

impl fmt::Display for DynamicBlockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParameter(message) => {
                write!(formatter, "invalid dynamic parameter: {message}")
            }
            Self::InvalidValue(message) => write!(formatter, "invalid dynamic value: {message}"),
            Self::UnknownParameter(name) => write!(formatter, "unknown dynamic parameter {name}"),
            Self::InvalidAction(message) => write!(formatter, "invalid dynamic action: {message}"),
            Self::InvalidVisibility(message) => write!(formatter, "invalid visibility: {message}"),
            Self::UnknownVisibility(name) => write!(formatter, "unknown visibility state {name}"),
            Self::UnsupportedStretchEntity(name) => {
                write!(formatter, "stretch does not support entity {name}")
            }
            Self::TooComplex => formatter.write_str("dynamic block exceeds complexity limits"),
            Self::Geometry(error) => write!(formatter, "dynamic geometry error: {error:?}"),
        }
    }
}

impl std::error::Error for DynamicBlockError {}
impl From<GeometryError> for DynamicBlockError {
    fn from(error: GeometryError) -> Self {
        Self::Geometry(error)
    }
}

pub fn evaluate_dynamic_insert(
    definition: &BlockDefinition,
    dynamic: &DynamicBlockDefinition,
    insert: &Insert,
    overrides: &DynamicBlockOverrides,
) -> Result<Vec<Entity>, DynamicBlockError> {
    let evaluated = evaluate_local_entities(definition, dynamic, overrides)?;
    let mut evaluated_definition = definition.clone();
    evaluated_definition.set_entities(evaluated);
    insert.explode(&evaluated_definition).map_err(Into::into)
}

pub fn evaluate_local_entities(
    definition: &BlockDefinition,
    dynamic: &DynamicBlockDefinition,
    overrides: &DynamicBlockOverrides,
) -> Result<Vec<Entity>, DynamicBlockError> {
    dynamic.validate_overrides(overrides)?;
    dynamic.validate(definition.entity_ids())?;
    let mut entities = visible_entities(definition, dynamic, overrides)?;
    for (action_index, action) in dynamic.actions().iter().enumerate() {
        let targets = dynamic
            .action_targets
            .get(action_index)
            .filter(|targets| !targets.is_empty())
            .map(Vec::as_slice);
        apply_action(&mut entities, action, targets, dynamic, overrides)?;
    }
    Ok(entities.into_iter().map(|(_, entity)| entity).collect())
}

/// Evaluate an INSERT using the overrides stored on that INSERT.
pub fn evaluate_insert(
    definition: &BlockDefinition,
    dynamic: &DynamicBlockDefinition,
    insert: &Insert,
) -> Result<Vec<Entity>, DynamicBlockError> {
    evaluate_dynamic_insert(definition, dynamic, insert, insert.dynamic_overrides())
}

fn visible_entities(
    definition: &BlockDefinition,
    dynamic: &DynamicBlockDefinition,
    overrides: &DynamicBlockOverrides,
) -> Result<Vec<(u64, Entity)>, DynamicBlockError> {
    let entities = definition.entities();
    let ids = definition.entity_ids();
    let parameter_visibility = dynamic
        .parameters
        .iter()
        .find(|parameter| {
            matches!(
                parameter.parameter_type,
                DynamicParameterType::Visibility { .. }
            )
        })
        .map(|parameter| {
            let value = overrides
                .values
                .get(&parameter.name)
                .unwrap_or(&parameter.default);
            match value {
                DynamicValue::Visibility(name) => Ok(name.as_str()),
                _ => Err(DynamicBlockError::InvalidValue("visibility value".into())),
            }
        })
        .transpose()?;
    let name = overrides.visibility.as_deref().or(parameter_visibility);
    let Some(name) = name else {
        return Ok(ids.iter().copied().zip(entities.iter().cloned()).collect());
    };
    let state = dynamic
        .visibility_states
        .iter()
        .find(|state| state.name == name)
        .ok_or_else(|| DynamicBlockError::UnknownVisibility(name.into()))?;
    Ok(state
        .entity_ids
        .iter()
        .filter_map(|id| {
            ids.iter()
                .position(|candidate| candidate == id)
                .map(|index| (*id, entities[index].clone()))
        })
        .collect())
}

fn apply_action(
    entities: &mut Vec<(u64, Entity)>,
    action: &DynamicAction,
    targets: Option<&[u64]>,
    dynamic: &DynamicBlockDefinition,
    overrides: &DynamicBlockOverrides,
) -> Result<(), DynamicBlockError> {
    match action {
        DynamicAction::Move {
            parameter,
            origin: _origin,
            direction,
        } => {
            let (value, default) = dynamic.parameter_values(parameter, overrides)?;
            let value = delta_value(value, default)?;
            let (dx, dy) = move_delta_value(value, *direction)?;
            let transform = Transform::translation(dx, dy, 0.0);
            transform_entities(entities, targets, transform)
        }
        DynamicAction::Scale { parameter, center } => {
            let (value, default) = dynamic.parameter_values(parameter, overrides)?;
            let (DynamicValue::Number(value), DynamicValue::Number(default)) = (value, default)
            else {
                return Err(DynamicBlockError::InvalidValue("scale value".into()));
            };
            if default <= 0.0 {
                return Err(DynamicBlockError::InvalidValue("scale default".into()));
            }
            let factor = value / default;
            transform_entities(entities, targets, Transform::scale(*center, factor)?)
        }
        DynamicAction::Rotate { parameter, center } => {
            let (value, default) = dynamic.parameter_values(parameter, overrides)?;
            let (DynamicValue::Number(value), DynamicValue::Number(default)) = (value, default)
            else {
                return Err(DynamicBlockError::InvalidValue("angle value".into()));
            };
            let angle = value - default;
            transform_entities(entities, targets, Transform::rotate_xy(*center, angle)?)
        }
        DynamicAction::Flip {
            parameter,
            first,
            second,
        } => {
            let (value, default) = dynamic.parameter_values(parameter, overrides)?;
            let (DynamicValue::Flip(value), DynamicValue::Flip(default)) = (value, default) else {
                return Err(DynamicBlockError::InvalidValue("flip value".into()));
            };
            if value != default {
                transform_entities(entities, targets, Transform::mirror_xy(*first, *second)?)
            } else {
                Ok(())
            }
        }
        DynamicAction::Stretch {
            parameter,
            min,
            max,
            direction,
        } => apply_stretch(
            entities,
            targets,
            StretchContext {
                parameter,
                min: *min,
                max: *max,
                direction: *direction,
            },
            dynamic,
            overrides,
        ),
        DynamicAction::Array {
            parameter,
            rows,
            columns,
            row_spacing,
            column_spacing,
        } => {
            let (rows, columns) = resolve_array_dimensions(
                parameter.as_deref(),
                *rows,
                *columns,
                *row_spacing,
                *column_spacing,
                dynamic,
                overrides,
            )?;
            apply_array(
                entities,
                targets,
                rows,
                columns,
                *row_spacing,
                *column_spacing,
            )
        }
    }
}

struct StretchContext<'a> {
    parameter: &'a str,
    min: Point3,
    max: Point3,
    direction: Point3,
}

fn resolve_array_dimensions(
    parameter: Option<&str>,
    base_rows: usize,
    base_columns: usize,
    row_spacing: f64,
    column_spacing: f64,
    dynamic: &DynamicBlockDefinition,
    overrides: &DynamicBlockOverrides,
) -> Result<(usize, usize), DynamicBlockError> {
    let Some(parameter) = parameter else {
        return Ok((base_rows, base_columns));
    };
    let (value, default) = dynamic.parameter_values(parameter, overrides)?;
    if value == default {
        return Ok((base_rows, base_columns));
    }
    let count_from_extent = |extent: f64, spacing: f64| {
        if !extent.is_finite() || !spacing.is_finite() || spacing == 0.0 || extent < 0.0 {
            return Err(DynamicBlockError::InvalidValue("array extent".into()));
        }
        let raw_count = extent / spacing.abs();
        if !raw_count.is_finite() || raw_count > (MAX_ARRAY_ITEMS - 1) as f64 {
            return Err(DynamicBlockError::TooComplex);
        }
        let count = raw_count.round() as usize;
        let count = count.checked_add(1).ok_or(DynamicBlockError::TooComplex)?;
        if count == 0 || count > MAX_ARRAY_ITEMS {
            return Err(DynamicBlockError::TooComplex);
        }
        Ok(count)
    };
    match value {
        DynamicValue::Number(extent) => Ok((base_rows, count_from_extent(extent, column_spacing)?)),
        DynamicValue::XY { x, y } => {
            let rows = count_from_extent(y, row_spacing)?;
            let columns = count_from_extent(x, column_spacing)?;
            if rows.saturating_mul(columns) > MAX_ARRAY_ITEMS {
                return Err(DynamicBlockError::TooComplex);
            }
            Ok((rows, columns))
        }
        _ => Err(DynamicBlockError::InvalidValue("array parameter".into())),
    }
}

fn move_delta_value(
    value: DynamicValue,
    direction: Point3,
) -> Result<(f64, f64), DynamicBlockError> {
    match value {
        DynamicValue::Number(distance) => {
            let length = (direction.x() * direction.x() + direction.y() * direction.y()).sqrt();
            if length == 0.0 {
                return Err(DynamicBlockError::InvalidAction("move direction".into()));
            }
            Ok((
                direction.x() * distance / length,
                direction.y() * distance / length,
            ))
        }
        DynamicValue::Point(point) => Ok((point.x(), point.y())),
        DynamicValue::XY { x, y } => Ok((x, y)),
        _ => Err(DynamicBlockError::InvalidValue("move value".into())),
    }
}

fn delta_value(
    value: DynamicValue,
    default: DynamicValue,
) -> Result<DynamicValue, DynamicBlockError> {
    match (value, default) {
        (DynamicValue::Number(value), DynamicValue::Number(default)) => {
            Ok(DynamicValue::Number(value - default))
        }
        (DynamicValue::Point(value), DynamicValue::Point(default)) => {
            Ok(DynamicValue::Point(Point3::new(
                value.x() - default.x(),
                value.y() - default.y(),
                value.z() - default.z(),
            )?))
        }
        (
            DynamicValue::XY { x, y },
            DynamicValue::XY {
                x: default_x,
                y: default_y,
            },
        ) => Ok(DynamicValue::XY {
            x: x - default_x,
            y: y - default_y,
        }),
        _ => Err(DynamicBlockError::InvalidValue("parameter baseline".into())),
    }
}

fn transform_entities(
    entities: &mut [(u64, Entity)],
    targets: Option<&[u64]>,
    transform: Transform,
) -> Result<(), DynamicBlockError> {
    for (id, entity) in entities.iter_mut() {
        if targets.is_none_or(|target_ids| target_ids.contains(id)) {
            *entity = entity.transformed(transform)?;
        }
    }
    Ok(())
}

fn apply_array(
    entities: &mut Vec<(u64, Entity)>,
    targets: Option<&[u64]>,
    rows: usize,
    columns: usize,
    row_spacing: f64,
    column_spacing: f64,
) -> Result<(), DynamicBlockError> {
    let source = entities.clone();
    let count = rows.saturating_mul(columns);
    if count > MAX_ARRAY_ITEMS || source.len().saturating_mul(count) > MAX_ARRAY_ITEMS {
        return Err(DynamicBlockError::TooComplex);
    }
    entities.retain(|(id, _)| !targets.is_none_or(|target_ids| target_ids.contains(id)));
    for row in 0..rows {
        for column in 0..columns {
            let transform = Transform::translation(
                column as f64 * column_spacing,
                row as f64 * row_spacing,
                0.0,
            );
            for (id, entity) in &source {
                if targets.is_none_or(|target_ids| target_ids.contains(id)) {
                    entities.push((*id, entity.transformed(transform)?));
                }
            }
        }
    }
    Ok(())
}

fn apply_stretch(
    entities: &mut [(u64, Entity)],
    targets: Option<&[u64]>,
    context: StretchContext<'_>,
    dynamic: &DynamicBlockDefinition,
    overrides: &DynamicBlockOverrides,
) -> Result<(), DynamicBlockError> {
    let (value, default) = dynamic.parameter_values(context.parameter, overrides)?;
    let value = delta_value(value, default)?;
    let (sx, sy) = move_delta_value(value, context.direction)?;
    for (id, entity) in entities.iter_mut() {
        if !targets.is_none_or(|target_ids| target_ids.contains(id)) {
            continue;
        }
        stretch_entity(entity, context.min, context.max, sx, sy)?;
    }
    Ok(())
}

fn stretch_entity(
    entity: &mut Entity,
    min: Point3,
    max: Point3,
    sx: f64,
    sy: f64,
) -> Result<(), DynamicBlockError> {
    let move_point = |point: Point3| -> Result<Point3, DynamicBlockError> {
        Point3::new(point.x() + sx, point.y() + sy, point.z()).map_err(Into::into)
    };
    let in_window = |point: Point3| {
        point.x() >= min.x() && point.x() <= max.x() && point.y() >= min.y() && point.y() <= max.y()
    };
    match entity {
        Entity::Point(point) if in_window(point.position()) => {
            *point = crate::PointEntity::new(move_point(point.position())?);
        }
        Entity::Point(_) => {}
        Entity::Line(line) => {
            let start = line.start();
            let end = line.end();
            let new_start = if in_window(start) {
                move_point(start)?
            } else {
                start
            };
            let new_end = if in_window(end) {
                move_point(end)?
            } else {
                end
            };
            *line = crate::Line3::new(new_start, new_end);
        }
        Entity::Polyline(polyline) => {
            let vertices = polyline
                .vertices()
                .iter()
                .copied()
                .map(|point| {
                    if in_window(point) {
                        move_point(point)
                    } else {
                        Ok(point)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            polyline.set_vertices(vertices)?;
        }
        Entity::Polyline3(polyline) => {
            let vertices = polyline
                .vertices()
                .iter()
                .copied()
                .map(|point| {
                    if in_window(point) {
                        move_point(point)
                    } else {
                        Ok(point)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            *polyline = crate::Polyline3::new(vertices, polyline.is_closed())?;
        }
        Entity::Face3(_) => {
            return Err(DynamicBlockError::UnsupportedStretchEntity("3DFACE"));
        }
        _ => return Err(DynamicBlockError::UnsupportedStretchEntity(entity.name())),
    }
    Ok(())
}

fn finite_point(point: Point3) -> bool {
    point.x().is_finite() && point.y().is_finite() && point.z().is_finite()
}

fn validate_parameter_type(parameter_type: &DynamicParameterType) -> Result<(), DynamicBlockError> {
    let valid_range = |min: f64, max: f64| min.is_finite() && max.is_finite() && min <= max;
    let valid_pair =
        |min: &[f64], max: &[f64]| min.iter().zip(max).all(|(a, b)| valid_range(*a, *b));
    match parameter_type {
        DynamicParameterType::Distance { min, max } | DynamicParameterType::Angle { min, max }
            if valid_range(*min, *max) =>
        {
            Ok(())
        }
        DynamicParameterType::Point { min, max } if valid_pair(min, max) => Ok(()),
        DynamicParameterType::XY { min, max } if valid_pair(min, max) => Ok(()),
        DynamicParameterType::Flip => Ok(()),
        DynamicParameterType::Visibility { states }
            if !states.is_empty() && states.iter().all(|state| !state.trim().is_empty()) =>
        {
            Ok(())
        }
        _ => Err(DynamicBlockError::InvalidParameter(
            "range or visibility states".into(),
        )),
    }
}

fn validate_value(
    parameter_type: &DynamicParameterType,
    value: &DynamicValue,
) -> Result<(), DynamicBlockError> {
    let in_range =
        |value: f64, min: f64, max: f64| value.is_finite() && value >= min && value <= max;
    let result = match (parameter_type, value) {
        (
            DynamicParameterType::Distance { min, max } | DynamicParameterType::Angle { min, max },
            DynamicValue::Number(value),
        ) => in_range(*value, *min, *max),
        (DynamicParameterType::Point { min, max }, DynamicValue::Point(point)) => {
            in_range(point.x(), min[0], max[0])
                && in_range(point.y(), min[1], max[1])
                && in_range(point.z(), min[2], max[2])
        }
        (DynamicParameterType::XY { min, max }, DynamicValue::XY { x, y }) => {
            in_range(*x, min[0], max[0]) && in_range(*y, min[1], max[1])
        }
        (DynamicParameterType::Flip, DynamicValue::Flip(_)) => true,
        (DynamicParameterType::Visibility { states }, DynamicValue::Visibility(value)) => {
            states.iter().any(|state| state == value)
        }
        _ => false,
    };
    if result {
        Ok(())
    } else {
        Err(DynamicBlockError::InvalidValue(
            "value does not match parameter type or range".into(),
        ))
    }
}
