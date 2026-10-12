use super::*;
use crate::{EntitySpace, Transform};

impl OpenCadApp {
    pub(super) fn zoom_camera(&mut self, factor: f64) -> Result<(), String> {
        let Some(active) = self.active_viewport.as_ref() else {
            return self.camera.zoom(factor);
        };
        let viewport = self
            .layouts
            .get(&active.layout)
            .and_then(|layout| layout.viewports().get(active.index))
            .ok_or("The active viewport no longer exists.")?;
        if viewport.is_display_locked() {
            return Err("The active viewport is locked.".into());
        }
        let paper = viewport.paper();
        let paper_center = active.paper_camera.center;
        let paper_scale = active.paper_camera.scale;
        let (camera_center, camera_scale) = self.camera.view_state();
        let current_viewport_scale = camera_scale / paper_scale;
        let paper_midpoint = [(paper[0] + paper[2]) / 2.0, (paper[1] + paper[3]) / 2.0];
        let viewport_center = [
            camera_center[0] + (paper_midpoint[0] - paper_center[0]) / current_viewport_scale,
            camera_center[1] + (paper_midpoint[1] - paper_center[1]) / current_viewport_scale,
        ];
        self.camera.zoom(factor)?;
        let next_scale = self.camera.view_state().1;
        let next_viewport_scale = next_scale / paper_scale;
        self.camera.set_view_state(
            [
                viewport_center[0] + (paper_center[0] - paper_midpoint[0]) / next_viewport_scale,
                viewport_center[1] + (paper_center[1] - paper_midpoint[1]) / next_viewport_scale,
            ],
            next_scale,
        )
    }

    pub(super) fn layouts_for_persistence(&self) -> Result<crate::LayoutCollection, String> {
        self.viewport_candidate_layouts(&self.layouts)
            .map(|(layouts, _)| layouts)
    }

    fn viewport_candidate_layouts(
        &self,
        source: &crate::LayoutCollection,
    ) -> Result<(crate::LayoutCollection, bool), String> {
        let Some(active) = self.active_viewport.as_ref() else {
            return Ok((source.clone(), false));
        };
        let (center, model_scale) = self.camera.view_state();
        if center == active.model_camera.center && model_scale == active.model_camera.scale {
            return Ok((source.clone(), false));
        }
        let original = source
            .get(&active.layout)
            .and_then(|layout| layout.viewports().get(active.index))
            .ok_or("The active viewport no longer exists.")?;
        let original_world = original.world();
        let original_scale = original.scale();
        let paper = original.paper();
        let scale = model_scale / active.paper_camera.scale;
        if !scale.is_finite() || scale <= 0.0 {
            return Err("The active viewport has an invalid camera scale.".into());
        }
        let world_min = [
            center[0] - (active.paper_camera.center[0] - paper[0]) / scale,
            center[1] - (active.paper_camera.center[1] - paper[1]) / scale,
        ];
        let world_max = [
            world_min[0] + (paper[2] - paper[0]) / scale,
            world_min[1] + (paper[3] - paper[1]) / scale,
        ];
        let changed = original_world[0] != world_min[0]
            || original_world[1] != world_min[1]
            || (original_scale - scale).abs() > f64::EPSILON;
        let candidate_world = if changed {
            [world_min[0], world_min[1], world_max[0], world_max[1]]
        } else {
            original_world
        };
        let mut layouts = source.clone();
        layouts
            .get_mut(&active.layout)
            .and_then(|layout| layout.viewports_mut().get_mut(active.index))
            .ok_or("The active viewport no longer exists.")?
            .set_view(candidate_world, scale)
            .map_err(|error| error.to_string())?;
        Ok((layouts, changed))
    }

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
            "MSPACE" => self.mspace_command(tokens),
            "PSPACE" => self.pspace_command(tokens),
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
        self.leave_model_space()?;
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

    fn mspace_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if self.layouts.active().name().eq_ignore_ascii_case("Model") {
            return Err("MSPACE requires an active paper-space layout.".into());
        }
        let index = match tokens {
            [] => self
                .active_viewport
                .as_ref()
                .map(|viewport| viewport.index + 1)
                .or_else(|| self.remembered_viewport_index().map(|index| index + 1))
                .unwrap_or(1),
            [token] => token
                .parse::<usize>()
                .map_err(|_| "MSPACE viewport number must be positive.")?,
            _ => return Err("MSPACE [viewport-number].".into()),
        };
        if index == 0 {
            return Err("MSPACE viewport number must be positive.".into());
        }
        let target = index - 1;
        if self.layouts.active().viewports().get(target).is_none() {
            return Err(format!("Viewport {index} does not exist."));
        }
        if let Some(active) = self.active_viewport.as_ref() {
            if active.layout == self.layouts.active().name() && active.index == target {
                return Ok(());
            }
            self.leave_model_space()?;
        }
        self.enter_model_space(target)
    }

    pub(super) fn remembered_viewport_index(&self) -> Option<usize> {
        let layout = self.layouts.active();
        let remembered = self
            .last_viewports
            .get(layout.name())
            .copied()
            .filter(|index| {
                layout
                    .viewports()
                    .get(*index)
                    .is_some_and(crate::Viewport::is_displayed)
            });
        remembered.or_else(|| {
            layout
                .viewports()
                .iter()
                .position(crate::Viewport::is_displayed)
        })
    }

    pub(super) fn viewport_navigation_locked(&self) -> bool {
        self.active_viewport.as_ref().is_some_and(|active| {
            self.layouts
                .get(&active.layout)
                .and_then(|layout| layout.viewports().get(active.index))
                .is_some_and(crate::Viewport::is_display_locked)
        })
    }

    pub(super) fn refresh_interaction_viewport(&mut self) {
        let frozen_layers = self.active_viewport.as_ref().and_then(|active| {
            self.layouts
                .get(&active.layout)
                .and_then(|layout| layout.viewports().get(active.index))
                .map(|viewport| viewport.frozen_layers().to_vec())
        });
        self.editor.set_interaction_frozen_layers(frozen_layers);
    }

    fn pspace_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if !tokens.is_empty() {
            return Err("PSPACE takes no arguments.".into());
        }
        self.leave_model_space()
    }

    pub(super) fn enter_model_space(&mut self, index: usize) -> Result<(), String> {
        if self.active_viewport.is_some() {
            return Ok(());
        }
        let layout = self.layouts.active();
        if layout.name().eq_ignore_ascii_case("Model") {
            return Err("MSPACE requires an active paper-space layout.".into());
        }
        let viewport = layout
            .viewports()
            .get(index)
            .ok_or_else(|| format!("Viewport {} does not exist.", index + 1))?;
        if !viewport.is_displayed() {
            return Err(format!("Viewport {} is off.", index + 1));
        }
        let (paper_center, paper_scale) = self.camera.view_state();
        let model_scale = paper_scale * viewport.scale();
        let world = viewport.world();
        let paper = viewport.paper();
        let model_center = [
            world[0] + (paper_center[0] - paper[0]) / viewport.scale(),
            world[1] + (paper_center[1] - paper[1]) / viewport.scale(),
        ];
        self.camera.set_view_state(model_center, model_scale)?;
        let (entry_center, entry_scale) = self.camera.view_state();
        self.active_viewport = Some(super::ActiveViewport {
            layout: layout.name().to_owned(),
            index,
            paper_camera: super::PaperCameraState {
                center: paper_center,
                scale: paper_scale,
            },
            model_camera: super::PaperCameraState {
                center: entry_center,
                scale: entry_scale,
            },
        });
        self.last_viewports.insert(layout.name().to_owned(), index);
        self.editor.set_interaction_space(Some(EntitySpace::Model));
        self.editor
            .set_interaction_frozen_layers(Some(viewport.frozen_layers().to_vec()));
        self.editor.select(Vec::new());
        self.finish();
        self.message(
            format!("Model space active in viewport {}.", index + 1),
            false,
        );
        Ok(())
    }

    pub(super) fn leave_model_space(&mut self) -> Result<(), String> {
        let Some(active) = self.active_viewport.as_ref() else {
            self.editor.set_interaction_space(None);
            self.editor.set_interaction_frozen_layers(None);
            return Ok(());
        };
        let valid_layout = self
            .layouts
            .get(&active.layout)
            .is_some_and(|layout| layout.viewports().get(active.index).is_some());
        if !valid_layout {
            return Err("The active viewport no longer exists.".into());
        }
        let (candidate_layouts, changed) = self.viewport_candidate_layouts(&self.layouts)?;
        self.camera
            .set_view_state(active.paper_camera.center, active.paper_camera.scale)?;
        self.active_viewport
            .take()
            .expect("active viewport validated above");
        if changed {
            self.layouts = candidate_layouts;
            self.editor.set_layouts(self.layouts.clone());
            self.layouts = self.editor.layouts().clone();
        }
        self.editor.set_interaction_space(None);
        self.editor.set_interaction_frozen_layers(None);
        self.editor.select(Vec::new());
        self.message("Paper space active.".to_owned(), false);
        Ok(())
    }

    pub(super) fn active_viewport_rect(&self, rect: egui::Rect) -> Option<egui::Rect> {
        let active = self.active_viewport.as_ref()?;
        let viewport = self
            .layouts
            .get(&active.layout)?
            .viewports()
            .get(active.index)?;
        let paper = viewport.paper();
        let center = active.paper_camera.center;
        let scale = active.paper_camera.scale;
        let a = egui::pos2(
            rect.center().x + ((paper[0] - center[0]) * scale) as f32,
            rect.center().y - ((paper[1] - center[1]) * scale) as f32,
        );
        let b = egui::pos2(
            rect.center().x + ((paper[2] - center[0]) * scale) as f32,
            rect.center().y - ((paper[3] - center[1]) * scale) as f32,
        );
        Some(egui::Rect::from_min_max(
            egui::pos2(a.x.min(b.x), a.y.min(b.y)),
            egui::pos2(a.x.max(b.x), a.y.max(b.y)),
        ))
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
        let working_layouts = self.layouts_for_persistence()?;
        let viewport = working_layouts
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
