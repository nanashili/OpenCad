use super::commands::entity_index;
use super::*;

#[derive(Default)]
pub(super) struct PeditInput {
    source: Option<usize>,
    conversion: bool,
    undo: Vec<(usize, u64)>,
}

#[derive(Clone, Copy)]
enum Operation {
    Close,
    Open,
    Reverse,
    Undo,
}

impl Operation {
    fn parse(input: &str) -> Option<Self> {
        Some(match input.to_ascii_uppercase().as_str() {
            "CLOSE" | "C" => Self::Close,
            "OPEN" | "O" => Self::Open,
            "REVERSE" | "R" => Self::Reverse,
            "UNDO" | "U" => Self::Undo,
            _ => return None,
        })
    }
}

impl OpenCadApp {
    pub(super) fn pedit_prompt(&self) -> &'static str {
        if self.pedit_input.conversion {
            "PEDIT · Object is not a polyline. Convert it? Yes/No <Yes>. Escape cancels."
        } else if let Some(source) = self.pedit_input.source {
            if matches!(self.editor.entities().get(source), Some(Entity::Polyline(polyline)) if polyline.is_closed())
            {
                "PEDIT · Open / Reverse / Undo; Enter or Exit finishes. Escape ends the session."
            } else {
                "PEDIT · Close / Reverse / Undo; Enter or Exit finishes. Escape ends the session."
            }
        } else {
            "PEDIT · Select one XY polyline, line or arc; or enter its object number. Escape cancels."
        }
    }

    pub(super) fn pedit_select(&mut self, index: usize) -> Result<(), String> {
        if self.pedit_input.source.is_some() {
            return Err("Finish the current PEDIT session before selecting another object.".into());
        }
        if !self.editor.is_entity_visible(index) {
            return Err("PEDIT objects must be visible.".into());
        }
        let entity = self
            .editor
            .entities()
            .get(index)
            .ok_or("No object at that index.")?;
        entity.to_polyline_xy().map_err(|error| error.to_string())?;
        self.pedit_input.conversion = !matches!(entity, Entity::Polyline(_));
        self.pedit_input.source = Some(index);
        self.editor.select(vec![index]);
        if self.pedit_input.conversion && self.pedit_accept {
            self.pedit_convert()?;
        } else {
            self.message(self.prompt(), false);
        }
        Ok(())
    }

    fn pedit_checkpoint(&mut self, previous_source: usize) {
        self.pedit_input
            .undo
            .push((previous_source, self.editor.revision()));
        if self.pedit_input.undo.len() > 100 {
            self.pedit_input.undo.remove(0);
        }
    }

    fn pedit_convert(&mut self) -> Result<(), String> {
        let source = self
            .pedit_input
            .source
            .ok_or("Select a PEDIT object first.")?;
        if self
            .editor
            .convert_to_polyline(source)
            .map_err(|error| error.to_string())?
        {
            self.pedit_checkpoint(source);
            self.pedit_input.source = self.editor.selection().first().copied();
        }
        self.pedit_input.conversion = false;
        self.message(self.prompt(), false);
        Ok(())
    }

    fn pedit_operation(&mut self, operation: Operation) -> Result<(), String> {
        let source = self
            .pedit_input
            .source
            .ok_or("Select a PEDIT object first.")?;
        if matches!(operation, Operation::Undo) {
            let Some(&(previous, revision)) = self.pedit_input.undo.last() else {
                self.message("No more operations in this PEDIT session.", false);
                return Ok(());
            };
            if self.editor.revision() != revision {
                self.finish();
                return Err("PEDIT ended because another edit changed the drawing.".into());
            }
            if !self.editor.undo() {
                return Err("No more PEDIT history is available.".into());
            }
            self.pedit_input.undo.pop();
            self.pedit_input.source = Some(previous);
            self.pedit_input.conversion = !matches!(
                self.editor.entities().get(previous),
                Some(Entity::Polyline(_))
            );
            self.editor.select(vec![previous]);
            self.message(self.prompt(), false);
            return Ok(());
        }
        let Some(Entity::Polyline(original)) = self.editor.entities().get(source) else {
            return Err("Confirm conversion to a polyline first.".into());
        };
        let mut polyline = original.clone();
        match operation {
            Operation::Close => polyline.set_closed(true),
            Operation::Open => polyline.set_closed(false),
            Operation::Reverse => polyline.reverse(),
            Operation::Undo => unreachable!("handled above"),
        }
        polyline.length().map_err(|error| error.to_string())?;
        polyline.extrema().map_err(|error| error.to_string())?;
        if self
            .editor
            .update(source, Entity::Polyline(polyline))
            .map_err(|error| error.to_string())?
        {
            self.pedit_checkpoint(source);
        }
        self.editor.select(vec![source]);
        self.message(self.prompt(), false);
        Ok(())
    }

    pub(super) fn pedit_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if self.tool != Tool::Pedit {
            return None;
        }
        if input.eq_ignore_ascii_case("EXIT") || (input.is_empty() && !self.pedit_input.conversion)
        {
            self.finish();
            self.message("PEDIT finished.", false);
            return Some(Ok(()));
        }
        if self.pedit_input.conversion {
            match input.to_ascii_uppercase().as_str() {
                "" | "YES" | "Y" => return Some(self.pedit_convert()),
                "NO" | "N" => {
                    self.pedit_input = PeditInput::default();
                    self.editor.select(Vec::new());
                    self.message(self.prompt(), false);
                    return Some(Ok(()));
                }
                _ => {}
            }
        }
        if let Some(operation) = Operation::parse(input) {
            return Some(self.pedit_operation(operation));
        }
        if input.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-')) {
            return Some(
                entity_index(input, self.editor.entities().len())
                    .and_then(|index| self.pedit_select(index)),
            );
        }
        if matches!(
            input.to_ascii_uppercase().as_str(),
            "J" | "JOIN"
                | "W"
                | "WIDTH"
                | "E"
                | "EDIT"
                | "F"
                | "FIT"
                | "S"
                | "SPLINE"
                | "D"
                | "DECURVE"
                | "M"
                | "MULTIPLE"
        ) {
            return Some(Err(
                "This PEDIT option is not implemented yet; use Close, Open, Reverse or Undo."
                    .into(),
            ));
        }
        None
    }

    pub(super) fn pedit_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() > 2 {
            return Err("Use PEDIT [object-number [Close|Open|Reverse]].".into());
        }
        let index = tokens
            .first()
            .map(|token| entity_index(token, self.editor.entities().len()))
            .transpose()?;
        let operation = tokens
            .get(1)
            .map(|token| {
                Operation::parse(token)
                    .filter(|operation| !matches!(operation, Operation::Undo))
                    .ok_or("Use Close, Open or Reverse after the PEDIT object number.")
            })
            .transpose()?;
        if operation.is_some()
            && !matches!(
                index.and_then(|index| self.editor.entities().get(index)),
                Some(Entity::Polyline(_))
            )
        {
            return Err(
                "Select and confirm line/arc conversion before applying a PEDIT option.".into(),
            );
        }
        let preselected = if self.editor.selection().len() == 1 {
            self.editor.selection().first().copied()
        } else {
            None
        };
        self.set_tool(Tool::Pedit);
        if let Some(index) = index.or(preselected) {
            self.pedit_select(index)?;
        }
        if let Some(operation) = operation {
            self.pedit_operation(operation)?;
        }
        Ok(())
    }
}
