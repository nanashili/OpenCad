use super::commands::{entity_index, number, parse_point};
use super::*;

#[derive(Clone, Copy)]
enum ValueKind {
    Delta,
    Percent,
    Total,
}
impl ValueKind {
    fn parse(input: &str) -> Option<Self> {
        match input.to_ascii_uppercase().as_str() {
            "DE" | "DELTA" => Some(Self::Delta),
            "P" | "PERCENT" => Some(Self::Percent),
            "T" | "TOTAL" => Some(Self::Total),
            _ => None,
        }
    }
    fn operation(self, value: f64, angle: bool) -> Result<Lengthen, String> {
        let operation = match (self, angle) {
            (Self::Delta, false) => Lengthen::Delta(value),
            (Self::Percent, false) => Lengthen::Percent(value),
            (Self::Total, false) => Lengthen::Total(value),
            (Self::Delta, true) => Lengthen::DeltaAngle(value.to_radians()),
            (Self::Total, true) => Lengthen::TotalAngle(value.to_radians()),
            (Self::Percent, true) => return Err("Angle is available with Delta or Total.".into()),
        };
        operation.validate().map_err(|error| error.to_string())?;
        Ok(operation)
    }
}

#[derive(Clone, Copy, Default)]
enum Mode {
    #[default]
    Inspect,
    Value(ValueKind, bool),
    Fixed(Lengthen),
    Dynamic,
}

#[derive(Default)]
pub(super) struct LengthenInput {
    mode: Mode,
    source: Option<(usize, Point3)>,
    undo: Vec<u64>,
}

impl OpenCadApp {
    pub(super) fn lengthen_prompt(&self) -> &'static str {
        match self.lengthen_input.mode {
            Mode::Inspect => {
                "LENGTHEN · Select a curve to measure, or [DElta/Percent/Total/DYnamic/Undo/Exit]."
            }
            Mode::Value(ValueKind::Delta, false) => {
                "LENGTHEN · Specify signed delta length, or [Angle]."
            }
            Mode::Value(ValueKind::Percent, _) => {
                "LENGTHEN · Specify a positive percentage (100 keeps the current length)."
            }
            Mode::Value(ValueKind::Total, false) => {
                "LENGTHEN · Specify a positive total length, or [Angle]."
            }
            Mode::Value(_, true) => {
                "LENGTHEN · Specify the angle in degrees (Delta may be negative)."
            }
            Mode::Fixed(_) => {
                "LENGTHEN · Pick near the endpoint to change, or enter entity-number x,y[,z]; [DElta/Percent/Total/DYnamic/Undo/Exit]."
            }
            Mode::Dynamic if self.lengthen_input.source.is_some() => {
                "LENGTHEN · Specify the new endpoint position: x,y[,z]."
            }
            Mode::Dynamic => {
                "LENGTHEN · Select near an endpoint, then specify its new position; you can also drag it. [Undo/Exit]."
            }
        }
    }

    pub(super) fn lengthen_awaits_source(&self) -> bool {
        self.tool == Tool::Lengthen
            && matches!(self.lengthen_input.mode, Mode::Dynamic)
            && self.lengthen_input.source.is_none()
    }

    pub(super) fn lengthen_awaits_point(&self) -> bool {
        self.tool == Tool::Lengthen
            && matches!(self.lengthen_input.mode, Mode::Dynamic)
            && self.lengthen_input.source.is_some()
    }

    pub(super) fn lengthen_click(
        &mut self,
        source: Option<usize>,
        pick: Point3,
    ) -> Result<(), String> {
        if self.lengthen_awaits_point() {
            return self.lengthen_point(pick);
        }
        if matches!(self.lengthen_input.mode, Mode::Value(_, _)) {
            return Err("Answer the length or angle prompt first.".into());
        }
        let source = source.ok_or("No curve at that position.")?;
        if !self.editor.is_entity_visible(source) {
            return Err("Lengthen sources must be visible.".into());
        }
        let entity = &self.editor.entities()[source];
        match self.lengthen_input.mode {
            Mode::Inspect => {
                let length = match entity {
                    Entity::Line(line) => line.length(),
                    Entity::Arc(arc) => arc.length(),
                    Entity::Circle(circle) => {
                        let length = circle.radius() * std::f64::consts::TAU;
                        if length.is_finite() {
                            Ok(length)
                        } else {
                            Err(crate::GeometryError::NonFiniteResult)
                        }
                    }
                    _ => Err(crate::GeometryError::UnsupportedLengthenEntity),
                }
                .map_err(|error| error.to_string())?;
                let angle = if let Entity::Arc(arc) = entity {
                    format!("; included angle = {:.4} degrees", arc.sweep().to_degrees())
                } else {
                    String::new()
                };
                self.editor.select(vec![source]);
                self.message(
                    format!("Length = {length:.4}{angle}. {}", self.prompt()),
                    false,
                );
                Ok(())
            }
            Mode::Fixed(operation) => self.apply_lengthen(source, pick, operation),
            Mode::Dynamic => {
                if matches!(entity, Entity::Circle(_)) {
                    self.message("Closed circles are unchanged by LENGTHEN.", false);
                    return Ok(());
                }
                let curve = crate::trim::Curve::from_entity(entity)
                    .ok_or_else(|| crate::GeometryError::UnsupportedLengthenEntity.to_string())?;
                curve
                    .validate()
                    .and_then(|()| curve.nearer_start(pick))
                    .map_err(|error| error.to_string())?;
                self.lengthen_input.source = Some((source, pick));
                self.editor.select(vec![source]);
                self.message(self.prompt(), false);
                Ok(())
            }
            Mode::Value(_, _) => unreachable!("value prompts were handled above"),
        }
    }

    pub(super) fn lengthen_point(&mut self, point: Point3) -> Result<(), String> {
        let (source, pick) = self
            .lengthen_input
            .source
            .ok_or("Select a Dynamic endpoint first.")?;
        self.apply_lengthen(source, pick, Lengthen::Dynamic(point))
    }

    fn apply_lengthen(
        &mut self,
        source: usize,
        pick: Point3,
        operation: Lengthen,
    ) -> Result<(), String> {
        if !self.editor.is_entity_visible(source) {
            return Err("Lengthen sources must be visible.".into());
        }
        let changed = self
            .editor
            .lengthen_entity(source, pick, operation)
            .map_err(|error| error.to_string())?;
        if changed && self.tool == Tool::Lengthen {
            self.lengthen_input.undo.push(self.editor.revision());
        }
        self.lengthen_input.source = None;
        self.message(
            format!(
                "{} {}",
                if changed {
                    "Curve length changed."
                } else {
                    "Geometry unchanged; closed circles are not lengthened."
                },
                self.prompt()
            ),
            false,
        );
        Ok(())
    }

    pub(super) fn lengthen_preview(
        &self,
        source: Option<usize>,
        pick: Point3,
    ) -> Option<(usize, Entity)> {
        if self.tool != Tool::Lengthen {
            return None;
        }
        let (source, pick, operation) = match self.lengthen_input.mode {
            Mode::Fixed(operation) => (source?, pick, operation),
            Mode::Dynamic => {
                let (source, first) = self.lengthen_input.source?;
                (source, first, Lengthen::Dynamic(pick))
            }
            _ => return None,
        };
        let entity = self
            .editor
            .entities()
            .get(source)?
            .lengthen_xy(pick, operation)
            .ok()?;
        Some((source, entity))
    }

    pub(super) fn lengthen_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if self.tool != Tool::Lengthen {
            return None;
        }
        let upper = input.to_ascii_uppercase();
        if let Some(kind) = ValueKind::parse(input) {
            self.lengthen_input.mode = Mode::Value(kind, false);
            self.lengthen_input.source = None;
            self.message(self.prompt(), false);
            return Some(Ok(()));
        }
        match upper.as_str() {
            "DY" | "DYNAMIC" => {
                self.lengthen_input.mode = Mode::Dynamic;
                self.lengthen_input.source = None;
            }
            "A" | "ANGLE" => {
                return Some(if let Mode::Value(kind, _) = self.lengthen_input.mode {
                    if matches!(kind, ValueKind::Percent) {
                        Err("Angle is available with Delta or Total.".into())
                    } else {
                        self.lengthen_input.mode = Mode::Value(kind, true);
                        self.message(self.prompt(), false);
                        Ok(())
                    }
                } else {
                    Err("Choose Delta or Total before Angle.".into())
                });
            }
            "U" | "UNDO" => return Some(self.undo_lengthen()),
            "" | "X" | "EXIT" => {
                self.finish();
                self.message("Lengthen command finished.", false);
                return Some(Ok(()));
            }
            _ => {
                if let Mode::Value(kind, angle) = self.lengthen_input.mode {
                    // Leave complete commands available while awaiting a value.
                    if input.split_whitespace().count() == 1
                        && (input.starts_with(|c: char| {
                            c.is_ascii_digit() || matches!(c, '+' | '-' | '.')
                        }) || input.parse::<f64>().is_ok())
                    {
                        return Some(
                            number(input)
                                .and_then(|value| kind.operation(value, angle))
                                .map(|operation| {
                                    self.lengthen_input.mode = Mode::Fixed(operation);
                                    self.message(self.prompt(), false);
                                }),
                        );
                    }
                    return None;
                }
                if !input.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
                {
                    return None;
                }
                return Some((|| {
                    if self.lengthen_awaits_point() {
                        return self.lengthen_point(parse_point(input, self.elevation)?);
                    }
                    let tokens: Vec<_> = input.split_whitespace().collect();
                    if tokens.len() != 2 {
                        return Err("Use entity-number pick-x,y[,z] to choose an endpoint.".into());
                    }
                    let source = entity_index(tokens[0], self.editor.entities().len())?;
                    self.lengthen_click(Some(source), parse_point(tokens[1], self.elevation)?)
                })());
            }
        }
        self.message(self.prompt(), false);
        Some(Ok(()))
    }

    fn undo_lengthen(&mut self) -> Result<(), String> {
        let revision = *self
            .lengthen_input
            .undo
            .last()
            .ok_or("There are no lengthen edits to undo in this command.")?;
        if self.editor.revision() != revision {
            self.lengthen_input.undo.clear();
            return Err("Lengthen Undo is unavailable after another drawing edit.".into());
        }
        if !self.editor.undo() {
            return Err("Lengthen undo history is unavailable.".into());
        }
        self.lengthen_input.undo.pop();
        self.lengthen_input.source = None;
        self.message(format!("Lengthen edit undone. {}", self.prompt()), false);
        Ok(())
    }

    pub(super) fn lengthen_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() {
            self.set_tool(Tool::Lengthen);
            return Ok(());
        }
        let (source, pick, operation) =
            if tokens[0].eq_ignore_ascii_case("DYNAMIC") || tokens[0].eq_ignore_ascii_case("DY") {
                if tokens.len() != 4 {
                    return Err(
                        "Use LENGTHEN DYNAMIC entity-number pick-x,y[,z] target-x,y[,z].".into(),
                    );
                }
                (
                    entity_index(tokens[1], self.editor.entities().len())?,
                    parse_point(tokens[2], self.elevation)?,
                    Lengthen::Dynamic(parse_point(tokens[3], self.elevation)?),
                )
            } else {
                let kind = ValueKind::parse(tokens[0])
                    .ok_or("Lengthen mode must be Delta, Percent, Total or Dynamic.")?;
                let angle = tokens
                    .get(1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("ANGLE"));
                let shift = usize::from(angle);
                if tokens.len() != 4 + shift {
                    return Err(
                    "Use LENGTHEN DELTA|PERCENT|TOTAL [ANGLE] value entity-number pick-x,y[,z]."
                        .into(),
                );
                }
                (
                    entity_index(tokens[2 + shift], self.editor.entities().len())?,
                    parse_point(tokens[3 + shift], self.elevation)?,
                    kind.operation(number(tokens[1 + shift])?, angle)?,
                )
            };
        self.finish();
        self.apply_lengthen(source, pick, operation)
    }
}
