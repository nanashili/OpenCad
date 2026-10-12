use super::*;
use crate::{BlockDefinition, DxfDocument, Editor, Point3};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(super) struct LocalEntityMap {
    pub(super) local_id: u64,
    pub(super) working_handle: u64,
}

pub(super) struct BlockEditSession {
    parent_editor: Editor,
    parent_path: Option<PathBuf>,
    parent_name: String,
    parent_selection: Vec<usize>,
    parent_view: crate::Camera3,
    parent_ucs: crate::Ucs3,
    parent_viewport: ([f64; 2], f64, f64),
    block_name: String,
    block_base: Point3,
    attribute_definitions: Vec<crate::AttributeDefinition>,
    pub(super) dynamic_definition: Option<crate::DynamicBlockDefinition>,
    pub(super) local_entities: Vec<LocalEntityMap>,
}

struct PreparedBlockSave {
    name: String,
    mappings: Vec<LocalEntityMap>,
    dynamic_definition: Option<crate::DynamicBlockDefinition>,
    document: DxfDocument,
}

impl BlockEditSession {
    fn working_document(
        parent: &DxfDocument,
        definition: &BlockDefinition,
    ) -> Result<(Editor, Vec<LocalEntityMap>), String> {
        let mut document = parent.clone();
        for index in (0..document.entities().len()).rev() {
            document
                .remove(index)
                .ok_or_else(|| "Could not clear the block editor working entities.".to_owned())?;
        }
        if document
            .remove_block_definition(definition.name())
            .is_none()
        {
            return Err(format!(
                "Block {} disappeared while preparing the block editor.",
                definition.name()
            ));
        }
        let mut mappings = Vec::with_capacity(definition.entities().len());
        for (index, entity) in definition.entities().iter().cloned().enumerate() {
            document
                .push_on_layer(entity, "0")
                .map_err(|error| error.to_string())?;
            let handle = document.entity_handle(index).ok_or_else(|| {
                "Working block entity did not receive a stable handle.".to_owned()
            })?;
            mappings.push(LocalEntityMap {
                local_id: definition.entity_ids()[index],
                working_handle: handle,
            });
        }
        let mut editor = Editor::new(document);
        editor.select((0..mappings.len()).collect());
        Ok((editor, mappings))
    }

    fn definition_from_working(
        &self,
        editor: &Editor,
        name: &str,
    ) -> Result<BlockDefinition, String> {
        let document = editor.document();
        let mut entities = Vec::new();
        let mut ids = Vec::new();
        let mut by_handle: HashMap<u64, u64> = self
            .local_entities
            .iter()
            .map(|mapping| (mapping.working_handle, mapping.local_id))
            .collect();
        let mut next_id = by_handle
            .values()
            .copied()
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        for (index, entity) in document.entities().iter().cloned().enumerate() {
            let handle = document
                .entity_handle(index)
                .ok_or_else(|| "Working block entity is missing its stable handle.".to_owned())?;
            let id = by_handle.remove(&handle).unwrap_or_else(|| {
                let value = next_id;
                next_id = next_id.saturating_add(1);
                value
            });
            entities.push(entity);
            ids.push(id);
        }
        let mut definition = BlockDefinition::new(name, self.block_base, entities);
        let definition_entities = definition.entities().to_vec();
        definition
            .set_entities_with_ids(definition_entities, ids)
            .map_err(|error| error.to_string())?;
        definition.set_attribute_definitions(self.attribute_definitions.clone());
        if let Some(mut dynamic) = self.dynamic_definition.clone() {
            dynamic
                .prune_targets(definition.entity_ids())
                .map_err(|error| error.to_string())?;
            dynamic
                .validate(definition.entity_ids())
                .map_err(|error| error.to_string())?;
            definition.set_dynamic_definition(Some(dynamic));
        }
        Ok(definition)
    }

    fn local_entity_mappings(editor: &Editor, ids: &[u64]) -> Result<Vec<LocalEntityMap>, String> {
        let document = editor.document();
        let mut mappings = Vec::with_capacity(ids.len());
        for (index, id) in ids.iter().copied().enumerate() {
            let handle = document
                .entity_handle(index)
                .ok_or("Working block entity is missing its stable handle.")?;
            mappings.push(LocalEntityMap {
                local_id: id,
                working_handle: handle,
            });
        }
        Ok(mappings)
    }

    fn prepare_save(
        &self,
        working_editor: &Editor,
        requested_name: Option<&str>,
    ) -> Result<PreparedBlockSave, String> {
        let name = requested_name.unwrap_or(&self.block_name).to_owned();
        if name.trim().is_empty() {
            return Err("BSAVEAS requires a non-empty block name.".into());
        }
        if !name.eq_ignore_ascii_case(&self.block_name)
            && self.parent_editor.document().block(&name).is_some()
        {
            return Err(format!("Block {name} already exists."));
        }
        let definition = self.definition_from_working(working_editor, &name)?;
        let definition_ids = definition.entity_ids().to_vec();
        let mappings = Self::local_entity_mappings(working_editor, &definition_ids)?;
        let dynamic_definition = definition.dynamic_definition().cloned();
        let mut document = self.parent_editor.document().clone();
        if name.eq_ignore_ascii_case(&self.block_name) {
            document
                .replace_block_definition(definition)
                .map_err(|error| error.to_string())?;
        } else {
            document
                .add_block_definition(definition)
                .map_err(|error| error.to_string())?;
        }
        for block in working_editor.document().block_definitions() {
            if document.block(block.name()).is_none() {
                document
                    .add_block_definition(block.clone())
                    .map_err(|error| error.to_string())?;
            }
        }
        copy_working_catalogs(&mut document, working_editor.document())?;
        Ok(PreparedBlockSave {
            name,
            mappings,
            dynamic_definition,
            document,
        })
    }

    pub(super) fn selected_local_ids(&mut self, editor: &Editor) -> Result<Vec<u64>, String> {
        let next_id = self
            .local_entities
            .iter()
            .map(|mapping| mapping.local_id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let mut next_id = next_id;
        let mut ids = Vec::new();
        for index in editor.selection() {
            let handle = editor
                .document()
                .entity_handle(*index)
                .ok_or("Selected working entity is missing a stable handle.")?;
            let id = if let Some(mapping) = self
                .local_entities
                .iter()
                .find(|mapping| mapping.working_handle == handle)
            {
                mapping.local_id
            } else {
                let id = next_id;
                next_id = next_id.saturating_add(1);
                self.local_entities.push(LocalEntityMap {
                    local_id: id,
                    working_handle: handle,
                });
                id
            };
            ids.push(id);
        }
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            return Err("Select at least one working entity for the dynamic action.".into());
        }
        Ok(ids)
    }

    fn restore(self, app: &mut OpenCadApp) {
        app.finish();
        app.editor = self.parent_editor;
        app.path = self.parent_path;
        app.name = self.parent_name;
        app.view_camera = self.parent_view;
        app.ucs = self.parent_ucs;
        app.camera.set_view(self.parent_view);
        app.camera.set_ucs(self.parent_ucs);
        if let Err(error) = app.camera.set_named_view_viewport(
            self.parent_viewport.0,
            self.parent_viewport.1,
            self.parent_viewport.2,
        ) {
            app.message(
                format!("Could not restore block editor viewport: {error}"),
                true,
            );
        }
        app.editor.select(self.parent_selection);
        app.fit_pending = false;
    }
}

fn copy_working_catalogs(target: &mut DxfDocument, working: &DxfDocument) -> Result<(), String> {
    if !working.mline_styles().is_empty() {
        target.set_mline_styles(working.mline_styles().to_vec());
    }
    if !working.text_styles().is_empty() {
        target
            .set_text_styles(working.text_styles().to_vec())
            .map_err(|error| format!("Could not save block text styles: {error}"))?;
    }
    if !working.dimension_styles().is_empty() {
        target
            .set_dimension_styles(working.dimension_styles().to_vec())
            .map_err(|error| format!("Could not save block dimension styles: {error}"))?;
    }
    if !working.mleader_styles().is_empty() {
        target
            .set_mleader_styles(working.mleader_styles().to_vec())
            .map_err(|error| format!("Could not save block multileader styles: {error}"))?;
    }
    target.set_layouts(working.layouts().clone());
    Ok(())
}

impl OpenCadApp {
    pub(super) fn feature_block_editor_command(
        &mut self,
        verb: &str,
        _rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "BEDIT" => self.begin_block_edit(tokens),
            "BSAVE" => self.save_block_edit(None),
            "BSAVEAS" => tokens
                .first()
                .ok_or_else(|| "BSAVEAS syntax: BSAVEAS block-name.".to_owned())
                .and_then(|name| {
                    if tokens.len() != 1 {
                        return Err("BSAVEAS syntax: BSAVEAS block-name.".to_owned());
                    }
                    self.save_block_edit(Some(name))
                }),
            "BCLOSE" => self.close_block_edit(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn begin_block_edit(&mut self, tokens: &[&str]) -> Result<(), String> {
        if self.block_edit_session.is_some() {
            return Err("BEDIT cannot be nested inside another block edit.".into());
        }
        if tokens.len() != 1 {
            return Err("BEDIT syntax: BEDIT block-name.".into());
        }
        let name = tokens[0];
        let definition = self
            .editor
            .document()
            .block(name)
            .cloned()
            .ok_or_else(|| format!("Block {name} was not found."))?;
        if definition.xref_path().is_some() {
            return Err("XREF block definitions cannot be edited in place.".into());
        }
        let parent_selection = self.editor.selection().to_vec();
        let parent_document = self.editor.document().clone();
        let (working_editor, mappings) =
            BlockEditSession::working_document(&parent_document, &definition)?;
        let local_view = crate::Camera3::new(definition.base(), 100.0, crate::ViewOrientation::Top)
            .map_err(|error| error.to_string())?;
        self.finish();
        let parent_editor = std::mem::replace(&mut self.editor, working_editor);
        let session = BlockEditSession {
            parent_editor,
            parent_path: self.path.take(),
            parent_name: std::mem::replace(&mut self.name, format!("Block Editor: {name}")),
            parent_selection,
            parent_view: self.view_camera,
            parent_ucs: self.ucs,
            parent_viewport: self.camera.named_view_viewport(),
            block_name: name.to_owned(),
            block_base: definition.base(),
            attribute_definitions: definition.attribute_definitions().to_vec(),
            dynamic_definition: definition.dynamic_definition().cloned(),
            local_entities: mappings,
        };
        self.block_edit_session = Some(session);
        self.view_camera = local_view;
        self.ucs = crate::Ucs3::world();
        self.camera.set_view(local_view);
        self.camera.set_ucs(crate::Ucs3::world());
        self.fit_pending = true;
        self.message(
            format!("Block Editor: {name}. Use BSAVE or BCLOSE SAVE."),
            false,
        );
        Ok(())
    }

    fn save_block_edit(&mut self, requested_name: Option<&str>) -> Result<(), String> {
        let prepared = {
            let session = self
                .block_edit_session
                .as_ref()
                .ok_or("BSAVE requires an active block editor session.")?;
            session.prepare_save(&self.editor, requested_name)?
        };
        let session = self
            .block_edit_session
            .as_mut()
            .ok_or("Block editor session ended unexpectedly.")?;
        session
            .parent_editor
            .commit_document(prepared.document)
            .map_err(|error| format!("Could not commit block {}: {error}", prepared.name))?;
        session.block_name = prepared.name.clone();
        session.dynamic_definition = prepared.dynamic_definition;
        session.local_entities = prepared.mappings;
        self.message(format!("Block {} saved.", prepared.name), false);
        Ok(())
    }

    fn close_block_edit(&mut self, tokens: &[&str]) -> Result<(), String> {
        let operation = tokens
            .first()
            .copied()
            .unwrap_or("DISCARD")
            .to_ascii_uppercase();
        if !matches!(operation.as_str(), "DISCARD" | "D" | "SAVE" | "S") {
            return Err("BCLOSE syntax: BCLOSE SAVE|DISCARD.".into());
        }
        let Some(session) = self.block_edit_session.take() else {
            return Err("BCLOSE requires an active block editor session.".into());
        };
        match operation.as_str() {
            "DISCARD" | "D" => {
                session.restore(self);
                self.message("Block changes discarded.", false);
                Ok(())
            }
            "SAVE" | "S" => {
                self.block_edit_session = Some(session);
                self.save_block_edit(None)?;
                let session = self
                    .block_edit_session
                    .take()
                    .ok_or_else(|| "Block editor session ended unexpectedly.".to_owned())?;
                session.restore(self);
                self.message("Block saved and editor closed.", false);
                Ok(())
            }
            _ => unreachable!("validated BCLOSE operation"),
        }
    }
}
