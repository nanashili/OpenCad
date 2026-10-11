use super::commands::{entity_index, parse_point};
use super::*;

pub(super) struct TrimInput {
    pub(super) boundaries: Vec<usize>,
    picking_boundaries: bool,
    all: bool,
    edge: BoundaryMode,
    edge_prompt: bool,
    undo: Vec<(u64, Vec<usize>)>,
}

impl Default for TrimInput {
    fn default() -> Self {
        Self {
            boundaries: Vec::new(),
            picking_boundaries: true,
            all: false,
            edge: BoundaryMode::NoExtend,
            edge_prompt: false,
            undo: Vec::new(),
        }
    }
}

impl OpenCadApp {
    pub(super) fn trim_prompt(&self) -> &'static str {
        if self.trim_input.edge_prompt {
            return "TRIM/EXTEND · Boundary Edge [Extend/NoExtend] (Enter keeps the current setting).";
        }
        if self.trim_input.picking_boundaries {
            return "TRIM/EXTEND · Select cutting/boundary edges (or enter entity numbers); Enter uses the selection, or all visible curves if empty.";
        }
        if self.tool == Tool::Extend {
            "EXTEND · Pick near the end to extend, or enter entity-number x,y[,z]; [Edge/Cutting edges/Undo/Exit]. Shift trims."
        } else {
            "TRIM · Pick the portion to remove, or enter entity-number x,y[,z]; [Edge/Cutting edges/Undo/Exit]. Shift extends."
        }
    }

    fn trim_boundaries(&self) -> Vec<usize> {
        if self.trim_input.all {
            (0..self.editor.entities().len())
                .filter(|index| self.editor.is_entity_visible(*index))
                .collect()
        } else {
            self.trim_input.boundaries.clone()
        }
    }

    pub(super) fn trim_click(
        &mut self,
        source: Option<usize>,
        pick: Point3,
        shift: bool,
    ) -> Result<(), String> {
        if self.trim_input.edge_prompt {
            return Err("Answer the Edge option in the command field first.".into());
        }
        let source = source.ok_or("No curve at that position.")?;
        if self.trim_input.picking_boundaries {
            if !self.editor.is_entity_visible(source) {
                return Err("Boundary curves must be visible.".into());
            }
            if self.trim_input.boundaries.contains(&source) {
                self.trim_input.boundaries.retain(|index| *index != source);
            } else {
                self.trim_input.boundaries.push(source);
            }
            self.editor.select(self.trim_input.boundaries.clone());
            self.message(self.prompt(), false);
            Ok(())
        } else {
            self.trim_target(source, pick, (self.tool == Tool::Extend) != shift)
        }
    }

    fn trim_target(&mut self, source: usize, pick: Point3, extend: bool) -> Result<(), String> {
        let converted =
            !extend && matches!(self.editor.entities().get(source), Some(Entity::Circle(_)));
        let previous = self.trim_input.boundaries.clone();
        let boundaries = self.trim_boundaries();
        let changed = if extend {
            self.editor
                .extend_entity(source, &boundaries, pick, self.trim_input.edge)
        } else {
            self.editor
                .trim_entity(source, &boundaries, pick, self.trim_input.edge)
        }
        .map_err(|error| error.to_string())?;
        if !changed {
            return Err("The curve no longer exists or its geometry is unchanged.".into());
        }
        // A circle becomes an ARC record at the end of the entity list. Splits
        // append additional fragments. Keep explicit edge picks attached to
        // their surviving geometry across both changes.
        let was_boundary = previous.contains(&source);
        self.trim_input.boundaries = previous
            .iter()
            .filter_map(|index| {
                if converted && *index == source {
                    None
                } else {
                    Some(if converted && *index > source {
                        *index - 1
                    } else {
                        *index
                    })
                }
            })
            .collect();
        if was_boundary {
            self.trim_input
                .boundaries
                .extend_from_slice(self.editor.selection());
            self.trim_input.boundaries.sort_unstable();
            self.trim_input.boundaries.dedup();
        }
        self.trim_input
            .undo
            .push((self.editor.revision(), previous));
        self.message(
            format!(
                "Curve {}. {}",
                if extend { "extended" } else { "trimmed" },
                self.prompt()
            ),
            false,
        );
        Ok(())
    }

    pub(super) fn trim_preview(
        &self,
        source: usize,
        pick: Point3,
        shift: bool,
    ) -> Option<Vec<Entity>> {
        if self.trim_input.picking_boundaries || self.trim_input.edge_prompt {
            return None;
        }
        let boundaries: Vec<_> = self
            .trim_boundaries()
            .into_iter()
            .filter(|index| *index != source && self.editor.is_entity_visible(*index))
            .filter_map(|index| self.editor.entities().get(index).cloned())
            .collect();
        let entity = self.editor.entities().get(source)?;
        if (self.tool == Tool::Extend) != shift {
            entity
                .extend_xy(&boundaries, pick, self.trim_input.edge)
                .ok()
                .map(|entity| vec![entity])
        } else {
            entity.trim_xy(&boundaries, pick, self.trim_input.edge).ok()
        }
    }

    pub(super) fn trim_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if !matches!(self.tool, Tool::Trim | Tool::Extend) {
            return None;
        }
        let upper = input.to_ascii_uppercase();
        if self.trim_input.edge_prompt {
            self.trim_input.edge = match upper.as_str() {
                "E" | "EXTEND" => BoundaryMode::Extend,
                "N" | "NO" | "NOEXTEND" => BoundaryMode::NoExtend,
                "" => self.trim_input.edge,
                _ => return Some(Err("Boundary Edge must be Extend or NoExtend.".into())),
            };
            self.trim_input.edge_prompt = false;
        } else {
            match upper.as_str() {
                "E" | "EDGE" => self.trim_input.edge_prompt = true,
                "C" | "CUTTING" | "CUTTING EDGES" | "BOUNDARIES" => {
                    self.trim_input.boundaries.clear();
                    self.trim_input.all = false;
                    self.trim_input.picking_boundaries = true;
                    self.editor.select(Vec::new());
                }
                "A" | "ALL" => {
                    self.trim_input.all = true;
                    self.trim_input.boundaries.clear();
                    self.trim_input.picking_boundaries = false;
                    self.editor.select(Vec::new());
                }
                "U" | "UNDO" => return Some(self.undo_trim()),
                "" if self.trim_input.picking_boundaries => {
                    self.trim_input.all = self.trim_input.boundaries.is_empty();
                    self.trim_input.picking_boundaries = false;
                }
                "" | "X" | "EXIT" => {
                    self.finish();
                    self.message("Trim/Extend command finished.", false);
                    return Some(Ok(()));
                }
                _ => {
                    if !input.starts_with(|character: char| {
                        character.is_ascii_digit() || matches!(character, '+' | '-' | '.')
                    }) {
                        return None;
                    }
                    return Some(if self.trim_input.picking_boundaries {
                        self.add_trim_boundaries(input)
                    } else {
                        self.trim_target_text(input)
                    });
                }
            }
        }
        self.message(self.prompt(), false);
        Some(Ok(()))
    }

    fn add_trim_boundaries(&mut self, input: &str) -> Result<(), String> {
        let indices = boundary_indices(input, self.editor.entities().len())?;
        if indices
            .iter()
            .any(|index| !self.editor.is_entity_visible(*index))
        {
            return Err("Boundary curves must be visible.".into());
        }
        self.trim_input.boundaries.extend(indices);
        self.trim_input.boundaries.sort_unstable();
        self.trim_input.boundaries.dedup();
        self.editor.select(self.trim_input.boundaries.clone());
        self.message(self.prompt(), false);
        Ok(())
    }

    fn trim_target_text(&mut self, input: &str) -> Result<(), String> {
        let tokens: Vec<_> = input.split_whitespace().collect();
        if tokens.len() != 2 {
            return Err("Use entity-number x,y[,z] to pick a portion or endpoint.".into());
        }
        let source = entity_index(tokens[0], self.editor.entities().len())?;
        let pick = parse_point(tokens[1], self.elevation)?;
        self.trim_target(source, pick, self.tool == Tool::Extend)
    }

    fn undo_trim(&mut self) -> Result<(), String> {
        let (revision, _) = self
            .trim_input
            .undo
            .last()
            .ok_or("There are no trim/extend operations to undo in this command.")?;
        if self.editor.revision() != *revision {
            self.trim_input.undo.clear();
            return Err("Trim/Extend Undo is unavailable after another drawing edit.".into());
        }
        if !self.editor.undo() {
            return Err("The trim/extend undo history is unavailable.".into());
        }
        self.trim_input.boundaries = self.trim_input.undo.pop().unwrap().1;
        self.message(format!("Trim/Extend undone. {}", self.prompt()), false);
        Ok(())
    }

    pub(super) fn trim_command(&mut self, tokens: &[&str], extend: bool) -> Result<(), String> {
        if tokens.is_empty() {
            self.set_tool(if extend { Tool::Extend } else { Tool::Trim });
            return Ok(());
        }
        if !(3..=4).contains(&tokens.len())
            || (tokens.len() == 4 && !tokens[3].eq_ignore_ascii_case("EDGEEXTEND"))
        {
            return Err("TRIM/EXTEND syntax: TRIM|EXTEND entity-number pick-x,y[,z] boundary-numbers|ALL [EDGEEXTEND]".into());
        }
        let source = entity_index(tokens[0], self.editor.entities().len())?;
        let pick = parse_point(tokens[1], self.elevation)?;
        let boundaries = if tokens[2].eq_ignore_ascii_case("ALL") {
            (0..self.editor.entities().len()).collect()
        } else {
            boundary_indices(tokens[2], self.editor.entities().len())?
        };
        let mode = if tokens.len() == 4 {
            BoundaryMode::Extend
        } else {
            BoundaryMode::NoExtend
        };
        let changed = if extend {
            self.editor.extend_entity(source, &boundaries, pick, mode)
        } else {
            self.editor.trim_entity(source, &boundaries, pick, mode)
        }
        .map_err(|error| error.to_string())?;
        if !changed {
            return Err("The curve's geometry is unchanged.".into());
        }
        self.finish();
        self.message(
            if extend {
                "Curve extended."
            } else {
                "Curve trimmed."
            },
            false,
        );
        Ok(())
    }
}

fn boundary_indices(input: &str, count: usize) -> Result<Vec<usize>, String> {
    input
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|token| !token.is_empty())
        .map(|token| entity_index(token, count))
        .collect()
}
