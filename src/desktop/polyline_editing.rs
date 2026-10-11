use super::commands::entity_index;
use super::*;
use crate::GeometryError;

#[derive(Default)]
pub(super) struct PeditInput {
    source: Option<usize>,
    conversion: bool,
    joining: bool,
    undo: Vec<(usize, u64)>,
}

#[derive(Clone, Copy)]
enum Operation {
    Close,
    Open,
    Reverse,
    Undo,
    Join,
}

impl Operation {
    fn parse(input: &str) -> Option<Self> {
        Some(match input.to_ascii_uppercase().as_str() {
            "CLOSE" | "C" => Self::Close,
            "OPEN" | "O" => Self::Open,
            "REVERSE" | "R" => Self::Reverse,
            "UNDO" | "U" => Self::Undo,
            "JOIN" | "J" => Self::Join,
            _ => return None,
        })
    }
}

impl OpenCadApp {
    pub(super) fn pedit_prompt(&self) -> &'static str {
        if self.pedit_input.joining {
            "PEDIT Join · Select touching XY lines, arcs or open polylines; Enter joins. Source stays selected. Shift removes; All selects visible objects. Escape ends PEDIT."
        } else if self.pedit_input.conversion {
            "PEDIT · Object is not a polyline. Convert it? Yes/No <Yes>. Escape cancels."
        } else if let Some(source) = self.pedit_input.source {
            if matches!(self.editor.entities().get(source), Some(Entity::Polyline(polyline)) if polyline.is_closed())
            {
                "PEDIT · Open / Reverse / Undo; Enter or Exit finishes. Escape ends the session."
            } else {
                "PEDIT · Close / Reverse / Undo / Join; Enter or Exit finishes. Escape ends the session."
            }
        } else {
            "PEDIT · Select one XY polyline, line or arc; or enter its object number. Escape cancels."
        }
    }

    pub(super) fn pedit_joining(&self) -> bool {
        self.tool == Tool::Pedit && self.pedit_input.joining
    }

    pub(super) fn retain_pedit_source(&self, selection: &mut Vec<usize>) {
        if self.pedit_joining()
            && let Some(source) = self.pedit_input.source
        {
            selection.push(source);
        }
    }

    fn pedit_join_selected(&mut self) -> Result<(), String> {
        let source = self
            .pedit_input
            .source
            .ok_or("Select a PEDIT object first.")?;
        let candidates = self.editor.selection().to_vec();
        let count = self
            .editor
            .join_polyline_entities(source, &candidates)
            .map_err(|error| error.to_string())?;
        if count != 0 {
            self.pedit_checkpoint(source);
            self.pedit_input.source = self.editor.selection().first().copied();
        }
        self.pedit_input.joining = false;
        self.editor
            .select(self.pedit_input.source.into_iter().collect());
        self.message(
            if count == 0 {
                "No touching compatible objects selected; geometry unchanged.".into()
            } else {
                format!("{count} objects joined to polyline. {}", self.prompt())
            },
            false,
        );
        Ok(())
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
        if self.pedit_input.joining {
            return Err("Press Enter to finish selecting Join objects first.".into());
        }
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
        if matches!(operation, Operation::Join) {
            if original.is_closed() {
                return Err(GeometryError::ClosedPolylineJoin.to_string());
            }
            self.pedit_input.joining = true;
            self.editor.select(vec![source]);
            self.message(self.prompt(), false);
            return Ok(());
        }
        let mut polyline = original.clone();
        match operation {
            Operation::Close => polyline.set_closed(true),
            Operation::Open => polyline.set_closed(false),
            Operation::Reverse => polyline.reverse(),
            Operation::Undo | Operation::Join => unreachable!("handled above"),
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
        if input.eq_ignore_ascii_case("EXIT") {
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
        if input.is_empty() {
            if self.pedit_joining() {
                return Some(self.pedit_join_selected());
            }
            self.finish();
            self.message("PEDIT finished.", false);
            return Some(Ok(()));
        }
        let tokens: Vec<_> = input.split_whitespace().collect();
        if tokens.first().is_some_and(|token| {
            token.eq_ignore_ascii_case("JOIN") || token.eq_ignore_ascii_case("J")
        }) {
            return Some((|| {
                let indices = self.join_indices(&tokens[1..])?;
                if !self.pedit_joining() {
                    self.pedit_operation(Operation::Join)?;
                }
                if indices.is_empty() {
                    self.message(self.prompt(), false);
                    return Ok(());
                }
                let mut selection = self.editor.selection().to_vec();
                selection.extend(indices);
                self.retain_pedit_source(&mut selection);
                self.editor.select(selection);
                self.pedit_join_selected()
            })());
        }
        if self.pedit_joining() && input.eq_ignore_ascii_case("ALL") {
            self.editor
                .select((0..self.editor.entities().len()).collect());
            self.message(self.prompt(), false);
            return Some(Ok(()));
        }
        if let Some(operation) = Operation::parse(input) {
            return Some(self.pedit_operation(operation));
        }
        if input.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-')) {
            if self.pedit_joining() {
                return Some((|| {
                    let indices = self.join_indices(&tokens)?;
                    let mut selection = self.editor.selection().to_vec();
                    selection.extend(indices);
                    self.retain_pedit_source(&mut selection);
                    self.editor.select(selection);
                    self.message(self.prompt(), false);
                    Ok(())
                })());
            }
            return Some(
                entity_index(input, self.editor.entities().len())
                    .and_then(|index| self.pedit_select(index)),
            );
        }
        if matches!(
            input.to_ascii_uppercase().as_str(),
            "W" | "WIDTH"
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
                "This PEDIT option is not implemented yet; use Close, Open, Reverse, Join or Undo."
                    .into(),
            ));
        }
        None
    }

    pub(super) fn pedit_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let index = tokens
            .first()
            .map(|token| entity_index(token, self.editor.entities().len()))
            .transpose()?;
        let operation = tokens
            .get(1)
            .map(|token| {
                Operation::parse(token)
                    .filter(|operation| !matches!(operation, Operation::Undo))
                    .ok_or("Use Close, Open, Reverse or Join after the PEDIT object number.")
            })
            .transpose()?;
        let candidates = if matches!(operation, Some(Operation::Join)) {
            self.join_indices(&tokens[2..])?
        } else if tokens.len() > 2 {
            return Err(
                "Use PEDIT [object-number [Close|Open|Reverse|Join [object-numbers...]]].".into(),
            );
        } else {
            Vec::new()
        };
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
        if !candidates.is_empty() {
            let mut selection = candidates;
            self.retain_pedit_source(&mut selection);
            self.editor.select(selection);
            self.pedit_join_selected()?;
        }
        Ok(())
    }
}
