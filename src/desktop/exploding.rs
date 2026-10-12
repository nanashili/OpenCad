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
        if indices
            .iter()
            .any(|index| matches!(self.editor.entities().get(*index), Some(Entity::MLeader(_))))
        {
            if indices.iter().any(|index| {
                !matches!(self.editor.entities().get(*index), Some(Entity::MLeader(_)))
            }) {
                return Err(
                    "Explode MULTILEADER selection must contain only MULTILEADER entities.".into(),
                );
            }
            let mut entities = Vec::new();
            for index in &indices {
                if let Some(Entity::MLeader(leader)) = self.editor.entities().get(*index) {
                    entities.extend(
                        leader
                            .geometry()
                            .unwrap_or_else(|_| leader.segments())
                            .into_iter()
                            .map(Entity::Line),
                    );
                    match leader.content() {
                        crate::MLeaderContent::MText(text) => {
                            entities.push(Entity::MText(text.clone()))
                        }
                        crate::MLeaderContent::Block(insert) => {
                            entities.push(Entity::Insert(insert.clone()))
                        }
                    }
                }
            }
            self.editor.select(indices);
            self.editor
                .replace_selection_with_entities(entities)
                .map_err(|error| error.to_string())?;
            self.finish();
            self.message("MULTILEADER exploded into leader lines and content.", false);
            return Ok(());
        }
        if indices.iter().any(|index| {
            matches!(
                self.editor.entities().get(*index),
                Some(Entity::Polyline3(_))
            )
        }) {
            if indices.iter().any(|index| {
                !matches!(
                    self.editor.entities().get(*index),
                    Some(Entity::Polyline3(_))
                )
            }) {
                return Err("Explode 3DPOLY selection must contain only 3DPOLY entities.".into());
            }
            let lines = indices
                .iter()
                .filter_map(|index| match self.editor.entities().get(*index) {
                    Some(Entity::Polyline3(polyline)) => Some(polyline.explode()),
                    _ => None,
                })
                .flatten()
                .map(Entity::Line)
                .collect();
            self.editor.select(indices);
            self.editor
                .replace_selection_with_entities(lines)
                .map_err(|error| error.to_string())?;
            self.finish();
            self.message("3DPOLY exploded into LINE entities.", false);
            return Ok(());
        }
        if indices
            .iter()
            .any(|index| matches!(self.editor.entities().get(*index), Some(Entity::Mesh(_))))
        {
            if indices
                .iter()
                .any(|index| !matches!(self.editor.entities().get(*index), Some(Entity::Mesh(_))))
            {
                return Err("Explode mesh selection must contain only MESH entities.".into());
            }
            let faces = indices
                .iter()
                .filter_map(|index| match self.editor.entities().get(*index) {
                    Some(Entity::Mesh(mesh)) => Some(mesh.faces()),
                    _ => None,
                })
                .flatten()
                .copied()
                .map(Entity::Face3)
                .collect();
            self.editor.select(indices);
            self.editor
                .replace_selection_with_entities(faces)
                .map_err(|error| error.to_string())?;
            self.finish();
            self.message("Mesh exploded into 3DFACE entities.", false);
            return Ok(());
        }
        self.finish();
        self.editor.select(indices);
        self.explode_selected()
    }
}
