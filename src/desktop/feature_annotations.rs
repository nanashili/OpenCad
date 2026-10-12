use super::commands::{number, parse_point};
use super::*;
use crate::{Entity, HatchPattern};

/// Editing commands for the annotation entities.  All edits go through the
/// editor transaction path, so source records and undo history stay aligned.
impl OpenCadApp {
    pub(super) fn feature_annotations_command(
        &mut self,
        verb: &str,
        rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "TEXTEDIT" => self.annotation_text_edit(rest),
            "TABLECELL" => self.annotation_table_cell(tokens),
            "DIMTEXT" => self.annotation_dim_text(rest),
            "DIM" => self.annotation_dim_settings(tokens),
            "HATCHPATTERN" => self.annotation_hatch_pattern(tokens),
            "HATCHEDIT" => self.annotation_hatch_edit(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn annotation_text_edit(&mut self, text: &str) -> Result<(), String> {
        let value = text.trim();
        if value.is_empty() {
            return Err("TEXTEDIT requires replacement text.".into());
        }
        if self.editor.selection().is_empty() {
            return Err("TEXTEDIT requires a selected TEXT or MTEXT entity.".into());
        }
        self.editor
            .map_selection_with(false, |entity| match entity {
                Entity::Text(item) => {
                    let mut next = item.clone();
                    next.set_content(value);
                    Ok(Entity::Text(next))
                }
                Entity::MText(item) => {
                    let mut next = item.clone();
                    next.set_content(value);
                    Ok(Entity::MText(next))
                }
                _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
            })
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Text updated.", false);
        Ok(())
    }

    fn annotation_table_cell(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 3 {
            return Err("TABLECELL syntax: TABLECELL row column text.".into());
        }
        let row = tokens[0]
            .parse::<usize>()
            .map_err(|_| "Table row must be a whole number.")?;
        let column = tokens[1]
            .parse::<usize>()
            .map_err(|_| "Table column must be a whole number.")?;
        let value = tokens[2..].join(" ");
        if self.editor.selection().is_empty() {
            return Err("TABLECELL requires a selected TABLE.".into());
        }
        self.editor
            .map_selection_with(false, move |entity| match entity {
                Entity::Table(item) => {
                    let mut next = item.clone();
                    next.set_cell(row, column, value.clone())?;
                    Ok(Entity::Table(next))
                }
                _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
            })
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Table cell updated.", false);
        Ok(())
    }

    fn annotation_dim_text(&mut self, text: &str) -> Result<(), String> {
        let value = text.trim();
        if value.is_empty() {
            return Err("DIMTEXT requires replacement text.".into());
        }
        self.editor
            .map_selection_with(false, |entity| match entity {
                Entity::Dimension(item) => {
                    let mut next = item.clone();
                    next.set_text_override(Some(value));
                    Ok(Entity::Dimension(next))
                }
                _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
            })
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Dimension text updated.", false);
        Ok(())
    }

    fn annotation_dim_settings(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 2 {
            return Err("DIM syntax: DIM HEIGHT value or DIM ARROW value.".into());
        }
        let value = number(tokens[1])?;
        let mode = tokens[0].to_ascii_uppercase();
        self.editor
            .map_selection_with(false, move |entity| match entity {
                Entity::Dimension(item) => {
                    let mut next = item.clone();
                    match mode.as_str() {
                        "HEIGHT" | "TEXTHEIGHT" => next.set_text_height(value)?,
                        "ARROW" | "ARROWSIZE" => next.set_arrow_size(value)?,
                        _ => return Err(crate::GeometryError::UnsupportedPolylineConversion),
                    }
                    Ok(Entity::Dimension(next))
                }
                _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
            })
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Dimension settings updated.", false);
        Ok(())
    }

    fn annotation_hatch_pattern(&mut self, tokens: &[&str]) -> Result<(), String> {
        let pattern = hatch_pattern(tokens)?;
        self.editor
            .map_selection_with(false, move |entity| match entity {
                Entity::Hatch(item) => {
                    let mut next = item.clone();
                    next.set_pattern(pattern)?;
                    Ok(Entity::Hatch(next))
                }
                _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
            })
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Hatch pattern updated.", false);
        Ok(())
    }

    fn annotation_hatch_edit(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() {
            return Err("HATCHEDIT syntax: HATCHEDIT SOLID, LINES angle spacing, or LOOP point point point.".into());
        }
        if tokens[0].eq_ignore_ascii_case("LOOP") {
            if tokens.len() < 4 {
                return Err("HATCHEDIT LOOP requires at least three points.".into());
            }
            let points = tokens[1..]
                .iter()
                .map(|value| parse_point(value, self.drawing_elevation()))
                .collect::<Result<Vec<_>, _>>()?;
            self.editor
                .map_selection_with(false, move |entity| match entity {
                    Entity::Hatch(item) => {
                        let mut next = item.clone();
                        next.set_boundaries(vec![points.clone()])?;
                        Ok(Entity::Hatch(next))
                    }
                    _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
                })
                .map_err(|e| e.to_string())?;
        } else {
            let pattern = hatch_pattern(tokens)?;
            self.editor
                .map_selection_with(false, move |entity| match entity {
                    Entity::Hatch(item) => {
                        let mut next = item.clone();
                        next.set_pattern(pattern)?;
                        Ok(Entity::Hatch(next))
                    }
                    _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
                })
                .map_err(|e| e.to_string())?;
        }
        self.finish();
        self.message("Hatch updated.", false);
        Ok(())
    }
}

fn hatch_pattern(tokens: &[&str]) -> Result<HatchPattern, String> {
    if tokens.is_empty() || tokens[0].eq_ignore_ascii_case("SOLID") {
        return Ok(HatchPattern::Solid);
    }
    if tokens.len() != 3 || !tokens[0].eq_ignore_ascii_case("LINES") {
        return Err("Hatch pattern must be SOLID or LINES angle spacing.".into());
    }
    let angle = number(tokens[1])?.to_radians();
    let spacing = number(tokens[2])?;
    if !angle.is_finite() {
        return Err("Hatch angle must be finite.".into());
    }
    if !spacing.is_finite() || spacing <= 0.0 {
        return Err("Hatch spacing must be positive and finite.".into());
    }
    Ok(HatchPattern::Lines { angle, spacing })
}
