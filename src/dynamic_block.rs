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
const MAX_LOOKUP_ROWS: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct LookupRow {
    input: f64,
    label: String,
    assignments: Vec<LookupAssignment>,
}

impl LookupRow {
    pub fn new(input: f64, label: impl Into<String>) -> Result<Self, DynamicBlockError> {
        let label = label.into();
        if !input.is_finite() || label.trim().is_empty() || label.len() > 256 {
            return Err(DynamicBlockError::InvalidParameter("lookup row".into()));
        }
        Ok(Self {
            input,
            label,
            assignments: Vec::new(),
        })
    }
    pub fn input(&self) -> f64 {
        self.input
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn with_assignments(
        mut self,
        assignments: Vec<LookupAssignment>,
    ) -> Result<Self, DynamicBlockError> {
        validate_assignments(&assignments)?;
        self.assignments = assignments;
        Ok(self)
    }
    pub fn assignments(&self) -> &[LookupAssignment] {
        &self.assignments
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LookupAssignment {
    parameter: String,
    value: DynamicValue,
}

impl LookupAssignment {
    pub fn new(parameter: impl Into<String>, value: DynamicValue) -> Self {
        Self {
            parameter: parameter.into(),
            value,
        }
    }
    pub fn parameter(&self) -> &str {
        &self.parameter
    }
    pub fn value(&self) -> &DynamicValue {
        &self.value
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DynamicParameterType {
    Distance {
        min: f64,
        max: f64,
    },
    Angle {
        min: f64,
        max: f64,
    },
    Point {
        min: [f64; 3],
        max: [f64; 3],
    },
    XY {
        min: [f64; 2],
        max: [f64; 2],
    },
    Polar {
        distance_min: f64,
        distance_max: f64,
        angle_min: f64,
        angle_max: f64,
    },
    Lookup {
        rows: Vec<LookupRow>,
        reverse: bool,
        input_parameters: Vec<String>,
    },
    Alignment {
        point_min: [f64; 3],
        point_max: [f64; 3],
        angle_min: f64,
        angle_max: f64,
    },
    BasePoint {
        min: [f64; 3],
        max: [f64; 3],
    },
    Flip,
    Visibility {
        states: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DynamicValue {
    Number(f64),
    Point(Point3),
    XY { x: f64, y: f64 },
    Polar { distance: f64, angle: f64 },
    Lookup(String),
    Alignment { point: Point3, angle: f64 },
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
    PolarStretch {
        parameter: String,
        center: Point3,
        min: Point3,
        max: Point3,
        rotate_only_targets: Vec<u64>,
    },
    Array {
        parameter: Option<String>,
        rows: usize,
        columns: usize,
        row_spacing: f64,
        column_spacing: f64,
    },
}

impl DynamicAction {
    pub fn polar_stretch(
        parameter: impl Into<String>,
        center: Point3,
        min: Point3,
        max: Point3,
        rotate_only_targets: Vec<u64>,
    ) -> Self {
        Self::PolarStretch {
            parameter: parameter.into(),
            center,
            min,
            max,
            rotate_only_targets,
        }
    }
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
        if let DynamicParameterType::Lookup {
            rows,
            reverse,
            input_parameters,
        } = &parameter.parameter_type
        {
            self.validate_lookup_rows(&parameter.name, rows, *reverse, input_parameters)?;
        }
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

    pub fn set_polar_rotate_only_targets(
        &mut self,
        action_index: usize,
        entity_ids: Vec<u64>,
    ) -> Result<(), DynamicBlockError> {
        if entity_ids.contains(&0)
            || entity_ids
                .iter()
                .enumerate()
                .any(|(index, id)| entity_ids[..index].contains(id))
        {
            return Err(DynamicBlockError::InvalidAction(
                "polar rotate-only target IDs".into(),
            ));
        }
        let Some(action) = self.actions.get_mut(action_index) else {
            return Err(DynamicBlockError::InvalidAction("action index".into()));
        };
        let DynamicAction::PolarStretch {
            rotate_only_targets,
            ..
        } = action
        else {
            return Err(DynamicBlockError::InvalidAction(
                "action is not polar stretch".into(),
            ));
        };
        *rotate_only_targets = entity_ids;
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
        if let Some(parameter) = self.parameter(name)
            && let DynamicParameterType::Lookup {
                input_parameters, ..
            } = &parameter.parameter_type
            && !input_parameters.is_empty()
            && !overrides.values.contains_key(name)
        {
            return Ok(DynamicValue::Lookup(
                self.lookup_label_for_overrides(name, overrides)?
                    .unwrap_or("<unmatched>")
                    .to_owned(),
            ));
        }
        self.parameter_value(name, overrides)
    }

    pub fn lookup_label(&self, name: &str, input: f64) -> Result<&str, DynamicBlockError> {
        let parameter = self
            .parameter(name)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))?;
        let DynamicParameterType::Lookup {
            rows,
            input_parameters,
            ..
        } = &parameter.parameter_type
        else {
            return Err(DynamicBlockError::InvalidValue(
                "not a lookup parameter".into(),
            ));
        };
        if !input_parameters.is_empty() {
            return Err(DynamicBlockError::InvalidValue(
                "multi-property lookup needs named inputs".into(),
            ));
        }
        rows.iter()
            .find(|row| row.input.total_cmp(&input).is_eq())
            .map(|row| row.label.as_str())
            .ok_or_else(|| DynamicBlockError::InvalidValue("lookup input has no row".into()))
    }

    pub fn lookup_label_for_overrides(
        &self,
        name: &str,
        overrides: &DynamicBlockOverrides,
    ) -> Result<Option<&str>, DynamicBlockError> {
        let parameter = self
            .parameter(name)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))?;
        let DynamicParameterType::Lookup {
            rows,
            input_parameters,
            ..
        } = &parameter.parameter_type
        else {
            return Err(DynamicBlockError::InvalidValue(
                "not a lookup parameter".into(),
            ));
        };
        for row in rows {
            let mut matches = true;
            for input_name in input_parameters {
                let Some(assignment) = row
                    .assignments
                    .iter()
                    .find(|assignment| assignment.parameter == *input_name)
                else {
                    matches = false;
                    break;
                };
                let current = self.parameter_value(input_name, overrides)?;
                if current != assignment.value {
                    matches = false;
                    break;
                }
            }
            if matches {
                return Ok(Some(row.label.as_str()));
            }
        }
        Ok(None)
    }

    pub fn lookup_assignments(
        &self,
        name: &str,
        label: &str,
    ) -> Result<&[LookupAssignment], DynamicBlockError> {
        let parameter = self
            .parameter(name)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))?;
        let DynamicParameterType::Lookup { rows, reverse, .. } = &parameter.parameter_type else {
            return Err(DynamicBlockError::InvalidValue(
                "not a lookup parameter".into(),
            ));
        };
        if !reverse {
            return Err(DynamicBlockError::InvalidValue(
                "reverse lookup is disabled".into(),
            ));
        }
        rows.iter()
            .find(|row| row.label == label)
            .map(|row| row.assignments.as_slice())
            .ok_or_else(|| DynamicBlockError::InvalidValue("unknown lookup label".into()))
    }

    pub fn add_lookup_row(&mut self, name: &str, row: LookupRow) -> Result<(), DynamicBlockError> {
        let (mut rows, reverse, input_parameters) = self.lookup_rows(name)?;
        rows.push(row);
        self.validate_lookup_rows(name, &rows, reverse, &input_parameters)?;
        self.replace_lookup_rows(name, rows)
    }

    pub fn replace_lookup_row(
        &mut self,
        name: &str,
        index: usize,
        row: LookupRow,
    ) -> Result<(), DynamicBlockError> {
        let (mut rows, reverse, input_parameters) = self.lookup_rows(name)?;
        let Some(existing) = rows.get_mut(index) else {
            return Err(DynamicBlockError::InvalidParameter(
                "lookup row index".into(),
            ));
        };
        *existing = row;
        self.validate_lookup_rows(name, &rows, reverse, &input_parameters)?;
        self.replace_lookup_rows(name, rows)
    }

    fn lookup_rows(
        &self,
        name: &str,
    ) -> Result<(Vec<LookupRow>, bool, Vec<String>), DynamicBlockError> {
        let parameter = self
            .parameter(name)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))?;
        let DynamicParameterType::Lookup {
            rows,
            reverse,
            input_parameters,
        } = &parameter.parameter_type
        else {
            return Err(DynamicBlockError::InvalidValue(
                "not a lookup parameter".into(),
            ));
        };
        Ok((rows.clone(), *reverse, input_parameters.clone()))
    }

    fn replace_lookup_rows(
        &mut self,
        name: &str,
        rows: Vec<LookupRow>,
    ) -> Result<(), DynamicBlockError> {
        let Some(parameter) = self.parameters.iter_mut().find(|item| item.name == name) else {
            return Err(DynamicBlockError::UnknownParameter(name.into()));
        };
        let DynamicParameterType::Lookup {
            rows: parameter_rows,
            ..
        } = &mut parameter.parameter_type
        else {
            return Err(DynamicBlockError::InvalidValue(
                "not a lookup parameter".into(),
            ));
        };
        *parameter_rows = rows;
        Ok(())
    }

    fn validate_lookup_rows(
        &self,
        parameter_name: &str,
        rows: &[LookupRow],
        reverse: bool,
        input_parameters: &[String],
    ) -> Result<(), DynamicBlockError> {
        let lookup_type = DynamicParameterType::Lookup {
            rows: rows.to_vec(),
            reverse,
            input_parameters: input_parameters.to_vec(),
        };
        validate_parameter_type(&lookup_type)?;
        if input_parameters.is_empty() {
            if rows.iter().any(|row| !row.assignments.is_empty()) {
                return Err(DynamicBlockError::InvalidParameter(
                    "legacy lookup assignments".into(),
                ));
            }
            return Ok(());
        }
        for input_name in input_parameters {
            let input = self
                .parameter(input_name)
                .ok_or_else(|| DynamicBlockError::UnknownParameter(input_name.clone()))?;
            if input_name == parameter_name
                || matches!(input.parameter_type, DynamicParameterType::Lookup { .. })
            {
                return Err(DynamicBlockError::InvalidParameter(
                    "lookup dependency cycle".into(),
                ));
            }
        }
        for row in rows {
            if row.assignments.len() != input_parameters.len()
                || input_parameters.iter().any(|input_name| {
                    row.assignments
                        .iter()
                        .filter(|assignment| assignment.parameter == *input_name)
                        .count()
                        != 1
                })
            {
                return Err(DynamicBlockError::InvalidParameter(
                    "lookup row assignments".into(),
                ));
            }
            for assignment in &row.assignments {
                self.validate_value(&assignment.parameter, &assignment.value)?;
            }
        }
        for (index, left) in rows.iter().enumerate() {
            for right in rows.iter().skip(index + 1) {
                let same_inputs = input_parameters.iter().all(|input_name| {
                    assignment_value(left, input_name) == assignment_value(right, input_name)
                });
                if same_inputs {
                    return Err(DynamicBlockError::InvalidParameter(
                        "duplicate lookup input row".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn lookup_input(&self, name: &str, label: &str) -> Result<f64, DynamicBlockError> {
        let parameter = self
            .parameter(name)
            .ok_or_else(|| DynamicBlockError::UnknownParameter(name.into()))?;
        let DynamicParameterType::Lookup { rows, reverse, .. } = &parameter.parameter_type else {
            return Err(DynamicBlockError::InvalidValue(
                "not a lookup parameter".into(),
            ));
        };
        if !reverse {
            return Err(DynamicBlockError::InvalidValue(
                "reverse lookup is disabled".into(),
            ));
        }
        rows.iter()
            .find(|row| row.label == label)
            .map(|row| row.input)
            .ok_or_else(|| DynamicBlockError::InvalidValue("unknown lookup label".into()))
    }

    pub fn apply_property(
        &self,
        overrides: &mut DynamicBlockOverrides,
        name: &str,
        value: DynamicValue,
    ) -> Result<(), DynamicBlockError> {
        self.validate_value(name, &value)?;
        let mut candidate = overrides.clone();
        if let DynamicValue::Lookup(label) = &value {
            self.apply_lookup_assignments(&mut candidate, name, label)?;
            candidate.set(name, value);
        } else {
            candidate.set(name, value);
        }
        self.refresh_lookup_values(&mut candidate, name)?;
        self.validate_overrides(&candidate)?;
        *overrides = candidate;
        Ok(())
    }

    fn apply_lookup_assignments(
        &self,
        candidate: &mut DynamicBlockOverrides,
        name: &str,
        label: &str,
    ) -> Result<(), DynamicBlockError> {
        for assignment in self.lookup_assignments(name, label)? {
            self.validate_value(&assignment.parameter, &assignment.value)?;
            candidate.set(assignment.parameter.clone(), assignment.value.clone());
        }
        Ok(())
    }

    fn refresh_lookup_values(
        &self,
        candidate: &mut DynamicBlockOverrides,
        edited_name: &str,
    ) -> Result<(), DynamicBlockError> {
        for parameter in &self.parameters {
            let DynamicParameterType::Lookup {
                input_parameters, ..
            } = &parameter.parameter_type
            else {
                continue;
            };
            if input_parameters.is_empty() || edited_name == parameter.name {
                continue;
            }
            if let Some(label) = self.lookup_label_for_overrides(&parameter.name, candidate)? {
                candidate.set(
                    parameter.name.clone(),
                    DynamicValue::Lookup(label.to_owned()),
                );
            } else {
                candidate.reset(&parameter.name);
            }
        }
        Ok(())
    }

    pub fn validate_overrides(
        &self,
        overrides: &DynamicBlockOverrides,
    ) -> Result<(), DynamicBlockError> {
        for (name, value) in &overrides.values {
            self.validate_value(name, value)?;
        }
        for parameter in &self.parameters {
            let DynamicParameterType::Lookup {
                input_parameters, ..
            } = &parameter.parameter_type
            else {
                continue;
            };
            if input_parameters.is_empty() {
                continue;
            }
            let Some(DynamicValue::Lookup(label)) = overrides.values.get(&parameter.name) else {
                continue;
            };
            match self.lookup_label_for_overrides(&parameter.name, overrides)? {
                Some(expected) if expected == label => {}
                _ => {
                    return Err(DynamicBlockError::InvalidValue(
                        "lookup label does not match input properties".into(),
                    ));
                }
            }
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
            if let DynamicParameterType::Lookup {
                rows,
                input_parameters,
                reverse,
                ..
            } = &parameter.parameter_type
            {
                self.validate_lookup_rows(&parameter.name, rows, *reverse, input_parameters)?;
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
            if let DynamicAction::PolarStretch {
                rotate_only_targets,
                ..
            } = action
                && rotate_only_targets
                    .iter()
                    .any(|id| !entity_ids.contains(id))
            {
                return Err(DynamicBlockError::InvalidAction(
                    "polar rotate-only target ID out of range".into(),
                ));
            }
        }
        for (action_index, action) in self.actions.iter().enumerate() {
            if let DynamicAction::PolarStretch {
                rotate_only_targets,
                ..
            } = action
                && let Some(action_targets) = self.action_targets.get(action_index)
                && !action_targets.is_empty()
                && rotate_only_targets
                    .iter()
                    .any(|id| !action_targets.contains(id))
            {
                return Err(DynamicBlockError::InvalidAction(
                    "polar rotate-only target is outside action targets".into(),
                ));
            }
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
                        | DynamicParameterType::Polar { .. }
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
                    DynamicParameterType::Distance { .. } | DynamicParameterType::Polar { .. }
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
                        | DynamicParameterType::Polar { .. }
                ) || !finite_point(*min)
                    || !finite_point(*max)
                    || !finite_point(*direction)
                    || min.x() > max.x()
                    || min.y() > max.y()
                {
                    return Err(DynamicBlockError::InvalidAction("stretch window".into()));
                }
            }
            DynamicAction::PolarStretch {
                parameter,
                center,
                min,
                max,
                rotate_only_targets,
                ..
            } => {
                if !matches!(
                    self.parameter_type(parameter)?,
                    DynamicParameterType::Polar { .. }
                ) || !finite_point(*center)
                    || !finite_point(*min)
                    || !finite_point(*max)
                    || min.x() > max.x()
                    || min.y() > max.y()
                {
                    return Err(DynamicBlockError::InvalidAction(
                        "polar stretch parameters".into(),
                    ));
                }
                if rotate_only_targets.contains(&0)
                    || rotate_only_targets
                        .iter()
                        .enumerate()
                        .any(|(index, id)| rotate_only_targets[..index].contains(id))
                {
                    return Err(DynamicBlockError::InvalidAction(
                        "polar rotate-only targets".into(),
                    ));
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
                        DynamicParameterType::Distance { .. }
                            | DynamicParameterType::XY { .. }
                            | DynamicParameterType::Polar { .. }
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
    apply_block_parameters(&mut entities, dynamic, overrides)?;
    Ok(entities.into_iter().map(|(_, entity)| entity).collect())
}

fn apply_block_parameters(
    entities: &mut [(u64, Entity)],
    dynamic: &DynamicBlockDefinition,
    overrides: &DynamicBlockOverrides,
) -> Result<(), DynamicBlockError> {
    for parameter in &dynamic.parameters {
        let (value, default) = dynamic.parameter_values(&parameter.name, overrides)?;
        match (&parameter.parameter_type, value, default) {
            (
                DynamicParameterType::Alignment { .. },
                DynamicValue::Alignment { point, angle },
                DynamicValue::Alignment {
                    point: default_point,
                    angle: default_angle,
                },
            ) => {
                let delta = delta_value(
                    DynamicValue::Point(point),
                    DynamicValue::Point(default_point),
                )?;
                let DynamicValue::Point(delta) = delta else {
                    unreachable!()
                };
                transform_entities(
                    entities,
                    None,
                    Transform::rotate_xy(default_point, angle - default_angle)?,
                )?;
                transform_entities(
                    entities,
                    None,
                    Transform::translation(delta.x(), delta.y(), delta.z()),
                )?;
            }
            (
                DynamicParameterType::BasePoint { .. },
                DynamicValue::Point(point),
                DynamicValue::Point(default_point),
            ) => {
                transform_entities(
                    entities,
                    None,
                    Transform::translation(
                        default_point.x() - point.x(),
                        default_point.y() - point.y(),
                        default_point.z() - point.z(),
                    ),
                )?;
            }
            _ => {}
        }
    }
    Ok(())
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
            let (dx, dy) = if matches!(value, DynamicValue::Polar { .. }) {
                polar_vector_delta(value, default)?
            } else {
                let value = delta_value(value, default)?;
                move_delta_value(value, *direction)?
            };
            let transform = Transform::translation(dx, dy, 0.0);
            transform_entities(entities, targets, transform)
        }
        DynamicAction::Scale { parameter, center } => {
            let (value, default) = dynamic.parameter_values(parameter, overrides)?;
            let factor = if matches!(value, DynamicValue::Polar { .. }) {
                polar_distance_ratio(value, default)?
            } else {
                let (DynamicValue::Number(value), DynamicValue::Number(default)) = (value, default)
                else {
                    return Err(DynamicBlockError::InvalidValue("scale value".into()));
                };
                if default <= 0.0 {
                    return Err(DynamicBlockError::InvalidValue("scale default".into()));
                }
                value / default
            };
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
        DynamicAction::PolarStretch {
            parameter,
            center,
            min,
            max,
            rotate_only_targets,
        } => {
            let (sx, sy, angle) = polar_action_delta(dynamic, parameter, overrides)?;
            let selected: Vec<u64> = match targets {
                Some(targets) => targets.to_vec(),
                None => entities.iter().map(|(id, _)| *id).collect(),
            };
            let rotate_ids: Vec<u64> = selected
                .iter()
                .copied()
                .filter(|id| rotate_only_targets.contains(id))
                .collect();
            if !rotate_ids.is_empty() {
                transform_entities(
                    entities,
                    Some(&rotate_ids),
                    Transform::rotate_xy(*center, angle)?,
                )?;
            }
            let stretch_ids: Vec<u64> = selected
                .into_iter()
                .filter(|id| !rotate_ids.contains(id))
                .collect();
            if stretch_ids.is_empty() {
                return Ok(());
            }
            apply_stretch_offset(entities, Some(&stretch_ids), *min, *max, sx, sy)
        }
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
        DynamicValue::Polar { distance, .. } => {
            Ok((base_rows, count_from_extent(distance, column_spacing)?))
        }
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

fn polar_vector_delta(
    value: DynamicValue,
    default: DynamicValue,
) -> Result<(f64, f64), DynamicBlockError> {
    let (
        DynamicValue::Polar { distance, angle },
        DynamicValue::Polar {
            distance: default_distance,
            angle: default_angle,
        },
    ) = (value, default)
    else {
        return Err(DynamicBlockError::InvalidValue("polar value".into()));
    };
    Ok((
        distance * angle.cos() - default_distance * default_angle.cos(),
        distance * angle.sin() - default_distance * default_angle.sin(),
    ))
}

fn polar_action_delta(
    dynamic: &DynamicBlockDefinition,
    parameter: &str,
    overrides: &DynamicBlockOverrides,
) -> Result<(f64, f64, f64), DynamicBlockError> {
    let (value, default) = dynamic.parameter_values(parameter, overrides)?;
    let (sx, sy) = polar_vector_delta(value.clone(), default.clone())?;
    let (
        DynamicValue::Polar { angle, .. },
        DynamicValue::Polar {
            angle: default_angle,
            ..
        },
    ) = (value, default)
    else {
        return Err(DynamicBlockError::InvalidValue("polar value".into()));
    };
    Ok((sx, sy, angle - default_angle))
}

fn polar_distance_ratio(
    value: DynamicValue,
    default: DynamicValue,
) -> Result<f64, DynamicBlockError> {
    let (
        DynamicValue::Polar { distance, .. },
        DynamicValue::Polar {
            distance: default_distance,
            ..
        },
    ) = (value, default)
    else {
        return Err(DynamicBlockError::InvalidValue("polar value".into()));
    };
    if default_distance <= 0.0 {
        return Err(DynamicBlockError::InvalidValue(
            "polar default distance".into(),
        ));
    }
    Ok(distance / default_distance)
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
    let (sx, sy) = if matches!(value, DynamicValue::Polar { .. }) {
        polar_vector_delta(value, default)?
    } else {
        let value = delta_value(value, default)?;
        move_delta_value(value, context.direction)?
    };
    apply_stretch_offset(entities, targets, context.min, context.max, sx, sy)
}

fn apply_stretch_offset(
    entities: &mut [(u64, Entity)],
    targets: Option<&[u64]>,
    min: Point3,
    max: Point3,
    sx: f64,
    sy: f64,
) -> Result<(), DynamicBlockError> {
    for (id, entity) in entities.iter_mut() {
        if !targets.is_none_or(|target_ids| target_ids.contains(id)) {
            continue;
        }
        stretch_entity(entity, min, max, sx, sy)?;
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
        DynamicParameterType::Polar {
            distance_min,
            distance_max,
            angle_min,
            angle_max,
        } if valid_range(*distance_min, *distance_max) && valid_range(*angle_min, *angle_max) => {
            Ok(())
        }
        DynamicParameterType::Lookup {
            rows,
            input_parameters,
            reverse,
        } if !rows.is_empty()
            && rows.len() <= MAX_LOOKUP_ROWS
            && input_parameters
                .iter()
                .all(|name| !name.trim().is_empty() && name.len() <= 128)
            && input_parameters.iter().enumerate().all(|(index, name)| {
                !input_parameters[..index].iter().any(|prior| prior == name)
            })
            && (!input_parameters.is_empty()
                || rows.windows(2).all(|pair| pair[0].input < pair[1].input))
            && (!*reverse
                || rows.iter().enumerate().all(|(index, row)| {
                    !rows[..index].iter().any(|prior| prior.label == row.label)
                }))
            && rows
                .iter()
                .all(|row| validate_assignments(&row.assignments).is_ok()) =>
        {
            Ok(())
        }
        DynamicParameterType::Alignment {
            point_min,
            point_max,
            angle_min,
            angle_max,
        } if valid_pair(point_min, point_max) && valid_range(*angle_min, *angle_max) => Ok(()),
        DynamicParameterType::BasePoint { min, max } if valid_pair(min, max) => Ok(()),
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

fn validate_assignments(assignments: &[LookupAssignment]) -> Result<(), DynamicBlockError> {
    if assignments.len() > MAX_PARAMETERS {
        return Err(DynamicBlockError::TooComplex);
    }
    for (index, assignment) in assignments.iter().enumerate() {
        if assignment.parameter.trim().is_empty()
            || assignment.parameter.len() > 128
            || assignments[..index]
                .iter()
                .any(|prior| prior.parameter == assignment.parameter)
        {
            return Err(DynamicBlockError::InvalidParameter(
                "lookup assignment parameter".into(),
            ));
        }
        if !dynamic_value_finite(&assignment.value) {
            return Err(DynamicBlockError::InvalidValue(
                "lookup assignment value".into(),
            ));
        }
    }
    Ok(())
}

fn assignment_value<'a>(row: &'a LookupRow, parameter: &str) -> Option<&'a DynamicValue> {
    row.assignments
        .iter()
        .find(|assignment| assignment.parameter == parameter)
        .map(|assignment| &assignment.value)
}

fn dynamic_value_finite(value: &DynamicValue) -> bool {
    match value {
        DynamicValue::Number(number) => number.is_finite(),
        DynamicValue::Point(point) => finite_point(*point),
        DynamicValue::XY { x, y } => x.is_finite() && y.is_finite(),
        DynamicValue::Polar { distance, angle } => distance.is_finite() && angle.is_finite(),
        DynamicValue::Alignment { point, angle } => finite_point(*point) && angle.is_finite(),
        DynamicValue::Flip(_) | DynamicValue::Visibility(_) => true,
        DynamicValue::Lookup(_) => false,
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
        (
            DynamicParameterType::Polar {
                distance_min,
                distance_max,
                angle_min,
                angle_max,
            },
            DynamicValue::Polar { distance, angle },
        ) => {
            in_range(*distance, *distance_min, *distance_max)
                && in_range(*angle, *angle_min, *angle_max)
        }
        (
            DynamicParameterType::Lookup {
                input_parameters, ..
            },
            DynamicValue::Number(value),
        ) => input_parameters.is_empty() && value.is_finite(),
        (DynamicParameterType::Lookup { rows, .. }, DynamicValue::Lookup(label)) => {
            rows.iter().any(|row| row.label == *label)
        }
        (
            DynamicParameterType::Alignment {
                point_min,
                point_max,
                angle_min,
                angle_max,
            },
            DynamicValue::Alignment { point, angle },
        ) => {
            in_range(point.x(), point_min[0], point_max[0])
                && in_range(point.y(), point_min[1], point_max[1])
                && in_range(point.z(), point_min[2], point_max[2])
                && in_range(*angle, *angle_min, *angle_max)
        }
        (DynamicParameterType::BasePoint { min, max }, DynamicValue::Point(point)) => {
            in_range(point.x(), min[0], max[0])
                && in_range(point.y(), min[1], max[1])
                && in_range(point.z(), min[2], max[2])
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
