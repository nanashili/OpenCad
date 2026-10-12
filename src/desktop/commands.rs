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
                .pline_text(input)
                .or_else(|| self.corner_text(input))
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

    pub(super) fn command_result(&mut self, input: &str) -> Result<(), String> {
        if let Some(result) = self.pline_text(input) {
            return result;
        }
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
        let token_storage = command_tokens(rest)?;
        let tokens: Vec<&str> = token_storage.iter().map(String::as_str).collect();
        if let Some(result) = self.feature_block_editor_command(verb, rest, &tokens) {
            return result;
        }
        if let Some(result) = self.feature_dynamic_blocks_command(verb, rest, &tokens) {
            return result;
        }
        if rest.is_empty()
            && let Some(form) = feature_forms::FeatureForm::for_command(verb)
        {
            self.feature_form = Some(form);
            return Ok(());
        }
        let upper = verb.to_ascii_uppercase();
        if matches!(upper.as_str(), "VIEW" | "UCS")
            && let Some(result) = self.feature_view_command(&upper, &tokens)
        {
            return result;
        }
        if matches!(
            upper.as_str(),
            "VIEW" | "VPOINT" | "UCS" | "PLAN" | "3DORBIT"
        ) {
            return self.view_command(&upper, rest, &tokens);
        }
        if let Some(result) = self
            .feature_space_command(&upper, &tokens)
            .or_else(|| self.feature_publish_command(&upper, &tokens))
            .or_else(|| self.feature_plot_settings_command(&upper, &tokens))
            .or_else(|| self.feature_layout_command(&upper, rest, &tokens))
            .or_else(|| self.feature_construction_command(&upper, &tokens))
            .or_else(|| self.feature_constraints_command(&upper, &tokens))
            .or_else(|| self.feature_reference_command(&upper, &tokens))
            .or_else(|| self.feature_region_command(&upper, &tokens))
            .or_else(|| self.feature_selection_command(&upper, &tokens))
            .or_else(|| self.feature_groups_command(&upper, &tokens))
            .or_else(|| self.feature_interchange_command(&upper, rest, &tokens))
            .or_else(|| self.feature_pdf_command(&upper, rest, &tokens))
            .or_else(|| {
                self.feature_data_command(&upper, rest).map(|result| {
                    result.map(|output| {
                        if !output.is_empty() {
                            self.message(output, false);
                        }
                    })
                })
            })
            .or_else(|| self.feature_utility_command(&upper, rest, &tokens))
            .or_else(|| self.feature_modify_command(&upper, rest, &tokens))
            .or_else(|| self.feature_solid_command(&upper, rest, &tokens))
            .or_else(|| self.feature_tables_command(&upper, rest, &tokens))
            .or_else(|| self.feature_annotations_command(&upper, rest, &tokens))
            .or_else(|| self.feature_blocks_command(&upper, rest, &tokens))
        {
            return result;
        }
        match verb.to_ascii_uppercase().as_str() {
            "MLINE" => {
                let (style_name, point_tokens) = tokens
                    .first()
                    .filter(|token| !token.contains(','))
                    .map_or((None, tokens.as_slice()), |name| {
                        (Some(*name), &tokens[1..])
                    });
                let style = self
                    .editor
                    .document()
                    .mline_styles()
                    .iter()
                    .find(|candidate| {
                        style_name.map_or_else(
                            || {
                                candidate
                                    .name()
                                    .eq_ignore_ascii_case(&self.current_mline_style)
                            },
                            |name| candidate.name().eq_ignore_ascii_case(name),
                        )
                    })
                    .cloned()
                    .or_else(|| crate::MLineStyle::new("STANDARD", vec![-0.5, 0.5]).ok())
                    .ok_or("No MLINESTYLE is available.")?;
                if self.editor.document().mline_styles().is_empty() {
                    let mut document = self.editor.document().clone();
                    document.set_mline_styles(vec![style.clone()]);
                    self.editor
                        .commit_document(document)
                        .map_err(|error| error.to_string())?;
                }
                let mline = feature_multiline::parse_mline(point_tokens, style)?;
                self.add_entities(vec![Entity::MLine(mline)])?;
                self.finish();
                self.message("Multiline created.", false);
            }
            "MLSTYLE" => {
                let action = tokens
                    .first()
                    .copied()
                    .unwrap_or("LIST")
                    .to_ascii_uppercase();
                match action.as_str() {
                    "LIST" => {
                        let names = self
                            .editor
                            .document()
                            .mline_styles()
                            .iter()
                            .map(|s| s.name())
                            .collect::<Vec<_>>()
                            .join(", ");
                        self.message(
                            if names.is_empty() {
                                "No MLINESTYLE definitions.".into()
                            } else {
                                names
                            },
                            false,
                        );
                    }
                    "CREATE" => {
                        let name = tokens
                            .get(1)
                            .ok_or("MLSTYLE CREATE name offset[,offset...].")?;
                        let offsets = tokens
                            .get(2)
                            .ok_or("MLSTYLE CREATE name offset[,offset...].")?
                            .split(',')
                            .map(|value| {
                                value.parse::<f64>().map_err(|_| "Offset must be numeric.")
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let style = feature_multiline::create_mlstyle(name, &offsets)?;
                        let mut document = self.editor.document().clone();
                        let mut styles = document.mline_styles().to_vec();
                        styles.retain(|existing| !existing.name().eq_ignore_ascii_case(name));
                        styles.push(style);
                        document.set_mline_styles(styles);
                        self.editor
                            .commit_document(document)
                            .map_err(|e| e.to_string())?;
                        self.message(format!("MLINESTYLE {name} created."), false);
                    }
                    "SET" => {
                        let name = tokens.get(1).ok_or("MLSTYLE SET name.")?;
                        if !self
                            .editor
                            .document()
                            .mline_styles()
                            .iter()
                            .any(|style| style.name().eq_ignore_ascii_case(name))
                        {
                            return Err(format!("Unknown MLINESTYLE {name}."));
                        }
                        self.current_mline_style = (*name).to_owned();
                        self.message(format!("MLINESTYLE {name} is current."), false);
                    }
                    _ => return Err("MLSTYLE supports LIST, CREATE, and SET.".into()),
                }
            }
            "MLEDIT" => {
                let index = *self
                    .editor
                    .selection()
                    .first()
                    .ok_or("Select an MLINE before using MLEDIT.")?;
                let Some(Entity::MLine(mut mline)) = self.editor.entities().get(index).cloned()
                else {
                    return Err("Selected entity is not an MLINE.".into());
                };
                let operation = tokens.first().copied().unwrap_or("LIST");
                match operation.to_ascii_uppercase().as_str() {
                    "SCALE" => {
                        let value = tokens
                            .get(1)
                            .ok_or("MLEDIT SCALE value.")?
                            .parse::<f64>()
                            .map_err(|_| "Scale must be numeric.")?;
                        feature_multiline::edit_mline(&mut mline, Some(value), None, None, None)?;
                    }
                    "CLOSE" | "OPEN" => feature_multiline::edit_mline(
                        &mut mline,
                        None,
                        None,
                        Some(operation.eq_ignore_ascii_case("CLOSE")),
                        None,
                    )?,
                    "TOP" | "ZERO" | "BOTTOM" => {
                        let justification = match operation.to_ascii_uppercase().as_str() {
                            "TOP" => crate::MLineJustification::Top,
                            "BOTTOM" => crate::MLineJustification::Bottom,
                            _ => crate::MLineJustification::Zero,
                        };
                        feature_multiline::edit_mline(
                            &mut mline,
                            None,
                            Some(justification),
                            None,
                            None,
                        )?;
                    }
                    "STYLE" => {
                        let name = tokens.get(1).ok_or("MLEDIT STYLE name.")?;
                        let style = self
                            .editor
                            .document()
                            .mline_styles()
                            .iter()
                            .find(|style| style.name().eq_ignore_ascii_case(name))
                            .cloned()
                            .ok_or_else(|| format!("Unknown MLINESTYLE {name}."))?;
                        feature_multiline::edit_mline(&mut mline, None, None, None, Some(style))?;
                    }
                    "LIST" => {
                        self.message(
                            format!(
                                "MLINE scale {} with {} elements.",
                                mline.scale(),
                                mline.style().elements().len()
                            ),
                            false,
                        );
                        return Ok(());
                    }
                    _ => {
                        return Err(
                            "MLEDIT supports SCALE, STYLE, TOP, ZERO, BOTTOM, CLOSE, OPEN, and LIST."
                                .into(),
                        );
                    }
                }
                self.editor
                    .update(index, Entity::MLine(mline))
                    .map_err(|e| e.to_string())?;
                self.message("MLINE updated.", false);
            }
            "POINT" => {
                let point = parse_point(rest, self.drawing_elevation())?;
                self.add_entities(vec![Entity::Point(crate::PointEntity::new(point))])?;
                self.finish();
                self.message("Point created.", false);
            }
            "ELLIPSE" => {
                let args: Vec<_> = rest.split_whitespace().collect();
                if args.len() < 3 {
                    return Err(
                        "ELLIPSE syntax: ELLIPSE center major-end ratio [start end].".into(),
                    );
                }
                let center = parse_point(args[0], self.drawing_elevation())?;
                let major = parse_point(args[1], center.z())?;
                let ratio = number(args[2])?;
                let start = args.get(3).map_or(Ok(0.0), |v| number(v))?.to_radians();
                let end = args.get(4).map_or(Ok(0.0), |v| number(v))?.to_radians();
                self.add_entities(vec![Entity::Ellipse(
                    crate::Ellipse3::new(
                        center,
                        Point3::new(
                            major.x() - center.x(),
                            major.y() - center.y(),
                            major.z() - center.z(),
                        )
                        .map_err(|e| e.to_string())?,
                        ratio,
                        start,
                        end,
                    )
                    .map_err(|e| e.to_string())?,
                )])?;
                self.finish();
                self.message("Ellipse created.", false);
            }
            "SPLINE" => {
                let args: Vec<_> = rest.split_whitespace().collect();
                if args.len() < 3 {
                    return Err("SPLINE syntax: SPLINE degree point;point;point...".into());
                }
                let (degree_token, point_tokens): (&str, &[&str]) = if args[0].contains(',') {
                    (
                        args.last().copied().ok_or("SPLINE requires a degree.")?,
                        &args[..args.len() - 1],
                    )
                } else {
                    (args[0], &args[1..])
                };
                let degree = degree_token
                    .parse::<usize>()
                    .map_err(|_| "Spline degree must be a whole number.".to_owned())?;
                let points = point_tokens
                    .iter()
                    .map(|v| parse_point(v.trim_matches(';'), self.drawing_elevation()))
                    .collect::<Result<Vec<_>, _>>()?;
                let count = points.len();
                let knots = (0..count + degree + 1).map(|i| i as f64).collect();
                self.add_entities(vec![Entity::Spline(
                    crate::Spline3::unweighted(degree, knots, points, false)
                        .map_err(|e| e.to_string())?,
                )])?;
                self.finish();
                self.message("Spline created.", false);
            }
            "TEXT" | "MTEXT" => {
                let mut args = rest.splitn(3, char::is_whitespace);
                let point = parse_point(
                    args.next().ok_or("TEXT syntax: TEXT x,y height content.")?,
                    self.drawing_elevation(),
                )?;
                let height = number(args.next().ok_or("TEXT requires a height.")?)?;
                let content = args.next().unwrap_or_default();
                let entity = if verb.eq_ignore_ascii_case("MTEXT") {
                    let mut text = crate::MText::new(content, point, height, height * 20.0)
                        .map_err(|e| e.to_string())?;
                    if let Some(style) = self
                        .editor
                        .document()
                        .text_style(self.editor.document().current_text_style())
                    {
                        text.text_mut()
                            .apply_style(style)
                            .map_err(|e| e.to_string())?;
                    }
                    Entity::MText(text)
                } else {
                    let mut text =
                        crate::Text::new(content, point, height).map_err(|e| e.to_string())?;
                    if let Some(style) = self
                        .editor
                        .document()
                        .text_style(self.editor.document().current_text_style())
                    {
                        text.apply_style(style).map_err(|e| e.to_string())?;
                    }
                    Entity::Text(text)
                };
                self.add_entities(vec![entity])?;
                self.finish();
                self.message("Text created.", false);
            }
            "LEADER" => {
                let points = rest
                    .split(';')
                    .map(|v| parse_point(v.trim(), self.drawing_elevation()))
                    .collect::<Result<Vec<_>, _>>()?;
                self.add_entities(vec![Entity::Leader(
                    crate::Leader::new(points).map_err(|e| e.to_string())?,
                )])?;
                self.finish();
                self.message("Leader created.", false);
            }
            "MLEADER" => {
                let (point_text, content_text) = rest
                    .split_once('|')
                    .map_or((rest, "Multileader"), |parts| parts);
                let points = point_text
                    .split(';')
                    .map(|v| parse_point(v.trim(), self.drawing_elevation()))
                    .collect::<Result<Vec<_>, _>>()?;
                let branch = crate::MLeaderBranch::new(points).map_err(|e| e.to_string())?;
                let anchor = *branch
                    .vertices()
                    .last()
                    .ok_or("MLEADER requires vertices.")?;
                let text = crate::MText::new(content_text.trim(), anchor, 2.5, 50.0)
                    .map_err(|e| e.to_string())?;
                let mut leader = crate::MLeader::new(branch, crate::MLeaderContent::MText(text))
                    .map_err(|e| e.to_string())?;
                if let Some(style) = self
                    .editor
                    .document()
                    .mleader_style(self.editor.document().current_mleader_style())
                {
                    leader.apply_style(style).map_err(|e| e.to_string())?;
                }
                self.add_entities(vec![Entity::MLeader(leader)])?;
                self.finish();
                self.message("Multileader created.", false);
            }
            "DIMLINEAR" | "DIMALIGNED" => {
                let points = rest
                    .split(';')
                    .map(|v| parse_point(v.trim(), self.drawing_elevation()))
                    .collect::<Result<Vec<_>, _>>()?;
                if points.len() != 3 {
                    return Err("DIMLINEAR syntax: DIMLINEAR first;second;offset.".into());
                }
                let kind = if verb.eq_ignore_ascii_case("DIMALIGNED") {
                    crate::DimensionKind::Aligned {
                        first: points[0],
                        second: points[1],
                        offset: points[2],
                    }
                } else {
                    crate::DimensionKind::Linear {
                        first: points[0],
                        second: points[1],
                        offset: points[2],
                    }
                };
                let mut dimension = crate::Dimension::new(kind).map_err(|e| e.to_string())?;
                if let Some(style) = self
                    .editor
                    .document()
                    .dimension_style(self.editor.document().current_dimension_style())
                {
                    dimension.apply_style(style).map_err(|e| e.to_string())?;
                }
                self.add_entities(vec![Entity::Dimension(dimension)])?;
                self.finish();
                self.message("Dimension created.", false);
            }
            "DIMRADIUS" | "DIMDIAMETRIC" | "DIMANGULAR" => {
                let points = rest
                    .split(';')
                    .map(|v| parse_point(v.trim(), self.drawing_elevation()))
                    .collect::<Result<Vec<_>, _>>()?;
                let kind = match verb.to_ascii_uppercase().as_str() {
                    "DIMRADIUS" => {
                        if points.len() != 2 {
                            return Err("DIMRADIUS syntax: DIMRADIUS center;definition.".into());
                        }
                        crate::DimensionKind::Radial {
                            center: points[0],
                            definition: points[1],
                        }
                    }
                    "DIMDIAMETRIC" => {
                        if points.len() != 2 {
                            return Err("DIMDIAMETRIC syntax: DIMDIAMETRIC first;second.".into());
                        }
                        crate::DimensionKind::Diametric {
                            first: points[0],
                            second: points[1],
                        }
                    }
                    _ => {
                        if points.len() != 3 {
                            return Err("DIMANGULAR syntax: DIMANGULAR center;first;second.".into());
                        }
                        crate::DimensionKind::Angular {
                            center: points[0],
                            first: points[1],
                            second: points[2],
                        }
                    }
                };
                let mut dimension = crate::Dimension::new(kind).map_err(|e| e.to_string())?;
                if let Some(style) = self
                    .editor
                    .document()
                    .dimension_style(self.editor.document().current_dimension_style())
                {
                    dimension.apply_style(style).map_err(|e| e.to_string())?;
                }
                self.add_entities(vec![Entity::Dimension(dimension)])?;
                self.finish();
                self.message("Dimension created.", false);
            }
            "TABLE" => {
                let args: Vec<_> = rest.split_whitespace().collect();
                if args.len() < 5 {
                    return Err(
                        "TABLE syntax: TABLE anchor rows columns row-height column-width.".into(),
                    );
                }
                let anchor = parse_point(args[0], self.drawing_elevation())?;
                let rows = args[1]
                    .parse::<usize>()
                    .map_err(|_| "Rows must be a whole number.".to_owned())?;
                let columns = args[2]
                    .parse::<usize>()
                    .map_err(|_| "Columns must be a whole number.".to_owned())?;
                if rows == 0
                    || columns == 0
                    || rows.checked_mul(columns).is_none_or(|count| count > 10_000)
                {
                    return Err(
                        "TABLE rows and columns must define between 1 and 10,000 cells.".into(),
                    );
                }
                let row_height = number(args[3])?;
                let column_width = number(args[4])?;
                let values: Vec<_> = args[5..]
                    .join(" ")
                    .split('|')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
                    .collect();
                if values.len() > rows * columns {
                    return Err(
                        "TABLE has more cell values than rows multiplied by columns.".into(),
                    );
                }
                let mut cells = vec![vec![String::new(); columns]; rows];
                for (index, value) in values.into_iter().enumerate() {
                    cells[index / columns][index % columns] = value;
                }
                self.add_entities(vec![Entity::Table(
                    crate::Table::new(
                        anchor,
                        vec![row_height; rows],
                        vec![column_width; columns],
                        cells,
                    )
                    .map_err(|e| e.to_string())?,
                )])?;
                self.finish();
                self.message("Table created.", false);
            }
            "HATCH" => {
                let points = rest
                    .split(';')
                    .map(|v| parse_point(v.trim(), self.drawing_elevation()))
                    .collect::<Result<Vec<_>, _>>()?;
                self.add_entities(vec![Entity::Hatch(
                    crate::Hatch::new(vec![points], crate::HatchPattern::Solid)
                        .map_err(|e| e.to_string())?,
                )])?;
                self.finish();
                self.message("Hatch created.", false);
            }
            "PLINEWID" => {
                if !rest.is_empty() {
                    self.editor
                        .set_pline_width(number(rest)?)
                        .map_err(|error| error.to_string())?;
                    if self.tool != Tool::Polyline {
                        self.finish();
                    }
                }
                self.message(format!("PLINEWID = {}", self.editor.pline_width()), false);
            }
            "PLINEREVERSEWIDTHS" => {
                if !rest.is_empty() {
                    self.pline_reverse_widths = match rest {
                        "0" => false,
                        "1" => true,
                        _ => return Err("PLINEREVERSEWIDTHS must be 0 or 1.".into()),
                    };
                    self.finish();
                }
                self.message(
                    format!(
                        "PLINEREVERSEWIDTHS = {}",
                        u8::from(self.pline_reverse_widths)
                    ),
                    false,
                );
            }
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
            "CHAMMODE" => {
                if !rest.is_empty() {
                    self.chamfer_method = match rest {
                        "0" => corners::ChamferMethod::Distance,
                        "1" => corners::ChamferMethod::Angle,
                        _ => return Err("CHAMMODE must be 0 or 1.".into()),
                    };
                    self.finish();
                }
                self.message(
                    format!(
                        "CHAMMODE = {}",
                        u8::from(self.chamfer_method == corners::ChamferMethod::Angle)
                    ),
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
                    let mut polyline =
                        Polyline2::new(vertices, closed).map_err(|error| error.to_string())?;
                    polyline
                        .set_uniform_width(self.editor.pline_width())
                        .map_err(|error| error.to_string())?;
                    polyline.extrema().map_err(|error| error.to_string())?;
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
            "CLOSE" if self.block_edit_session.is_some() => {
                return Err("Use BCLOSE SAVE or BCLOSE DISCARD while editing a block.".into());
            }
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

            "NEW" | "OPEN" if self.block_edit_session.is_some() => {
                return Err(
                    "Finish the active block editor session before changing drawings.".into(),
                );
            }
            "NEW" => self.request(PendingAction::New),
            "OPEN" => self.request(PendingAction::Open(path_argument(rest))),
            "SAVE" | "SAVEAS" => {
                if self.block_edit_session.is_some() {
                    return Err("Use BSAVE or BSAVEAS while editing a block.".into());
                }
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

    fn view_command(&mut self, verb: &str, rest: &str, tokens: &[&str]) -> Result<(), String> {
        let orientation = |name: &str| match name.to_ascii_uppercase().as_str() {
            "TOP" | "T" => Some(crate::ViewOrientation::Top),
            "BOTTOM" | "B" => Some(crate::ViewOrientation::Bottom),
            "FRONT" | "F" => Some(crate::ViewOrientation::Front),
            "BACK" => Some(crate::ViewOrientation::Back),
            "LEFT" | "L" => Some(crate::ViewOrientation::Left),
            "RIGHT" | "R" => Some(crate::ViewOrientation::Right),
            "SW" | "SOUTHWEST" => Some(crate::ViewOrientation::SouthWest),
            "SE" | "SOUTHEAST" => Some(crate::ViewOrientation::SouthEast),
            "NE" | "NORTHEAST" => Some(crate::ViewOrientation::NorthEast),
            "NW" | "NORTHWEST" => Some(crate::ViewOrientation::NorthWest),
            _ => None,
        };
        match verb {
            "UCS" => {
                if tokens.is_empty() || tokens[0].eq_ignore_ascii_case("WORLD") {
                    self.ucs = crate::Ucs3::world();
                } else if tokens.len() >= 3 {
                    let origin = parse_point(tokens[0], self.drawing_elevation())?;
                    let x = parse_point(tokens[1], self.drawing_elevation())?;
                    let y = parse_point(tokens[2], self.drawing_elevation())?;
                    self.ucs = crate::Ucs3::from_points(origin, x, y).map_err(|e| e.to_string())?;
                } else {
                    return Err(
                        "UCS syntax: UCS WORLD or UCS origin x-axis-point y-axis-point.".into(),
                    );
                }
                self.camera.set_ucs(self.ucs);
                self.message("UCS updated.", false);
            }
            "3DORBIT" => {
                let yaw = tokens
                    .first()
                    .map(|v| number(v))
                    .transpose()?
                    .unwrap_or(15.0)
                    .to_radians();
                let pitch = tokens
                    .get(1)
                    .map(|v| number(v))
                    .transpose()?
                    .unwrap_or(0.0)
                    .to_radians();
                self.view_camera
                    .orbit(yaw, pitch)
                    .map_err(|e| e.to_string())?;
                self.camera.set_view(self.view_camera);
                self.message("3D orbit view updated.", false);
            }
            "PLAN" => {
                self.view_camera
                    .set_orientation(crate::ViewOrientation::Top);
                self.camera.set_view(self.view_camera);
                self.message("Plan view restored.", false);
            }
            "VIEW" | "VPOINT" => {
                let name = tokens.first().copied().unwrap_or("TOP");
                let view = orientation(name)
                    .ok_or("VIEW orientation: TOP, FRONT, BACK, LEFT, RIGHT, SW, SE, NE, NW.")?;
                self.view_camera.set_orientation(view);
                self.camera.set_view(self.view_camera);
                self.message(format!("View set to {name}."), false);
            }
            _ => return Err(format!("Unsupported view command: {verb} {rest}")),
        }
        Ok(())
    }

    pub(super) fn undo_line_point(&mut self) {
        if self.tool == Tool::Polyline {
            self.pline_undo_point();
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

impl OpenCadApp {
    fn feature_region_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        if matches!(verb, "UNION" | "SUBTRACT" | "INTERSECT")
            && self.editor.selection().len() == 2
            && self
                .editor
                .selection()
                .iter()
                .all(|index| matches!(self.editor.entities().get(*index), Some(Entity::Mesh(_))))
        {
            let indices = self.editor.selection().to_vec();
            let (Some(Entity::Mesh(first)), Some(Entity::Mesh(second))) = (
                self.editor.entities().get(indices[0]),
                self.editor.entities().get(indices[1]),
            ) else {
                return Some(Err("Selected mesh operands are unavailable.".into()));
            };
            let first = first.clone();
            let second = second.clone();
            let operation = match verb {
                "UNION" => crate::MeshBooleanOp::Union,
                "SUBTRACT" => crate::MeshBooleanOp::Subtract,
                _ => crate::MeshBooleanOp::Intersect,
            };
            return Some(
                crate::mesh_boolean(&first, &second, operation)
                    .map_err(|error| error.to_string())
                    .and_then(|mesh| {
                        self.editor
                            .replace_selection_with_entities(vec![Entity::Mesh(mesh)])
                            .map_err(|error| error.to_string())?;
                        self.finish();
                        self.message(format!("{verb} completed on mesh operands."), false);
                        Ok(())
                    }),
            );
        }
        let operation = feature_regions::parse_region_command(verb, tokens)?;
        Some(operation.and_then(|operation| {
            let results = feature_regions::region_operation(
                self.editor.entities(),
                self.editor.selection(),
                operation,
            )?;
            self.editor
                .replace_selection_with_entities(results)
                .map_err(|e| e.to_string())?;
            self.finish();
            self.message(format!("{verb} completed."), false);
            Ok(())
        }))
    }

    fn feature_construction_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let parsed = feature_construction::parse_construction_command(
            verb,
            tokens,
            self.drawing_elevation(),
        )?;
        Some(parsed.and_then(|command| {
            let entity = match command {
                feature_construction::ConstructionCommand::XLine(line) => Entity::XLine(line),
                feature_construction::ConstructionCommand::Ray(ray) => Entity::Ray(ray),
            };
            self.add_entities(vec![entity])?;
            self.finish();
            self.message(format!("{verb} created."), false);
            Ok(())
        }))
    }

    fn feature_reference_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result =
            feature_references::feature_references_command(&mut self.references, verb, tokens)?;
        Some(result.and_then(|message| {
            if verb.eq_ignore_ascii_case("XATTACH")
                || (verb.eq_ignore_ascii_case("XREF")
                    && tokens
                        .first()
                        .is_some_and(|t| t.eq_ignore_ascii_case("ATTACH")))
            {
                let name = if verb.eq_ignore_ascii_case("XREF") {
                    tokens.get(2).copied()
                } else {
                    tokens.get(1).copied()
                }
                .or_else(|| {
                    self.references
                        .references
                        .references()
                        .last()
                        .map(|r| r.name())
                })
                .ok_or("XATTACH did not produce a reference name.")?
                .to_owned();
                let insertion =
                    Point3::new(0.0, 0.0, self.drawing_elevation()).map_err(|e| e.to_string())?;
                let mut candidate = self.editor.document().clone();
                self.references
                    .attach_dxf(&mut candidate, &name, insertion)?;
                self.editor
                    .commit_document(candidate)
                    .map_err(|error| error.to_string())?;
            } else if verb.eq_ignore_ascii_case("XREF")
                && tokens
                    .first()
                    .is_some_and(|t| t.eq_ignore_ascii_case("RELOAD"))
            {
                let name = tokens
                    .get(1)
                    .ok_or("XREF RELOAD requires a reference name.")?;
                let mut candidate = self.editor.document().clone();
                self.references.reload_dxf(&mut candidate, name)?;
                self.editor
                    .commit_document(candidate)
                    .map_err(|error| error.to_string())?;
            } else if verb.eq_ignore_ascii_case("XREF")
                && tokens
                    .first()
                    .is_some_and(|t| t.eq_ignore_ascii_case("UNLOAD"))
            {
                let name = tokens
                    .get(1)
                    .ok_or("XREF UNLOAD requires a reference name.")?;
                let mut candidate = self.editor.document().clone();
                self.references.unload_dxf(&mut candidate, name)?;
                self.editor
                    .commit_document(candidate)
                    .map_err(|error| error.to_string())?;
            } else if verb.eq_ignore_ascii_case("XREF")
                && tokens
                    .first()
                    .is_some_and(|t| t.eq_ignore_ascii_case("DETACH"))
            {
                let name = tokens
                    .get(1)
                    .ok_or("XREF DETACH requires a reference name.")?;
                let mut candidate = self.editor.document().clone();
                self.references.detach_dxf(&mut candidate, name)?;
                self.editor
                    .commit_document(candidate)
                    .map_err(|error| error.to_string())?;
            } else if verb.eq_ignore_ascii_case("XREF")
                && tokens
                    .first()
                    .is_some_and(|t| t.eq_ignore_ascii_case("BIND"))
            {
                let name = tokens
                    .get(1)
                    .ok_or("XREF BIND requires a reference name.")?;
                let mut candidate = self.editor.document().clone();
                self.references.bind_dxf(&mut candidate, name)?;
                self.editor
                    .commit_document(candidate)
                    .map_err(|error| error.to_string())?;
            } else if verb.eq_ignore_ascii_case("IMAGEATTACH") {
                if let Some(image) = self.references.images.last().cloned() {
                    self.add_entities(vec![Entity::Image(image)])?;
                }
            } else if verb.eq_ignore_ascii_case("IMAGECLIP") {
                let image_index = tokens
                    .first()
                    .and_then(|value| value.parse::<usize>().ok())
                    .and_then(|index| index.checked_sub(1));
                if let Some(image_index) = image_index {
                    let entity_index = self
                        .editor
                        .entities()
                        .iter()
                        .enumerate()
                        .filter_map(|(index, entity)| {
                            matches!(entity, Entity::Image(_)).then_some(index)
                        })
                        .nth(image_index);
                    if let (Some(entity_index), Some(image)) = (
                        entity_index,
                        self.references.images.get(image_index).cloned(),
                    ) {
                        self.editor
                            .update(entity_index, Entity::Image(image))
                            .map_err(|error| error.to_string())?;
                    }
                }
            }
            self.message(message, false);
            Ok(())
        }))
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

pub(super) fn command_tokens(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut token_started = false;
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(delimiter) = quote {
            if character == delimiter {
                if characters.peek().copied() == Some(delimiter) {
                    characters.next();
                    current.push(delimiter);
                } else {
                    quote = None;
                }
            } else {
                current.push(character);
            }
            continue;
        }
        match character {
            '"' | '\'' if !token_started => {
                quote = Some(character);
                token_started = true;
            }
            c if c.is_whitespace() => {
                if token_started {
                    tokens.push(std::mem::take(&mut current));
                    token_started = false;
                }
            }
            c => {
                current.push(c);
                token_started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("Unmatched quote in command arguments.".into());
    }
    if token_started {
        tokens.push(current);
    }
    Ok(tokens)
}
