use super::commands::{number, parse_point};
use super::*;
use crate::{
    DynamicAction, DynamicBlockDefinition, DynamicParameter, DynamicParameterType, DynamicValue,
    DynamicVisibilityState, Entity,
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
            "BVSTATE" | "BVISIBILITY" => self.dynamic_visibility(tokens),
            "BSHOW" | "BHIDE" => self.dynamic_visibility_toggle(verb, tokens),
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
        definition
            .set_action_targets(definition.actions().len() - 1, ids)
            .map_err(|error| error.to_string())
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
            let state_ids = if verb.eq_ignore_ascii_case("BHIDE") {
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
                overrides.set(name, parse_dynamic_value(dynamic, name, raw_value)?);
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

fn parse_dynamic_parameter(tokens: &[&str]) -> Result<DynamicParameter, String> {
    if tokens.is_empty() {
        return Err("BPARAMETER requires a parameter type.".into());
    }
    match tokens[0].to_ascii_uppercase().as_str() {
        "DISTANCE" | "ANGLE" => {
            if tokens.len() != 5 {
                return Err(
                    "BPARAMETER syntax: BPARAMETER DISTANCE|ANGLE name default min max.".into(),
                );
            }
            let default = number(tokens[2])?;
            let min = number(tokens[3])?;
            let max = number(tokens[4])?;
            let (default, min, max) = if tokens[0].eq_ignore_ascii_case("ANGLE") {
                (default.to_radians(), min.to_radians(), max.to_radians())
            } else {
                (default, min, max)
            };
            let parameter_type = if tokens[0].eq_ignore_ascii_case("ANGLE") {
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
        _ => Err("BPARAMETER supports DISTANCE, ANGLE, POINT, XY, FLIP, and VISIBILITY.".into()),
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
        _ => Err("BACTION supports MOVE, SCALE, ROTATE, FLIP, STRETCH, and ARRAY.".into()),
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
        DynamicParameterType::Point { .. } => DynamicValue::Point(parse_point(raw, 0.0)?),
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
        DynamicValue::Number(value) => value.to_string(),
        DynamicValue::Point(point) => format!("{},{},{}", point.x(), point.y(), point.z()),
        DynamicValue::XY { x, y } => format!("{x},{y}"),
        DynamicValue::Flip(value) => value.to_string(),
        DynamicValue::Visibility(value) => value.clone(),
    }
}
