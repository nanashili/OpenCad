use super::commands::{entity_index, number, parse_point};
use super::*;

#[derive(Clone, Copy, Default)]
enum Mode {
    #[default]
    Distance,
    Fixed(f64),
    Through,
}

#[derive(Clone, Copy, Default)]
enum OptionPrompt {
    #[default]
    None,
    Erase,
    Layer,
}

#[derive(Default)]
pub(super) struct OffsetInput {
    mode: Mode,
    source: Option<usize>,
    options: OffsetOptions,
    option_prompt: OptionPrompt,
    undo_revisions: Vec<u64>,
}

impl OffsetInput {
    fn geometry(&self, point: Point3) -> Result<Offset, String> {
        match self.mode {
            Mode::Distance => Err("Specify a positive offset distance or Through first.".into()),
            Mode::Fixed(distance) => {
                Offset::distance(distance, point).map_err(|error| error.to_string())
            }
            Mode::Through => Ok(Offset::through(point)),
        }
    }
}

impl OpenCadApp {
    pub(super) fn offset_prompt(&self) -> &'static str {
        match self.offset_input.option_prompt {
            OptionPrompt::Erase => {
                "OFFSET · Erase source? [Yes/No] (Enter keeps the current setting)."
            }
            OptionPrompt::Layer => {
                "OFFSET · Target layer [Current/Source] (Enter keeps the current setting)."
            }
            OptionPrompt::None => match self.offset_input.mode {
                Mode::Distance => "OFFSET · Positive distance or [Through/Erase/Layer] <Through>.",
                _ if self.offset_input.source.is_some() => {
                    "OFFSET · Specify side or through point, or [Undo/Exit]."
                }
                _ => {
                    "OFFSET · Select a line, circle or arc (or enter its number); [Undo/Exit/Erase/Layer]."
                }
            },
        }
    }

    pub(super) fn offset_awaits_source(&self) -> bool {
        self.tool == Tool::Offset
            && !matches!(self.offset_input.mode, Mode::Distance)
            && matches!(self.offset_input.option_prompt, OptionPrompt::None)
            && self.offset_input.source.is_none()
    }

    pub(super) fn offset_source(&mut self, source: Option<usize>) -> Result<(), String> {
        let source = source.ok_or("No source curve at that position.")?;
        let entity = self
            .editor
            .entities()
            .get(source)
            .ok_or("Entity number is outside this drawing.")?;
        if !self.editor.is_entity_visible(source) {
            return Err("The source curve must be visible.".into());
        }
        match entity {
            Entity::Line(line) if line.start().z() == line.end().z() => {}
            Entity::Circle(_) | Entity::Arc(_) => {}
            _ => return Err(crate::GeometryError::UnsupportedOffsetEntity.to_string()),
        }
        self.offset_input.source = Some(source);
        self.editor.select(vec![source]);
        self.message(self.prompt(), false);
        Ok(())
    }

    pub(super) fn offset_point(&mut self, point: Point3) -> Result<(), String> {
        if !matches!(self.offset_input.option_prompt, OptionPrompt::None) {
            return Err("Answer the offset option in the command field first.".into());
        }
        let offset = self.offset_input.geometry(point)?;
        let source = self
            .offset_input
            .source
            .ok_or("Select an offset source curve first.")?;
        if !self
            .editor
            .offset_entity(source, offset, self.offset_input.options)
            .map_err(|error| error.to_string())?
        {
            return Err("The source curve no longer exists.".into());
        }
        self.offset_input
            .undo_revisions
            .push(self.editor.revision());
        self.offset_input.source = None;
        self.message(format!("Offset created. {}", self.prompt()), false);
        Ok(())
    }

    pub(super) fn offset_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if self.tool != Tool::Offset {
            return None;
        }
        let upper = input.to_ascii_uppercase();
        match self.offset_input.option_prompt {
            OptionPrompt::Erase => {
                match upper.as_str() {
                    "Y" | "YES" => self.offset_input.options.erase_source = true,
                    "N" | "NO" => self.offset_input.options.erase_source = false,
                    "" => {}
                    _ => return Some(Err("Erase source must be Yes or No.".into())),
                }
                self.offset_input.option_prompt = OptionPrompt::None;
            }
            OptionPrompt::Layer => {
                match upper.as_str() {
                    "C" | "CURRENT" => self.offset_input.options.layer = OffsetLayer::Current,
                    "S" | "SOURCE" => self.offset_input.options.layer = OffsetLayer::Source,
                    "" => {}
                    _ => return Some(Err("Offset layer must be Current or Source.".into())),
                }
                self.offset_input.option_prompt = OptionPrompt::None;
            }
            OptionPrompt::None => match upper.as_str() {
                "E" | "ERASE" => self.offset_input.option_prompt = OptionPrompt::Erase,
                "L" | "LAYER" => self.offset_input.option_prompt = OptionPrompt::Layer,
                "T" | "THROUGH" => {
                    self.offset_input.mode = Mode::Through;
                    self.offset_input.source = None;
                }
                "D" | "DISTANCE" => {
                    self.offset_input.mode = Mode::Distance;
                    self.offset_input.source = None;
                }
                "U" | "UNDO" => return Some(self.undo_offset()),
                "" if matches!(self.offset_input.mode, Mode::Distance) => {
                    self.offset_input.mode = Mode::Through;
                }
                "" | "X" | "EXIT" => {
                    self.finish();
                    self.message("Offset command finished.", false);
                    return Some(Ok(()));
                }
                _ => {
                    if matches!(self.offset_input.mode, Mode::Distance) {
                        let value = input.parse::<f64>().ok()?;
                        return Some(
                            Offset::distance(value, Point3::new(0.0, 0.0, 0.0).unwrap())
                                .map_err(|error| error.to_string())
                                .map(|_| {
                                    self.offset_input.mode = Mode::Fixed(value);
                                    self.message(self.prompt(), false);
                                }),
                        );
                    }
                    if self.offset_awaits_source() && input.parse::<f64>().is_ok() {
                        return Some(
                            entity_index(input, self.editor.entities().len())
                                .and_then(|index| self.offset_source(Some(index))),
                        );
                    }
                    return None;
                }
            },
        }
        self.message(self.prompt(), false);
        Some(Ok(()))
    }

    fn undo_offset(&mut self) -> Result<(), String> {
        self.offset_input.source = None;
        let last = self
            .offset_input
            .undo_revisions
            .last()
            .copied()
            .ok_or("There are no offsets to undo in this command.")?;
        if self.editor.revision() != last {
            self.offset_input.undo_revisions.clear();
            return Err("Offset Undo is unavailable after another drawing edit.".into());
        }
        if !self.editor.undo() {
            return Err("The offset undo history is unavailable.".into());
        }
        self.offset_input.undo_revisions.pop();
        self.message(format!("Offset undone. {}", self.prompt()), false);
        Ok(())
    }

    pub(super) fn offset_preview(&self, point: Point3) -> Option<Entity> {
        if !matches!(self.offset_input.option_prompt, OptionPrompt::None) {
            return None;
        }
        self.editor
            .entities()
            .get(self.offset_input.source?)?
            .offset_xy(self.offset_input.geometry(point).ok()?)
            .ok()
    }

    pub(super) fn offset_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() {
            self.set_tool(Tool::Offset);
            return Ok(());
        }
        if !(3..=5).contains(&tokens.len()) {
            return Err("OFFSET syntax: OFFSET distance|THROUGH entity-number side-x,y[,z] [ERASE] [CURRENT|SOURCE]".into());
        }
        let source = entity_index(tokens[1], self.editor.entities().len())?;
        let point = parse_point(tokens[2], self.elevation)?;
        let offset = if tokens[0].eq_ignore_ascii_case("THROUGH") {
            Offset::through(point)
        } else {
            Offset::distance(number(tokens[0])?, point).map_err(|error| error.to_string())?
        };
        let mut options = OffsetOptions::default();
        let mut layer_seen = false;
        for option in &tokens[3..] {
            match option.to_ascii_uppercase().as_str() {
                "ERASE" if !options.erase_source => options.erase_source = true,
                "CURRENT" | "SOURCE" if !layer_seen => {
                    options.layer = if option.eq_ignore_ascii_case("CURRENT") {
                        OffsetLayer::Current
                    } else {
                        OffsetLayer::Source
                    };
                    layer_seen = true;
                }
                _ => return Err("Use ERASE and one of CURRENT or SOURCE once each.".into()),
            }
        }
        self.editor
            .offset_entity(source, offset, options)
            .map_err(|error| error.to_string())?;
        self.finish();
        self.message("Offset created.", false);
        Ok(())
    }
}
