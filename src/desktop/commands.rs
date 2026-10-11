use super::*;

pub(super) fn parse_entity_color(input: &str) -> Result<i16, String> {
    let input = input.trim();
    if input.eq_ignore_ascii_case("BYLAYER") {
        return Ok(256);
    }
    if input.eq_ignore_ascii_case("BYBLOCK") {
        return Ok(0);
    }
    input
        .parse::<i16>()
        .ok()
        .filter(|color| (0..=256).contains(color))
        .ok_or_else(|| crate::EntityStyleError::InvalidColor.to_string())
}

impl OpenCadApp {
    pub(super) fn execute(&mut self, input: &str) {
        let input = input.trim();
        if input.is_empty() {
            if let Some(result) = self
                .corner_text(input)
                .or_else(|| self.explode_text(input))
                .or_else(|| self.join_text(input))
                .or_else(|| self.pedit_text(input))
                .or_else(|| self.lengthen_text(input))
                .or_else(|| self.break_text(input))
                .or_else(|| self.trim_text(input))
                .or_else(|| self.offset_text(input))
                .or_else(|| self.transform_text(input))
            {
                if let Err(error) = result {
                    self.message(error, true);
                }
            } else if self.tool == Tool::Polyline && !self.points.is_empty() {
                if let Err(error) = self.finish_polyline(false) {
                    self.message(error, true);
                }
            } else {
                self.finish();
                self.message("Command finished.", false);
            }
            return;
        }
        self.history.push_back((format!("> {input}"), false));
        if let Err(error) = self.command_result(input) {
            self.message(error, true);
        }
    }

    fn command_result(&mut self, input: &str) -> Result<(), String> {
        if let Some(result) = self.pedit_text(input) {
            return result;
        }
        if let Some(result) = self.join_text(input) {
            return result;
        }
        if let Some(result) = self.explode_text(input) {
            return result;
        }
        if let Some(result) = self.lengthen_text(input) {
            return result;
        }
        if let Some(result) = self.break_text(input) {
            return result;
        }
        if let Some(result) = self.corner_text(input) {
            return result;
        }
        if let Some(result) = self.trim_text(input) {
            return result;
        }
        if input.contains(',') && !input.chars().next().is_some_and(char::is_alphabetic) {
            let relative = input.starts_with('@');
            let mut point = parse_point(
                input.trim_start_matches('@'),
                if relative {
                    0.0
                } else {
                    self.drawing_elevation()
                },
            )?;
            if relative {
                let base = self
                    .points
                    .last()
                    .copied()
                    .ok_or("A relative point needs a previous point.")?;
                point = Point3::new(
                    base.x() + point.x(),
                    base.y() + point.y(),
                    base.z() + point.z(),
                )
                .map_err(|e| e.to_string())?;
            }
            return self.point(point);
        }
        if self.tool == Tool::Circle && self.points.len() == 1 && input.parse::<f64>().is_ok() {
            let circle = Circle3::new(self.points[0], number(input)?).map_err(|e| e.to_string())?;
            self.add_entities(vec![Entity::Circle(circle)])?;
            self.finish();
            self.message("Circle created.", false);
            return Ok(());
        }
        if let Some(result) = self.transform_text(input) {
            return result;
        }
        if let Some(result) = self.offset_text(input) {
            return result;
        }
        let (verb, rest) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
        let rest = rest.trim();
        let tokens: Vec<_> = rest.split_whitespace().collect();
        match verb.to_ascii_uppercase().as_str() {
            "PEDITACCEPT" => {
                if !rest.is_empty() {
                    self.pedit_accept = match rest {
                        "0" => false,
                        "1" => true,
                        _ => return Err("PEDITACCEPT must be 0 or 1.".into()),
                    };
                    self.finish();
                }
                self.message(
                    format!("PEDITACCEPT = {}", u8::from(self.pedit_accept)),
                    false,
                );
            }
            "ENTITY" => {
                if tokens.len() != 2 {
                    return Err("ENTITY syntax: ENTITY COLOR BYLAYER|BYBLOCK|1..255; ENTITY LINETYPE name; ENTITY LTSCALE nonnegative-number".into());
                }
                self.finish();
                match tokens[0].to_ascii_uppercase().as_str() {
                    "COLOR" => self
                        .editor
                        .assign_selection_color(parse_entity_color(tokens[1])?),
                    "LINETYPE" => self.editor.assign_selection_linetype(tokens[1]),
                    "LTSCALE" => self
                        .editor
                        .assign_selection_linetype_scale(number(tokens[1])?),
                    _ => {
                        return Err(
                            "ENTITY supports COLOR, LINETYPE, or LTSCALE for the selection.".into(),
                        );
                    }
                }
                .map_err(|error| error.to_string())?;
                self.inspector.style.state.clear();
                self.message("Selection style applied.", false);
            }
            "LAYER" => {
                let (action, arguments) = rest
                    .split_once(char::is_whitespace)
                    .ok_or("LAYER syntax: LAYER NEW|SET|SHOW|HIDE|LOCK|UNLOCK|ASSIGN name; LAYER COLOR name index; LAYER LINETYPE name pattern-name")?;
                let mut name = arguments.trim().trim_matches('"');
                self.finish();
                match action.to_ascii_uppercase().as_str() {
                    "NEW" => {
                        self.editor.add_layer(name).map_err(|e| e.to_string())?;
                        self.editor
                            .set_current_layer(name)
                            .map_err(|e| e.to_string())?;
                    }
                    "SET" => self
                        .editor
                        .set_current_layer(name)
                        .map_err(|e| e.to_string())?,
                    "SHOW" | "HIDE" => {
                        self.editor
                            .set_layer_visible(name, action.eq_ignore_ascii_case("SHOW"))
                            .map_err(|e| e.to_string())?;
                    }
                    "LOCK" | "UNLOCK" => {
                        self.editor
                            .set_layer_locked(name, action.eq_ignore_ascii_case("LOCK"))
                            .map_err(|e| e.to_string())?;
                    }
                    "ASSIGN" => {
                        self.editor
                            .assign_selection_layer(name)
                            .map_err(|e| e.to_string())?;
                    }
                    "COLOR" => {
                        let (layer, index) = arguments
                            .trim()
                            .rsplit_once(char::is_whitespace)
                            .ok_or("LAYER COLOR syntax: LAYER COLOR name index (1–255)")?;
                        name = layer.trim().trim_matches('"');
                        let index = index
                            .parse::<i16>()
                            .map_err(|_| "Enter a color index from 1 to 255.")?;
                        self.editor
                            .set_layer_color(name, index)
                            .map_err(|error| error.to_string())?;
                    }
                    "LINETYPE" => {
                        let (layer, kind) = arguments
                            .trim()
                            .rsplit_once(char::is_whitespace)
                            .ok_or("LAYER LINETYPE syntax: LAYER LINETYPE name pattern-name")?;
                        name = layer.trim().trim_matches('"');
                        self.editor
                            .set_layer_linetype(name, kind)
                            .map_err(|error| error.to_string())?;
                    }
                    _ => {
                        return Err(
                            "LAYER syntax: LAYER NEW|SET|SHOW|HIDE|LOCK|UNLOCK|ASSIGN name; LAYER COLOR name index; LAYER LINETYPE name pattern-name".into(),
                        );
                    }
                }
                self.message(
                    format!("Layer {name}: {}.", action.to_ascii_lowercase()),
                    false,
                );
            }
            "LINETYPE" => {
                if tokens.len() != 2 {
                    return Err("LINETYPE syntax: LINETYPE name dash,gap,dot,... (example: LINETYPE DASHDOT 1,-0.25,0,-0.25)".into());
                }
                let kind = Linetype::new(tokens[0], parse_pattern(tokens[1])?)
                    .map_err(|error| error.to_string())?;
                self.finish();
                self.editor
                    .add_linetype(kind)
                    .map_err(|error| error.to_string())?;
                self.message(format!("Linetype {} created.", tokens[0]), false);
            }
            "LTSCALE" => {
                let scale = number(rest)?;
                self.finish();
                self.editor
                    .set_linetype_scale(scale)
                    .map_err(|error| error.to_string())?;
                self.message("Linetype scale applied.", false);
            }
            "FILLETRAD" | "CHAMFERA" | "CHAMFERB" => {
                let parameter = match verb.to_ascii_uppercase().as_str() {
                    "FILLETRAD" => CornerParameter::FilletRadius,
                    "CHAMFERA" => CornerParameter::ChamferFirst,
                    _ => CornerParameter::ChamferSecond,
                };
                if !rest.is_empty() {
                    self.editor
                        .set_corner_value(parameter, number(rest)?)
                        .map_err(|error| error.to_string())?;
                    self.finish();
                }
                self.message(
                    format!(
                        "{} = {}",
                        parameter.name(),
                        self.editor.corner_value(parameter)
                    ),
                    false,
                );
            }
            "LINE" | "L" => {
                if tokens.is_empty() {
                    self.set_tool(Tool::Line);
                } else {
                    let points = tokens
                        .iter()
                        .map(|s| parse_point(s, self.elevation))
                        .collect::<Result<Vec<_>, _>>()?;
                    if points.len() < 2 {
                        return Err("LINE needs at least two points: LINE 0,0 100,0".into());
                    }
                    let lines = points
                        .windows(2)
                        .map(|p| Entity::Line(Line3::new(p[0], p[1])))
                        .collect();
                    self.add_entities(lines)?;
                    self.finish();
                    self.message(
                        format!("Created {} line segments.", points.len() - 1),
                        false,
                    );
                }
            }
            "PLINE" | "POLYLINE" | "PL" => {
                if tokens.is_empty() {
                    self.set_tool(Tool::Polyline);
                } else {
                    let closed = tokens
                        .last()
                        .is_some_and(|token| token.eq_ignore_ascii_case("CLOSE"));
                    let count = tokens.len() - usize::from(closed);
                    let vertices = tokens[..count]
                        .iter()
                        .map(|token| parse_point(token, self.elevation))
                        .collect::<Result<Vec<_>, _>>()?;
                    let polyline =
                        Polyline2::new(vertices, closed).map_err(|error| error.to_string())?;
                    self.add_entities(vec![Entity::Polyline(polyline)])?;
                    self.finish();
                    self.message("Polyline created.", false);
                }
            }
            "C" | "CLOSE" if self.tool == Tool::Polyline => self.finish_polyline(true)?,
            "C" if self.tool == Tool::Line => self.close_line(),
            "CIRCLE" | "C" => {
                if tokens.is_empty() {
                    self.set_tool(Tool::Circle);
                } else if tokens.len() == 2 {
                    let circle =
                        Circle3::new(parse_point(tokens[0], self.elevation)?, number(tokens[1])?)
                            .map_err(|e| e.to_string())?;
                    self.add_entities(vec![Entity::Circle(circle)])?;
                    self.finish();
                    self.message("Circle created.", false);
                } else {
                    return Err("CIRCLE syntax: CIRCLE x,y[,z] radius".into());
                }
            }
            "ARC" | "A" => {
                if tokens.is_empty() {
                    self.set_tool(Tool::Arc);
                } else if tokens.len() == 4 {
                    let arc = Arc3::new(
                        parse_point(tokens[0], self.elevation)?,
                        number(tokens[1])?,
                        number(tokens[2])?.to_radians(),
                        number(tokens[3])?.to_radians(),
                    )
                    .map_err(|e| e.to_string())?;
                    self.add_entities(vec![Entity::Arc(arc)])?;
                    self.finish();
                    self.message(
                        "Arc created. Angles are degrees, counterclockwise from +X.",
                        false,
                    );
                } else {
                    return Err("ARC syntax: ARC x,y[,z] radius start-degrees end-degrees".into());
                }
            }
            "RECTANG" | "RECTANGLE" | "R" => {
                if tokens.is_empty() {
                    self.set_tool(Tool::Rectangle);
                } else if tokens.len() == 2 {
                    let entities = rectangle(
                        parse_point(tokens[0], self.elevation)?,
                        parse_point(tokens[1], self.elevation)?,
                    )?;
                    self.add_entities(entities)?;
                    self.finish();
                    self.message("Rectangle created as four independent lines.", false);
                } else {
                    return Err("RECTANG syntax: RECTANG x,y[,z] x,y".into());
                }
            }
            "SELECT" => {
                let indices = if rest.eq_ignore_ascii_case("ALL") {
                    (0..self.editor.entities().len()).collect()
                } else {
                    tokens
                        .iter()
                        .map(|token| {
                            let value = token
                                .parse::<usize>()
                                .map_err(|_| "SELECT needs entity numbers or ALL.")?;
                            if value == 0 || value > self.editor.entities().len() {
                                return Err("Entity number is outside this drawing.");
                            }
                            Ok(value - 1)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                self.finish();
                self.editor.select(indices);
                self.message(
                    format!("{} entities selected.", self.editor.selection().len()),
                    false,
                );
            }
            "OFFSET" | "O" => self.offset_command(&tokens)?,
            "TRIM" | "TR" | "T" => self.trim_command(&tokens, false)?,
            "FILLET" | "F" => self.corner_command(&tokens, false)?,
            "BREAK" => self.break_command(&tokens, false)?,
            "BREAKATPOINT" => self.break_command(&tokens, true)?,
            "LENGTHEN" | "LEN" => self.lengthen_command(&tokens)?,
            "EXPLODE" | "X" => self.explode_command(&tokens)?,
            "JOIN" | "J" => self.join_command(&tokens)?,
            "PEDIT" | "PE" => self.pedit_command(&tokens)?,
            "CHAMFER" | "CHA" => self.corner_command(&tokens, true)?,
            "EXTEND" | "EX" | "E" => self.trim_command(&tokens, true)?,
            "ROTATE" | "RO" | "SCALE" | "SC" => {
                let rotate = verb.eq_ignore_ascii_case("ROTATE") || verb.eq_ignore_ascii_case("RO");
                if tokens.is_empty() {
                    self.set_tool(if rotate { Tool::Rotate } else { Tool::Scale });
                } else {
                    if !(2..=3).contains(&tokens.len())
                        || (tokens.len() == 3 && !tokens[2].eq_ignore_ascii_case("COPY"))
                    {
                        return Err("ROTATE/SCALE syntax: ROTATE base-x,y[,z] degrees [COPY]; SCALE base-x,y[,z] positive-factor [COPY]".into());
                    }
                    let base = parse_point(tokens[0], self.elevation)?;
                    let value = number(tokens[1])?;
                    let transform = if rotate {
                        Transform::rotate_xy(base, value.rem_euclid(360.0).to_radians())
                    } else {
                        Transform::scale(base, value)
                    }
                    .map_err(|error| error.to_string())?;
                    self.apply_transform(transform, tokens.len() == 3)?;
                }
            }
            "MIRROR" | "MI" => {
                if tokens.is_empty() {
                    self.set_tool(Tool::Mirror);
                } else {
                    if !(2..=3).contains(&tokens.len()) {
                        return Err("MIRROR syntax: MIRROR first-x,y second-x,y [YES|NO] (erase source; default NO)".into());
                    }
                    let erase = match tokens
                        .get(2)
                        .map(|value| value.to_ascii_uppercase())
                        .as_deref()
                    {
                        None | Some("NO" | "N") => false,
                        Some("YES" | "Y") => true,
                        _ => return Err("MIRROR erase source option must be Yes or No.".into()),
                    };
                    let transform = Transform::mirror_xy(
                        parse_point(tokens[0], self.elevation)?,
                        parse_point(tokens[1], self.elevation)?,
                    )
                    .map_err(|error| error.to_string())?;
                    self.apply_transform(transform, !erase)?;
                }
            }
            "MOVE" | "M" | "COPY" => {
                let copy = verb.eq_ignore_ascii_case("COPY");
                if tokens.is_empty() {
                    self.set_tool(if copy { Tool::Copy } else { Tool::Move });
                } else {
                    let values = if tokens.len() == 1 {
                        tokens[0].split(',').collect::<Vec<_>>()
                    } else {
                        tokens
                    };
                    if !(2..=3).contains(&values.len()) {
                        return Err("MOVE/COPY needs dx dy [dz].".into());
                    }
                    let dx = number(values[0])?;
                    let dy = number(values[1])?;
                    let dz = if values.len() == 3 {
                        number(values[2])?
                    } else {
                        0.0
                    };
                    if !self
                        .editor
                        .translate_selection(dx, dy, dz, copy)
                        .map_err(|e| e.to_string())?
                    {
                        return Err("No selected entities changed. Select entities and use a nonzero displacement.".into());
                    }
                    self.finish();
                    self.message(
                        if copy {
                            "Selection copied."
                        } else {
                            "Selection moved."
                        },
                        false,
                    );
                }
            }
            "DELETE" | "ERASE" => {
                self.finish();
                if self
                    .editor
                    .delete_selection()
                    .map_err(|error| error.to_string())?
                {
                    self.message("Selection deleted.", false);
                } else {
                    return Err("Nothing selected.".into());
                }
            }
            "UNDO" => self.undo(false),
            "REDO" => self.undo(true),
            "U" => self.undo_line_point(),
            "CLOSE" => self.close_line(),
            "ZOOM" if rest.eq_ignore_ascii_case("EXTENTS") || rest.eq_ignore_ascii_case("E") => {
                self.fit_pending = true;
            }
            "FIT" => self.fit_pending = true,
            "ZOOM" if rest.to_ascii_uppercase().ends_with('X') => {
                let factor = number(&rest[..rest.len() - 1])?;
                self.camera.zoom(factor)?;
            }
            "PAN" if !rest.is_empty() => self.camera.pan(rest)?,
            "PAN" | "P" => self.set_tool(Tool::Pan),
            "CLOSEDRAWING" => self.request(PendingAction::CloseDrawing),
            "TOOLSETS" => self.workspace.tools = !self.workspace.tools,
            "LAYERPALETTE" => self.workspace.layers = !self.workspace.layers,
            "PROPERTIESTOGGLE" => self.workspace.properties = !self.workspace.properties,
            "COMMANDLINETOGGLE" => self.workspace.command = !self.workspace.command,
            "TOOLBAR" => self.workspace.toolbar = !self.workspace.toolbar,
            "FILETAB" => self.workspace.file_tabs = !self.workspace.file_tabs,
            "STATUSBAR" => self.workspace.status = !self.workspace.status,
            "VIEWCUBE" => self.workspace.view_cube = !self.workspace.view_cube,
            "UCSICON" => self.workspace.ucs_icon = !self.workspace.ucs_icon,
            "RESETPALETTES" => {
                self.workspace.tools = true;
                self.workspace.layers = true;
                self.workspace.properties = true;
                self.workspace.command = true;
                self.workspace.toolbar = true;
                self.workspace.file_tabs = true;
                self.workspace.status = true;
            }
            "PALETTES" => {
                let visible =
                    !(self.workspace.tools || self.workspace.layers || self.workspace.properties);
                self.workspace.tools = visible;
                self.workspace.layers = visible;
                self.workspace.properties = visible;
            }
            "REGEN" => {}

            "NEW" => self.request(PendingAction::New),
            "OPEN" => self.request(PendingAction::Open(path_argument(rest))),
            "SAVE" | "SAVEAS" => {
                if let Some(path) = path_argument(rest) {
                    let exists_elsewhere = path.exists() && self.path.as_ref() != Some(&path);
                    if exists_elsewhere
                        && rfd::MessageDialog::new()
                            .set_title("Replace drawing?")
                            .set_description(format!("Replace {}?", path.display()))
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show()
                            != rfd::MessageDialogResult::Yes
                    {
                        return Ok(());
                    }
                    self.save_path(path);
                } else {
                    self.save(verb.eq_ignore_ascii_case("SAVEAS"));
                }
            }
            "EXPORTSVG" => self.export_svg(path_argument(rest)),
            "GRID" | "SNAP" | "OSNAP" | "ORTHO" | "POLAR" => {
                let current = match verb.to_ascii_uppercase().as_str() {
                    "GRID" => self.grid,
                    "SNAP" => self.grid_snap,
                    "OSNAP" => self.object_snap,
                    "ORTHO" => self.ortho,
                    _ => self.polar_tracking,
                };
                let enabled = match rest.to_ascii_uppercase().as_str() {
                    "ON" => true,
                    "OFF" => false,
                    "" => !current,
                    _ => return Err("Use ON or OFF.".into()),
                };
                match verb.to_ascii_uppercase().as_str() {
                    "GRID" => self.grid = enabled,
                    "SNAP" => self.grid_snap = enabled,
                    "OSNAP" => self.object_snap = enabled,
                    "ORTHO" => self.set_ortho(enabled),
                    _ => self.set_polar_tracking(enabled),
                }
                self.message(
                    format!(
                        "{} {}.",
                        verb.to_ascii_uppercase(),
                        if enabled { "on" } else { "off" }
                    ),
                    false,
                );
            }
            "POLARANG" => {
                if !rest.is_empty() {
                    let angle = number(rest)?;
                    if angle <= 0.0 || angle > 360.0 {
                        return Err(
                            "Polar increment must be greater than 0 and at most 360 degrees."
                                .into(),
                        );
                    }
                    if !(360.0 / angle).is_finite() {
                        return Err("Polar increment is too small to resolve a turn.".into());
                    }
                    self.polar_angle = angle;
                    self.polar_guide = None;
                }
                self.message(
                    format!("Polar increment: {} degrees.", self.polar_angle),
                    false,
                );
            }
            "ELEVATION" => {
                self.elevation = number(rest)?;
                self.message(
                    format!("New drawing points use Z = {}.", self.elevation),
                    false,
                );
            }
            "HELP" | "?" => self.show_help = true,
            "ESC" | "CANCEL" => {
                self.finish();
                self.message("Command canceled.", false);
            }
            _ => {
                return Err(format!(
                    "Unknown command '{verb}'. Use HELP for commands and shortcuts."
                ));
            }
        }
        Ok(())
    }

    pub(super) fn undo_line_point(&mut self) {
        if self.tool == Tool::Polyline && !self.points.is_empty() {
            self.points.pop();
            self.message(self.prompt(), false);
            return;
        }
        if self.tool == Tool::Line && self.points.len() > 1 {
            self.editor.undo();
            self.points.pop();
            self.message(self.prompt(), false);
        } else {
            self.message("No segment in the current line chain to undo.", true);
        }
    }

    pub(super) fn close_line(&mut self) {
        if self.tool == Tool::Polyline {
            if let Err(error) = self.finish_polyline(true) {
                self.message(error, true);
            }
            return;
        }
        if self.tool != Tool::Line || self.points.len() < 3 {
            self.message(
                "Close needs at least two segments in the current line chain.",
                true,
            );
            return;
        }
        if let Err(error) = self.add_entities(vec![Entity::Line(Line3::new(
            *self.points.last().unwrap(),
            self.points[0],
        ))]) {
            self.message(error, true);
            return;
        }
        self.finish();
        self.message("Line chain closed.", false);
    }
}

fn path_argument(value: &str) -> Option<PathBuf> {
    let value = value.trim().trim_matches('"');
    (!value.is_empty()).then(|| PathBuf::from(value))
}

pub(super) fn parse_pattern(value: &str) -> Result<Vec<f64>, String> {
    value.split(',').map(|part| number(part.trim())).collect()
}

pub(super) fn number(value: &str) -> Result<f64, String> {
    let number = value
        .parse::<f64>()
        .map_err(|_| format!("Invalid number: {value}"))?;
    if !number.is_finite() {
        return Err("Numbers must be finite.".into());
    }
    Ok(number)
}

pub(super) fn entity_index(input: &str, count: usize) -> Result<usize, String> {
    input
        .parse::<usize>()
        .ok()
        .filter(|index| (1..=count).contains(index))
        .map(|index| index - 1)
        .ok_or_else(|| "Entity number is outside this drawing; use a whole number from 1.".into())
}

pub(super) fn parse_point(value: &str, elevation: f64) -> Result<Point3, String> {
    let coordinates = value
        .split(',')
        .map(number)
        .collect::<Result<Vec<_>, _>>()?;
    if !(2..=3).contains(&coordinates.len()) {
        return Err("Use x,y or x,y,z for a point.".into());
    }
    Point3::new(
        coordinates[0],
        coordinates[1],
        coordinates.get(2).copied().unwrap_or(elevation),
    )
    .map_err(|e| e.to_string())
}
