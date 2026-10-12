use super::*;

impl OpenCadApp {
    pub(super) fn feature_selection_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb {
            "QSELECT" => self.qselect_command(tokens),
            "SELECTSIMILAR" => self.select_similar_command(tokens),
            "LAYFRZ" => self.layer_freeze_command(tokens, true),
            "LAYTHW" => self.layer_freeze_command(tokens, false),
            "LAYOFF" => self.layer_visibility_command(tokens, false),
            "LAYON" => self.layer_visibility_command(tokens, true),
            "LAYLCK" => self.layer_lock_command(tokens, true),
            "LAYULK" => self.layer_lock_command(tokens, false),
            "LAYISO" => self.layer_isolate_command(tokens),
            "LAYUNISO" => self.layer_restore_command(tokens),
            "LAYP" | "LAYERP" => self.layer_previous_command(tokens),
            "LAYCUR" => self.layer_current_command(tokens),
            "LAYMCH" => self.layer_match_command(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn qselect_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 2 {
            return Err(
                "QSELECT property value [=|!= value] [REPLACE|ADD|REMOVE|INTERSECT].".into(),
            );
        }
        let property = tokens[0].to_ascii_uppercase();
        if !matches!(
            property.as_str(),
            "TYPE" | "ENTITY" | "KIND" | "LAYER" | "COLOR" | "LINETYPE"
        ) {
            return Err("QSELECT property must be TYPE, LAYER, COLOR, or LINETYPE.".into());
        }
        let (operator, value, mode_start) = if matches!(tokens[1], "=" | "!=") {
            if tokens.len() < 3 {
                return Err("QSELECT needs a value after the comparison operator.".into());
            }
            (tokens[1], tokens[2], 3)
        } else {
            ("=", tokens[1], 2)
        };
        let mode = tokens
            .get(mode_start)
            .copied()
            .unwrap_or("REPLACE")
            .to_ascii_uppercase();
        if !matches!(mode.as_str(), "REPLACE" | "ADD" | "REMOVE" | "INTERSECT") {
            return Err("QSELECT mode must be REPLACE, ADD, REMOVE, or INTERSECT.".into());
        }
        let matches = |index: usize| {
            if !self.editor.is_entity_visible(index) {
                return false;
            }
            let equal = match property.as_str() {
                "TYPE" | "ENTITY" | "KIND" => self.editor.entities()[index]
                    .name()
                    .eq_ignore_ascii_case(value),
                "LAYER" => self
                    .editor
                    .entity_layer(index)
                    .is_some_and(|layer| layer.eq_ignore_ascii_case(value)),
                "COLOR" => self.editor.entity_style(index).is_some_and(|style| {
                    value.eq_ignore_ascii_case("BYLAYER") && style.color_index() == 256
                        || value.eq_ignore_ascii_case("BYBLOCK") && style.color_index() == 0
                        || value
                            .parse::<i16>()
                            .is_ok_and(|color| style.color_index() == color)
                }),
                "LINETYPE" => self
                    .editor
                    .entity_style(index)
                    .is_some_and(|style| style.linetype().eq_ignore_ascii_case(value)),
                _ => false,
            };
            if operator == "!=" { !equal } else { equal }
        };
        let candidates: Vec<usize> = (0..self.editor.entities().len())
            .filter(|index| matches(*index))
            .collect();
        let previous = self.editor.selection().to_vec();
        let selected = match mode.as_str() {
            "REPLACE" => candidates,
            "ADD" => previous.into_iter().chain(candidates).collect(),
            "REMOVE" => previous
                .into_iter()
                .filter(|index| !candidates.contains(index))
                .collect(),
            "INTERSECT" => previous
                .into_iter()
                .filter(|index| candidates.contains(index))
                .collect(),
            _ => unreachable!(),
        };
        let count = selected.len();
        self.editor.select(selected);
        self.message(format!("QSELECT selected {count} object(s)."), false);
        Ok(())
    }

    fn select_similar_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let seed = tokens
            .first()
            .and_then(|value| value.parse::<usize>().ok())
            .or_else(|| self.editor.selection().first().copied())
            .ok_or("SELECTSIMILAR needs a selected entity or entity index.")?;
        let source = self
            .editor
            .entities()
            .get(seed)
            .ok_or("Unknown entity index.")?;
        let source_layer = self.editor.entity_layer(seed).map(str::to_owned);
        let source_style = self.editor.entity_style(seed).cloned();
        let type_only = tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case("TYPE"));
        let layer_only = tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case("LAYER"));
        let color_only = tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case("COLOR"));
        let linetype_only = tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case("LINETYPE"));
        let block_only = tokens.iter().any(|token| {
            token.eq_ignore_ascii_case("BLOCK") || token.eq_ignore_ascii_case("BLOCKNAME")
        });
        let block_name = match source {
            Entity::Insert(insert) => Some(insert.block_name()),
            _ => None,
        };
        let block_filter = tokens
            .iter()
            .skip(1)
            .find(|token| {
                !matches!(
                    token.to_ascii_uppercase().as_str(),
                    "TYPE" | "LAYER" | "COLOR" | "LINETYPE" | "BLOCK" | "BLOCKNAME"
                )
            })
            .copied()
            .or(block_name);
        let restricted = type_only || layer_only || color_only || linetype_only || block_only;
        let selected = (0..self.editor.entities().len())
            .filter(|index| self.editor.is_entity_visible(*index))
            .filter(|index| {
                let entity = &self.editor.entities()[*index];
                let style = self.editor.entity_style(*index);
                (restricted
                    && type_only
                    && entity.name() == source.name())
                    || (restricted
                        && layer_only
                        && source_layer.as_deref() == self.editor.entity_layer(*index))
                    || (restricted
                        && color_only
                        && source_style
                            .as_ref()
                            .zip(style)
                            .is_some_and(|(a, b)| a.color_index() == b.color_index()))
                    || (restricted
                        && linetype_only
                        && source_style
                            .as_ref()
                            .zip(style)
                            .is_some_and(|(a, b)| a.linetype() == b.linetype()))
                    || (restricted
                        && block_only
                        && matches!(entity, Entity::Insert(insert) if block_filter.is_some_and(|name| insert.block_name().eq_ignore_ascii_case(name))))
                    || (!restricted
                        && entity.name() == source.name()
                        && source_layer.as_deref() == self.editor.entity_layer(*index)
                        && source_style.as_ref().zip(style).is_some_and(|(a, b)| {
                            a.color_index() == b.color_index() && a.linetype() == b.linetype()
                        }))
            })
            .collect::<Vec<_>>();
        self.editor.select(selected.clone());
        self.message(
            format!("SELECTSIMILAR selected {} object(s).", selected.len()),
            false,
        );
        Ok(())
    }

    fn layer_visibility_command(&mut self, tokens: &[&str], visible: bool) -> Result<(), String> {
        let name = tokens.first().ok_or("Layer name required.")?;
        self.editor
            .set_layer_visible(name, visible)
            .map_err(|error| error.to_string())?;
        self.message(
            format!("Layer {name} {}.", if visible { "on" } else { "off" }),
            false,
        );
        Ok(())
    }

    fn layer_freeze_command(&mut self, tokens: &[&str], frozen: bool) -> Result<(), String> {
        let name = tokens.first().ok_or("Layer name required.")?;
        if frozen {
            self.editor
                .freeze_layer(name)
                .map_err(|error| error.to_string())?;
        } else {
            self.editor
                .thaw_layer(name)
                .map_err(|error| error.to_string())?;
        }
        self.message(
            format!("Layer {name} {}.", if frozen { "frozen" } else { "thawed" }),
            false,
        );
        Ok(())
    }

    fn layer_lock_command(&mut self, tokens: &[&str], locked: bool) -> Result<(), String> {
        let name = tokens.first().ok_or("Layer name required.")?;
        self.editor
            .set_layer_locked(name, locked)
            .map_err(|error| error.to_string())?;
        self.message(
            format!(
                "Layer {name} {}.",
                if locked { "locked" } else { "unlocked" }
            ),
            false,
        );
        Ok(())
    }

    fn layer_isolate_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let name = tokens.first().ok_or("Layer name required.")?;
        self.editor
            .isolate_layer(name)
            .map_err(|error| error.to_string())?;
        self.message(format!("Layer {name} isolated."), false);
        Ok(())
    }

    fn layer_restore_command(&mut self, _tokens: &[&str]) -> Result<(), String> {
        self.editor
            .restore_layer_visibility()
            .map_err(|error| error.to_string())?;
        self.message("Previous layer visibility restored.", false);
        Ok(())
    }

    fn layer_previous_command(&mut self, _tokens: &[&str]) -> Result<(), String> {
        self.editor
            .restore_previous_layer_state()
            .map_err(|error| error.to_string())?;
        self.message("Previous layer state restored.", false);
        Ok(())
    }

    fn layer_current_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if let Some(name) = tokens.first() {
            self.editor
                .set_current_layer(name)
                .map_err(|error| error.to_string())?;
        } else if !self
            .editor
            .make_selection_layer_current()
            .map_err(|e| e.to_string())?
        {
            return Err("LAYCUR needs a layer name or a selected entity.".into());
        }
        self.message(
            format!("Current layer: {}.", self.editor.current_layer()),
            false,
        );
        Ok(())
    }

    fn layer_match_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let name = tokens.first().ok_or("LAYMCH needs a target layer name.")?;
        self.editor
            .assign_selection_layer(name)
            .map_err(|error| error.to_string())?;
        self.message(format!("Selected objects moved to layer {name}."), false);
        Ok(())
    }
}
