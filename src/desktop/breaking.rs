use super::commands::{entity_index, parse_point};
use super::*;

#[derive(Default)]
pub(super) struct BreakInput {
    source: Option<usize>,
    first: Option<Point3>,
}

impl OpenCadApp {
    pub(super) fn break_prompt(&self) -> &'static str {
        match (self.tool, self.break_input.source, self.break_input.first) {
            (Tool::BreakAtPoint, None, _) => {
                "BREAKATPOINT · Select an open line or arc (or enter its entity number)."
            }
            (Tool::BreakAtPoint, Some(_), _) => "BREAKATPOINT · Specify the break point: x,y[,z].",
            (_, None, _) => {
                "BREAK · Select a curve; the selection pick is the first break point. Or enter entity-number [pick-x,y[,z]]."
            }
            (_, Some(_), None) => "BREAK · Specify the first break point: x,y[,z].",
            (_, Some(_), Some(_)) => {
                "BREAK · Specify the second point: x,y[,z]; [First point]. @ splits without a gap."
            }
        }
    }

    fn break_source(&mut self, index: usize, pick: Option<Point3>) -> Result<(), String> {
        if !self.editor.is_entity_visible(index) {
            return Err("Break sources must be visible.".into());
        }
        let entity = self
            .editor
            .entities()
            .get(index)
            .ok_or("The curve no longer exists.")?;
        let curve = crate::trim::Curve::from_entity(entity)
            .ok_or_else(|| crate::GeometryError::UnsupportedBreakEntity.to_string())?;
        curve.validate().map_err(|error| error.to_string())?;
        if self.tool == Tool::BreakAtPoint && curve.closed() {
            return Err(crate::GeometryError::UnsupportedBreakAtPoint.to_string());
        }
        self.break_input.source = Some(index);
        self.break_input.first = if self.tool == Tool::Break { pick } else { None };
        self.editor.select(vec![index]);
        self.message(self.prompt(), false);
        Ok(())
    }

    pub(super) fn break_click(
        &mut self,
        source: Option<usize>,
        pick: Point3,
    ) -> Result<(), String> {
        if self.break_input.source.is_none() {
            self.break_source(source.ok_or("No curve at that position.")?, Some(pick))
        } else {
            self.break_point(pick)
        }
    }

    pub(super) fn break_point(&mut self, pick: Point3) -> Result<(), String> {
        let source = self
            .break_input
            .source
            .ok_or("Select a curve before specifying break points.")?;
        if self.tool == Tool::BreakAtPoint {
            return self.apply_break(source, pick, pick, true);
        }
        if let Some(first) = self.break_input.first {
            self.apply_break(source, first, pick, false)
        } else {
            self.break_input.first = Some(pick);
            self.message(self.prompt(), false);
            Ok(())
        }
    }

    fn apply_break(
        &mut self,
        source: usize,
        first: Point3,
        second: Point3,
        at_point: bool,
    ) -> Result<(), String> {
        if !self.editor.is_entity_visible(source) {
            return Err("Break sources must be visible.".into());
        }
        let changed = if at_point {
            self.editor.break_entity_at(source, first)
        } else {
            self.editor.break_entity(source, first, second)
        }
        .map_err(|error| error.to_string())?;
        if !changed {
            return Err("The curve no longer exists or its geometry is unchanged.".into());
        }
        self.finish();
        self.message("Curve broken. UNDO restores the complete source.", false);
        Ok(())
    }

    pub(super) fn break_preview(&self, pick: Point3) -> Option<Vec<Entity>> {
        let source = self.break_input.source?;
        let entity = self.editor.entities().get(source)?;
        if self.tool == Tool::BreakAtPoint {
            entity.break_at_xy(pick).ok()
        } else {
            entity.break_xy(self.break_input.first?, pick).ok()
        }
    }

    pub(super) fn break_preview_source(&self) -> Option<usize> {
        if matches!(self.tool, Tool::Break | Tool::BreakAtPoint)
            && self
                .cursor
                .and_then(|pick| self.break_preview(pick))
                .is_some()
        {
            self.break_input.source
        } else {
            None
        }
    }

    pub(super) fn break_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if !matches!(self.tool, Tool::Break | Tool::BreakAtPoint) {
            return None;
        }
        match input.to_ascii_uppercase().as_str() {
            "" | "X" | "EXIT" => {
                self.finish();
                self.message("Break command cancelled.", false);
                return Some(Ok(()));
            }
            "F" | "FIRST" | "FIRST POINT" if self.tool == Tool::Break => {
                return Some(if self.break_input.source.is_none() {
                    Err("Select a curve before overriding the first point.".into())
                } else {
                    self.break_input.first = None;
                    self.message(self.prompt(), false);
                    Ok(())
                });
            }
            "@" | "@0,0" | "@0,0,0" => {
                return Some(if self.tool == Tool::Break {
                    self.break_input
                        .first
                        .ok_or("Specify the first break point before using @.".into())
                        .and_then(|point| self.break_point(point))
                } else {
                    Err("BREAKATPOINT requires an absolute break point.".into())
                });
            }
            _ => {}
        }
        if !input.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | '@')) {
            return None;
        }
        Some((|| {
            if self.break_input.source.is_none() {
                let tokens: Vec<_> = input.split_whitespace().collect();
                if !(1..=2).contains(&tokens.len()) {
                    return Err("Select using entity-number [pick-x,y[,z]].".into());
                }
                let source = entity_index(tokens[0], self.editor.entities().len())?;
                let pick = tokens
                    .get(1)
                    .map(|value| parse_point(value, self.elevation))
                    .transpose()?;
                self.break_source(source, pick)
            } else {
                self.break_point(parse_point(input, self.elevation)?)
            }
        })())
    }

    pub(super) fn break_command(&mut self, tokens: &[&str], at_point: bool) -> Result<(), String> {
        if tokens.is_empty() {
            self.set_tool(if at_point {
                Tool::BreakAtPoint
            } else {
                Tool::Break
            });
            return Ok(());
        }
        if tokens.len() != if at_point { 2 } else { 3 } {
            return Err("Use BREAK entity-number first-x,y[,z] second-x,y[,z], or BREAKATPOINT entity-number x,y[,z].".into());
        }
        let source = entity_index(tokens[0], self.editor.entities().len())?;
        let first = parse_point(tokens[1], self.elevation)?;
        let second = if at_point || matches!(tokens[2], "@" | "@0,0" | "@0,0,0") {
            first
        } else {
            parse_point(tokens[2], self.elevation)?
        };
        self.apply_break(source, first, second, at_point)
    }
}
