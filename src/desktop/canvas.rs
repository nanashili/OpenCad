use super::*;
use egui::{Align2, FontId, Painter, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind};

pub(super) struct Camera {
    center: [f64; 2],
    scale: f64,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0],
            scale: 4.0,
        }
    }
}
impl Camera {
    pub(super) fn zoom(&mut self, factor: f64) -> Result<(), String> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err("Zoom factor must be a positive finite number.".into());
        }
        self.scale = (self.scale * factor).clamp(1e-8, 1e6);
        Ok(())
    }

    pub(super) fn pan(&mut self, direction: &str) -> Result<(), String> {
        let amount = 100.0 / self.scale;
        let [dx, dy] = match direction.to_ascii_uppercase().as_str() {
            "LEFT" => [-amount, 0.0],
            "RIGHT" => [amount, 0.0],
            "UP" => [0.0, amount],
            "DOWN" => [0.0, -amount],
            _ => return Err("PAN accepts LEFT, RIGHT, UP, or DOWN.".into()),
        };
        let next = [self.center[0] + dx, self.center[1] + dy];
        if next.iter().any(|value| !value.is_finite()) {
            return Err("Pan exceeds the finite coordinate range.".into());
        }
        self.center = next;
        Ok(())
    }

    fn screen(&self, x: f64, y: f64, rect: Rect) -> Option<Pos2> {
        let x = f64::from(rect.center().x) + (x - self.center[0]) * self.scale;
        let y = f64::from(rect.center().y) - (y - self.center[1]) * self.scale;
        (x.is_finite() && y.is_finite() && x.abs() < 1e10 && y.abs() < 1e10)
            .then(|| egui::pos2(x as f32, y as f32))
    }
    fn world(&self, position: Pos2, rect: Rect, z: f64) -> Option<Point3> {
        Point3::new(
            self.center[0] + f64::from(position.x - rect.center().x) / self.scale,
            self.center[1] - f64::from(position.y - rect.center().y) / self.scale,
            z,
        )
        .ok()
    }
    fn fit(&mut self, entities: &[Entity], rect: Rect) -> Result<(), String> {
        if entities.is_empty() {
            *self = Self::default();
            return Ok(());
        }
        let [min_x, min_y, max_x, max_y] =
            crate::svg::entity_bounds(entities).map_err(|e| e.to_string())?;
        let scale = (f64::from((rect.width() - 100.0).max(1.0)) / (max_x - min_x).max(1.0))
            .min(f64::from((rect.height() - 100.0).max(1.0)) / (max_y - min_y).max(1.0));
        if !scale.is_finite() || scale < 1e-8 {
            return Err("Drawing extents exceed the viewport zoom range.".into());
        }
        self.center = [min_x / 2.0 + max_x / 2.0, min_y / 2.0 + max_y / 2.0];
        self.scale = scale.min(1e6);
        Ok(())
    }
}

impl OpenCadApp {
    pub(super) fn canvas(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(CANVAS))
            .show(ui, |ui| {
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                let rect = response.rect;
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Other,
                        response.enabled(),
                        "Drawing canvas, XY top view. Use the command field for exact coordinates.",
                    )
                });
                if self.fit_pending {
                    self.fit_pending = false;
                    if let Err(e) = self.camera.fit(&self.editor.visible_entities(), rect) {
                        self.message(e, true);
                    }
                }
                let pointer = response
                    .hover_pos()
                    .or_else(|| response.interact_pointer_pos());
                let panning = response.dragged_by(PointerButton::Middle)
                    || response.dragged_by(PointerButton::Secondary)
                    || (response.dragged_by(PointerButton::Primary) && self.tool == Tool::Pan);
                if panning {
                    let delta = ui.input(|i| i.pointer.delta());
                    let next = [
                        self.camera.center[0] - f64::from(delta.x) / self.camera.scale,
                        self.camera.center[1] + f64::from(delta.y) / self.camera.scale,
                    ];
                    if next.iter().all(|n| n.is_finite()) {
                        self.camera.center = next;
                    }
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    let pinch = ui.input(|i| i.zoom_delta());
                    let factor = (f64::from(scroll) * 0.002).exp() * f64::from(pinch);
                    if (factor - 1.0).abs() > 0.00001
                        && let Some(position) = pointer
                        && let Some(before) = self.camera.world(position, rect, 0.0)
                    {
                        self.camera.scale = (self.camera.scale * factor).clamp(1e-8, 1e6);
                        if let Some(after) = self.camera.world(position, rect, 0.0) {
                            let next = [
                                self.camera.center[0] + before.x() - after.x(),
                                self.camera.center[1] + before.y() - after.y(),
                            ];
                            if next.iter().all(|n| n.is_finite()) {
                                self.camera.center = next;
                            }
                        }
                    }
                    if !panning {
                        ui.ctx().set_cursor_icon(if self.tool == Tool::Pan {
                            egui::CursorIcon::Grab
                        } else {
                            egui::CursorIcon::None
                        });
                    }
                }
                self.snap_label = None;
                self.polar_guide = None;
                self.cursor = pointer.and_then(|position| {
                    self.camera.world(position, rect, self.drawing_elevation())
                });
                if !matches!(
                    self.tool,
                    Tool::Select | Tool::Pan | Tool::Trim | Tool::Extend
                ) && let Some(point) = self.cursor
                {
                    self.cursor = Some(self.snap(point, rect, ui.input(|i| i.modifiers.shift)));
                }
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    let mut description = format!("Current tool: {}", self.tool.name());
                    if let Some((_, angle)) = self.polar_guide {
                        description.push_str(&format!("; Polar guide: {angle:.2} degrees"));
                    }
                    node.set_description(description);
                });
                if response.clicked_by(PointerButton::Primary) {
                    response.request_focus();
                    if self.tool == Tool::Select {
                        let hit = pointer.and_then(|p| self.hit(p, rect));
                        if ui.input(|i| i.modifiers.shift) {
                            if let Some(index) = hit {
                                self.editor.toggle_selection(index);
                            }
                        } else {
                            self.editor.select(hit.into_iter().collect());
                        }
                    } else if matches!(self.tool, Tool::Trim | Tool::Extend) {
                        let hit = pointer.and_then(|position| self.hit(position, rect));
                        if let Some(point) = self.cursor
                            && let Err(error) =
                                self.trim_click(hit, point, ui.input(|i| i.modifiers.shift))
                        {
                            self.message(error, true);
                        }
                    } else if self.offset_awaits_source() {
                        let hit = pointer.and_then(|position| self.hit(position, rect));
                        if let Err(error) = self.offset_source(hit) {
                            self.message(error, true);
                        }
                    } else if self.tool != Tool::Pan
                        && let Some(point) = self.cursor
                        && let Err(e) = self.point(point)
                    {
                        self.message(e, true);
                    }
                }
                if response.drag_started_by(PointerButton::Primary) && self.tool == Tool::Select {
                    response.request_focus();
                    self.marquee = ui.input(|i| i.pointer.press_origin());
                }
                if response.drag_stopped_by(PointerButton::Primary)
                    && let Some(start) = self.marquee.take()
                    && let Some(end) = ui.input(|i| i.pointer.latest_pos())
                {
                    let window = Rect::from_two_pos(start, end);
                    let mut selection = if ui.input(|i| i.modifiers.shift) {
                        self.editor.selection().to_vec()
                    } else {
                        Vec::new()
                    };
                    for (index, entity) in self.editor.entities().iter().enumerate() {
                        if !self.editor.is_entity_visible(index) {
                            continue;
                        }
                        if let Ok([x1, y1, x2, y2]) =
                            crate::svg::entity_bounds(std::slice::from_ref(entity))
                            && let (Some(a), Some(b)) = (
                                self.camera.screen(x1, y1, rect),
                                self.camera.screen(x2, y2, rect),
                            )
                            && window.contains(a)
                            && window.contains(b)
                        {
                            selection.push(index);
                        }
                    }
                    self.editor.select(selection);
                }
                if response.double_clicked() && self.tool == Tool::Select {
                    self.fit_pending = true;
                }
                if self.grid {
                    self.draw_grid(&painter, rect);
                }
                for (index, entity) in self.editor.entities().iter().enumerate() {
                    if !self.editor.is_entity_visible(index) {
                        continue;
                    }
                    let selected = self.editor.selection().contains(&index);
                    self.draw_styled_entity(
                        &painter,
                        entity,
                        index,
                        rect,
                        Stroke::new(
                            if selected { 2.0 } else { 1.3 },
                            if selected {
                                SELECTED
                            } else {
                                let [r, g, b] = self
                                    .editor
                                    .document()
                                    .entity_rgb(index, false)
                                    .expect("entity style exists");
                                Color32::from_rgb(r, g, b)
                            },
                        ),
                    );
                    if selected {
                        let grips = match entity {
                            Entity::Line(_) => usize::MAX,
                            Entity::Polyline(polyline) => polyline.vertices().len(),
                            _ => 1,
                        };
                        for point in entity_points(entity).into_iter().take(grips) {
                            if let Some(p) = self.camera.screen(point.x(), point.y(), rect)
                                && rect.contains(p)
                            {
                                painter.rect_filled(
                                    Rect::from_center_size(p, egui::vec2(5.0, 5.0)),
                                    0.0,
                                    SELECTED,
                                );
                            }
                        }
                    }
                }
                self.draw_polar_guide(&painter, rect);
                self.draw_preview(&painter, rect);
                if let Some(start) = self.marquee
                    && let Some(end) = pointer
                {
                    let window = Rect::from_two_pos(start, end);
                    painter.rect_filled(
                        window,
                        0.0,
                        Color32::from_rgba_unmultiplied(110, 231, 211, 18),
                    );
                    painter.rect_stroke(window, 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
                }
                if response.hovered()
                    && !panning
                    && self.tool != Tool::Pan
                    && let Some(point) = self.cursor
                    && let Some(p) = self.camera.screen(point.x(), point.y(), rect)
                {
                    painter.line_segment(
                        [p - egui::vec2(48.0, 0.0), p + egui::vec2(48.0, 0.0)],
                        Stroke::new(1.0, Color32::from_gray(160)),
                    );
                    painter.line_segment(
                        [p - egui::vec2(0.0, 48.0), p + egui::vec2(0.0, 48.0)],
                        Stroke::new(1.0, Color32::from_gray(160)),
                    );
                    painter.rect_stroke(
                        Rect::from_center_size(p, egui::vec2(6.0, 6.0)),
                        0.0,
                        Stroke::new(1.0, Color32::from_gray(160)),
                        StrokeKind::Inside,
                    );
                    if let Some(label) = self.snap_label {
                        painter.rect_stroke(
                            Rect::from_center_size(p, egui::vec2(10.0, 10.0)),
                            0.0,
                            Stroke::new(1.5, ACCENT),
                            StrokeKind::Inside,
                        );
                        painter.text(
                            p + egui::vec2(15.0, -16.0),
                            Align2::LEFT_BOTTOM,
                            label,
                            FontId::monospace(11.0),
                            ACCENT,
                        );
                    }
                }
                self.view_controls(ui, &painter, rect);
            });
    }

    fn view_controls(&mut self, ui: &mut egui::Ui, painter: &Painter, rect: Rect) {
        let controls = rect.left_top() + egui::vec2(6.0, 2.0);
        let viewport = ui
            .put(
                Rect::from_min_size(controls, egui::vec2(28.0, 20.0)),
                egui::Button::new("[−]").frame(false),
            )
            .on_hover_text("Viewport controls");
        viewport.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Viewport controls")
        });
        egui::Popup::menu(&viewport).show(|ui| {
            ui.checkbox(&mut self.workspace.view_cube, "ViewCube");
            ui.checkbox(&mut self.workspace.ucs_icon, "UCS icon");
            if ui.button("Zoom extents").clicked() {
                self.fit_pending = true;
                ui.close();
            }
        });
        if ui
            .put(
                Rect::from_min_size(controls + egui::vec2(30.0, 0.0), egui::vec2(36.0, 20.0)),
                egui::Button::new("Top").frame(false),
            )
            .on_hover_text("World XY top view")
            .clicked()
        {
            self.fit_pending = true;
        }
        ui.put(
            Rect::from_min_size(controls + egui::vec2(70.0, 0.0), egui::vec2(96.0, 20.0)),
            egui::Label::new("|  2D Wireframe"),
        );
        if self.workspace.view_cube {
            let center = rect.right_top() + egui::vec2(-80.0, 82.0);
            painter.circle_stroke(
                center,
                55.0,
                Stroke::new(9.0, Color32::from_rgb(74, 81, 89)),
            );
            for (direction, offset) in [
                ("N", egui::vec2(0.0, -58.0)),
                ("E", egui::vec2(58.0, 0.0)),
                ("S", egui::vec2(0.0, 58.0)),
                ("W", egui::vec2(-58.0, 0.0)),
            ] {
                painter.text(
                    center + offset,
                    Align2::CENTER_CENTER,
                    direction,
                    FontId::proportional(22.0),
                    Color32::from_gray(151),
                );
            }
            let cube = Rect::from_center_size(center, egui::vec2(50.0, 50.0));
            painter.rect_filled(cube, 0.0, Color32::from_gray(171));
            painter.rect_filled(cube.shrink(9.0), 0.0, Color32::from_gray(158));
            let top = ui.put(
                cube,
                egui::Button::new(
                    egui::RichText::new("TOP")
                        .size(12.0)
                        .strong()
                        .color(Color32::from_gray(72)),
                )
                .frame(false),
            );
            top.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Top view")
            });
            if top.clicked() {
                self.fit_pending = true;
            }
            ui.put(
                Rect::from_center_size(center + egui::vec2(0.0, 91.0), egui::vec2(48.0, 15.0)),
                egui::Label::new(egui::RichText::new("WCS  ⌄").size(10.0).color(MUTED)),
            );
        }
        if self.workspace.ucs_icon {
            let origin = rect.left_bottom() + egui::vec2(25.0, -28.0);
            let stroke = Stroke::new(1.0, Color32::from_gray(160));
            painter.line_segment([origin, origin + egui::vec2(50.0, 0.0)], stroke);
            painter.line_segment([origin, origin + egui::vec2(0.0, -50.0)], stroke);
            painter.rect_stroke(
                Rect::from_center_size(origin, egui::vec2(8.0, 8.0)),
                0.0,
                stroke,
                StrokeKind::Inside,
            );
            painter.text(
                origin + egui::vec2(57.0, 0.0),
                Align2::LEFT_CENTER,
                "X",
                FontId::proportional(15.0),
                MUTED,
            );
            painter.text(
                origin + egui::vec2(0.0, -55.0),
                Align2::CENTER_BOTTOM,
                "Y",
                FontId::proportional(15.0),
                MUTED,
            );
        }
    }

    fn draw_grid(&self, painter: &Painter, rect: Rect) {
        let Some(a) = self.camera.world(rect.left_top(), rect, 0.0) else {
            return;
        };
        let Some(b) = self.camera.world(rect.right_bottom(), rect, 0.0) else {
            return;
        };
        let power = (20.0 / (self.grid_step * self.camera.scale))
            .log10()
            .ceil()
            .max(0.0);
        let step = self.grid_step * 10.0f64.powf(power);
        if !step.is_finite() {
            return;
        }
        for (low, high, vertical) in [(a.x(), b.x(), true), (b.y(), a.y(), false)] {
            let start = (low / step).ceil() * step;
            let count = ((high - start) / step).ceil();
            if !start.is_finite() || !count.is_finite() || !(0.0..=250.0).contains(&count) {
                continue;
            }
            for index in 0..=count as usize {
                let value = start + index as f64 * step;
                let position = if vertical {
                    self.camera.screen(value, self.camera.center[1], rect)
                } else {
                    self.camera.screen(self.camera.center[0], value, rect)
                };
                if let Some(p) = position {
                    let (start, end) = if vertical {
                        (egui::pos2(p.x, rect.top()), egui::pos2(p.x, rect.bottom()))
                    } else {
                        (egui::pos2(rect.left(), p.y), egui::pos2(rect.right(), p.y))
                    };
                    painter.line_segment(
                        [start, end],
                        Stroke::new(
                            1.0,
                            if value.abs() < step * 0.000001 {
                                Color32::from_rgb(59, 77, 87)
                            } else {
                                Color32::from_rgb(30, 43, 54)
                            },
                        ),
                    );
                }
            }
        }
    }

    fn draw_styled_entity(
        &self,
        painter: &Painter,
        entity: &Entity,
        index: usize,
        rect: Rect,
        stroke: Stroke,
    ) {
        let document = self.editor.document();
        let kind = document
            .linetype_for_entity(index)
            .expect("effective linetype");
        let scale = document
            .effective_linetype_scale(index)
            .expect("entity style exists");
        // Dense patterns merge at this zoom. Export still reports its mark limit.
        if kind.pattern().is_empty() || kind.period() * scale * self.camera.scale < 2.0 {
            self.draw_entity(painter, entity, rect, stroke);
            return;
        }
        let Ok(parts) = kind.parts(entity, scale) else {
            self.draw_entity(painter, entity, rect, stroke);
            return;
        };
        for part in parts {
            match part {
                crate::linetype::PatternPart::Geometry(entity) => {
                    self.draw_entity(painter, &entity, rect, stroke)
                }
                crate::linetype::PatternPart::Dot(point) => {
                    if let Some(at) = self.camera.screen(point.x(), point.y(), rect) {
                        painter.circle_filled(at, (stroke.width / 2.0).max(0.75), stroke.color);
                    }
                }
            }
        }
    }

    fn draw_entity(&self, painter: &Painter, entity: &Entity, rect: Rect, stroke: Stroke) {
        match entity {
            Entity::Line(line) => {
                if let (Some(a), Some(b)) = (
                    self.camera.screen(line.start().x(), line.start().y(), rect),
                    self.camera.screen(line.end().x(), line.end().y(), rect),
                ) {
                    if a.distance_sq(b) < 0.25 {
                        painter.circle_stroke(a, 2.0, stroke);
                    } else {
                        painter.line_segment([a, b], stroke);
                    }
                }
            }
            Entity::Circle(circle) => {
                let radius = circle.radius() * self.camera.scale;
                if radius.is_finite()
                    && radius < 1e7
                    && let Some(center) =
                        self.camera
                            .screen(circle.center().x(), circle.center().y(), rect)
                {
                    painter.circle_stroke(center, (radius as f32).max(1.5), stroke);
                }
            }
            Entity::Arc(arc) => {
                let pixels = arc.radius() * self.camera.scale;
                if !pixels.is_finite() || pixels >= 1e7 {
                    return;
                }
                let steps = ((arc.sweep() * pixels.sqrt()).ceil() as usize).clamp(1, 4096);
                let points: Option<Vec<_>> = (0..=steps)
                    .map(|step| {
                        let point = arc.point_at_fraction(step as f64 / steps as f64).ok()?;
                        self.camera.screen(point.x(), point.y(), rect)
                    })
                    .collect();
                if let Some(points) = points {
                    if pixels * arc.sweep() < 1.5 {
                        painter.circle_stroke(points[0], 1.5, stroke);
                    } else {
                        painter.add(egui::Shape::line(points, stroke));
                    }
                }
            }
            Entity::Polyline(polyline) => {
                for segment in polyline.segments().flatten() {
                    self.draw_entity(painter, &segment.geometry(), rect, stroke);
                }
            }
        }
    }

    fn draw_preview(&self, painter: &Painter, rect: Rect) {
        let Some(cursor) = self.cursor else {
            return;
        };
        let stroke = Stroke::new(1.3, ACCENT);
        if self.tool == Tool::Offset {
            if let Some(entity) = self.offset_preview(cursor) {
                self.draw_entity(painter, &entity, rect, stroke);
            }
            return;
        }
        if matches!(self.tool, Tool::Trim | Tool::Extend) {
            if let Some(position) = self.camera.screen(cursor.x(), cursor.y(), rect)
                && let Some(source) = self.hit(position, rect)
                && let Some(parts) =
                    self.trim_preview(source, cursor, painter.ctx().input(|i| i.modifiers.shift))
            {
                for part in parts {
                    self.draw_entity(painter, &part, rect, stroke);
                }
            }
            return;
        }
        let Some(base) = self.points.last().copied() else {
            return;
        };
        match self.tool {
            Tool::Polyline => {
                for pair in self.points.windows(2) {
                    self.draw_entity(
                        painter,
                        &Entity::Line(Line3::new(pair[0], pair[1])),
                        rect,
                        stroke,
                    );
                }
                self.draw_entity(
                    painter,
                    &Entity::Line(Line3::new(base, cursor)),
                    rect,
                    stroke,
                );
            }
            Tool::Line => self.draw_entity(
                painter,
                &Entity::Line(Line3::new(base, cursor)),
                rect,
                stroke,
            ),
            Tool::Circle => {
                if let Ok(circle) =
                    Circle3::new(base, (cursor.x() - base.x()).hypot(cursor.y() - base.y()))
                {
                    self.draw_entity(painter, &Entity::Circle(circle), rect, stroke);
                }
            }
            Tool::Arc => {
                if self.points.len() == 2 {
                    if let Ok(arc) = arc_from_points(self.points[0], self.points[1], cursor) {
                        self.draw_entity(painter, &Entity::Arc(arc), rect, stroke);
                    }
                } else {
                    self.draw_entity(
                        painter,
                        &Entity::Line(Line3::new(base, cursor)),
                        rect,
                        stroke,
                    );
                }
            }
            Tool::Rectangle => {
                if let Ok(entities) = rectangle(base, cursor) {
                    for entity in entities {
                        self.draw_entity(painter, &entity, rect, stroke);
                    }
                }
            }
            Tool::Rotate | Tool::Scale | Tool::Mirror => {
                let end = if self.tool == Tool::Mirror {
                    self.points.get(1).copied().unwrap_or(cursor)
                } else {
                    cursor
                };
                let start = if self.tool == Tool::Mirror {
                    self.points[0]
                } else {
                    base
                };
                self.draw_entity(painter, &Entity::Line(Line3::new(start, end)), rect, stroke);
                if let Some(transform) = self.transform_preview(cursor) {
                    for index in self.editor.selection() {
                        if let Ok(entity) = self.editor.entities()[*index].transformed(transform) {
                            self.draw_entity(painter, &entity, rect, stroke);
                        }
                    }
                }
            }
            Tool::Move | Tool::Copy => {
                for index in self.editor.selection() {
                    if let Ok(entity) = self.editor.entities()[*index].translated(
                        cursor.x() - base.x(),
                        cursor.y() - base.y(),
                        cursor.z() - base.z(),
                    ) {
                        self.draw_entity(painter, &entity, rect, stroke);
                    }
                }
            }
            _ => {}
        }
        if let Some(p) = self.camera.screen(cursor.x(), cursor.y(), rect) {
            let distance = (cursor.x() - base.x()).hypot(cursor.y() - base.y());
            if distance.is_finite() {
                painter.text(
                    p + egui::vec2(16.0, 17.0),
                    Align2::LEFT_TOP,
                    format!("{distance:.3}"),
                    FontId::monospace(12.0),
                    ACCENT,
                );
            }
        }
    }

    fn snap(&mut self, mut point: Point3, rect: Rect, shift: bool) -> Point3 {
        if (self.ortho || shift)
            && let Some(base) = self.points.last().copied()
        {
            point = if (point.x() - base.x()).abs() >= (point.y() - base.y()).abs() {
                Point3::new(point.x(), base.y(), point.z())
            } else {
                Point3::new(base.x(), point.y(), point.z())
            }
            .unwrap_or(point);
        }
        if self.object_snap {
            let mut nearest = None;
            let mut distance = 9.0;
            for (index, entity) in self.editor.entities().iter().enumerate() {
                if !self.editor.is_entity_visible(index) {
                    continue;
                }
                for candidate in entity_points(entity) {
                    let d = (candidate.x() - point.x()).hypot(candidate.y() - point.y())
                        * self.camera.scale;
                    if d < distance {
                        distance = d;
                        nearest = Some(candidate);
                    }
                }
            }
            if let Some(candidate) = nearest {
                self.snap_label = Some("OBJECT");
                return candidate;
            }
        }
        if self.polar_tracking
            && !shift
            && let Some(base) = self.points.last().copied()
            && let Some((snapped, angle)) =
                polar_point(base, point, self.polar_angle, self.camera.scale)
        {
            self.polar_guide = Some((base, angle));
            return snapped;
        }
        if self.grid_snap
            && let Ok(snapped) = Point3::new(
                (point.x() / self.grid_step).round() * self.grid_step,
                (point.y() / self.grid_step).round() * self.grid_step,
                point.z(),
            )
        {
            self.snap_label = Some("GRID");
            point = snapped;
        }
        let _ = rect;
        point
    }

    fn draw_polar_guide(&self, painter: &Painter, rect: Rect) {
        let (Some((base, angle)), Some(cursor)) = (self.polar_guide, self.cursor) else {
            return;
        };
        let (Some(a), Some(b)) = (
            self.camera.screen(base.x(), base.y(), rect),
            self.camera.screen(cursor.x(), cursor.y(), rect),
        ) else {
            return;
        };
        let end = b + (b - a).normalized() * rect.size().length();
        painter.line_segment([a, end], Stroke::new(1.0, Color32::from_rgb(95, 170, 110)));
        painter.text(
            b + egui::vec2(16.0, -20.0),
            Align2::LEFT_BOTTOM,
            format!("Polar: {angle:.2}°"),
            FontId::monospace(11.0),
            ACCENT,
        );
    }

    fn hit(&self, position: Pos2, rect: Rect) -> Option<usize> {
        let line_distance = |line: Line3| -> Option<f32> {
            let (Some(a), Some(b)) = (
                self.camera.screen(line.start().x(), line.start().y(), rect),
                self.camera.screen(line.end().x(), line.end().y(), rect),
            ) else {
                return None;
            };
            let segment = b - a;
            let length = segment.length_sq();
            let t = if length > 0.0 {
                ((position - a).dot(segment) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            Some(position.distance(a + segment * t))
        };
        let arc_distance = |arc: Arc3| -> Option<f32> {
            let start = arc.start_point().ok()?;
            let end = arc.end_point().ok()?;
            let center = self
                .camera
                .screen(arc.center().x(), arc.center().y(), rect)?;
            let start = self.camera.screen(start.x(), start.y(), rect)?;
            let end = self.camera.screen(end.x(), end.y(), rect)?;
            let delta = position - center;
            let angle = (-f64::from(delta.y)).atan2(f64::from(delta.x));
            Some(if arc.contains_angle(angle) {
                (position.distance(center) - (arc.radius() * self.camera.scale) as f32).abs()
            } else {
                position.distance(start).min(position.distance(end))
            })
        };
        let mut closest = None;
        let mut distance = 8.0;
        for (index, entity) in self.editor.entities().iter().enumerate() {
            if !self.editor.is_entity_visible(index) {
                continue;
            }
            let d = match entity {
                Entity::Line(line) => line_distance(*line).unwrap_or(f32::INFINITY),
                Entity::Polyline(polyline) => polyline
                    .segments()
                    .filter_map(|segment| match segment.ok()? {
                        crate::PolylineSegment::Line(line) => line_distance(line),
                        crate::PolylineSegment::Arc { arc, .. } => arc_distance(arc),
                    })
                    .fold(f32::INFINITY, f32::min),
                Entity::Circle(circle) => {
                    let Some(center) =
                        self.camera
                            .screen(circle.center().x(), circle.center().y(), rect)
                    else {
                        continue;
                    };
                    (position.distance(center) - ((circle.radius() * self.camera.scale) as f32))
                        .abs()
                }
                Entity::Arc(arc) => arc_distance(*arc).unwrap_or(f32::INFINITY),
            };
            if d < distance {
                distance = d;
                closest = Some(index);
            }
        }
        closest
    }
}

// Original pointer capture policy: within 6 degrees and 8 screen pixels of a
// forward ray. Object snaps take precedence; typed coordinates bypass this.
fn polar_point(base: Point3, point: Point3, increment: f64, scale: f64) -> Option<(Point3, f64)> {
    let dx = point.x() - base.x();
    let dy = point.y() - base.y();
    let distance = dx.hypot(dy);
    if !distance.is_finite() || distance * scale < 2.0 {
        return None;
    }
    let angle = dy.atan2(dx).to_degrees().rem_euclid(360.0);
    let last = (360.0 / increment).ceil() - 1.0;
    let nearest = (angle / increment).round().min(last) * increment;
    let difference = |candidate: f64| (angle - candidate + 180.0).rem_euclid(360.0) - 180.0;
    let tracked = if difference(0.0).abs() < difference(nearest).abs() {
        0.0
    } else {
        nearest
    };
    let error = difference(tracked).abs();
    if error > 6.0 || distance * error.to_radians().sin() * scale > 8.0 {
        return None;
    }
    let (sin, cos) = match tracked {
        0.0 | 360.0 => (0.0, 1.0),
        90.0 => (1.0, 0.0),
        180.0 => (0.0, -1.0),
        270.0 => (-1.0, 0.0),
        _ => tracked.to_radians().sin_cos(),
    };
    let projection = dx * cos + dy * sin;
    if projection <= 0.0 {
        return None;
    }
    Point3::new(
        base.x() + projection * cos,
        base.y() + projection * sin,
        point.z(),
    )
    .ok()
    .map(|snapped| (snapped, tracked))
}

fn entity_points(entity: &Entity) -> Vec<Point3> {
    match entity {
        Entity::Line(line) => vec![line.start(), line.end()],
        Entity::Circle(circle) => {
            let center = circle.center();
            let r = circle.radius();
            let mut points = vec![center];
            for (x, y) in [
                (center.x() + r, center.y()),
                (center.x() - r, center.y()),
                (center.x(), center.y() + r),
                (center.x(), center.y() - r),
            ] {
                if let Ok(point) = Point3::new(x, y, center.z()) {
                    points.push(point);
                }
            }
            points
        }
        Entity::Arc(arc) => {
            let mut points = vec![arc.center()];
            if let Ok(extrema) = arc.extrema() {
                points.extend(extrema);
            }
            points
        }
        Entity::Polyline(polyline) => polyline
            .extrema()
            .unwrap_or_else(|_| polyline.vertices().to_vec()),
    }
}
