use super::commands::entity_index;
use super::*;

impl OpenCadApp {
    pub(super) fn join_prompt(&self) -> &'static str {
        if self.join_source.is_some() {
            "JOIN · Select objects to join; Enter completes. Source stays selected. Shift removes; cLose (L) closes an arc; Escape cancels."
        } else {
            "JOIN · Select one source, then Enter; or select multiple objects and Enter. Shift removes; All selects visible objects; Escape cancels."
        }
    }

    pub(super) fn join_indices(&self, tokens: &[&str]) -> Result<Vec<usize>, String> {
        tokens
            .iter()
            .map(|token| {
                let index = entity_index(token, self.editor.entities().len())?;
                if !self.editor.is_entity_visible(index) {
                    return Err("Join objects must be visible.".into());
                }
                Ok(index)
            })
            .collect()
    }

    pub(super) fn retain_join_source(&self, selection: &mut Vec<usize>) {
        if self.tool == Tool::Join
            && let Some(source) = self.join_source
        {
            selection.push(source);
        }
    }

    fn confirm_join_source(&mut self, source: usize) -> Result<(), String> {
        self.editor.entities()[source]
            .join_xy(&[])
            .map_err(|error| error.to_string())?;
        self.join_source = Some(source);
        self.message(self.prompt(), false);
        Ok(())
    }

    fn join_selected(&mut self) -> Result<(), String> {
        if self.join_source.is_none() {
            let selection = self.editor.selection();
            if selection.is_empty() {
                self.finish();
                self.message("Join command canceled.", false);
                return Ok(());
            }
            let source = selection
                .iter()
                .copied()
                .find(|index| {
                    matches!(
                        self.editor.entities()[*index],
                        Entity::Line(_) | Entity::Arc(_)
                    )
                })
                .ok_or_else(|| "Select an open XY line or arc as the Join source.".to_string())?;
            if selection.len() == 1 {
                return self.confirm_join_source(source);
            }
            self.confirm_join_source(source)?;
        }
        let source = self.join_source.expect("confirmed join source");
        let selection = self.editor.selection().to_vec();
        let count = self
            .editor
            .join_entities(source, &selection)
            .map_err(|error| error.to_string())?;
        self.finish();
        self.message(
            if count == 0 {
                "No compatible objects selected; geometry unchanged.".into()
            } else {
                format!("{count} objects joined to source.")
            },
            false,
        );
        Ok(())
    }

    fn close_join_arc(&mut self) -> Result<(), String> {
        let source = self
            .join_source
            .ok_or("Select one arc and press Enter before cLose.")?;
        self.editor
            .close_arc_entity(source)
            .map_err(|error| error.to_string())?;
        self.finish();
        self.message("Arc closed into a circle.", false);
        Ok(())
    }

    pub(super) fn join_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if self.tool != Tool::Join {
            return None;
        }
        if input.is_empty() {
            return Some(self.join_selected());
        }
        if input.eq_ignore_ascii_case("CLOSE") || input.eq_ignore_ascii_case("L") {
            return Some(self.close_join_arc());
        }
        if input.eq_ignore_ascii_case("EXIT") {
            self.finish();
            self.message("Join command canceled.", false);
            return Some(Ok(()));
        }
        if input.eq_ignore_ascii_case("ALL") {
            self.editor
                .select((0..self.editor.entities().len()).collect());
            self.message(self.prompt(), false);
            return Some(Ok(()));
        }
        if !input.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-')) {
            return None;
        }
        Some((|| {
            let mut selection = self.editor.selection().to_vec();
            selection.extend(self.join_indices(&input.split_whitespace().collect::<Vec<_>>())?);
            self.retain_join_source(&mut selection);
            self.editor.select(selection);
            self.message(self.prompt(), false);
            Ok(())
        })())
    }

    pub(super) fn join_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() {
            self.set_tool(Tool::Join);
            return if self.editor.selection().is_empty() {
                Ok(())
            } else {
                self.join_selected()
            };
        }
        let close = tokens.last().is_some_and(|token| {
            token.eq_ignore_ascii_case("CLOSE") || token.eq_ignore_ascii_case("L")
        });
        if close && tokens.len() != 2 {
            return Err("Use JOIN source-number cLose.".into());
        }
        let indices = self.join_indices(if close { &tokens[..1] } else { tokens })?;
        let source = indices[0];
        self.set_tool(Tool::Join);
        self.editor.select(indices.clone());
        self.confirm_join_source(source)?;
        if close {
            self.close_join_arc()
        } else if indices.len() > 1 {
            self.join_selected()
        } else {
            Ok(())
        }
    }
}
