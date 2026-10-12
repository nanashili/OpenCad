use super::commands::{number, parse_point};
use super::*;
use crate::{Attribute, AttributeDefinition, Entity};

/// Block library and INSERT attribute workflows.  Definition and placement
/// edits are delegated to Editor so they remain undoable and validated.
impl OpenCadApp {
    pub(super) fn feature_blocks_command(
        &mut self,
        verb: &str,
        _rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "BLOCK" => self.block_define(tokens),
            "INSERT" => self.block_insert(tokens),
            "ATTDEF" => self.block_attribute_definition(tokens),
            "ATTEDIT" => self.block_attribute_edit(tokens),
            "BLOCKINFO" => self.block_info(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn block_define(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 2 || tokens.len() > 3 {
            return Err("BLOCK syntax: BLOCK name base-point [REPLACE].".into());
        }
        let name = tokens[0];
        let base = parse_point(tokens[1], self.drawing_elevation())?;
        let replace = tokens
            .get(2)
            .is_some_and(|v| v.eq_ignore_ascii_case("REPLACE"));
        self.editor
            .define_block(name, base, replace)
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message(format!("Block {name} defined."), false);
        Ok(())
    }

    fn block_insert(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 2 {
            return Err("INSERT syntax: INSERT name point [scale-x scale-y scale-z rotation] [rows columns row-spacing column-spacing] [tag=value ...].".into());
        }
        let name = tokens[0];
        let point = parse_point(tokens[1], self.drawing_elevation())?;
        let mut cursor = 2;
        let mut scale = [1.0; 3];
        let mut rotation = 0.0;
        if tokens.len() >= cursor + 2 && !tokens[cursor].contains('=') {
            if tokens[cursor].contains(',') {
                scale = parse_scale_triplet(tokens[cursor])?;
                cursor += 1;
                rotation = number(tokens[cursor])?.to_radians();
                cursor += 1;
            } else if tokens.len() >= cursor + 4 {
                for value in &mut scale {
                    *value = number(tokens[cursor])?;
                    cursor += 1;
                }
                rotation = number(tokens[cursor])?.to_radians();
                cursor += 1;
            }
        }
        let mut array = None;
        if tokens.len() >= cursor + 4 && !tokens[cursor].contains('=') {
            let rows = tokens[cursor]
                .parse::<usize>()
                .map_err(|_| "INSERT rows must be a whole number.")?;
            cursor += 1;
            let columns = tokens[cursor]
                .parse::<usize>()
                .map_err(|_| "INSERT columns must be a whole number.")?;
            cursor += 1;
            let row_spacing = number(tokens[cursor])?;
            cursor += 1;
            let column_spacing = number(tokens[cursor])?;
            cursor += 1;
            array = Some((rows, columns, row_spacing, column_spacing));
        }
        self.editor
            .insert_block(name, point, scale, rotation, array)
            .map_err(|e| e.to_string())?;
        if cursor < tokens.len() {
            let attrs = parse_attributes(&tokens[cursor..])?;
            let index = *self
                .editor
                .selection()
                .first()
                .ok_or("INSERT did not create an entity.")?;
            self.editor
                .set_insert_attributes(index, attrs)
                .map_err(|e| e.to_string())?;
        }
        self.finish();
        self.message(format!("Inserted block {name}."), false);
        Ok(())
    }

    fn block_attribute_definition(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 5 {
            return Err("ATTDEF syntax: ATTDEF block tag prompt default position.".into());
        }
        let block = tokens[0];
        let tag = tokens[1];
        let position = parse_point(tokens[tokens.len() - 1], self.drawing_elevation())?;
        let prompt = tokens[2].to_owned();
        let default_value = tokens[3..tokens.len() - 1].join(" ");
        self.editor
            .add_block_attribute_definition(
                block,
                AttributeDefinition::new(tag, prompt, default_value, position),
            )
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message(format!("Attribute {tag} added to block {block}."), false);
        Ok(())
    }

    fn block_attribute_edit(&mut self, tokens: &[&str]) -> Result<(), String> {
        let attrs = parse_attributes(tokens)?;
        if attrs.is_empty() {
            return Err("ATTEDIT syntax: ATTEDIT tag=value [tag=value ...].".into());
        }
        let indices = self.editor.selection().to_vec();
        if indices.len() != 1 {
            return Err("ATTEDIT requires exactly one selected INSERT.".into());
        }
        self.editor
            .set_insert_attributes(indices[0], attrs)
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Insert attributes updated.", false);
        Ok(())
    }

    fn block_info(&mut self, tokens: &[&str]) -> Result<(), String> {
        let name = if let Some(name) = tokens.first() {
            (*name).to_owned()
        } else if let Some(index) = self.editor.selection().first() {
            match self.editor.entities().get(*index) {
                Some(Entity::Insert(insert)) => insert.block_name().to_owned(),
                _ => return Err("BLOCKINFO requires a block name or selected INSERT.".into()),
            }
        } else {
            return Err("BLOCKINFO requires a block name or selected INSERT.".into());
        };
        let block = self
            .editor
            .document()
            .block(&name)
            .ok_or_else(|| format!("Block {name} is not defined."))?;
        self.message(
            format!(
                "Block {}: {} entities, {} attribute definition(s), base=({:.3},{:.3},{:.3})",
                block.name(),
                block.entities().len(),
                block.attribute_definitions().len(),
                block.base().x(),
                block.base().y(),
                block.base().z()
            ),
            false,
        );
        Ok(())
    }
}

fn parse_attributes(tokens: &[&str]) -> Result<Vec<Attribute>, String> {
    tokens
        .iter()
        .map(|value| {
            let (tag, value) = value
                .split_once('=')
                .ok_or("Attributes must use tag=value syntax.")?;
            if tag.trim().is_empty() {
                return Err("Attribute tags cannot be empty.".into());
            }
            Ok(Attribute::new(tag.trim(), value))
        })
        .collect()
}

fn parse_scale_triplet(value: &str) -> Result<[f64; 3], String> {
    let values: Vec<_> = value.split(',').collect();
    if values.len() != 3 {
        return Err("INSERT scale must be x,y,z.".into());
    }
    let mut scale = [0.0; 3];
    for (slot, value) in scale.iter_mut().zip(values) {
        *slot = number(value)?;
        if !slot.is_finite() || *slot <= 0.0 {
            return Err("INSERT scale values must be positive and finite.".into());
        }
    }
    Ok(scale)
}
