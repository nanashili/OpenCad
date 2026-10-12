use super::commands::{number, parse_point};
use super::*;
use crate::{
    DynamicAction, DynamicActionAxis, DynamicActionOptions, DynamicBlockDefinition,
    DynamicParameter, DynamicParameterType, DynamicValue, DynamicValueComponent, DynamicValueSet,
    DynamicVisibilityState, Entity, LookupAssignment, LookupRow,
};

impl OpenCadApp {
    pub(super) fn feature_dynamic_blocks_command(
        &mut self,
        verb: &str,
        _rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "BPROPERTIES" => self.dynamic_properties(tokens),
            "RESETBLOCK" => self.reset_block_overrides(),
            "BPARAMETER" => self.dynamic_parameter(tokens),
            "BACTION" => self.dynamic_action(tokens),
            "BACTIONPROPERTIES" => self.dynamic_action_properties(tokens),
            "BVALUESET" => self.dynamic_value_set(tokens),
            "BVSTATE" | "BVISIBILITY" => self.dynamic_visibility(tokens),
            "BSHOW" | "BHIDE" | "BVSHOW" | "BVHIDE" => self.dynamic_visibility_toggle(verb, tokens),
            "BLOOKUPTABLE" => self.dynamic_lookup_table(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn dynamic_parameter(&mut self, tokens: &[&str]) -> Result<(), String> {
        let parameter = parse_dynamic_parameter(tokens)?;
        let session = self
            .block_edit_session
            .as_mut()
            .ok_or("BPARAMETER requires an active block editor session.")?;
        session
            .dynamic_definition
            .get_or_insert_with(DynamicBlockDefinition::new)
            .add_parameter(parameter)
            .map_err(|error| error.to_string())
    }

    fn dynamic_action(&mut self, tokens: &[&str]) -> Result<(), String> {
        let action = parse_dynamic_action(tokens)?;
        let ids = {
            let session = self
                .block_edit_session
                .as_mut()
                .ok_or("BACTION requires an active block editor session.")?;
            session.selected_local_ids(&self.editor)?
        };
        let session = self
            .block_edit_session
            .as_mut()
            .ok_or("BACTION requires an active block editor session.")?;
        let definition = session
            .dynamic_definition
            .get_or_insert_with(DynamicBlockDefinition::new);
        definition
            .add_action(action)
            .map_err(|error| error.to_string())?;
        let action_index = definition.actions().len() - 1;
        definition
            .set_action_targets(action_index, ids.clone())
            .map_err(|error| error.to_string())
    }

    fn dynamic_action_properties(&mut self, tokens: &[&str]) -> Result<(), String> {
        let index = tokens
            .first()
            .ok_or("BACTIONPROPERTIES requires a 1-based action index.")?
            .parse::<usize>()
            .map_err(|_| "Action index must be a positive integer.")?;
        if index == 0 {
            return Err("Action index is 1-based and must be positive.".into());
        }
        let session = self
            .block_edit_session
            .as_mut()
            .ok_or("BACTIONPROPERTIES requires an active block editor session.")?;
        let mut definition = session.dynamic_definition.clone().unwrap_or_default();
        let action_index = index - 1;
        let mut options = definition
            .action_options(action_index)
            .ok_or("Action index is outside the active block definition.")?;
        if tokens.len() == 1 {
            self.message(
                format!(
                    "Action {index}: multiplier={}, offset={}°, axis={:?}",
                    options.distance_multiplier,
                    options.angle_offset.to_degrees(),
                    options.axis
                ),
                false,
            );
            return Ok(());
        }
        if !(tokens.len() - 1).is_multiple_of(2) {
            return Err(
                "BACTIONPROPERTIES options require MULTIPLIER, OFFSET, or AXIS pairs.".into(),
            );
        }
        options = parse_action_property_options(tokens, options)?;
        definition
            .set_action_options(action_index, options)
            .map_err(|error| error.to_string())?;
        commit_dynamic_definition(session, definition)
    }

    fn dynamic_value_set(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens
            .first()
            .is_some_and(|value| value.eq_ignore_ascii_case("LIST"))
        {
            return self.list_value_sets(tokens.get(1).copied());
        }
        let name = tokens.first().ok_or(
            "BVALUESET syntax: name component INCREMENT step [origin], LIST value [...], or CLEAR.",
        )?;
        let component = parse_value_component(
            tokens
                .get(1)
                .copied()
                .ok_or("BVALUESET requires a parameter component.")?,
        )?;
        let operation = tokens
            .get(2)
            .copied()
            .ok_or("BVALUESET requires INCREMENT, LIST, or CLEAR.")?;
        let session = self
            .block_edit_session
            .as_mut()
            .ok_or("BVALUESET requires an active block editor session.")?;
        let mut definition = session.dynamic_definition.clone().unwrap_or_default();
        apply_value_set_operation(&mut definition, name, component, operation, tokens)?;
        commit_dynamic_definition(session, definition)
    }

    fn list_value_sets(&mut self, requested: Option<&str>) -> Result<(), String> {
        let session = self
            .block_edit_session
            .as_ref()
            .ok_or("BVALUESET LIST requires an active block editor session.")?;
        let dynamic = session
            .dynamic_definition
            .as_ref()
            .ok_or("The active block has no dynamic definition.")?;
        let mut lines = Vec::new();
        for parameter in dynamic.parameters() {
            if requested.is_some_and(|name| !parameter.name().eq_ignore_ascii_case(name)) {
                continue;
            }
            for (component, values) in parameter.value_sets() {
                lines.push(format!(
                    "{} {:?}: {:?}",
                    parameter.name(),
                    component,
                    values
                ));
            }
        }
        if lines.is_empty() {
            return Err("No matching dynamic value sets are defined.".into());
        }
        self.message(lines.join("\n"), false);
        Ok(())
    }

    fn dynamic_visibility(&mut self, tokens: &[&str]) -> Result<(), String> {
        let session = self
            .block_edit_session
            .as_mut()
            .ok_or("BVSTATE requires an active block editor session.")?;
        if tokens.len() < 2 {
            return Err("BVSTATE syntax: BVSTATE state entity-id [...].".into());
        }
        let ids = tokens[1..]
            .iter()
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| "Visibility IDs must be positive integers.".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let state =
            DynamicVisibilityState::new(tokens[0], ids).map_err(|error| error.to_string())?;
        session
            .dynamic_definition
            .get_or_insert_with(DynamicBlockDefinition::new)
            .add_visibility_state(state)
            .map_err(|error| error.to_string())
    }

    fn dynamic_visibility_toggle(&mut self, verb: &str, tokens: &[&str]) -> Result<(), String> {
        let state = tokens.first().ok_or("BSHOW/BHIDE requires a state name.")?;
        if let Some(session) = self.block_edit_session.as_mut() {
            let ids = session.selected_local_ids(&self.editor)?;
            let mut definition = session.dynamic_definition.clone().unwrap_or_default();
            let all_ids = session
                .local_entities
                .iter()
                .map(|mapping| mapping.local_id)
                .collect::<Vec<_>>();
            let selected = if tokens.len() > 1 {
                tokens[1..]
                    .iter()
                    .map(|value| {
                        value.parse::<u64>().map_err(|_| {
                            "Visibility target IDs must be positive integers.".to_owned()
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                ids
            };
            let visible = selected;
            let state_ids =
                if verb.eq_ignore_ascii_case("BHIDE") || verb.eq_ignore_ascii_case("BVHIDE") {
                    all_ids
                        .into_iter()
                        .filter(|id| !visible.contains(id))
                        .collect()
                } else {
                    visible
                };
            let state_name = (*state).to_owned();
            let state = DynamicVisibilityState::new(state_name.clone(), state_ids)
                .map_err(|error| error.to_string())?;
            definition
                .replace_visibility_state(&state_name, state)
                .map_err(|error| error.to_string())?;
            session.dynamic_definition = Some(definition);
            return Ok(());
        }
        let assignment = format!("VISIBILITY={state}");
        self.dynamic_properties(&[assignment.as_str()])
    }

    fn dynamic_lookup_table(&mut self, tokens: &[&str]) -> Result<(), String> {
        let operation = tokens
            .first()
            .ok_or("BLOOKUPTABLE requires LIST or AUDIT.")?;
        if operation.eq_ignore_ascii_case("CREATE") {
            return self.create_lookup_table(tokens);
        }
        if operation.eq_ignore_ascii_case("ADD") {
            return self.add_lookup_table_row(tokens);
        }
        let session = self
            .block_edit_session
            .as_ref()
            .ok_or("BLOOKUPTABLE requires an active block editor session.")?;
        let dynamic = session
            .dynamic_definition
            .as_ref()
            .ok_or("The active block has no dynamic definition.")?;
        match operation.to_ascii_uppercase().as_str() {
            "LIST" => {
                let requested = tokens.get(1).copied();
                let mut lines = Vec::new();
                for parameter in dynamic.parameters() {
                    let DynamicParameterType::Lookup {
                        rows,
                        reverse,
                        input_parameters,
                    } = parameter.parameter_type()
                    else {
                        continue;
                    };
                    if requested.is_some_and(|name| !parameter.name().eq_ignore_ascii_case(name)) {
                        continue;
                    }
                    lines.push(format!(
                        "{} reverse={} inputs={}",
                        parameter.name(),
                        reverse,
                        input_parameters.join(",")
                    ));
                    for row in rows {
                        lines.push(format!("  {}: {}", row.input(), row.label()));
                        for assignment in row.assignments() {
                            let value = dynamic
                                .parameter(assignment.parameter())
                                .map(|parameter| {
                                    format_dynamic_value(
                                        assignment.value(),
                                        parameter.parameter_type(),
                                    )
                                })
                                .unwrap_or_else(|| format!("{:?}", assignment.value()));
                            lines.push(format!("    {}={value}", assignment.parameter(),));
                        }
                    }
                }
                if lines.is_empty() {
                    return Err(requested.map_or_else(
                        || "No lookup parameters are defined.".to_owned(),
                        |name| format!("Lookup parameter {name} was not found."),
                    ));
                }
                self.message(lines.join("\n"), false);
                Ok(())
            }
            "AUDIT" => {
                let ids = session
                    .local_entities
                    .iter()
                    .map(|mapping| mapping.local_id)
                    .collect::<Vec<_>>();
                dynamic.validate(&ids).map_err(|error| error.to_string())?;
                self.message("Dynamic lookup tables are valid.", false);
                Ok(())
            }
            _ => {
                Err("BLOOKUPTABLE syntax: CREATE ..., ADD name row, LIST [name], or AUDIT.".into())
            }
        }
    }

    fn create_lookup_table(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 5 {
            return Err(
                "BLOOKUPTABLE CREATE name default-label input1,input2 label=value|value [...]."
                    .into(),
            );
        }

        let session = self
            .block_edit_session
            .as_mut()
            .ok_or("BLOOKUPTABLE requires an active block editor session.")?;
        let mut definition = session.dynamic_definition.clone().unwrap_or_default();
        let parameter = prepare_lookup_parameter(&definition, tokens)?;

        definition
            .add_parameter(parameter)
            .map_err(|error| error.to_string())?;

        let entity_ids = session
            .local_entities
            .iter()
            .map(|mapping| mapping.local_id)
            .collect::<Vec<_>>();
        definition
            .validate(&entity_ids)
            .map_err(|error| error.to_string())?;

        session.dynamic_definition = Some(definition);
        Ok(())
    }

    fn add_lookup_table_row(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 3 {
            return Err("BLOOKUPTABLE ADD name label=value|value.".into());
        }
        let session = self
            .block_edit_session
            .as_mut()
            .ok_or("BLOOKUPTABLE requires an active block editor session.")?;
        let mut definition = session.dynamic_definition.clone().unwrap_or_default();
        let (input_parameters, next_input) = match definition
            .parameter(tokens[1])
            .map(|parameter| parameter.parameter_type())
        {
            Some(DynamicParameterType::Lookup {
                rows,
                input_parameters,
                ..
            }) => (input_parameters.clone(), rows.len() as f64),
            Some(_) => return Err(format!("{} is not a lookup parameter.", tokens[1])),
            None => return Err(format!("Unknown lookup parameter {}.", tokens[1])),
        };
        let row = parse_lookup_row(&definition, &input_parameters, tokens[2], next_input)?;
        definition
            .add_lookup_row(tokens[1], row)
            .map_err(|error| error.to_string())?;
        definition
            .validate(
                &session
                    .local_entities
                    .iter()
                    .map(|mapping| mapping.local_id)
                    .collect::<Vec<_>>(),
            )
            .map_err(|error| error.to_string())?;
        session.dynamic_definition = Some(definition);
        Ok(())
    }

    fn dynamic_properties(&mut self, tokens: &[&str]) -> Result<(), String> {
        let selection = self.editor.selection().to_vec();
        if selection.is_empty() {
            return Err("Select an INSERT before changing dynamic properties.".into());
        }
        if tokens.is_empty() {
            let listing = self.list_dynamic_properties(&selection)?;
            self.message(listing, false);
            return Ok(());
        }
        if tokens.len() != 1 {
            return Err(
                "BPROPERTIES accepts one name=value assignment or no arguments to list values."
                    .into(),
            );
        }
        let replacements = self.prepare_dynamic_property_replacements(&selection, tokens[0])?;
        self.editor
            .update_selection_atomic(replacements)
            .map_err(|error| error.to_string())?;
        self.message("Dynamic block properties updated.", false);
        Ok(())
    }

    fn list_dynamic_properties(&self, selection: &[usize]) -> Result<String, String> {
        let mut lines = Vec::new();
        for index in selection {
            let Entity::Insert(insert) = &self.editor.entities()[*index] else {
                return Err("BPROPERTIES selection must contain only INSERT entities.".into());
            };
            let definition = self
                .editor
                .document()
                .block(insert.block_name())
                .ok_or_else(|| format!("Block {} was not found.", insert.block_name()))?;
            let dynamic = definition.dynamic_definition().ok_or_else(|| {
                format!("Block {} has no dynamic definition.", insert.block_name())
            })?;
            dynamic
                .validate_overrides(insert.dynamic_overrides())
                .map_err(|error| error.to_string())?;
            for parameter in dynamic.parameters() {
                let value = dynamic
                    .resolved_value(parameter.name(), insert.dynamic_overrides())
                    .map_err(|error| error.to_string())?;
                lines.push(format!(
                    "{}: {}",
                    parameter.name(),
                    format_dynamic_value(&value, parameter.parameter_type())
                ));
            }
        }
        Ok(lines.join(", "))
    }

    fn prepare_dynamic_property_replacements(
        &self,
        selection: &[usize],
        assignment: &str,
    ) -> Result<Vec<(usize, Entity)>, String> {
        let (name, raw_value) = assignment
            .split_once('=')
            .ok_or("BPROPERTIES syntax: BPROPERTIES parameter=value.")?;
        if name.trim().is_empty() || raw_value.trim().is_empty() {
            return Err("BPROPERTIES requires a non-empty parameter and value.".into());
        }
        let mut replacements = Vec::with_capacity(selection.len());
        for index in selection {
            let Entity::Insert(mut insert) = self.editor.entities()[*index].clone() else {
                return Err("BPROPERTIES selection must contain only INSERT entities.".into());
            };
            let definition = self
                .editor
                .document()
                .block(insert.block_name())
                .ok_or_else(|| format!("Block {} was not found.", insert.block_name()))?;
            let dynamic = definition.dynamic_definition().ok_or_else(|| {
                format!("Block {} has no dynamic definition.", insert.block_name())
            })?;
            let mut overrides = insert.dynamic_overrides().clone();
            if name.eq_ignore_ascii_case("VISIBILITY") {
                if !dynamic
                    .visibility_states()
                    .iter()
                    .any(|state| state.name().eq_ignore_ascii_case(raw_value))
                {
                    return Err(format!("Unknown visibility state {raw_value}."));
                }
                overrides.set_visibility(raw_value);
            } else {
                let value = parse_dynamic_value(dynamic, name, raw_value)?;
                dynamic
                    .apply_property(&mut overrides, name, value)
                    .map_err(|error| error.to_string())?;
            }
            dynamic
                .validate_overrides(&overrides)
                .map_err(|error| error.to_string())?;
            insert.set_dynamic_overrides(overrides);
            crate::evaluate_insert(definition, dynamic, &insert)
                .map_err(|error| format!("Dynamic block evaluation failed: {error}"))?;
            replacements.push((*index, Entity::Insert(insert)));
        }
        Ok(replacements)
    }

    fn reset_block_overrides(&mut self) -> Result<(), String> {
        let selection = self.editor.selection().to_vec();
        if selection.is_empty() {
            return Err("RESETBLOCK requires a selected INSERT.".into());
        }
        let mut replacements = Vec::with_capacity(selection.len());
        for index in selection {
            let Entity::Insert(mut insert) = self.editor.entities()[index].clone() else {
                return Err("RESETBLOCK selection must contain only INSERT entities.".into());
            };
            insert.reset_dynamic_overrides();
            replacements.push((index, Entity::Insert(insert)));
        }
        self.editor
            .update_selection_atomic(replacements)
            .map_err(|error| error.to_string())?;
        self.message("Dynamic block overrides reset.", false);
        Ok(())
    }
}

fn commit_dynamic_definition(
    session: &mut super::feature_block_editor::BlockEditSession,
    definition: DynamicBlockDefinition,
) -> Result<(), String> {
    let entity_ids = session
        .local_entities
        .iter()
        .map(|mapping| mapping.local_id)
        .collect::<Vec<_>>();
    definition
        .validate(&entity_ids)
        .map_err(|error| error.to_string())?;
    session.dynamic_definition = Some(definition);
    Ok(())
}

fn parse_action_property_options(
    tokens: &[&str],
    mut options: DynamicActionOptions,
) -> Result<DynamicActionOptions, String> {
    let mut cursor = 1;
    while cursor < tokens.len() {
        let option = tokens[cursor].to_ascii_uppercase();
        let value = tokens[cursor + 1];
        match option.as_str() {
            "MULTIPLIER" => {
                options.distance_multiplier = number(value)?;
                if options.distance_multiplier <= 0.0 {
                    return Err("MULTIPLIER must be finite and positive.".into());
                }
            }
            "OFFSET" => options.angle_offset = number(value)?.to_radians(),
            "AXIS" => {
                options.axis = match value.to_ascii_uppercase().as_str() {
                    "X" => DynamicActionAxis::X,
                    "Y" => DynamicActionAxis::Y,
                    "XY" => DynamicActionAxis::XY,
                    _ => return Err("AXIS must be X, Y, or XY.".into()),
                };
            }
            _ => return Err("Unknown BACTIONPROPERTIES option.".into()),
        }
        cursor += 2;
    }
    Ok(options)
}

fn apply_value_set_operation(
    definition: &mut DynamicBlockDefinition,
    name: &str,
    component: DynamicValueComponent,
    operation: &str,
    tokens: &[&str],
) -> Result<(), String> {
    if operation.eq_ignore_ascii_case("CLEAR") {
        if tokens.len() != 3 {
            return Err("BVALUESET CLEAR syntax: name component CLEAR.".into());
        }
        return definition
            .clear_parameter_value_set(name, component)
            .map_err(|error| error.to_string());
    }
    let parameter = definition
        .parameter(name)
        .ok_or_else(|| format!("Unknown dynamic parameter {name}."))?;
    let value_set = match operation.to_ascii_uppercase().as_str() {
        "INCREMENT" => {
            if !(4..=5).contains(&tokens.len()) {
                return Err(
                    "BVALUESET INCREMENT syntax: name component INCREMENT step [origin].".into(),
                );
            }
            let step = parse_component_number(tokens[3], component)?;
            if step == 0.0 {
                return Err("BVALUESET increment step must be nonzero.".into());
            }
            let origin = tokens
                .get(4)
                .map(|value| parse_component_number(value, component))
                .transpose()?
                .unwrap_or_else(|| parameter_component_min(parameter, component));
            DynamicValueSet::Increment { step, origin }
        }
        "LIST" => {
            if tokens.len() < 4 {
                return Err("BVALUESET LIST syntax: name component LIST value [...].".into());
            }
            let values = tokens[3..]
                .iter()
                .map(|value| parse_component_number(value, component))
                .collect::<Result<Vec<_>, _>>()?;
            DynamicValueSet::List(values)
        }
        _ => return Err("BVALUESET requires INCREMENT, LIST, or CLEAR.".into()),
    };
    definition
        .set_parameter_value_set(name, component, value_set)
        .map_err(|error| error.to_string())
}

fn parse_polar_pair(value: &str) -> Result<(f64, f64), String> {
    let values: Vec<_> = value.split(',').collect();
    if values.len() != 2 {
        return Err("Polar values require distance,angle.".into());
    }
    Ok((number(values[0])?, number(values[1])?.to_radians()))
}

fn parse_value_component(value: &str) -> Result<DynamicValueComponent, String> {
    match value.to_ascii_uppercase().as_str() {
        "DISTANCE" => Ok(DynamicValueComponent::Distance),
        "ANGLE" => Ok(DynamicValueComponent::Angle),
        "X" => Ok(DynamicValueComponent::X),
        "Y" => Ok(DynamicValueComponent::Y),
        _ => Err("BVALUESET component must be DISTANCE, ANGLE, X, or Y.".into()),
    }
}

fn parse_component_number(value: &str, component: DynamicValueComponent) -> Result<f64, String> {
    let parsed = number(value)?;
    if !parsed.is_finite() {
        return Err("BVALUESET values must be finite.".into());
    }
    Ok(if component == DynamicValueComponent::Angle {
        parsed.to_radians()
    } else {
        parsed
    })
}

fn parameter_component_min(parameter: &DynamicParameter, component: DynamicValueComponent) -> f64 {
    match (parameter.parameter_type(), component) {
        (DynamicParameterType::Distance { min, .. }, DynamicValueComponent::Distance)
        | (DynamicParameterType::Angle { min, .. }, DynamicValueComponent::Angle) => *min,
        (DynamicParameterType::Polar { distance_min, .. }, DynamicValueComponent::Distance) => {
            *distance_min
        }
        (DynamicParameterType::Polar { angle_min, .. }, DynamicValueComponent::Angle) => *angle_min,
        (DynamicParameterType::XY { min, .. }, DynamicValueComponent::X) => min[0],
        (DynamicParameterType::XY { min, .. }, DynamicValueComponent::Y) => min[1],
        _ => 0.0,
    }
}

fn prepare_lookup_parameter(
    definition: &DynamicBlockDefinition,
    tokens: &[&str],
) -> Result<DynamicParameter, String> {
    let input_parameters = tokens[3]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if input_parameters.is_empty() {
        return Err("BLOOKUPTABLE requires at least one input parameter.".into());
    }
    for name in &input_parameters {
        if definition.parameter(name).is_none() {
            return Err(format!("Unknown lookup input parameter {name}."));
        }
    }

    let reverse = tokens
        .last()
        .is_some_and(|value| value.eq_ignore_ascii_case("REVERSE"));
    let row_end = if reverse {
        tokens.len() - 1
    } else {
        tokens.len()
    };
    if row_end == 4 {
        return Err("BLOOKUPTABLE CREATE requires at least one lookup row.".into());
    }
    let rows = tokens[4..row_end]
        .iter()
        .enumerate()
        .map(|(index, row_token)| {
            parse_lookup_row(definition, &input_parameters, row_token, index as f64)
        })
        .collect::<Result<Vec<_>, _>>()?;

    DynamicParameter::new(
        tokens[1],
        DynamicParameterType::Lookup {
            rows,
            reverse,
            input_parameters,
        },
        DynamicValue::Lookup(tokens[2].to_owned()),
    )
    .map_err(|error| error.to_string())
}

fn parse_lookup_row(
    definition: &DynamicBlockDefinition,
    input_parameters: &[String],
    row_token: &str,
    fallback_input: f64,
) -> Result<LookupRow, String> {
    let (label, values) = row_token
        .split_once('=')
        .ok_or("Lookup rows require label=value|value assignments.")?;
    if label.trim().is_empty() {
        return Err("Lookup row labels cannot be empty.".into());
    }
    let raw_values = values.split('|').collect::<Vec<_>>();
    if raw_values.len() != input_parameters.len() {
        return Err(format!(
            "Lookup row {label} requires {} values.",
            input_parameters.len()
        ));
    }
    let assignments = input_parameters
        .iter()
        .zip(raw_values)
        .map(|(name, raw)| {
            parse_dynamic_value(definition, name, raw)
                .map(|value| LookupAssignment::new(name, value))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let input = match assignments.first().map(LookupAssignment::value) {
        Some(DynamicValue::Number(value)) => *value,
        _ => fallback_input,
    };
    LookupRow::new(input, label)
        .map_err(|error| error.to_string())?
        .with_assignments(assignments)
        .map_err(|error| error.to_string())
}

fn parse_dynamic_parameter(tokens: &[&str]) -> Result<DynamicParameter, String> {
    if tokens.is_empty() {
        return Err("BPARAMETER requires a parameter type.".into());
    }
    match tokens[0].to_ascii_uppercase().as_str() {
        "DISTANCE" | "LINEAR" | "ANGLE" | "ROTATION" => {
            if tokens.len() != 5 {
                return Err(
                    "BPARAMETER syntax: BPARAMETER DISTANCE|LINEAR|ANGLE|ROTATION name default min max.".into(),
                );
            }
            let default = number(tokens[2])?;
            let min = number(tokens[3])?;
            let max = number(tokens[4])?;
            let is_angle = matches!(
                tokens[0].to_ascii_uppercase().as_str(),
                "ANGLE" | "ROTATION"
            );
            let (default, min, max) = if is_angle {
                (default.to_radians(), min.to_radians(), max.to_radians())
            } else {
                (default, min, max)
            };
            let parameter_type = if is_angle {
                DynamicParameterType::Angle { min, max }
            } else {
                DynamicParameterType::Distance { min, max }
            };
            DynamicParameter::new(tokens[1], parameter_type, DynamicValue::Number(default))
                .map_err(|error| error.to_string())
        }
        "POINT" => {
            if tokens.len() != 5 {
                return Err("BPARAMETER syntax: BPARAMETER POINT name default min max.".into());
            }
            let default = parse_point(tokens[2], 0.0)?;
            let min = parse_point(tokens[3], 0.0)?;
            let max = parse_point(tokens[4], 0.0)?;
            DynamicParameter::new(
                tokens[1],
                DynamicParameterType::Point {
                    min: [min.x(), min.y(), min.z()],
                    max: [max.x(), max.y(), max.z()],
                },
                DynamicValue::Point(default),
            )
            .map_err(|error| error.to_string())
        }
        "XY" => {
            if tokens.len() != 5 {
                return Err("BPARAMETER syntax: BPARAMETER XY name default min max.".into());
            }
            let parse_xy = |value: &str| -> Result<[f64; 2], String> {
                let values: Vec<_> = value.split(',').collect();
                if values.len() != 2 {
                    return Err("XY values require x,y.".into());
                }
                Ok([number(values[0])?, number(values[1])?])
            };
            let default = parse_xy(tokens[2])?;
            let min = parse_xy(tokens[3])?;
            let max = parse_xy(tokens[4])?;
            DynamicParameter::new(
                tokens[1],
                DynamicParameterType::XY { min, max },
                DynamicValue::XY {
                    x: default[0],
                    y: default[1],
                },
            )
            .map_err(|error| error.to_string())
        }
        "POLAR" => {
            if tokens.len() != 5 {
                return Err("BPARAMETER POLAR name defaultD,defaultA minD,minA maxD,maxA.".into());
            }
            let default = parse_polar_pair(tokens[2])?;
            let min = parse_polar_pair(tokens[3])?;
            let max = parse_polar_pair(tokens[4])?;
            DynamicParameter::new(
                tokens[1],
                DynamicParameterType::Polar {
                    distance_min: min.0,
                    distance_max: max.0,
                    angle_min: min.1,
                    angle_max: max.1,
                },
                DynamicValue::Polar {
                    distance: default.0,
                    angle: default.1,
                },
            )
            .map_err(|error| error.to_string())
        }
        "ALIGNMENT" => {
            if tokens.len() != 8 {
                return Err(
                    "BPARAMETER ALIGNMENT name default-point default-angle min-point min-angle max-point max-angle."
                        .into(),
                );
            }
            let default_point = parse_point(tokens[2], 0.0)?;
            let min_point = parse_point(tokens[4], 0.0)?;
            let max_point = parse_point(tokens[6], 0.0)?;
            DynamicParameter::new(
                tokens[1],
                DynamicParameterType::Alignment {
                    point_min: [min_point.x(), min_point.y(), min_point.z()],
                    point_max: [max_point.x(), max_point.y(), max_point.z()],
                    angle_min: number(tokens[5])?.to_radians(),
                    angle_max: number(tokens[7])?.to_radians(),
                },
                DynamicValue::Alignment {
                    point: default_point,
                    angle: number(tokens[3])?.to_radians(),
                },
            )
            .map_err(|error| error.to_string())
        }
        "BASEPOINT" => {
            if tokens.len() != 5 {
                return Err("BPARAMETER BASEPOINT name default-point min-point max-point.".into());
            }
            let default = parse_point(tokens[2], 0.0)?;
            let min = parse_point(tokens[3], 0.0)?;
            let max = parse_point(tokens[4], 0.0)?;
            DynamicParameter::new(
                tokens[1],
                DynamicParameterType::BasePoint {
                    min: [min.x(), min.y(), min.z()],
                    max: [max.x(), max.y(), max.z()],
                },
                DynamicValue::Point(default),
            )
            .map_err(|error| error.to_string())
        }
        "FLIP" => {
            if tokens.len() != 3 {
                return Err("BPARAMETER syntax: BPARAMETER FLIP name ON|OFF.".into());
            }
            let enabled = matches!(tokens[2].to_ascii_uppercase().as_str(), "ON" | "TRUE" | "1");
            if !enabled
                && !matches!(
                    tokens[2].to_ascii_uppercase().as_str(),
                    "OFF" | "FALSE" | "0"
                )
            {
                return Err("Flip default must be ON or OFF.".into());
            }
            DynamicParameter::new(
                tokens[1],
                DynamicParameterType::Flip,
                DynamicValue::Flip(enabled),
            )
            .map_err(|error| error.to_string())
        }
        "VISIBILITY" => {
            if tokens.len() < 4 {
                return Err(
                    "BPARAMETER syntax: BPARAMETER VISIBILITY name default state [...].".into(),
                );
            }
            let states = tokens[3..]
                .iter()
                .map(|value| (*value).to_owned())
                .collect();
            DynamicParameter::new(
                tokens[1],
                DynamicParameterType::Visibility { states },
                DynamicValue::Visibility(tokens[2].to_owned()),
            )
            .map_err(|error| error.to_string())
        }
        _ => Err("BPARAMETER supports DISTANCE, LINEAR, ANGLE, ROTATION, POINT, XY, POLAR, ALIGNMENT, BASEPOINT, FLIP, and VISIBILITY.".into()),
    }
}

fn parse_dynamic_action(tokens: &[&str]) -> Result<DynamicAction, String> {
    if tokens.len() < 2 {
        return Err("BACTION syntax: BACTION MOVE|SCALE|ROTATE parameter [point].".into());
    }
    match tokens[0].to_ascii_uppercase().as_str() {
        "MOVE" => Ok(DynamicAction::Move {
            parameter: tokens[1].to_owned(),
            origin: parse_point(tokens.get(2).copied().unwrap_or("0,0"), 0.0)?,
            direction: parse_point(tokens.get(3).copied().unwrap_or("1,0"), 0.0)?,
        }),
        "SCALE" => Ok(DynamicAction::Scale {
            parameter: tokens[1].to_owned(),
            center: parse_point(tokens.get(2).copied().unwrap_or("0,0"), 0.0)?,
        }),
        "ROTATE" => Ok(DynamicAction::Rotate {
            parameter: tokens[1].to_owned(),
            center: parse_point(tokens.get(2).copied().unwrap_or("0,0"), 0.0)?,
        }),
        "FLIP" => Ok(DynamicAction::Flip {
            parameter: tokens[1].to_owned(),
            first: parse_point(tokens.get(2).copied().unwrap_or("0,0"), 0.0)?,
            second: parse_point(tokens.get(3).copied().unwrap_or("1,0"), 0.0)?,
        }),
        "STRETCH" => Ok(DynamicAction::Stretch {
            parameter: tokens[1].to_owned(),
            min: parse_point(tokens.get(2).copied().unwrap_or("0,0"), 0.0)?,
            max: parse_point(tokens.get(3).copied().unwrap_or("1,1"), 0.0)?,
            direction: parse_point(tokens.get(4).copied().unwrap_or("1,0"), 0.0)?,
        }),
        "POLARSTRETCH" => {
            if tokens.len() < 5 {
                return Err(
                    "POLARSTRETCH syntax: parameter center min max [ROTATE id ...].".into(),
                );
            }
            let rotate_index = tokens
                .iter()
                .position(|value| value.eq_ignore_ascii_case("ROTATE"));
            let body_end = rotate_index.unwrap_or(tokens.len());
            let rotate_only_targets = if let Some(index) = rotate_index {
                if index + 1 == tokens.len() {
                    return Err("POLARSTRETCH ROTATE requires stable target IDs.".into());
                }
                tokens[index + 1..]
                    .iter()
                    .map(|value| {
                        value.parse::<u64>().map_err(|_| {
                            "POLARSTRETCH target IDs must be positive integers.".to_owned()
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            };
            if body_end != 5 {
                return Err(
                    "POLARSTRETCH requires parameter, center, min, and max before ROTATE.".into(),
                );
            }
            Ok(DynamicAction::polar_stretch(
                tokens[1],
                parse_point(tokens[2], 0.0)?,
                parse_point(tokens[3], 0.0)?,
                parse_point(tokens[4], 0.0)?,
                rotate_only_targets,
            ))
        }
        "ARRAY" => {
            if tokens.len() != 6 {
                return Err(
                    "BACTION ARRAY parameter rows columns row-spacing column-spacing.".into(),
                );
            }
            Ok(DynamicAction::Array {
                parameter: Some(tokens[1].to_owned()),
                rows: tokens[2]
                    .parse()
                    .map_err(|_| "ARRAY rows must be a positive integer.")?,
                columns: tokens[3]
                    .parse()
                    .map_err(|_| "ARRAY columns must be a positive integer.")?,
                row_spacing: number(tokens[4])?,
                column_spacing: number(tokens[5])?,
            })
        }
        _ => Err(
            "BACTION supports MOVE, SCALE, ROTATE, FLIP, STRETCH, POLARSTRETCH, and ARRAY.".into(),
        ),
    }
}

fn parse_dynamic_value(
    definition: &crate::DynamicBlockDefinition,
    name: &str,
    raw: &str,
) -> Result<DynamicValue, String> {
    let parameter = definition
        .parameter(name)
        .ok_or_else(|| format!("Unknown dynamic parameter {name}."))?;
    let value = match parameter.parameter_type() {
        DynamicParameterType::Distance { .. } => DynamicValue::Number(number(raw)?),
        DynamicParameterType::Angle { .. } => DynamicValue::Number(number(raw)?.to_radians()),
        DynamicParameterType::Lookup { .. } => DynamicValue::Lookup(raw.to_owned()),
        DynamicParameterType::Polar { .. } => {
            let (distance, angle) = parse_polar_pair(raw)?;
            DynamicValue::Polar { distance, angle }
        }
        DynamicParameterType::Point { .. } => DynamicValue::Point(parse_point(raw, 0.0)?),
        DynamicParameterType::BasePoint { .. } => DynamicValue::Point(parse_point(raw, 0.0)?),
        DynamicParameterType::XY { .. } => {
            let values: Vec<_> = raw.split(',').collect();
            if values.len() != 2 {
                return Err("XY dynamic values require x,y.".into());
            }
            DynamicValue::XY {
                x: number(values[0])?,
                y: number(values[1])?,
            }
        }
        DynamicParameterType::Flip => match raw.to_ascii_uppercase().as_str() {
            "ON" | "TRUE" | "1" => DynamicValue::Flip(true),
            "OFF" | "FALSE" | "0" => DynamicValue::Flip(false),
            _ => return Err("Flip values must be ON or OFF.".into()),
        },
        DynamicParameterType::Alignment { .. } => {
            let (point, angle) = raw
                .split_once(';')
                .ok_or("Alignment values require x,y,z;angle.")?;
            DynamicValue::Alignment {
                point: parse_point(point, 0.0)?,
                angle: number(angle)?.to_radians(),
            }
        }
        DynamicParameterType::Visibility { .. } => DynamicValue::Visibility(raw.to_owned()),
    };
    definition
        .validate_value(name, &value)
        .map_err(|error| error.to_string())?;
    Ok(value)
}

fn format_dynamic_value(value: &DynamicValue, parameter_type: &DynamicParameterType) -> String {
    match value {
        DynamicValue::Number(value)
            if matches!(parameter_type, DynamicParameterType::Angle { .. }) =>
        {
            value.to_degrees().to_string()
        }
        DynamicValue::Polar { distance, angle }
            if matches!(parameter_type, DynamicParameterType::Polar { .. }) =>
        {
            format!("{distance},{}", angle.to_degrees())
        }
        DynamicValue::Alignment { point, angle }
            if matches!(parameter_type, DynamicParameterType::Alignment { .. }) =>
        {
            format!(
                "{},{},{};{}",
                point.x(),
                point.y(),
                point.z(),
                angle.to_degrees()
            )
        }
        DynamicValue::Polar { distance, angle } => {
            format!("{distance},{}", angle.to_degrees())
        }
        DynamicValue::Alignment { point, angle } => {
            format!(
                "{},{},{};{}",
                point.x(),
                point.y(),
                point.z(),
                angle.to_degrees()
            )
        }
        DynamicValue::Number(value) => value.to_string(),
        DynamicValue::Point(point) => format!("{},{},{}", point.x(), point.y(), point.z()),
        DynamicValue::XY { x, y } => format!("{x},{y}"),
        DynamicValue::Flip(value) => value.to_string(),
        DynamicValue::Visibility(value) => value.clone(),
        DynamicValue::Lookup(value) => value.clone(),
    }
}
