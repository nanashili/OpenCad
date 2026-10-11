use super::commands::{entity_index, parse_point};
use super::*;
use crate::GeometryError;

#[derive(Default)]
pub(super) struct PeditInput {
    source: Option<usize>,
    conversion: bool,
    joining: bool,
    vertex: Option<usize>,
    point_edit: Option<VertexPointEdit>,
    range: Option<VertexRangeEdit>,
    width: Option<WidthInput>,
    undo: Vec<(usize, u64)>,
}

#[derive(Clone, Copy)]
enum WidthInput {
    Uniform,
    VertexStart,
    VertexEnd(f64),
}

#[derive(Clone, Copy)]
enum VertexPointEdit {
    Move,
    Insert,
}

#[derive(Clone, Copy)]
enum VertexRangeEdit {
    Straighten(usize),
    Break(usize),
}

impl VertexRangeEdit {
    fn label(self) -> &'static str {
        match self {
            Self::Straighten(_) => "Straighten",
            Self::Break(_) => "Break",
        }
    }

    fn start(self) -> usize {
        match self {
            Self::Straighten(start) | Self::Break(start) => start,
        }
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Close,
    Open,
    Reverse,
    Undo,
    Join,
    Edit,
    Width,
}

impl Operation {
    fn parse(input: &str) -> Option<Self> {
        Some(match input.to_ascii_uppercase().as_str() {
            "CLOSE" | "C" => Self::Close,
            "OPEN" | "O" => Self::Open,
            "REVERSE" | "R" => Self::Reverse,
            "UNDO" | "U" => Self::Undo,
            "JOIN" | "J" => Self::Join,
            "EDIT" | "E" | "EDIT VERTEX" => Self::Edit,
            "WIDTH" | "W" => Self::Width,
            _ => return None,
        })
    }
}

impl OpenCadApp {
    pub(super) fn pedit_prompt(&self) -> &'static str {
        if let Some(width) = self.pedit_input.width {
            return match width {
                WidthInput::Uniform => {
                    "PEDIT Width · Enter a finite nonnegative uniform width. Enter or Exit cancels; Escape ends PEDIT."
                }
                WidthInput::VertexStart => {
                    "PEDIT Vertex Width · Enter the outgoing segment's starting width. Enter or Exit cancels; Escape ends PEDIT."
                }
                WidthInput::VertexEnd(_) => {
                    "PEDIT Vertex Width · Enter the outgoing segment's ending width. Enter or Exit cancels both values; Escape ends PEDIT."
                }
            };
        }
        if matches!(self.pedit_input.point_edit, Some(VertexPointEdit::Move)) {
            "PEDIT Move vertex · Specify x,y[,z] or a canvas point. Relative @dx,dy[,dz] uses the marked vertex. Enter or Exit cancels this move; Escape ends PEDIT."
        } else if matches!(self.pedit_input.point_edit, Some(VertexPointEdit::Insert)) {
            "PEDIT Insert vertex · Specify x,y[,z] or a canvas point after the marked vertex. Relative @dx,dy[,dz] uses the marked vertex. Enter or Exit cancels insertion; Escape ends PEDIT."
        } else if matches!(self.pedit_input.range, Some(VertexRangeEdit::Straighten(_))) {
            "PEDIT Straighten · Next / Previous chooses the other vertex. Go applies; Enter or Exit cancels the range. Escape ends PEDIT."
        } else if matches!(self.pedit_input.range, Some(VertexRangeEdit::Break(_))) {
            "PEDIT Break · Next / Previous chooses the other vertex. Go removes the range; Enter or Exit cancels. Escape ends PEDIT."
        } else if self.pedit_input.vertex.is_some() {
            "PEDIT Edit vertex · Next / Previous / Move / Insert / Straighten / Break / Width / Regen; Enter or Exit returns to PEDIT. Escape ends the session."
        } else if self.pedit_input.joining {
            "PEDIT Join · Select touching XY lines, arcs or open polylines; Enter joins. Source stays selected. Shift removes; All selects visible objects. Escape ends PEDIT."
        } else if self.pedit_input.conversion {
            "PEDIT · Object is not a polyline. Convert it? Yes/No <Yes>. Escape cancels."
        } else if let Some(source) = self.pedit_input.source {
            if matches!(self.editor.entities().get(source), Some(Entity::Polyline(polyline)) if polyline.is_closed())
            {
                "PEDIT · Open / Reverse / Width / Undo / Edit vertex; Enter or Exit finishes. Escape ends the session."
            } else {
                "PEDIT · Close / Reverse / Width / Undo / Join / Edit vertex; Enter or Exit finishes. Escape ends the session."
            }
        } else {
            "PEDIT · Select one XY polyline, line or arc; or enter its object number. Escape cancels."
        }
    }

    fn pedit_start_width(&mut self, tokens: &[&str]) -> Result<(), String> {
        if self.pedit_input.conversion {
            return Err("Confirm conversion to a polyline first.".into());
        }
        if self.pedit_input.joining {
            return Err("Press Enter to finish selecting Join objects first.".into());
        }
        if self.pedit_input.point_edit.is_some() {
            return Err("Exit point entry before choosing Width.".into());
        }
        if let Some(range) = self.pedit_input.range {
            return Err(format!("Use Go or Exit to finish {} first.", range.label()));
        }
        let source = self
            .pedit_input
            .source
            .ok_or("Select a PEDIT object first.")?;
        if !matches!(
            self.editor.entities().get(source),
            Some(Entity::Polyline(_))
        ) {
            return Err("Select a polyline first.".into());
        }
        let vertex = self.pedit_input.vertex.is_some();
        if tokens.len() > if vertex { 2 } else { 1 } {
            return Err("Use Width [value], or Width [start [end]] in Edit vertex.".into());
        }
        let values = tokens
            .iter()
            .map(|token| {
                let value = token
                    .parse::<f64>()
                    .map_err(|_| "Enter a finite nonnegative width.".to_owned())?;
                crate::PolylineWidth::new(value, value).map_err(|error| error.to_string())?;
                Ok(value)
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.pedit_input.width = Some(if vertex {
            WidthInput::VertexStart
        } else {
            WidthInput::Uniform
        });
        for value in values {
            self.pedit_width_value(value)?;
        }
        self.message(self.prompt(), false);
        Ok(())
    }

    fn pedit_width_value(&mut self, value: f64) -> Result<(), String> {
        crate::PolylineWidth::new(value, value).map_err(|error| error.to_string())?;
        let input = self.pedit_input.width.ok_or("Choose Width first.")?;
        if matches!(input, WidthInput::VertexStart) {
            self.pedit_input.width = Some(WidthInput::VertexEnd(value));
            self.message(self.prompt(), false);
            return Ok(());
        }
        let source = self
            .pedit_input
            .source
            .ok_or("Select a PEDIT object first.")?;
        let Some(Entity::Polyline(original)) = self.editor.entities().get(source) else {
            return Err("Select a polyline first.".into());
        };
        let mut replacement = original.clone();
        match input {
            WidthInput::Uniform => replacement.set_uniform_width(value),
            WidthInput::VertexEnd(start) => replacement.set_width(
                self.pedit_input.vertex.ok_or("Choose Edit vertex first.")?,
                start,
                value,
            ),
            WidthInput::VertexStart => unreachable!("handled above"),
        }
        .map_err(|error| error.to_string())?;
        replacement.extrema().map_err(|error| error.to_string())?;
        if self
            .editor
            .update(source, Entity::Polyline(replacement))
            .map_err(|error| error.to_string())?
        {
            self.pedit_checkpoint(source);
        }
        self.pedit_input.width = None;
        self.editor.select(vec![source]);
        self.message(self.prompt(), false);
        Ok(())
    }

    fn pedit_width_text(&mut self, input: &str) -> Option<Result<(), String>> {
        self.pedit_input.width?;
        if input.is_empty() || input.eq_ignore_ascii_case("EXIT") || input.eq_ignore_ascii_case("X")
        {
            self.pedit_input.width = None;
            self.message(self.prompt(), false);
            return Some(Ok(()));
        }
        let verb = input.split_whitespace().next().unwrap_or("");
        if matches!(verb.to_ascii_uppercase().as_str(), "SAVE" | "SAVEAS") {
            return None;
        }
        Some(
            input
                .parse::<f64>()
                .map_err(|_| {
                    "Enter a finite nonnegative width, or Exit to cancel width entry.".to_owned()
                })
                .and_then(|value| self.pedit_width_value(value)),
        )
    }

    pub(super) fn pedit_vertex_active(&self) -> bool {
        self.tool == Tool::Pedit && self.pedit_input.vertex.is_some()
    }

    pub(super) fn pedit_awaits_point(&self) -> bool {
        self.pedit_vertex_active() && self.pedit_input.point_edit.is_some()
    }

    pub(super) fn pedit_marked_vertex(&self) -> Option<(usize, usize, Point3)> {
        if self.tool != Tool::Pedit {
            return None;
        }
        let source = self.pedit_input.source?;
        let vertex = self.pedit_input.vertex?;
        let Entity::Polyline(polyline) = self.editor.entities().get(source)? else {
            return None;
        };
        Some((
            vertex,
            polyline.vertices().len(),
            *polyline.vertices().get(vertex)?,
        ))
    }

    pub(super) fn pedit_range_start(&self) -> Option<(usize, Point3, &'static str)> {
        self.pedit_marked_vertex()?;
        let source = self.pedit_input.source?;
        let range = self.pedit_input.range?;
        let start = range.start();
        let Entity::Polyline(polyline) = self.editor.entities().get(source)? else {
            return None;
        };
        Some((start, *polyline.vertices().get(start)?, range.label()))
    }

    pub(super) fn pedit_straightened_polyline(&self) -> Result<(Polyline2, usize), String> {
        let (vertex, _, _) = self.pedit_marked_vertex().ok_or("No marked vertex.")?;
        let Some(VertexRangeEdit::Straighten(start)) = self.pedit_input.range else {
            return Err("Choose Straighten in Edit vertex first.".into());
        };
        let source = self.pedit_input.source.expect("marked source exists");
        let Entity::Polyline(original) = &self.editor.entities()[source] else {
            unreachable!("marked source is a polyline");
        };
        let lower = start.min(vertex);
        let upper = start.max(vertex);
        let mut vertices = original.vertices().to_vec();
        let mut bulges = original.bulges().to_vec();
        let mut widths = original.widths().to_vec();
        if lower < upper {
            vertices.drain(lower + 1..upper);
            bulges.drain(lower + 1..upper);
            widths.drain(lower + 1..upper);
            widths[lower] = crate::PolylineWidth::new(
                original.widths()[lower].start(),
                original.widths()[upper - 1].end(),
            )
            .expect("existing widths");
        }
        if original.is_closed() || lower + 1 < vertices.len() {
            bulges[lower] = 0.0;
        }
        let mut replacement = Polyline2::with_bulges(vertices, bulges, original.is_closed())
            .map_err(|error| error.to_string())?;
        replacement
            .set_widths(widths)
            .map_err(|error| error.to_string())?;
        replacement.length().map_err(|error| error.to_string())?;
        replacement.extrema().map_err(|error| error.to_string())?;
        Ok((replacement, lower))
    }

    pub(super) fn pedit_range_preview(&self) -> Result<Vec<Polyline2>, String> {
        match self.pedit_input.range {
            Some(VertexRangeEdit::Straighten(_)) => self
                .pedit_straightened_polyline()
                .map(|(polyline, _)| vec![polyline]),
            Some(VertexRangeEdit::Break(start)) => {
                let (vertex, _, _) = self.pedit_marked_vertex().ok_or("No marked vertex.")?;
                let source = self.pedit_input.source.expect("marked source exists");
                let Entity::Polyline(polyline) = &self.editor.entities()[source] else {
                    unreachable!("marked source is a polyline");
                };
                polyline
                    .break_vertices(start, vertex)
                    .map_err(|error| error.to_string())
            }
            None => Err("Choose Straighten or Break in Edit vertex first.".into()),
        }
    }

    pub(super) fn pedit_range_preview_source(&self) -> Option<usize> {
        self.pedit_range_start()?;
        self.pedit_range_preview().ok()?;
        self.pedit_input.source
    }

    fn pedit_range_go(&mut self) -> Result<(), String> {
        let (current, _, _) = self.pedit_marked_vertex().ok_or("No marked vertex.")?;
        let source = self.pedit_input.source.expect("marked source exists");
        let (changed, vertex) = match self.pedit_input.range {
            Some(VertexRangeEdit::Straighten(_)) => {
                let (polyline, vertex) = self.pedit_straightened_polyline()?;
                let changed = self
                    .editor
                    .update(source, Entity::Polyline(polyline))
                    .map_err(|error| error.to_string())?;
                self.editor.select(vec![source]);
                (changed, vertex)
            }
            Some(VertexRangeEdit::Break(start)) => {
                let changed = self
                    .editor
                    .break_polyline_vertices(source, start, current)
                    .map_err(|error| error.to_string())?;
                (changed, 0)
            }
            None => return Err("Choose Straighten or Break in Edit vertex first.".into()),
        };
        if changed {
            self.pedit_checkpoint(source);
        }
        self.pedit_input.range = None;
        self.pedit_input.vertex = Some(vertex);
        self.pedit_vertex_message();
        Ok(())
    }

    fn pedit_vertex_message(&mut self) {
        if let Some((vertex, count, point)) = self.pedit_marked_vertex() {
            self.message(
                format!(
                    "Vertex {} of {count}: {},{},{}. {}",
                    vertex + 1,
                    point.x(),
                    point.y(),
                    point.z(),
                    self.prompt()
                ),
                false,
            );
        }
    }

    fn pedit_start_vertex_point(&mut self, edit: VertexPointEdit) -> Result<(), String> {
        if let Some(range) = self.pedit_input.range {
            return Err(format!("Use Go or Exit to finish {} first.", range.label()));
        }
        let (_, _, point) = self
            .pedit_marked_vertex()
            .ok_or("Choose Edit vertex first.")?;
        self.points = vec![point];
        self.pedit_input.point_edit = Some(edit);
        self.pedit_vertex_message();
        Ok(())
    }

    pub(super) fn pedit_vertex_preview(&self, point: Point3) -> Result<Polyline2, String> {
        let (vertex, _, _) = self
            .pedit_marked_vertex()
            .ok_or("Choose Edit vertex first.")?;
        let source = self.pedit_input.source.expect("marked source exists");
        let Entity::Polyline(original) = &self.editor.entities()[source] else {
            unreachable!("marked source is a polyline");
        };
        let mut vertices = original.vertices().to_vec();
        let replacement = match self.pedit_input.point_edit {
            Some(VertexPointEdit::Move) => {
                vertices[vertex] = point;
                let segments = vertices.len() - usize::from(!original.is_closed());
                for (index, bulge) in original.bulges().iter().take(segments).enumerate() {
                    let next = (index + 1) % vertices.len();
                    if *bulge != 0.0
                        && original.vertices()[index] != original.vertices()[next]
                        && vertices[index] == vertices[next]
                    {
                        return Err("Moving this vertex would collapse a curved segment.".into());
                    }
                }
                let mut replacement = original.clone();
                replacement
                    .set_vertices(vertices)
                    .map_err(|error| error.to_string())?;
                replacement
            }
            Some(VertexPointEdit::Insert) => {
                let append = vertex + 1 == vertices.len() && !original.is_closed();
                if point == vertices[vertex]
                    || (!append && point == vertices[(vertex + 1) % vertices.len()])
                {
                    return Err("Insert a point distinct from the adjacent vertices.".into());
                }
                let mut bulges = original.bulges().to_vec();
                let mut widths = original.widths().to_vec();
                vertices.insert(vertex + 1, point);
                bulges.insert(vertex + 1, 0.0);
                let outgoing = widths[vertex];
                let middle = outgoing.start() / 2.0 + outgoing.end() / 2.0;
                widths[vertex] =
                    crate::PolylineWidth::new(outgoing.start(), middle).expect("existing widths");
                widths.insert(
                    vertex + 1,
                    crate::PolylineWidth::new(middle, outgoing.end()).expect("existing widths"),
                );
                if append {
                    bulges[vertex + 1] = bulges[vertex];
                    bulges[vertex] = 0.0;
                    widths[vertex + 1] = outgoing;
                    widths[vertex] = crate::PolylineWidth::default();
                }
                let mut replacement =
                    Polyline2::with_bulges(vertices, bulges, original.is_closed())
                        .map_err(|error| error.to_string())?;
                replacement
                    .set_widths(widths)
                    .map_err(|error| error.to_string())?;
                replacement
            }
            None => return Err("Choose Move or Insert in Edit vertex first.".into()),
        };
        replacement.length().map_err(|error| error.to_string())?;
        replacement.extrema().map_err(|error| error.to_string())?;
        Ok(replacement)
    }

    pub(super) fn pedit_apply_vertex_point(&mut self, point: Point3) -> Result<(), String> {
        if !self.pedit_awaits_point() {
            return Err("Choose Move or Insert in Edit vertex first.".into());
        }
        let polyline = self.pedit_vertex_preview(point)?;
        let source = self.pedit_input.source.expect("marked source exists");
        if self
            .editor
            .update(source, Entity::Polyline(polyline))
            .map_err(|error| error.to_string())?
        {
            self.pedit_checkpoint(source);
            if matches!(self.pedit_input.point_edit, Some(VertexPointEdit::Insert)) {
                self.pedit_input.vertex = self.pedit_input.vertex.map(|vertex| vertex + 1);
            }
        }
        self.pedit_input.point_edit = None;
        self.points.clear();
        self.editor.select(vec![source]);
        self.pedit_vertex_message();
        Ok(())
    }

    fn pedit_vertex_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if self.pedit_input.width.is_some() {
            return self.pedit_width_text(input);
        }
        let tokens: Vec<_> = input.split_whitespace().collect();
        if tokens.first().is_some_and(|token| {
            token.eq_ignore_ascii_case("WIDTH") || token.eq_ignore_ascii_case("W")
        }) {
            return Some(self.pedit_start_width(&tokens[1..]));
        }
        if input.eq_ignore_ascii_case("REGEN") {
            return Some(
                if self.pedit_input.point_edit.is_some() || self.pedit_input.range.is_some() {
                    Err("Finish the pending vertex edit before Regen.".into())
                } else {
                    self.message(
                        "Polyline regenerated. Width changes are displayed immediately in OpenCad.",
                        false,
                    );
                    Ok(())
                },
            );
        }
        if input.is_empty() || input.eq_ignore_ascii_case("EXIT") || input.eq_ignore_ascii_case("X")
        {
            if self.pedit_input.point_edit.is_some() {
                self.pedit_input.point_edit = None;
                self.points.clear();
                self.pedit_vertex_message();
            } else if self.pedit_input.range.take().is_some() {
                self.pedit_vertex_message();
            } else {
                self.pedit_input.vertex = None;
                self.message(self.prompt(), false);
            }
            return Some(Ok(()));
        }
        if matches!(
            input.to_ascii_uppercase().as_str(),
            "STRAIGHTEN" | "S" | "BREAK" | "B"
        ) {
            return Some((|| {
                if self.pedit_input.point_edit.is_some() {
                    let label =
                        if input.eq_ignore_ascii_case("BREAK") || input.eq_ignore_ascii_case("B") {
                            "Break"
                        } else {
                            "Straighten"
                        };
                    return Err(format!("Exit point entry before choosing {label}."));
                }
                if let Some(range) = self.pedit_input.range {
                    return Err(format!("Use Go or Exit to finish {} first.", range.label()));
                }
                let (vertex, _, _) = self.pedit_marked_vertex().ok_or("No marked vertex.")?;
                self.pedit_input.range = Some(
                    if input.eq_ignore_ascii_case("BREAK") || input.eq_ignore_ascii_case("B") {
                        VertexRangeEdit::Break(vertex)
                    } else {
                        VertexRangeEdit::Straighten(vertex)
                    },
                );
                self.pedit_vertex_message();
                Ok(())
            })());
        }
        if input.eq_ignore_ascii_case("GO") || input.eq_ignore_ascii_case("G") {
            return Some(self.pedit_range_go());
        }
        if matches!(
            input.to_ascii_uppercase().as_str(),
            "NEXT" | "N" | "PREVIOUS" | "P"
        ) {
            return Some((|| {
                if self.pedit_input.point_edit.is_some() {
                    return Err(
                        "Specify the new vertex point, or Exit to cancel point entry first.".into(),
                    );
                }
                let (vertex, count, _) = self.pedit_marked_vertex().ok_or("No marked vertex.")?;
                self.pedit_input.vertex = Some(
                    if input.eq_ignore_ascii_case("NEXT") || input.eq_ignore_ascii_case("N") {
                        (vertex + 1).min(count - 1)
                    } else {
                        vertex.saturating_sub(1)
                    },
                );
                self.pedit_vertex_message();
                Ok(())
            })());
        }
        let tokens: Vec<_> = input.split_whitespace().collect();
        let point_edit =
            tokens
                .first()
                .and_then(|token| match token.to_ascii_uppercase().as_str() {
                    "MOVE" | "M" => Some(VertexPointEdit::Move),
                    "INSERT" | "I" => Some(VertexPointEdit::Insert),
                    _ => None,
                });
        if let Some(point_edit) = point_edit {
            return Some((|| {
                if tokens.len() > 2 {
                    return Err("Use Move [x,y[,z]] or Insert [x,y[,z]] in Edit vertex.".into());
                }
                let point = tokens
                    .get(1)
                    .map(|token| parse_point(token, self.drawing_elevation()))
                    .transpose()?;
                self.pedit_start_vertex_point(point_edit)?;
                if let Some(point) = point {
                    self.pedit_apply_vertex_point(point)?;
                }
                Ok(())
            })());
        }
        if Operation::parse(input).is_some() {
            return Some(Err(
                "Exit Edit vertex before using a PEDIT operation.".into()
            ));
        }
        if matches!(input.to_ascii_uppercase().as_str(), "TANGENT" | "T") {
            return Some(Err(
                "This Edit vertex option is not implemented yet; use Next, Previous, Move, Insert, Straighten or Exit."
                    .into(),
            ));
        }
        None
    }

    pub(super) fn pedit_joining(&self) -> bool {
        self.tool == Tool::Pedit && self.pedit_input.joining
    }

    pub(super) fn retain_pedit_source(&self, selection: &mut Vec<usize>) {
        if self.pedit_joining()
            && let Some(source) = self.pedit_input.source
        {
            selection.push(source);
        }
    }

    fn pedit_join_selected(&mut self) -> Result<(), String> {
        let source = self
            .pedit_input
            .source
            .ok_or("Select a PEDIT object first.")?;
        let candidates = self.editor.selection().to_vec();
        let count = self
            .editor
            .join_polyline_entities(source, &candidates)
            .map_err(|error| error.to_string())?;
        if count != 0 {
            self.pedit_checkpoint(source);
            self.pedit_input.source = self.editor.selection().first().copied();
        }
        self.pedit_input.joining = false;
        self.editor
            .select(self.pedit_input.source.into_iter().collect());
        self.message(
            if count == 0 {
                "No touching compatible objects selected; geometry unchanged.".into()
            } else {
                format!("{count} objects joined to polyline. {}", self.prompt())
            },
            false,
        );
        Ok(())
    }

    pub(super) fn pedit_select(&mut self, index: usize) -> Result<(), String> {
        if self.pedit_input.source.is_some() {
            return Err("Finish the current PEDIT session before selecting another object.".into());
        }
        if !self.editor.is_entity_visible(index) {
            return Err("PEDIT objects must be visible.".into());
        }
        let entity = self
            .editor
            .entities()
            .get(index)
            .ok_or("No object at that index.")?;
        entity.to_polyline_xy().map_err(|error| error.to_string())?;
        self.pedit_input.conversion = !matches!(entity, Entity::Polyline(_));
        self.pedit_input.source = Some(index);
        self.editor.select(vec![index]);
        if self.pedit_input.conversion && self.pedit_accept {
            self.pedit_convert()?;
        } else {
            self.message(self.prompt(), false);
        }
        Ok(())
    }

    fn pedit_checkpoint(&mut self, previous_source: usize) {
        self.pedit_input
            .undo
            .push((previous_source, self.editor.revision()));
        if self.pedit_input.undo.len() > 100 {
            self.pedit_input.undo.remove(0);
        }
    }

    fn pedit_convert(&mut self) -> Result<(), String> {
        let source = self
            .pedit_input
            .source
            .ok_or("Select a PEDIT object first.")?;
        if self
            .editor
            .convert_to_polyline(source)
            .map_err(|error| error.to_string())?
        {
            self.pedit_checkpoint(source);
            self.pedit_input.source = self.editor.selection().first().copied();
        }
        self.pedit_input.conversion = false;
        self.message(self.prompt(), false);
        Ok(())
    }

    fn pedit_operation(&mut self, operation: Operation) -> Result<(), String> {
        if self.pedit_input.joining {
            return Err("Press Enter to finish selecting Join objects first.".into());
        }
        if self.pedit_input.vertex.is_some() {
            return Err("Exit Edit vertex before using a PEDIT operation.".into());
        }
        let source = self
            .pedit_input
            .source
            .ok_or("Select a PEDIT object first.")?;
        if matches!(operation, Operation::Undo) {
            let Some(&(previous, revision)) = self.pedit_input.undo.last() else {
                self.message("No more operations in this PEDIT session.", false);
                return Ok(());
            };
            if self.editor.revision() != revision {
                self.finish();
                return Err("PEDIT ended because another edit changed the drawing.".into());
            }
            if !self.editor.undo() {
                return Err("No more PEDIT history is available.".into());
            }
            self.pedit_input.undo.pop();
            self.pedit_input.source = Some(previous);
            self.pedit_input.conversion = !matches!(
                self.editor.entities().get(previous),
                Some(Entity::Polyline(_))
            );
            self.editor.select(vec![previous]);
            self.message(self.prompt(), false);
            return Ok(());
        }
        let Some(Entity::Polyline(original)) = self.editor.entities().get(source) else {
            return Err("Confirm conversion to a polyline first.".into());
        };
        if matches!(operation, Operation::Width) {
            return self.pedit_start_width(&[]);
        }
        if matches!(operation, Operation::Edit) {
            self.pedit_input.vertex = Some(0);
            self.pedit_vertex_message();
            return Ok(());
        }
        if matches!(operation, Operation::Join) {
            if original.is_closed() {
                return Err(GeometryError::ClosedPolylineJoin.to_string());
            }
            self.pedit_input.joining = true;
            self.editor.select(vec![source]);
            self.message(self.prompt(), false);
            return Ok(());
        }
        let mut polyline = original.clone();
        match operation {
            Operation::Close => polyline.set_closed(true),
            Operation::Open => polyline.set_closed(false),
            Operation::Reverse => polyline.reverse_with_widths(self.pline_reverse_widths),
            Operation::Undo | Operation::Join | Operation::Edit | Operation::Width => {
                unreachable!("handled above")
            }
        }
        polyline.length().map_err(|error| error.to_string())?;
        polyline.extrema().map_err(|error| error.to_string())?;
        if self
            .editor
            .update(source, Entity::Polyline(polyline))
            .map_err(|error| error.to_string())?
        {
            self.pedit_checkpoint(source);
        }
        self.editor.select(vec![source]);
        self.message(self.prompt(), false);
        Ok(())
    }

    pub(super) fn pedit_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if self.tool != Tool::Pedit {
            return None;
        }
        if self.pedit_input.width.is_some() {
            return self.pedit_width_text(input);
        }
        if self.pedit_vertex_active() {
            return self.pedit_vertex_text(input);
        }
        if input.eq_ignore_ascii_case("EXIT") {
            self.finish();
            self.message("PEDIT finished.", false);
            return Some(Ok(()));
        }
        if self.pedit_input.conversion {
            match input.to_ascii_uppercase().as_str() {
                "" | "YES" | "Y" => return Some(self.pedit_convert()),
                "NO" | "N" => {
                    self.pedit_input = PeditInput::default();
                    self.editor.select(Vec::new());
                    self.message(self.prompt(), false);
                    return Some(Ok(()));
                }
                _ => {}
            }
        }
        if input.is_empty() {
            if self.pedit_joining() {
                return Some(self.pedit_join_selected());
            }
            self.finish();
            self.message("PEDIT finished.", false);
            return Some(Ok(()));
        }
        let tokens: Vec<_> = input.split_whitespace().collect();
        if tokens.first().is_some_and(|token| {
            token.eq_ignore_ascii_case("WIDTH") || token.eq_ignore_ascii_case("W")
        }) {
            return Some(self.pedit_start_width(&tokens[1..]));
        }
        if tokens.first().is_some_and(|token| {
            token.eq_ignore_ascii_case("JOIN") || token.eq_ignore_ascii_case("J")
        }) {
            return Some((|| {
                let indices = self.join_indices(&tokens[1..])?;
                if !self.pedit_joining() {
                    self.pedit_operation(Operation::Join)?;
                }
                if indices.is_empty() {
                    self.message(self.prompt(), false);
                    return Ok(());
                }
                let mut selection = self.editor.selection().to_vec();
                selection.extend(indices);
                self.retain_pedit_source(&mut selection);
                self.editor.select(selection);
                self.pedit_join_selected()
            })());
        }
        if self.pedit_joining() && input.eq_ignore_ascii_case("ALL") {
            self.editor
                .select((0..self.editor.entities().len()).collect());
            self.message(self.prompt(), false);
            return Some(Ok(()));
        }
        if let Some(operation) = Operation::parse(input) {
            return Some(self.pedit_operation(operation));
        }
        if input.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-')) {
            if self.pedit_joining() {
                return Some((|| {
                    let indices = self.join_indices(&tokens)?;
                    let mut selection = self.editor.selection().to_vec();
                    selection.extend(indices);
                    self.retain_pedit_source(&mut selection);
                    self.editor.select(selection);
                    self.message(self.prompt(), false);
                    Ok(())
                })());
            }
            return Some(
                entity_index(input, self.editor.entities().len())
                    .and_then(|index| self.pedit_select(index)),
            );
        }
        if matches!(
            input.to_ascii_uppercase().as_str(),
            "F" | "FIT" | "S" | "SPLINE" | "D" | "DECURVE" | "M" | "MULTIPLE"
        ) {
            return Some(Err(
                "This PEDIT option is not implemented yet; use Close, Open, Reverse, Join, Edit or Undo."
                    .into(),
            ));
        }
        None
    }

    pub(super) fn pedit_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let index = tokens
            .first()
            .map(|token| entity_index(token, self.editor.entities().len()))
            .transpose()?;
        let operation = tokens
            .get(1)
            .map(|token| {
                Operation::parse(token)
                    .filter(|operation| !matches!(operation, Operation::Undo))
                    .ok_or("Use Close, Open, Reverse, Width, Join or Edit after the PEDIT object number.")
            })
            .transpose()?;
        let candidates = if matches!(operation, Some(Operation::Join)) {
            self.join_indices(&tokens[2..])?
        } else if matches!(operation, Some(Operation::Width)) {
            if tokens.len() > 3 {
                return Err("Use PEDIT object-number Width [value].".into());
            }
            Vec::new()
        } else if tokens.len() > 2
            && !(matches!(operation, Some(Operation::Edit))
                && tokens.len() == 3
                && tokens[2].eq_ignore_ascii_case("VERTEX"))
        {
            return Err(
                "Use PEDIT [object-number [Close|Open|Reverse|Edit|Join [object-numbers...]]]."
                    .into(),
            );
        } else {
            Vec::new()
        };
        if operation.is_some()
            && !matches!(
                index.and_then(|index| self.editor.entities().get(index)),
                Some(Entity::Polyline(_))
            )
        {
            return Err(
                "Select and confirm line/arc conversion before applying a PEDIT option.".into(),
            );
        }
        let preselected = if self.editor.selection().len() == 1 {
            self.editor.selection().first().copied()
        } else {
            None
        };
        self.set_tool(Tool::Pedit);
        if let Some(index) = index.or(preselected) {
            self.pedit_select(index)?;
        }
        if let Some(operation) = operation {
            self.pedit_operation(operation)?;
        }
        if matches!(operation, Some(Operation::Width))
            && let Some(value) = tokens.get(2)
        {
            let value = value
                .parse::<f64>()
                .map_err(|_| "Enter a finite nonnegative width.".to_owned())?;
            self.pedit_width_value(value)?;
        }
        if !candidates.is_empty() {
            let mut selection = candidates;
            self.retain_pedit_source(&mut selection);
            self.editor.select(selection);
            self.pedit_join_selected()?;
        }
        Ok(())
    }
}
