use super::commands::entity_index;
use super::*;

impl OpenCadApp {
    fn explode_indices(&self, tokens: &[&str]) -> Result<Vec<usize>, String> {
        tokens
            .iter()
            .map(|token| {
                let index = entity_index(token, self.editor.entities().len())?;
                if !self.editor.is_entity_visible(index) {
                    return Err("Explode sources must be visible.".into());
                }
                Ok(index)
            })
            .collect()
    }

    pub(super) fn explode_selected(&mut self) -> Result<(), String> {
        let count = self
            .editor
            .explode_selection()
            .map_err(|error| error.to_string())?;
        let components = self.editor.selection().len();
        self.finish();
        self.message(
            if count == 0 {
                "No polylines selected; geometry unchanged.".into()
            } else {
                format!("{count} polylines exploded into {components} components.")
            },
            false,
        );
        Ok(())
    }

    pub(super) fn explode_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if self.tool != Tool::Explode {
            return None;
        }
        if input.is_empty() {
            return Some(self.explode_selected());
        }
        if input.eq_ignore_ascii_case("EXIT") {
            self.finish();
            self.message("Explode command canceled.", false);
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
            selection.extend(self.explode_indices(&input.split_whitespace().collect::<Vec<_>>())?);
            self.editor.select(selection);
            self.message(self.prompt(), false);
            Ok(())
        })())
    }

    pub(super) fn explode_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() && self.editor.selection().is_empty() {
            self.set_tool(Tool::Explode);
            return Ok(());
        }
        let indices = if tokens.is_empty() {
            self.editor.selection().to_vec()
        } else {
            self.explode_indices(tokens)?
        };
        self.finish();
        self.editor.select(indices);
        self.explode_selected()
    }
}
