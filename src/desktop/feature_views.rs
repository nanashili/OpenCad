use super::OpenCadApp;
use crate::{NamedUcs, NamedView};

impl OpenCadApp {
    pub(super) fn feature_view_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        match verb {
            "VIEW"
                if tokens.is_empty()
                    || tokens.first().is_some_and(|token| {
                        matches!(
                            token.to_ascii_uppercase().as_str(),
                            "LIST" | "SAVE" | "CREATE" | "RESTORE" | "SET" | "DELETE" | "REMOVE"
                        )
                    }) =>
            {
                Some(self.named_view_command(tokens))
            }
            "UCS"
                if tokens.is_empty()
                    || tokens.first().is_some_and(|token| {
                        matches!(
                            token.to_ascii_uppercase().as_str(),
                            "LIST" | "SAVE" | "CREATE" | "RESTORE" | "SET" | "DELETE" | "REMOVE"
                        )
                    }) =>
            {
                Some(self.named_ucs_command(tokens))
            }
            _ => None,
        }
    }

    fn named_view_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let operation = tokens
            .first()
            .copied()
            .unwrap_or("LIST")
            .to_ascii_uppercase();
        match operation.as_str() {
            "LIST" => {
                let names = self
                    .editor
                    .document()
                    .named_views()
                    .views()
                    .iter()
                    .map(|view| view.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.message(
                    if names.is_empty() {
                        "No named views."
                    } else {
                        &names
                    },
                    false,
                );
            }
            "SAVE" | "CREATE" => {
                let name = tokens.get(1).ok_or("VIEW SAVE name.")?;
                let (center, height, width) = self.camera.named_view_viewport();
                let camera = self.view_camera;
                let mut view =
                    NamedView::with_viewport(name.to_owned(), camera, center, height, width)
                        .map_err(|e| e.to_string())?;
                view.set_associated_ucs(
                    Some(self.ucs),
                    Some(self.editor.document().current_ucs_name().to_owned()),
                )
                .map_err(|e| e.to_string())?;
                let mut document = self.editor.document().clone();
                let mut catalog = document.named_views().clone();
                catalog.upsert(view.clone()).map_err(|e| e.to_string())?;
                document.set_named_views(catalog);
                document.set_active_view(Some(view.clone()));
                self.editor
                    .commit_document(document)
                    .map_err(|e| e.to_string())?;
                self.message(format!("Named view {name} saved."), false);
            }
            "RESTORE" | "SET" => {
                let name = tokens.get(1).ok_or("VIEW RESTORE name.")?;
                let view = self
                    .editor
                    .document()
                    .named_views()
                    .get(name)
                    .map_err(|e| e.to_string())?
                    .clone();
                self.view_camera = view.camera();
                self.camera.set_view(self.view_camera);
                self.camera.set_named_view_viewport(
                    view.center(),
                    view.view_height(),
                    view.view_width(),
                )?;
                if let Some(ucs) = view.associated_ucs() {
                    self.ucs = ucs;
                    self.camera.set_ucs(ucs);
                }
                let mut document = self.editor.document().clone();
                document.set_active_view(Some(view.clone()));
                if let Some(name) = view.associated_ucs_name() {
                    document
                        .set_current_ucs(name.to_owned(), self.ucs)
                        .map_err(|e| e.to_string())?;
                }
                self.editor
                    .commit_document(document)
                    .map_err(|e| e.to_string())?;
                self.message(format!("Named view {name} restored."), false);
            }
            "DELETE" | "REMOVE" => {
                let name = tokens.get(1).ok_or("VIEW DELETE name.")?;
                let mut document = self.editor.document().clone();
                let mut catalog = document.named_views().clone();
                catalog.remove(name).map_err(|e| e.to_string())?;
                document.set_named_views(catalog);
                self.editor
                    .commit_document(document)
                    .map_err(|e| e.to_string())?;
                self.message(format!("Named view {name} deleted."), false);
            }
            _ => {
                return None
                    .ok_or_else(|| "VIEW accepts SAVE, RESTORE, LIST, or DELETE.".to_owned());
            }
        }
        Ok(())
    }

    fn named_ucs_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let operation = tokens
            .first()
            .copied()
            .unwrap_or("LIST")
            .to_ascii_uppercase();
        match operation.as_str() {
            "LIST" => {
                let names = self
                    .editor
                    .document()
                    .named_ucs()
                    .ucs()
                    .iter()
                    .map(|ucs| ucs.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.message(
                    if names.is_empty() {
                        "No named UCS."
                    } else {
                        &names
                    },
                    false,
                );
            }
            "SAVE" | "CREATE" => {
                let name = tokens.get(1).ok_or("UCS SAVE name.")?;
                let value = NamedUcs::new(name.to_owned(), self.ucs).map_err(|e| e.to_string())?;
                let mut document = self.editor.document().clone();
                let mut catalog = document.named_ucs().clone();
                catalog.upsert(value.clone()).map_err(|e| e.to_string())?;
                document.set_named_ucs(catalog);
                document
                    .set_current_ucs(name.to_owned(), value.ucs())
                    .map_err(|e| e.to_string())?;
                self.editor
                    .commit_document(document)
                    .map_err(|e| e.to_string())?;
                self.message(format!("Named UCS {name} saved."), false);
            }
            "RESTORE" | "SET" => {
                let name = tokens.get(1).ok_or("UCS RESTORE name.")?;
                let value = self
                    .editor
                    .document()
                    .named_ucs()
                    .get(name)
                    .map_err(|e| e.to_string())?
                    .ucs();
                self.ucs = value;
                self.camera.set_ucs(value);
                let mut document = self.editor.document().clone();
                document
                    .set_current_ucs(name.to_owned(), value)
                    .map_err(|e| e.to_string())?;
                self.editor
                    .commit_document(document)
                    .map_err(|e| e.to_string())?;
                self.message(format!("Named UCS {name} restored."), false);
            }
            "DELETE" | "REMOVE" => {
                let name = tokens.get(1).ok_or("UCS DELETE name.")?;
                let mut document = self.editor.document().clone();
                let mut catalog = document.named_ucs().clone();
                catalog.remove(name).map_err(|e| e.to_string())?;
                document.set_named_ucs(catalog);
                self.editor
                    .commit_document(document)
                    .map_err(|e| e.to_string())?;
                self.message(format!("Named UCS {name} deleted."), false);
            }
            _ => return Err("UCS accepts SAVE, RESTORE, LIST, or DELETE.".into()),
        }
        Ok(())
    }
}
