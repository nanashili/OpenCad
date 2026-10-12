use super::*;
use crate::{EntitySpace, Transform};

impl OpenCadApp {
    pub(super) fn entity_in_active_space(&self, index: usize) -> bool {
        let Some(space) = self.editor.entity_space(index) else {
            return false;
        };
        match self.editor.active_space() {
            EntitySpace::Model => matches!(space, EntitySpace::Model),
            EntitySpace::Paper(name) => space.matches_layout(&name),
        }
    }

    pub(super) fn feature_space_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb {
            "SPACE" => self.space_command(tokens),
            "CHSPACE" => self.chspace_command(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn space_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        match tokens {
            [] => self.list_spaces(),
            [command] if command.eq_ignore_ascii_case("LIST") => self.list_spaces(),
            [name] if name.eq_ignore_ascii_case("MODEL") => self.activate_space("Model"),
            [name] => {
                let layout = self
                    .layouts
                    .names()
                    .find(|candidate| candidate.eq_ignore_ascii_case(name))
                    .map(str::to_owned)
                    .ok_or_else(|| format!("Unknown paper-space layout '{name}'."))?;
                self.activate_space(&layout)
            }
            _ => Err("SPACE LIST|MODEL|layout-name.".into()),
        }
    }

    fn list_spaces(&mut self) -> Result<(), String> {
        let active = self.editor.active_space();
        let layouts = self.layouts.names().map(str::to_owned).collect::<Vec<_>>();
        let mut model_count = 0usize;
        let mut paper_counts = HashMap::<String, usize>::new();
        for index in 0..self.editor.entities().len() {
            match self.editor.entity_space(index) {
                Some(EntitySpace::Model) => model_count += 1,
                Some(EntitySpace::Paper(name)) => {
                    *paper_counts.entry(name.to_owned()).or_default() += 1;
                }
                None => {}
            }
        }
        let mut memberships = vec![format!("Model={model_count}")];
        memberships.extend(
            layouts
                .iter()
                .filter(|name| !name.eq_ignore_ascii_case("Model"))
                .map(|name| {
                    format!(
                        "{}={}",
                        name,
                        paper_counts.get(name).copied().unwrap_or_default()
                    )
                }),
        );
        self.message(
            format!(
                "Active space: {}. Layout memberships: {}.",
                space_label(active),
                memberships.join(", ")
            ),
            false,
        );
        Ok(())
    }

    fn activate_space(&mut self, layout: &str) -> Result<(), String> {
        self.editor
            .activate_layout(layout)
            .map_err(|error| error.to_string())?;
        self.layouts = self.editor.layouts().clone();
        self.editor.select(Vec::new());
        self.finish();
        self.message(
            format!("{} space active.", space_label(self.editor.active_space())),
            false,
        );
        Ok(())
    }

    fn chspace_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let viewport_index = tokens
            .first()
            .ok_or("CHSPACE viewport-number [entity-number ...].")?
            .parse::<usize>()
            .map_err(|_| "CHSPACE viewport number must be a positive integer.")?;
        if viewport_index == 0 {
            return Err("CHSPACE viewport number must be a positive integer.".into());
        }
        let indices = self.chspace_selection(&tokens[1..])?;
        let layout_name = self.layouts.active().name().to_owned();
        if layout_name.eq_ignore_ascii_case("Model") {
            return Err("CHSPACE requires an active paper-space layout.".into());
        }
        let viewport = self
            .layouts
            .active()
            .viewports()
            .get(viewport_index - 1)
            .ok_or_else(|| format!("Viewport {viewport_index} does not exist."))?;
        let source = self
            .editor
            .entity_space(indices[0])
            .ok_or("Selected entity has no space membership.")?;
        if indices
            .iter()
            .any(|index| self.editor.entity_space(*index) != Some(source))
        {
            return Err("CHSPACE selection must belong to one space.".into());
        }
        let (target, transform) = match source {
            EntitySpace::Model => (
                EntitySpace::Paper(layout_name),
                viewport_transform(viewport.world(), viewport.paper(), viewport.scale(), false)?,
            ),
            EntitySpace::Paper(name) if name.eq_ignore_ascii_case(&layout_name) => (
                EntitySpace::Model,
                viewport_transform(viewport.world(), viewport.paper(), viewport.scale(), true)?,
            ),
            EntitySpace::Paper(name) => {
                return Err(format!(
                    "Selected paper-space entity belongs to layout '{name}', not '{layout_name}'."
                ));
            }
        };
        self.editor
            .change_entities_space(&indices, target, transform)
            .map_err(|error| error.to_string())?;
        self.editor.select(Vec::new());
        Ok(())
    }

    fn chspace_selection(&self, tokens: &[&str]) -> Result<Vec<usize>, String> {
        if tokens.is_empty() {
            if self.editor.selection().is_empty() {
                return Err("CHSPACE requires a selection or entity numbers.".into());
            }
            return Ok(self.editor.selection().to_vec());
        }
        let mut indices = Vec::with_capacity(tokens.len());
        for token in tokens {
            let number = token
                .parse::<usize>()
                .map_err(|_| "CHSPACE entity numbers must be positive integers.")?;
            if number == 0 || number > self.editor.entities().len() {
                return Err(format!("Entity number {number} does not exist."));
            }
            indices.push(number - 1);
        }
        indices.sort_unstable();
        indices.dedup();
        Ok(indices)
    }
}

fn space_label(space: EntitySpace) -> String {
    match space {
        EntitySpace::Model => "Model".into(),
        EntitySpace::Paper(name) => format!("Paper ({name})"),
    }
}

pub(super) fn viewport_transform(
    world: [f64; 4],
    paper: [f64; 4],
    scale: f64,
    inverse: bool,
) -> Result<Transform, String> {
    let (factor, tx, ty) = if inverse {
        let factor = 1.0 / scale;
        (
            factor,
            world[0] - paper[0] * factor,
            world[1] - paper[1] * factor,
        )
    } else {
        (
            scale,
            paper[0] - world[0] * scale,
            paper[1] - world[1] * scale,
        )
    };
    if !factor.is_finite() || !tx.is_finite() || !ty.is_finite() {
        return Err("CHSPACE viewport mapping is not finite.".into());
    }
    if (factor - 1.0).abs() <= f64::EPSILON {
        return Ok(Transform::translation(tx, ty, 0.0));
    }
    let base = Point3::new(tx / (1.0 - factor), ty / (1.0 - factor), 0.0)
        .map_err(|error| error.to_string())?;
    Transform::scale(base, factor).map_err(|error| error.to_string())
}
