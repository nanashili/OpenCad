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
    undo: Vec<(usize, u64)>,
}

#[derive(Clone, Copy)]
enum VertexPointEdit {
    Move,
    Insert,
}

#[derive(Clone, Copy)]
enum Operation {
    Close,
    Open,
    Reverse,
    Undo,
    Join,
    Edit,
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
            _ => return None,
        })
    }
}

impl OpenCadApp {
    pub(super) fn pedit_prompt(&self) -> &'static str {
        if matches!(self.pedit_input.point_edit, Some(VertexPointEdit::Move)) {
            "PEDIT Move vertex · Specify x,y[,z] or a canvas point. Relative @dx,dy[,dz] uses the marked vertex. Enter or Exit cancels this move; Escape ends PEDIT."
        } else if matches!(self.pedit_input.point_edit, Some(VertexPointEdit::Insert)) {
            "PEDIT Insert vertex · Specify x,y[,z] or a canvas point after the marked vertex. Relative @dx,dy[,dz] uses the marked vertex. Enter or Exit cancels insertion; Escape ends PEDIT."
        } else if self.pedit_input.vertex.is_some() {
            "PEDIT Edit vertex · Next / Previous / Move / Insert; Enter or Exit returns to PEDIT. Escape ends the session."
        } else if self.pedit_input.joining {
            "PEDIT Join · Select touching XY lines, arcs or open polylines; Enter joins. Source stays selected. Shift removes; All selects visible objects. Escape ends PEDIT."
        } else if self.pedit_input.conversion {
            "PEDIT · Object is not a polyline. Convert it? Yes/No <Yes>. Escape cancels."
        } else if let Some(source) = self.pedit_input.source {
            if matches!(self.editor.entities().get(source), Some(Entity::Polyline(polyline)) if polyline.is_closed())
            {
                "PEDIT · Open / Reverse / Undo / Edit vertex; Enter or Exit finishes. Escape ends the session."
            } else {
                "PEDIT · Close / Reverse / Undo / Join / Edit vertex; Enter or Exit finishes. Escape ends the session."
            }
        } else {
            "PEDIT · Select one XY polyline, line or arc; or enter its object number. Escape cancels."
        }
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
                vertices.insert(vertex + 1, point);
                bulges.insert(vertex + 1, 0.0);
                if append {
                    bulges[vertex + 1] = bulges[vertex];
                    bulges[vertex] = 0.0;
                }
                Polyline2::with_bulges(vertices, bulges, original.is_closed())
                    .map_err(|error| error.to_string())?
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
        if input.is_empty() || input.eq_ignore_ascii_case("EXIT") || input.eq_ignore_ascii_case("X")
        {
            if self.pedit_input.point_edit.is_some() {
                self.pedit_input.point_edit = None;
                self.points.clear();
                self.pedit_vertex_message();
            } else {
                self.pedit_input.vertex = None;
                self.message(self.prompt(), false);
            }
            return Some(Ok(()));
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
        if matches!(
            input.to_ascii_uppercase().as_str(),
            "STRAIGHTEN" | "S" | "BREAK" | "B" | "TANGENT" | "T" | "WIDTH" | "W" | "REGEN"
        ) {
            return Some(Err(
                "This Edit vertex option is not implemented yet; use Next, Previous, Move, Insert or Exit."
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
            Operation::Reverse => polyline.reverse(),
            Operation::Undo | Operation::Join | Operation::Edit => unreachable!("handled above"),
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
            "W" | "WIDTH" | "F" | "FIT" | "S" | "SPLINE" | "D" | "DECURVE" | "M" | "MULTIPLE"
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
                    .ok_or("Use Close, Open, Reverse, Join or Edit after the PEDIT object number.")
            })
            .transpose()?;
        let candidates = if matches!(operation, Some(Operation::Join)) {
            self.join_indices(&tokens[2..])?
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
        if !candidates.is_empty() {
            let mut selection = candidates;
            self.retain_pedit_source(&mut selection);
            self.editor.select(selection);
            self.pedit_join_selected()?;
        }
        Ok(())
    }
}
