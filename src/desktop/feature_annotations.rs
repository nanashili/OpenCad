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
            "TABLECELL" => self.annotation_table_cell(rest),
            "DIMTEXT" => self.annotation_dim_text(rest),
            "DIM" => self.annotation_dim_settings(tokens),
            "MLEADEREDIT" => self.mleader_edit(tokens),
            "MLEADERALIGN" => self.mleader_align(tokens),
            "MLEADERCOLLECT" => self.mleader_collect(),
            "STYLE" => self.annotation_text_style(tokens),
            "DIMSTYLE" => self.annotation_dimension_style(tokens),
            "MLEADERSTYLE" => self.annotation_mleader_style(tokens),
            "HATCHPATTERN" => self.annotation_hatch_pattern(tokens),
            "HATCHEDIT" => self.annotation_hatch_edit(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn mleader_edit(&mut self, tokens: &[&str]) -> Result<(), String> {
        let operation = tokens.first().copied().unwrap_or("LIST");
        if self.editor.selection().is_empty() {
            return Err("MLEADEREDIT requires selected MULTILEADER entities.".into());
        }
        match operation.to_ascii_uppercase().as_str() {
            "ADD" => {
                if tokens.len() < 2 {
                    return Err("MLEADEREDIT ADD point;point [...].".into());
                }
                let vertices = tokens[1..]
                    .iter()
                    .map(|value| parse_point(value, self.drawing_elevation()))
                    .collect::<Result<Vec<_>, _>>()?;
                self.editor
                    .map_selection_with(false, move |entity| match entity {
                        Entity::MLeader(item) => {
                            let mut next = item.clone();
                            next.add_branch(crate::MLeaderBranch::new(vertices.clone())?);
                            Ok(Entity::MLeader(next))
                        }
                        _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
                    })
                    .map_err(|e| e.to_string())?;
            }
            "REMOVE" => {
                let branch = tokens
                    .get(1)
                    .ok_or("MLEADEREDIT REMOVE branch-index.")?
                    .parse::<usize>()
                    .map_err(|_| "Branch index must be a whole number.")?;
                self.editor
                    .map_selection_with(false, move |entity| match entity {
                        Entity::MLeader(item) => {
                            let mut next = item.clone();
                            next.remove_branch(branch)?;
                            Ok(Entity::MLeader(next))
                        }
                        _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
                    })
                    .map_err(|e| e.to_string())?;
            }
            "LIST" => {
                let count = self
                    .editor
                    .selection()
                    .iter()
                    .filter_map(|index| match self.editor.entities().get(*index) {
                        Some(Entity::MLeader(item)) => Some(item.branches().len()),
                        _ => None,
                    })
                    .sum::<usize>();
                self.message(
                    format!("Selected multileaders contain {count} branches."),
                    false,
                );
                return Ok(());
            }
            _ => return Err("MLEADEREDIT operation must be ADD, REMOVE, or LIST.".into()),
        }
        self.finish();
        self.message("Multileader updated.", false);
        Ok(())
    }

    fn annotation_text_style(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() || tokens[0].eq_ignore_ascii_case("LIST") {
            let names = self
                .editor
                .document()
                .text_styles()
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(", ");
            self.message(
                if names.is_empty() {
                    "No text styles.".into()
                } else {
                    names
                },
                false,
            );
            return Ok(());
        }
        let action = tokens[0].to_ascii_uppercase();
        let mut document = self.editor.document().clone();
        match action.as_str() {
            "CREATE" => {
                let name = tokens.get(1).ok_or("STYLE CREATE name.")?;
                let style = crate::TextStyle::new(*name).map_err(|e| e.to_string())?;
                let mut styles = document.text_styles().to_vec();
                styles.push(style);
                document
                    .set_text_styles(styles)
                    .map_err(|e| e.to_string())?;
            }
            "SET" | "APPLY" => {
                let name = tokens.get(1).ok_or("STYLE SET name.")?;
                document
                    .set_current_text_style(*name)
                    .map_err(|e| e.to_string())?;
                let style = document
                    .text_style(name)
                    .cloned()
                    .ok_or("Unknown text style.")?;
                if !self.editor.selection().is_empty() {
                    self.editor
                        .map_selection_with(false, |entity| match entity {
                            Entity::Text(item) => {
                                let mut next = item.clone();
                                next.apply_style(&style)?;
                                Ok(Entity::Text(next))
                            }
                            Entity::MText(item) => {
                                let mut next = item.clone();
                                next.text_mut().apply_style(&style)?;
                                Ok(Entity::MText(next))
                            }
                            _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
                        })
                        .map_err(|e| e.to_string())?;
                }
            }
            "EDIT" => {
                let name = tokens.get(1).ok_or("STYLE EDIT name property value.")?;
                let property = tokens
                    .get(2)
                    .ok_or("STYLE EDIT name property value.")?
                    .to_ascii_uppercase();
                let value = tokens.get(3).ok_or("STYLE EDIT name property value.")?;
                let mut styles = document.text_styles().to_vec();
                let style = styles
                    .iter_mut()
                    .find(|s| s.name().eq_ignore_ascii_case(name))
                    .ok_or("Unknown text style.")?;
                match property.as_str() {
                    "HEIGHT" => style
                        .set_height(number(value)?)
                        .map_err(|e| e.to_string())?,
                    "WIDTH" | "WIDTHFACTOR" => style
                        .set_width_factor(number(value)?)
                        .map_err(|e| e.to_string())?,
                    "OBLIQUE" => style
                        .set_oblique(number(value)?.to_radians())
                        .map_err(|e| e.to_string())?,
                    "FONT" => style.set_font(*value).map_err(|e| e.to_string())?,
                    _ => {
                        return Err(
                            "STYLE property must be HEIGHT, WIDTH, OBLIQUE, or FONT.".into()
                        );
                    }
                }
                document
                    .set_text_styles(styles)
                    .map_err(|e| e.to_string())?;
            }
            _ => return Err("STYLE operation must be LIST, CREATE, SET, APPLY, or EDIT.".into()),
        }
        self.editor
            .commit_document(document)
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Text style updated.", false);
        Ok(())
    }

    fn annotation_dimension_style(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() || tokens[0].eq_ignore_ascii_case("LIST") {
            let names = self
                .editor
                .document()
                .dimension_styles()
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(", ");
            self.message(
                if names.is_empty() {
                    "No dimension styles.".into()
                } else {
                    names
                },
                false,
            );
            return Ok(());
        }
        let action = tokens[0].to_ascii_uppercase();
        let mut document = self.editor.document().clone();
        match action.as_str() {
            "CREATE" => {
                let name = tokens.get(1).ok_or("DIMSTYLE CREATE name.")?;
                let style = crate::DimensionStyle::new(*name).map_err(|e| e.to_string())?;
                let mut styles = document.dimension_styles().to_vec();
                styles.push(style);
                document
                    .set_dimension_styles(styles)
                    .map_err(|e| e.to_string())?;
            }
            "SET" | "APPLY" => {
                let name = tokens.get(1).ok_or("DIMSTYLE SET name.")?;
                document
                    .set_current_dimension_style(*name)
                    .map_err(|e| e.to_string())?;
                let style = document
                    .dimension_style(name)
                    .cloned()
                    .ok_or("Unknown dimension style.")?;
                if !self.editor.selection().is_empty() {
                    self.editor
                        .map_selection_with(false, |entity| match entity {
                            Entity::Dimension(item) => {
                                let mut next = item.clone();
                                next.apply_style(&style)?;
                                Ok(Entity::Dimension(next))
                            }
                            _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
                        })
                        .map_err(|e| e.to_string())?;
                }
            }
            "EDIT" => {
                let name = tokens.get(1).ok_or("DIMSTYLE EDIT name property value.")?;
                let property = tokens
                    .get(2)
                    .ok_or("DIMSTYLE EDIT name property value.")?
                    .to_ascii_uppercase();
                let value = number(tokens.get(3).ok_or("DIMSTYLE EDIT name property value.")?)?;
                let mut styles = document.dimension_styles().to_vec();
                let style = styles
                    .iter_mut()
                    .find(|s| s.name().eq_ignore_ascii_case(name))
                    .ok_or("Unknown dimension style.")?;
                match property.as_str() {
                    "HEIGHT" => style.set_text_height(value).map_err(|e| e.to_string())?,
                    "ARROW" => style.set_arrow_size(value).map_err(|e| e.to_string())?,
                    "SCALE" => style.set_overall_scale(value).map_err(|e| e.to_string())?,
                    "LINEARSCALE" => style.set_linear_scale(value).map_err(|e| e.to_string())?,
                    "GAP" => style.set_gap(value).map_err(|e| e.to_string())?,
                    _ => {
                        return Err(
                            "DIMSTYLE property is HEIGHT, ARROW, SCALE, LINEARSCALE, or GAP."
                                .into(),
                        );
                    }
                }
                document
                    .set_dimension_styles(styles)
                    .map_err(|e| e.to_string())?;
            }
            _ => {
                return Err("DIMSTYLE operation must be LIST, CREATE, SET, APPLY, or EDIT.".into());
            }
        }
        self.editor
            .commit_document(document)
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Dimension style updated.", false);
        Ok(())
    }

    fn annotation_mleader_style(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() || tokens[0].eq_ignore_ascii_case("LIST") {
            let names = self
                .editor
                .document()
                .mleader_styles()
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(", ");
            self.message(
                if names.is_empty() {
                    "No multileader styles.".into()
                } else {
                    names
                },
                false,
            );
            return Ok(());
        }
        let action = tokens[0].to_ascii_uppercase();
        let mut document = self.editor.document().clone();
        match action.as_str() {
            "CREATE" => {
                let style =
                    crate::MLeaderStyle::new(*tokens.get(1).ok_or("MLEADERSTYLE CREATE name.")?)
                        .map_err(|e| e.to_string())?;
                let mut styles = document.mleader_styles().to_vec();
                styles.push(style);
                document
                    .set_mleader_styles(styles)
                    .map_err(|e| e.to_string())?;
            }
            "SET" | "APPLY" => {
                let name = tokens.get(1).ok_or("MLEADERSTYLE SET name.")?;
                document
                    .set_current_mleader_style(*name)
                    .map_err(|e| e.to_string())?;
                let style = document
                    .mleader_style(name)
                    .cloned()
                    .ok_or("Unknown multileader style.")?;
                if !self.editor.selection().is_empty() {
                    self.editor
                        .map_selection_with(false, |entity| match entity {
                            Entity::MLeader(item) => {
                                let mut next = item.clone();
                                next.apply_style(&style)?;
                                Ok(Entity::MLeader(next))
                            }
                            _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
                        })
                        .map_err(|e| e.to_string())?;
                }
            }
            "EDIT" => {
                let name = tokens
                    .get(1)
                    .ok_or("MLEADERSTYLE EDIT name property value.")?;
                let property = tokens
                    .get(2)
                    .ok_or("MLEADERSTYLE EDIT name property value.")?
                    .to_ascii_uppercase();
                let value = number(
                    tokens
                        .get(3)
                        .ok_or("MLEADERSTYLE EDIT name property value.")?,
                )?;
                let mut styles = document.mleader_styles().to_vec();
                let style = styles
                    .iter_mut()
                    .find(|s| s.name().eq_ignore_ascii_case(name))
                    .ok_or("Unknown multileader style.")?;
                match property.as_str() {
                    "HEIGHT" => style.set_text_height(value).map_err(|e| e.to_string())?,
                    "ARROW" => style.set_arrow_size(value).map_err(|e| e.to_string())?,
                    "GAP" => style.set_landing_gap(value).map_err(|e| e.to_string())?,
                    "DOGLEG" => style.set_dogleg_length(value).map_err(|e| e.to_string())?,
                    _ => {
                        return Err(
                            "MLEADERSTYLE property is HEIGHT, ARROW, GAP, or DOGLEG.".into()
                        );
                    }
                }
                document
                    .set_mleader_styles(styles)
                    .map_err(|e| e.to_string())?;
            }
            _ => {
                return Err(
                    "MLEADERSTYLE operation must be LIST, CREATE, SET, APPLY, or EDIT.".into(),
                );
            }
        }
        self.editor
            .commit_document(document)
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Multileader style updated.", false);
        Ok(())
    }

    fn mleader_align(&mut self, tokens: &[&str]) -> Result<(), String> {
        let target = tokens
            .first()
            .ok_or_else(|| "MLEADERALIGN syntax: x,y[,z].".to_owned())
            .and_then(|value| parse_point(value, self.drawing_elevation()))?;
        self.editor
            .map_selection_with(false, move |entity| match entity {
                Entity::MLeader(item) => {
                    let branches = item
                        .branches()
                        .iter()
                        .map(|branch| {
                            let mut points = branch.vertices().to_vec();
                            if let Some(last) = points.last_mut() {
                                *last = crate::Point3::new(target.x(), target.y(), last.z())?;
                            }
                            crate::MLeaderBranch::new(points)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut next = crate::MLeader::with_branches(branches, item.content().clone())?;
                    next.set_style_name(item.style_name().to_owned())?;
                    next.set_arrow_size(item.arrow_size())?;
                    next.set_landing_gap(item.landing_gap())?;
                    next.set_dogleg_length(item.dogleg_length())?;
                    next.set_text_frame(item.text_frame());
                    Ok(Entity::MLeader(next))
                }
                _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
            })
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Multileaders aligned.", false);
        Ok(())
    }

    fn mleader_collect(&mut self) -> Result<(), String> {
        let selected = self.editor.selection().to_vec();
        if selected.len() < 2 {
            return Err("MLEADERCOLLECT requires at least two selected multileaders.".into());
        }
        let mut source = None;
        let mut branches = Vec::new();
        for index in &selected {
            match self.editor.entities().get(*index) {
                Some(Entity::MLeader(item)) => {
                    if source.is_none() {
                        source = Some(item.content().clone());
                    }
                    branches.extend(item.branches().iter().cloned());
                }
                _ => {
                    return Err(
                        "MLEADERCOLLECT selection must contain only MULTILEADER entities.".into(),
                    );
                }
            }
        }
        let content = source.ok_or("No multileader content found.")?;
        let mut combined =
            crate::MLeader::with_branches(branches, content).map_err(|e| e.to_string())?;
        if let Some(Entity::MLeader(first)) = self.editor.entities().get(selected[0]) {
            combined
                .set_style_name(first.style_name().to_owned())
                .map_err(|e| e.to_string())?;
            combined
                .set_arrow_size(first.arrow_size())
                .map_err(|e| e.to_string())?;
            combined
                .set_landing_gap(first.landing_gap())
                .map_err(|e| e.to_string())?;
            combined
                .set_dogleg_length(first.dogleg_length())
                .map_err(|e| e.to_string())?;
            combined.set_text_frame(first.text_frame());
        }
        self.editor.select(selected);
        self.editor
            .replace_selection_with_entities(vec![Entity::MLeader(combined)])
            .map_err(|e| e.to_string())?;
        self.finish();
        self.message("Multileaders collected.", false);
        Ok(())
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

    fn annotation_table_cell(&mut self, rest: &str) -> Result<(), String> {
        let tokens = super::commands::command_tokens(rest)?;
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
        if self.editor.selection().len() != 1 {
            return Err("TABLECELL requires exactly one selected TABLE.".into());
        }
        let selected_index = self.editor.selection()[0];
        if !matches!(
            self.editor.entities().get(selected_index),
            Some(Entity::Table(_))
        ) {
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
