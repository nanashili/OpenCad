//! Command workflows for external DXF references and raster image attachments.
//! The desktop owner supplies this state to the command dispatcher and may
//! later mirror its members into the drawing's Entity registry.

use super::commands::{number, parse_point};
use crate::{Point3, RasterImage, ReferenceManager};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct ReferenceWorkspace {
    pub references: ReferenceManager,
    pub images: Vec<RasterImage>,
    pub bound_documents: Vec<(String, crate::DxfDocument)>,
}

impl ReferenceWorkspace {
    /// Rebuild reference/image state from persisted drawing entities and xref
    /// block metadata. External files are resolved lazily on demand.
    pub fn from_document(drawing: &crate::DxfDocument, base_dir: Option<&Path>) -> Self {
        let mut workspace = Self::default();
        workspace.sync_from_document(drawing, base_dir);
        workspace
    }

    /// Synchronize the workspace after opening or replacing a drawing. Missing
    /// xref files remain listed as unloaded so LIST, LOAD and RELOAD still work.
    pub fn sync_from_document(&mut self, drawing: &crate::DxfDocument, base_dir: Option<&Path>) {
        self.references = ReferenceManager::default();
        self.images = drawing
            .entities()
            .iter()
            .filter_map(|entity| match entity {
                crate::Entity::Image(image) => Some(image.clone()),
                _ => None,
            })
            .collect();
        self.bound_documents.clear();
        for block in drawing.block_definitions() {
            let Some(path) = block.xref_path() else {
                continue;
            };
            let path = Path::new(path);
            let resolved = if path.is_absolute() {
                path.to_path_buf()
            } else if let Some(base) = base_dir {
                base.join(path)
            } else {
                path.to_path_buf()
            };
            let _ = self
                .references
                .attach_lazy(block.name().to_owned(), resolved);
        }
    }

    pub fn attach_dxf(
        &mut self,
        drawing: &mut crate::DxfDocument,
        name: &str,
        insertion: Point3,
    ) -> Result<(), String> {
        self.references.attach_to_document(name, drawing, insertion)
    }
    pub fn reload_dxf(
        &mut self,
        drawing: &mut crate::DxfDocument,
        name: &str,
    ) -> Result<(), String> {
        self.references.reload_in_document(name, drawing)
    }
    pub fn unload_dxf(
        &mut self,
        drawing: &mut crate::DxfDocument,
        name: &str,
    ) -> Result<(), String> {
        remove_reference_inserts(drawing, name);
        self.references.unload(name)
    }
    pub fn detach_dxf(
        &mut self,
        drawing: &mut crate::DxfDocument,
        name: &str,
    ) -> Result<(), String> {
        remove_reference_inserts(drawing, name);
        drawing.remove_block_definition(name);
        self.references.detach(name).map(|_| ())
    }
    pub fn bind_dxf(&mut self, drawing: &mut crate::DxfDocument, name: &str) -> Result<(), String> {
        remove_reference_inserts(drawing, name);
        let reference = self.references.bind(name)?;
        for entity in reference.entities().to_vec() {
            drawing
                .push_on_layer(entity, "0")
                .map_err(|e| e.to_string())?;
        }
        drawing.remove_block_definition(name);
        Ok(())
    }
}

fn remove_reference_inserts(drawing: &mut crate::DxfDocument, name: &str) {
    let indices = drawing
        .entities()
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| match entity {
            crate::Entity::Insert(insert) if insert.block_name().eq_ignore_ascii_case(name) => {
                Some(index)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for index in indices.into_iter().rev() {
        drawing.remove(index);
    }
}

pub fn feature_references_command(
    workspace: &mut ReferenceWorkspace,
    verb: &str,
    tokens: &[&str],
) -> Option<Result<String, String>> {
    let result = match verb.to_ascii_uppercase().as_str() {
        "XATTACH" | "XREF" => xref_command(workspace, tokens),
        "IMAGEATTACH" => image_attach(workspace, tokens),
        "IMAGECLIP" => image_clip(workspace, tokens),
        _ => return None,
    };
    Some(result)
}

fn xref_command(workspace: &mut ReferenceWorkspace, tokens: &[&str]) -> Result<String, String> {
    if tokens.is_empty() {
        return Err("XATTACH syntax: XATTACH path [name]. XREF requires ATTACH, LIST, RELOAD, DETACH, BIND, UNLOAD, or LOAD.".into());
    }
    if tokens[0].eq_ignore_ascii_case("ATTACH") {
        return attach_reference(workspace, &tokens[1..]);
    }
    match tokens[0].to_ascii_uppercase().as_str() {
        "LIST" => Ok(workspace
            .references
            .references()
            .iter()
            .map(|reference| {
                format!(
                    "{} {} {:?}",
                    reference.name(),
                    reference.path().display(),
                    reference.status()
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")),
        "RELOAD" => {
            let name = tokens
                .get(1)
                .ok_or("XREF RELOAD requires a reference name.")?;
            workspace.references.reload(name)?;
            Ok(format!("Reloaded reference {name}."))
        }
        "DETACH" => {
            let name = tokens
                .get(1)
                .ok_or("XREF DETACH requires a reference name.")?;
            workspace.references.detach(name)?;
            Ok(format!("Detached reference {name}."))
        }
        "UNLOAD" => {
            let name = tokens
                .get(1)
                .ok_or("XREF UNLOAD requires a reference name.")?;
            workspace.references.unload(name)?;
            Ok(format!("Unloaded reference {name}."))
        }
        "LOAD" => {
            let name = tokens
                .get(1)
                .ok_or("XREF LOAD requires a reference name.")?;
            workspace.references.load(name)?;
            Ok(format!("Loaded reference {name}."))
        }
        "BIND" => {
            let name = tokens
                .get(1)
                .ok_or("XREF BIND requires a reference name.")?;
            let document = workspace.references.bind(name)?;
            workspace.bound_documents.push((name.to_string(), document));
            Ok(format!("Bound reference {name}."))
        }
        _ => attach_reference(workspace, tokens),
    }
}

fn attach_reference(workspace: &mut ReferenceWorkspace, tokens: &[&str]) -> Result<String, String> {
    let path = tokens
        .first()
        .ok_or("XATTACH syntax: XATTACH path [name].")?;
    let name = tokens.get(1).copied().unwrap_or_else(|| {
        path.rsplit('/')
            .next()
            .unwrap_or(path)
            .trim_end_matches(".dxf")
    });
    workspace.references.attach(name, path)?;
    Ok(format!("Attached DXF reference {name}."))
}

fn image_attach(workspace: &mut ReferenceWorkspace, tokens: &[&str]) -> Result<String, String> {
    if tokens.len() != 6 {
        return Err("IMAGEATTACH syntax: IMAGEATTACH path insertion u-vector v-vector pixel-width pixel-height.".into());
    }
    let image = RasterImage::new(
        tokens[0],
        parse_point(tokens[1], 0.0)?,
        parse_point(tokens[2], 0.0)?,
        parse_point(tokens[3], 0.0)?,
        tokens[4]
            .parse()
            .map_err(|_| "Pixel width must be a positive whole number.")?,
        tokens[5]
            .parse()
            .map_err(|_| "Pixel height must be a positive whole number.")?,
    )
    .map_err(|e| e.to_string())?;
    workspace.images.push(image);
    Ok(format!("Attached image {}.", tokens[0]))
}

fn image_clip(workspace: &mut ReferenceWorkspace, tokens: &[&str]) -> Result<String, String> {
    if tokens.len() < 4 {
        return Err(
            "IMAGECLIP syntax: IMAGECLIP image-index point point point [point ...].".into(),
        );
    }
    let index = tokens[0]
        .parse::<usize>()
        .map_err(|_| "Image index must be a positive whole number.")?
        .checked_sub(1)
        .ok_or("Image index must be positive.")?;
    let boundary = tokens[1..]
        .iter()
        .map(|token| parse_point(token, 0.0))
        .collect::<Result<Vec<Point3>, _>>()?;
    let image = workspace
        .images
        .get_mut(index)
        .ok_or("Image index is outside the current drawing.")?;
    image
        .set_clip_boundary(Some(boundary))
        .map_err(|e| e.to_string())?;
    Ok(format!("Clipped image {}.", index + 1))
}

#[allow(dead_code)]
fn _parse_opacity(value: &str) -> Result<f64, String> {
    number(value)
}
