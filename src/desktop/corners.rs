use super::commands::{entity_index, number, parse_point};
use super::*;

#[derive(Clone, Copy)]
enum OptionPrompt {
    Radius,
    FirstDistance,
    SecondDistance(f64),
    Trim,
}

#[derive(Default)]
pub(super) struct CornerInput {
    first: Option<(usize, Point3)>,
    option: Option<OptionPrompt>,
    multiple: bool,
    undo: Vec<u64>,
}

impl OpenCadApp {
    pub(super) fn corner_prompt(&self) -> &'static str {
        match self.corner_input.option {
            Some(OptionPrompt::Radius) => {
                "FILLET · Enter a nonnegative radius (Enter keeps the drawing's current radius)."
            }
            Some(OptionPrompt::FirstDistance) => {
                "CHAMFER · Enter the first distance (Enter keeps the drawing's first distance)."
            }
            Some(OptionPrompt::SecondDistance(_)) => {
                "CHAMFER · Enter the second distance (Enter keeps the drawing's second distance)."
            }
            Some(OptionPrompt::Trim) => {
                "FILLET/CHAMFER · Enter Trim or NoTrim (Enter keeps the workspace setting)."
            }
            None if self.corner_input.first.is_some() => {
                "FILLET/CHAMFER · Pick the second line on the side to keep, or enter entity-number x,y[,z]. Shift makes a sharp corner."
            }
            None if self.tool == Tool::Fillet => {
                "FILLET · Pick the first line on the side to keep, or enter entity-number x,y[,z]; [Radius/Trim/Multiple/Undo/Exit]."
            }
            None => {
                "CHAMFER · Pick the first line on the side to keep, or enter entity-number x,y[,z]; [Distance/Trim/Multiple/Undo/Exit]."
            }
        }
    }

    fn current_corner(&self, sharp: bool) -> Result<Corner, String> {
        if self.tool == Tool::Fillet {
            Corner::fillet(if sharp {
                0.0
            } else {
                self.editor.corner_value(CornerParameter::FilletRadius)
            })
        } else {
            Corner::chamfer(
                if sharp {
                    0.0
                } else {
                    self.editor.corner_value(CornerParameter::ChamferFirst)
                },
                if sharp {
                    0.0
                } else {
                    self.editor.corner_value(CornerParameter::ChamferSecond)
                },
            )
        }
        .map_err(|error| error.to_string())
    }

    fn corner_settings(&self) -> String {
        let dimensions = if self.tool == Tool::Fillet {
            format!(
                "Radius {}",
                self.editor.corner_value(CornerParameter::FilletRadius)
            )
        } else {
            format!(
                "Distances {}, {}",
                self.editor.corner_value(CornerParameter::ChamferFirst),
                self.editor.corner_value(CornerParameter::ChamferSecond)
            )
        };
        format!(
            "{dimensions}; {}. {}",
            if self.corner_trim { "Trim" } else { "NoTrim" },
            self.prompt()
        )
    }

    pub(super) fn corner_click(
        &mut self,
        source: Option<usize>,
        pick: Point3,
        sharp: bool,
    ) -> Result<(), String> {
        if self.corner_input.option.is_some() {
            return Err("Answer the corner option in the command field first.".into());
        }
        let source = source.ok_or("No line at that position.")?;
        if !matches!(self.editor.entities().get(source), Some(Entity::Line(_)))
            || !self.editor.is_entity_visible(source)
        {
            return Err("Pick a visible straight line for this corner.".into());
        }
        let Some(first) = self.corner_input.first else {
            self.corner_input.first = Some((source, pick));
            self.editor.select(vec![source]);
            self.message(self.prompt(), false);
            return Ok(());
        };
        let corner = self.current_corner(sharp)?;
        if !self
            .editor
            .corner_lines([first, (source, pick)], corner, self.corner_trim)
            .map_err(|error| error.to_string())?
        {
            return Err("The corner leaves the drawing unchanged.".into());
        }
        self.corner_input.undo.push(self.editor.revision());
        self.corner_input.first = None;
        let created = format!("{} created.", self.tool.name());
        if !self.corner_input.multiple {
            self.finish();
        }
        self.message(format!("{created} {}", self.prompt()), false);
        Ok(())
    }

    pub(super) fn corner_preview(
        &self,
        second: usize,
        pick: Point3,
        sharp: bool,
    ) -> Option<Vec<Entity>> {
        if self.corner_input.option.is_some() {
            return None;
        }
        let (first, first_pick) = self.corner_input.first?;
        if first == second
            || !self.editor.is_entity_visible(first)
            || !self.editor.is_entity_visible(second)
        {
            return None;
        }
        let (Entity::Line(a), Entity::Line(b)) = (
            self.editor.entities().get(first)?,
            self.editor.entities().get(second)?,
        ) else {
            return None;
        };
        let result = self
            .current_corner(sharp)
            .ok()?
            .between_lines(*a, first_pick, *b, pick)
            .ok()?;
        let mut parts = Vec::new();
        if self.corner_trim {
            parts.extend([Entity::Line(result.first()), Entity::Line(result.second())]);
        }
        parts.extend(result.connector().cloned());
        Some(parts)
    }

    fn set_corner_dimensions(&mut self, values: &[(CornerParameter, f64)]) -> Result<(), String> {
        if self
            .editor
            .set_corner_values(values)
            .map_err(|error| error.to_string())?
        {
            self.corner_input.undo.push(self.editor.revision());
        }
        Ok(())
    }

    pub(super) fn corner_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if !matches!(self.tool, Tool::Fillet | Tool::Chamfer) {
            return None;
        }
        if let Some(option) = self.corner_input.option {
            let result = match option {
                OptionPrompt::Trim => {
                    let trim = match input.to_ascii_uppercase().as_str() {
                        "" => self.corner_trim,
                        "T" | "TRIM" => true,
                        "N" | "NO" | "NOTRIM" => false,
                        _ => return Some(Err("Enter Trim or NoTrim.".into())),
                    };
                    self.corner_trim = trim;
                    self.corner_input.option = None;
                    Ok(())
                }
                _ => self.corner_dimension_text(input, option),
            };
            if result.is_ok() {
                self.message(self.corner_settings(), false);
            }
            return Some(result);
        }
        match input.to_ascii_uppercase().as_str() {
            "R" | "RADIUS" if self.tool == Tool::Fillet => {
                self.corner_input.option = Some(OptionPrompt::Radius)
            }
            "D" | "DISTANCE" if self.tool == Tool::Chamfer => {
                self.corner_input.option = Some(OptionPrompt::FirstDistance)
            }
            "T" | "TRIM" => self.corner_input.option = Some(OptionPrompt::Trim),
            "M" | "MULTIPLE" => self.corner_input.multiple = true,
            "U" | "UNDO" => return Some(self.undo_corner()),
            "A" | "ANGLE" | "METHOD" if self.tool == Tool::Chamfer => {
                return Some(Err(
                    "The chamfer angle method is not implemented yet; use Distance.".into(),
                ));
            }
            "P" | "POLYLINE" => {
                return Some(Err(
                    "Polyline corners are not implemented yet; pick two straight lines.".into(),
                ));
            }
            "" | "X" | "EXIT" => {
                self.finish();
                self.message("Corner command finished.", false);
                return Some(Ok(()));
            }
            _ if input
                .starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '.')) =>
            {
                let result = self
                    .corner_pick_text(input)
                    .and_then(|(index, pick)| self.corner_click(Some(index), pick, false));
                return Some(result);
            }
            _ => return None,
        }
        self.message(self.corner_settings(), false);
        Some(Ok(()))
    }

    fn corner_dimension_text(&mut self, input: &str, option: OptionPrompt) -> Result<(), String> {
        let parameter = match option {
            OptionPrompt::Radius => CornerParameter::FilletRadius,
            OptionPrompt::FirstDistance => CornerParameter::ChamferFirst,
            _ => CornerParameter::ChamferSecond,
        };
        let value = if input.is_empty() {
            self.editor.corner_value(parameter)
        } else {
            number(input)?
        };
        crate::corner::dimension(value).map_err(|error| error.to_string())?;
        if matches!(option, OptionPrompt::FirstDistance) {
            self.corner_input.option = Some(OptionPrompt::SecondDistance(value));
        } else {
            if let OptionPrompt::SecondDistance(first) = option {
                self.set_corner_dimensions(&[
                    (CornerParameter::ChamferFirst, first),
                    (CornerParameter::ChamferSecond, value),
                ])?;
            } else {
                self.set_corner_dimensions(&[(parameter, value)])?;
            }
            self.corner_input.option = None;
        }
        Ok(())
    }

    fn corner_pick_text(&self, input: &str) -> Result<(usize, Point3), String> {
        let tokens: Vec<_> = input.split_whitespace().collect();
        if tokens.len() != 2 {
            return Err("Pick syntax: entity-number x,y[,z].".into());
        }
        Ok((
            entity_index(tokens[0], self.editor.entities().len())?,
            parse_point(tokens[1], self.elevation)?,
        ))
    }

    fn undo_corner(&mut self) -> Result<(), String> {
        let revision = self
            .corner_input
            .undo
            .last()
            .ok_or("There are no drawing edits to undo in this corner command.")?;
        if *revision != self.editor.revision() {
            self.corner_input.undo.clear();
            return Err("Corner Undo is unavailable after another drawing edit.".into());
        }
        if !self.editor.undo() {
            return Err("The corner undo history is unavailable.".into());
        }
        self.corner_input.undo.pop();
        self.corner_input.first = None;
        self.message(
            format!("Corner edit undone. {}", self.corner_settings()),
            false,
        );
        Ok(())
    }

    pub(super) fn corner_command(&mut self, tokens: &[&str], chamfer: bool) -> Result<(), String> {
        if tokens.is_empty() {
            self.set_tool(if chamfer { Tool::Chamfer } else { Tool::Fillet });
            self.message(self.corner_settings(), false);
            return Ok(());
        }
        let count = if chamfer { 6 } else { 5 };
        if tokens.len() != count && tokens.len() != count + 1 {
            return Err("Corner syntax: FILLET radius first-number first-pick second-number second-pick [TRIM|NOTRIM]; CHAMFER first-distance second-distance first-number first-pick second-number second-pick [TRIM|NOTRIM].".into());
        }
        let trim = if tokens.len() == count {
            self.corner_trim
        } else {
            match tokens[count].to_ascii_uppercase().as_str() {
                "TRIM" => true,
                "NOTRIM" => false,
                _ => return Err("The corner trim option must be TRIM or NOTRIM.".into()),
            }
        };
        let (corner, offset) = if chamfer {
            (Corner::chamfer(number(tokens[0])?, number(tokens[1])?), 2)
        } else {
            (Corner::fillet(number(tokens[0])?), 1)
        };
        let corner = corner.map_err(|error| error.to_string())?;
        let picks = [
            (
                entity_index(tokens[offset], self.editor.entities().len())?,
                parse_point(tokens[offset + 1], self.elevation)?,
            ),
            (
                entity_index(tokens[offset + 2], self.editor.entities().len())?,
                parse_point(tokens[offset + 3], self.elevation)?,
            ),
        ];
        if !self
            .editor
            .corner_lines(picks, corner, trim)
            .map_err(|error| error.to_string())?
        {
            return Err("The corner leaves the drawing unchanged.".into());
        }
        self.finish();
        self.message(
            if chamfer {
                "Chamfer created."
            } else {
                "Fillet created."
            },
            false,
        );
        Ok(())
    }
}
