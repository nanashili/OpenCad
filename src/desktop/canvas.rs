use super::*;
use egui::{
    Align2, FontId, Painter, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle,
};
use std::cell::RefCell;

struct PdfTextureEntry {
    key: String,
    texture: TextureHandle,
}

pub(super) struct Camera {
    center: [f64; 2],
    scale: f64,
    view: crate::Camera3,
    ucs: crate::Ucs3,
    last_canvas_size: [f64; 2],
    pdf_cache: RefCell<Vec<PdfTextureEntry>>,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            center: [0.0, 0.0],
            scale: 4.0,
            view: crate::Camera3::new(Point3::default(), 100.0, crate::ViewOrientation::Top)
                .expect("valid camera"),
            ucs: crate::Ucs3::world(),
            last_canvas_size: [1024.0, 768.0],
            pdf_cache: RefCell::new(Vec::new()),
        }
    }
}
impl Camera {
    pub(super) fn clear_pdf_cache(&self) {
        self.pdf_cache.borrow_mut().clear();
    }

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
        self.screen_point(Point3::new(x, y, 0.0).ok()?, rect)
    }
    fn screen_point(&self, point: Point3, rect: Rect) -> Option<Pos2> {
        let projected = self.view.project(point).ok()?;
        let x = f64::from(rect.center().x) + (projected.x() - self.center[0]) * self.scale;
        let y = f64::from(rect.center().y) - (projected.y() - self.center[1]) * self.scale;
        (x.is_finite() && y.is_finite() && x.abs() < 1e10 && y.abs() < 1e10)
            .then(|| egui::pos2(x as f32, y as f32))
    }
    fn world(&self, position: Pos2, rect: Rect, z: f64) -> Option<Point3> {
        let x = self.center[0] + f64::from(position.x - rect.center().x) / self.scale;
        let y = self.center[1] - f64::from(position.y - rect.center().y) / self.scale;
        let point = self.view.intersect_ucs(x, y, self.ucs).ok()?;
        Point3::new(
            point.x(),
            point.y(),
            if self.view.orientation() == crate::ViewOrientation::Top {
                z
            } else {
                point.z()
            },
        )
        .ok()
    }
    pub(super) fn set_view(&mut self, view: crate::Camera3) {
        self.view = view;
    }
    pub(super) fn set_ucs(&mut self, ucs: crate::Ucs3) {
        self.ucs = ucs;
    }
    pub(super) fn named_view_viewport(&self) -> ([f64; 2], f64, f64) {
        (
            self.center,
            self.last_canvas_size[1] / self.scale,
            self.last_canvas_size[0] / self.scale,
        )
    }

    pub(super) fn set_named_view_viewport(
        &mut self,
        center: [f64; 2],
        height: f64,
        width: f64,
    ) -> Result<(), String> {
        if !height.is_finite() || !width.is_finite() || height <= 0.0 || width <= 0.0 {
            return Err("Named view has invalid viewport dimensions.".into());
        }
        let sx = self.last_canvas_size[0] / width;
        let sy = self.last_canvas_size[1] / height;
        self.set_named_view_state(center, sx.min(sy))
    }

    pub(super) fn set_named_view_state(
        &mut self,
        center: [f64; 2],
        scale: f64,
    ) -> Result<(), String> {
        if !scale.is_finite() || scale <= 0.0 || center.iter().any(|value| !value.is_finite()) {
            return Err("Named view has invalid viewport values.".into());
        }
        self.center = center;
        self.scale = scale.clamp(1e-8, 1e6);
        Ok(())
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
                let layout_names = self.layouts.names().map(str::to_owned).collect::<Vec<_>>();
                ui.horizontal(|ui| {
                    ui.label("Layouts:");
                    for name in layout_names {
                        let selected = self.layouts.active().name() == name;
                        if ui.selectable_label(selected, &name).clicked() {
                            match self.editor.activate_layout(&name) {
                                Ok(true) => {
                                    self.layouts = self.editor.layouts().clone();
                                    self.message(format!("Layout {name} active."), false);
                                }
                                Ok(false) => {
                                    self.message(format!("Layout {name} is already active."), false)
                                }
                                Err(error) => self.message(error.to_string(), true),
                            }
                        }
                    }
                });
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                let rect = response.rect;
                self.camera.last_canvas_size = [f64::from(rect.width()), f64::from(rect.height())];
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Other,
                        response.enabled(),
                        "Drawing canvas, XY top view. Use the command field for exact coordinates.",
                    )
                });
                if self.fit_pending {
                    self.fit_pending = false;
                    match self.visible_expanded_geometry() {
                        Ok(geometry) => {
                            if let Err(error) = self.camera.fit(&geometry, rect) {
                                self.message(error, true);
                            }
                        }
                        Err(error) => self.message(error, true),
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
                if (!matches!(
                    self.tool,
                    Tool::Select
                        | Tool::Explode
                        | Tool::Join
                        | Tool::Pedit
                        | Tool::Pan
                        | Tool::Trim
                        | Tool::Extend
                        | Tool::Fillet
                        | Tool::Chamfer
                        | Tool::Break
                        | Tool::BreakAtPoint
                        | Tool::Lengthen
                ) || self.pedit_awaits_point())
                    && let Some(point) = self.cursor
                {
                    self.cursor = Some(self.snap(point, rect, ui.input(|i| i.modifiers.shift)));
                }
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    let mut description = format!("Current tool: {}", self.tool.name());
                    if let Some((vertex, count, point)) = self.pedit_marked_vertex() {
                        description.push_str(&format!(
                            "; PEDIT vertex {} of {count}: {},{},{}",
                            vertex + 1,
                            point.x(),
                            point.y(),
                            point.z()
                        ));
                    }
                    if let Some((start, _, label)) = self.pedit_range_start() {
                        description.push_str(&format!("; {label} start vertex {}", start + 1));
                    }
                    if let Some((_, angle)) = self.polar_guide {
                        description.push_str(&format!("; Polar guide: {angle:.2} degrees"));
                    }
                    node.set_description(description);
                });
                if response.clicked_by(PointerButton::Primary) {
                    response.request_focus();
                    if matches!(self.tool, Tool::Select | Tool::Explode | Tool::Join)
                        || self.pedit_joining()
                    {
                        let hit = pointer.and_then(|p| self.hit(p, rect));
                        if matches!(self.tool, Tool::Explode | Tool::Join) || self.pedit_joining() {
                            if let Some(index) = hit {
                                let mut selection = self.editor.selection().to_vec();
                                if ui.input(|i| i.modifiers.shift) {
                                    selection.retain(|selected| *selected != index);
                                } else {
                                    selection.push(index);
                                }
                                self.retain_join_source(&mut selection);
                                self.retain_pedit_source(&mut selection);
                                self.editor.select(selection);
                            }
                        } else if ui.input(|i| i.modifiers.shift) {
                            if let Some(index) = hit {
                                self.editor.toggle_selection(index);
                            }
                        } else {
                            self.editor.select(hit.into_iter().collect());
                        }
                    } else if self.tool == Tool::Pedit {
                        let result = if self.pedit_awaits_point() {
                            self.cursor
                                .ok_or_else(|| "Choose a finite XY point.".to_string())
                                .and_then(|point| self.pedit_apply_vertex_point(point))
                        } else if self.pedit_vertex_active() {
                            Ok(())
                        } else {
                            pointer
                                .and_then(|position| self.hit(position, rect))
                                .ok_or_else(|| "Select an XY polyline, line or arc.".to_string())
                                .and_then(|index| self.pedit_select(index))
                        };
                        if let Err(error) = result {
                            self.message(error, true);
                        }
                    } else if self.tool == Tool::Lengthen {
                        let hit = pointer.and_then(|position| self.hit(position, rect));
                        if let Some(point) = self.cursor
                            && let Err(error) = self.lengthen_click(hit, point)
                        {
                            self.message(error, true);
                        }
                    } else if matches!(self.tool, Tool::Break | Tool::BreakAtPoint) {
                        let hit = pointer.and_then(|position| self.hit(position, rect));
                        if let Some(point) = self.cursor
                            && let Err(error) = self.break_click(hit, point)
                        {
                            self.message(error, true);
                        }
                    } else if matches!(self.tool, Tool::Fillet | Tool::Chamfer) {
                        let hit = pointer.and_then(|position| self.hit(position, rect));
                        if let Some(point) = self.cursor
                            && let Err(error) =
                                self.corner_click(hit, point, ui.input(|i| i.modifiers.shift))
                        {
                            self.message(error, true);
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
                if response.drag_started_by(PointerButton::Primary) && self.lengthen_awaits_source()
                {
                    response.request_focus();
                    if let Some(position) = ui.input(|i| i.pointer.press_origin())
                        && let Some(point) = self.camera.world(position, rect, self.elevation)
                    {
                        let hit = self.hit(position, rect);
                        if let Err(error) = self.lengthen_click(hit, point) {
                            self.message(error, true);
                        }
                    }
                }
                if response.drag_stopped_by(PointerButton::Primary)
                    && self.lengthen_awaits_point()
                    && let Some(point) = self.cursor
                    && let Err(error) = self.lengthen_point(point)
                {
                    self.message(error, true);
                }
                if response.drag_started_by(PointerButton::Primary)
                    && (matches!(self.tool, Tool::Select | Tool::Explode | Tool::Join)
                        || self.pedit_joining())
                {
                    response.request_focus();
                    self.marquee = ui.input(|i| i.pointer.press_origin());
                }
                if response.drag_stopped_by(PointerButton::Primary)
                    && let Some(start) = self.marquee.take()
                    && let Some(end) = ui.input(|i| i.pointer.latest_pos())
                {
                    let window = Rect::from_two_pos(start, end);
                    let additive =
                        matches!(self.tool, Tool::Explode | Tool::Join) || self.pedit_joining();
                    let removing = additive && ui.input(|i| i.modifiers.shift);
                    let mut selection = if additive || ui.input(|i| i.modifiers.shift) {
                        self.editor.selection().to_vec()
                    } else {
                        Vec::new()
                    };
                    for (index, entity) in self.editor.entities().iter().enumerate() {
                        if !self.editor.is_entity_visible(index)
                            || !self.entity_in_active_space(index)
                        {
                            continue;
                        }
                        if let Ok(expanded) =
                            crate::block_expansion::expand_entity(self.editor.document(), entity)
                            && let Ok([x1, y1, x2, y2]) = crate::svg::entity_bounds(&expanded)
                            && let (Some(a), Some(b)) = (
                                self.camera.screen(x1, y1, rect),
                                self.camera.screen(x2, y2, rect),
                            )
                            && window.contains(a)
                            && window.contains(b)
                        {
                            if removing {
                                selection.retain(|selected| *selected != index);
                            } else {
                                selection.push(index);
                            }
                        }
                    }
                    self.retain_join_source(&mut selection);
                    self.retain_pedit_source(&mut selection);
                    self.editor.select(selection);
                }
                if response.double_clicked() && self.tool == Tool::Select {
                    self.fit_pending = true;
                }
                if self.grid {
                    self.draw_grid(&painter, rect);
                }
                if self.layouts.active().name() != "Model" {
                    let [paper_w, paper_h] = self.layouts.active().setup().dimensions();
                    let paper = self
                        .paper_rect([0.0, 0.0], [paper_w, paper_h], rect)
                        .unwrap_or(rect);
                    painter.rect_filled(paper, 0.0, Color32::from_gray(225));
                    painter.rect_stroke(
                        paper,
                        0.0,
                        Stroke::new(1.0, Color32::from_gray(80)),
                        egui::StrokeKind::Inside,
                    );
                    for viewport in self.layouts.active().viewports() {
                        let p = viewport.paper();
                        let viewport_rect = self
                            .paper_rect([p[0], p[1]], [p[2], p[3]], rect)
                            .unwrap_or(rect);
                        painter.rect_stroke(
                            viewport_rect,
                            0.0,
                            Stroke::new(1.0, ACCENT),
                            egui::StrokeKind::Inside,
                        );
                        if viewport.clip_enabled()
                            && let Some(boundary) = viewport.clip_boundary()
                        {
                            for pair in boundary
                                .iter()
                                .zip(boundary.iter().cycle().skip(1).take(boundary.len()))
                            {
                                if let (Some(a), Some(b)) = (
                                    self.camera.screen_point(*pair.0, rect),
                                    self.camera.screen_point(*pair.1, rect),
                                ) {
                                    painter.line_segment(
                                        [a, b],
                                        Stroke::new(2.0, Color32::from_rgb(210, 90, 90)),
                                    );
                                }
                            }
                        }
                        self.draw_model_viewport(&painter, viewport, viewport_rect, rect);
                    }
                }
                let break_preview_source = self.break_preview_source();
                let pedit_range_source = self.pedit_range_preview_source();
                let lengthen_preview = if self.tool == Tool::Lengthen {
                    self.cursor.and_then(|point| {
                        let hit = self
                            .camera
                            .screen(point.x(), point.y(), rect)
                            .and_then(|position| self.hit(position, rect));
                        self.lengthen_preview(hit, point)
                    })
                } else {
                    None
                };
                let mut block_render_error = None;
                for (index, entity) in self.editor.entities().iter().enumerate() {
                    if !self.editor.is_entity_visible(index)
                        || !self.entity_in_active_space(index)
                        || break_preview_source == Some(index)
                        || pedit_range_source == Some(index)
                        || lengthen_preview
                            .as_ref()
                            .is_some_and(|(source, _)| *source == index)
                    {
                        continue;
                    }
                    if requires_block_expansion(entity)
                        && let Err(error) =
                            crate::block_expansion::expand_entity(self.editor.document(), entity)
                    {
                        block_render_error.get_or_insert_with(|| error.to_string());
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
                                    .entity_rgb(index, self.drawing_on_white_background())
                                    .expect("entity style exists");
                                Color32::from_rgb(r, g, b)
                            },
                        ),
                    );
                    if selected {
                        let grips = match entity {
                            Entity::Line(_) | Entity::Insert(_) => usize::MAX,
                            Entity::Polyline(polyline) => polyline.vertices().len(),
                            _ => 1,
                        };
                        let geometry =
                            crate::block_expansion::expand_entity(self.editor.document(), entity)
                                .unwrap_or_else(|_| vec![entity.clone()]);
                        for point in geometry.iter().flat_map(entity_points).take(grips) {
                            if let Some(p) = self.camera.screen_point(point, rect)
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
                if let Some(error) = block_render_error {
                    self.message(format!("Cannot evaluate block reference: {error}"), true);
                }
                self.draw_polar_guide(&painter, rect);
                self.draw_preview(&painter, rect);
                if let Some((start, point, _)) = self.pedit_range_start()
                    && let Some(position) = self.camera.screen_point(point, rect)
                    && rect.contains(position)
                {
                    painter.circle_stroke(position, 8.0, Stroke::new(1.5, ACCENT));
                    painter.text(
                        position + egui::vec2(-10.0, 10.0),
                        egui::Align2::RIGHT_TOP,
                        format!("Start {}", start + 1),
                        egui::FontId::proportional(11.0),
                        ACCENT,
                    );
                }
                if let Some((vertex, _, point)) = self.pedit_marked_vertex()
                    && let Some(position) = self.camera.screen_point(point, rect)
                    && rect.contains(position)
                {
                    let stroke = Stroke::new(2.0, ACCENT);
                    painter.line_segment(
                        [
                            position - egui::vec2(6.0, 6.0),
                            position + egui::vec2(6.0, 6.0),
                        ],
                        stroke,
                    );
                    painter.line_segment(
                        [
                            position - egui::vec2(6.0, -6.0),
                            position + egui::vec2(6.0, -6.0),
                        ],
                        stroke,
                    );
                    painter.text(
                        position + egui::vec2(10.0, -10.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("Vertex {}", vertex + 1),
                        egui::FontId::proportional(11.0),
                        ACCENT,
                    );
                }
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
                    && let Some(p) = self.camera.screen_point(point, rect)
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
                    if let Some(at) = self.camera.screen_point(point, rect) {
                        painter.circle_filled(at, (stroke.width / 2.0).max(0.75), stroke.color);
                    }
                }
            }
        }
    }

    fn paper_rect(&self, first: [f64; 2], second: [f64; 2], rect: Rect) -> Option<Rect> {
        let a = self.camera.screen(first[0], first[1], rect)?;
        let b = self.camera.screen(second[0], second[1], rect)?;
        Some(Rect::from_min_max(
            egui::pos2(a.x.min(b.x), a.y.min(b.y)),
            egui::pos2(a.x.max(b.x), a.y.max(b.y)),
        ))
    }

    fn drawing_on_white_background(&self) -> bool {
        !matches!(self.editor.active_space(), crate::EntitySpace::Model)
    }

    fn draw_model_viewport(
        &self,
        painter: &Painter,
        viewport: &crate::Viewport,
        viewport_rect: Rect,
        rect: Rect,
    ) {
        let Ok(transform) = super::feature_spaces::viewport_transform(
            viewport.world(),
            viewport.paper(),
            viewport.scale(),
            false,
        ) else {
            return;
        };
        let clipped = painter.with_clip_rect(viewport_rect);
        for (index, entity) in self.editor.entities().iter().enumerate() {
            if !self
                .editor
                .layer_for_entity(index)
                .is_some_and(crate::Layer::is_visible)
                || !matches!(
                    self.editor.entity_space(index),
                    Some(crate::EntitySpace::Model)
                )
            {
                continue;
            }
            let Ok(expanded) =
                crate::block_expansion::expand_entity(self.editor.document(), entity)
            else {
                continue;
            };
            for child in expanded {
                let Ok(mapped) = child.transformed(transform) else {
                    continue;
                };
                let color = self
                    .editor
                    .document()
                    .entity_rgb(index, true)
                    .expect("entity style exists");
                self.draw_styled_entity(
                    &clipped,
                    &mapped,
                    index,
                    rect,
                    Stroke::new(1.0, Color32::from_rgb(color[0], color[1], color[2])),
                );
            }
        }
    }

    fn visible_expanded_geometry(&self) -> Result<Vec<Entity>, String> {
        let mut geometry = Vec::new();
        for (index, entity) in self.editor.entities().iter().enumerate() {
            if !self.editor.is_entity_visible(index) || !self.entity_in_active_space(index) {
                continue;
            }
            let expanded = crate::block_expansion::expand_entity(self.editor.document(), entity)
                .map_err(|error| format!("Cannot evaluate block references: {error}"))?;
            geometry.extend(expanded);
        }
        Ok(geometry)
    }

    fn draw_entity(&self, painter: &Painter, entity: &Entity, rect: Rect, stroke: Stroke) {
        if let Entity::Insert(insert) = entity {
            self.draw_insert_marker(painter, insert, rect, stroke);
        }
        match crate::block_expansion::expand_entity(self.editor.document(), entity) {
            Ok(expanded) => {
                for child in expanded {
                    self.draw_expanded_entity(painter, &child, rect, stroke);
                }
            }
            Err(_) if !matches!(entity, Entity::Insert(_)) => {
                self.draw_expanded_entity(painter, entity, rect, stroke);
            }
            Err(_) => {}
        }
    }

    fn draw_expanded_entity(&self, painter: &Painter, entity: &Entity, rect: Rect, stroke: Stroke) {
        match entity {
            Entity::Line(line) => {
                if let (Some(a), Some(b)) = (
                    self.camera.screen_point(line.start(), rect),
                    self.camera.screen_point(line.end(), rect),
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
                    && let Some(center) = self.camera.screen_point(circle.center(), rect)
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
                        self.camera.screen_point(point, rect)
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
                if let Ok(quads) = polyline.width_quads_xy() {
                    let mut mesh = egui::Mesh::default();
                    for quad in quads {
                        let points: Option<Vec<_>> = quad
                            .iter()
                            .map(|point| self.camera.screen_point(*point, rect))
                            .collect();
                        if let Some(points) = points {
                            let base = mesh.vertices.len() as u32;
                            for point in points {
                                mesh.colored_vertex(point, stroke.color);
                            }
                            mesh.add_triangle(base, base + 1, base + 2);
                            mesh.add_triangle(base, base + 2, base + 3);
                        }
                    }
                    painter.add(egui::Shape::mesh(mesh));
                }
                for (segment, width) in polyline.segments().zip(polyline.widths()) {
                    if width.start() == 0.0
                        && width.end() == 0.0
                        && let Ok(segment) = segment
                    {
                        self.draw_entity(painter, &segment.geometry(), rect, stroke);
                    }
                }
            }
            Entity::Polyline3(polyline) => {
                let points: Option<Vec<_>> = polyline
                    .vertices()
                    .iter()
                    .copied()
                    .map(|point| self.camera.screen_point(point, rect))
                    .collect();
                if let Some(points) = points {
                    if polyline.is_closed() {
                        painter.add(egui::Shape::closed_line(points, stroke));
                    } else {
                        painter.add(egui::Shape::line(points, stroke));
                    }
                }
            }
            Entity::Point(point) => {
                if let Some(at) = self.camera.screen_point(point.position(), rect) {
                    painter.circle_filled(at, 2.0, stroke.color);
                }
            }
            Entity::Ellipse(ellipse) => {
                if let Ok(points) = ellipse
                    .tessellate_for_svg()
                    .iter()
                    .copied()
                    .map(|p| self.camera.screen_point(p, rect).ok_or(()))
                    .collect::<Result<Vec<_>, _>>()
                {
                    painter.add(egui::Shape::line(points, stroke));
                }
            }
            Entity::Spline(spline) => {
                if let Ok(points) = spline.tessellate(64).and_then(|ps| {
                    ps.into_iter()
                        .map(|p| {
                            self.camera
                                .screen_point(p, rect)
                                .ok_or(crate::GeometryError::NonFiniteResult)
                        })
                        .collect()
                }) {
                    painter.add(egui::Shape::line(points, stroke));
                }
            }
            Entity::Leader(leader) => {
                for segment in leader.segments() {
                    if let (Some(a), Some(b)) = (
                        self.camera.screen_point(segment.start(), rect),
                        self.camera.screen_point(segment.end(), rect),
                    ) {
                        painter.line_segment([a, b], stroke);
                    }
                }
            }
            Entity::MLeader(leader) => {
                self.draw_mleader_geometry(painter, leader, rect, stroke);
                match leader.content() {
                    crate::MLeaderContent::MText(text) => {
                        self.draw_text(painter, text.text(), rect, stroke)
                    }
                    crate::MLeaderContent::Block(insert) => {
                        self.draw_insert_marker(painter, insert, rect, stroke)
                    }
                }
            }
            Entity::Table(table) => {
                let display_cells = table.display_cells();
                if let Ok(lines) = table.grid_lines() {
                    for line in lines {
                        if let (Some(a), Some(b)) = (
                            self.camera.screen_point(line.start(), rect),
                            self.camera.screen_point(line.end(), rect),
                        ) {
                            painter.line_segment([a, b], stroke);
                        }
                    }
                }
                let anchor = table.anchor();
                let mut y = anchor.y();
                for (row, height) in table.row_heights().iter().enumerate() {
                    let mut x = anchor.x();
                    for (column, width) in table.column_widths().iter().enumerate() {
                        if let Some(value) =
                            display_cells.get(row).and_then(|cells| cells.get(column))
                            && let Some(at) =
                                self.camera.screen(x + width / 2.0, y + height / 2.0, rect)
                        {
                            painter.text(
                                at,
                                Align2::CENTER_CENTER,
                                value,
                                FontId::proportional(
                                    (height * self.camera.scale * 0.55).clamp(7.0, 32.0) as f32,
                                ),
                                stroke.color,
                            );
                        }
                        x += width;
                    }
                    y += height;
                }
            }
            Entity::Hatch(hatch) => {
                if let Ok(primitives) = hatch.primitives() {
                    for primitive in primitives {
                        match primitive {
                            crate::HatchPrimitive::Line(line) => {
                                if let (Some(a), Some(b)) = (
                                    self.camera.screen_point(line.start(), rect),
                                    self.camera.screen_point(line.end(), rect),
                                ) {
                                    painter.line_segment([a, b], stroke);
                                }
                            }
                            crate::HatchPrimitive::Polygon(polygon) => {
                                let points: Option<Vec<_>> = polygon
                                    .iter()
                                    .map(|p| self.camera.screen_point(*p, rect))
                                    .collect();
                                if let Some(points) = points {
                                    painter.add(egui::Shape::convex_polygon(
                                        points,
                                        stroke.color.gamma_multiply(0.18),
                                        stroke,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            Entity::Text(text) => self.draw_text(painter, text, rect, stroke),
            Entity::MText(text) => self.draw_text(painter, text.text(), rect, stroke),
            Entity::Dimension(dimension) => {
                if let Ok(primitives) = dimension.geometry() {
                    for primitive in primitives {
                        match primitive {
                            crate::DimensionPrimitive::Line(line) => {
                                self.draw_entity(painter, &Entity::Line(line), rect, stroke)
                            }
                            crate::DimensionPrimitive::Arc(arc) => {
                                self.draw_entity(painter, &Entity::Arc(arc), rect, stroke)
                            }
                            crate::DimensionPrimitive::Text(text) => {
                                self.draw_text(painter, &text, rect, stroke)
                            }
                        }
                    }
                }
            }
            Entity::Insert(insert) => {
                self.draw_insert_marker(painter, insert, rect, stroke);
            }
            Entity::Face3(face) => {
                let (vertices, count) = face.vertices();
                for i in 0..count {
                    let a = vertices[i];
                    let b = vertices[(i + 1) % count];
                    if let (Some(pa), Some(pb)) = (
                        self.camera.screen_point(a, rect),
                        self.camera.screen_point(b, rect),
                    ) {
                        painter.line_segment([pa, pb], stroke);
                    }
                }
            }
            Entity::Mesh(mesh) => {
                for face in mesh.faces() {
                    let (vertices, count) = face.vertices();
                    let points: Option<Vec<_>> = vertices[..count]
                        .iter()
                        .copied()
                        .map(|point| self.camera.screen_point(point, rect))
                        .collect();
                    if let Some(points) = points {
                        painter.add(egui::Shape::closed_line(points, stroke));
                    }
                }
            }
            Entity::XLine(line) => {
                let corners = [
                    self.camera.world(rect.left_top(), rect, 0.0),
                    self.camera.world(rect.right_bottom(), rect, 0.0),
                ];
                if let (Some(min), Some(max)) = (corners[0], corners[1])
                    && let Ok(Some((a, b))) = line.clip_to_bounds(
                        Point3::new(min.x().min(max.x()), min.y().min(max.y()), -1.0e9)
                            .unwrap_or(min),
                        Point3::new(min.x().max(max.x()), min.y().max(max.y()), 1.0e9)
                            .unwrap_or(max),
                    )
                    && let (Ok(pa), Ok(pb)) = (line.point_at(a), line.point_at(b))
                    && let (Some(pa), Some(pb)) = (
                        self.camera.screen_point(pa, rect),
                        self.camera.screen_point(pb, rect),
                    )
                {
                    painter.line_segment([pa, pb], stroke);
                }
            }
            Entity::Ray(ray) => {
                let corners = [
                    self.camera.world(rect.left_top(), rect, 0.0),
                    self.camera.world(rect.right_bottom(), rect, 0.0),
                ];
                if let (Some(min), Some(max)) = (corners[0], corners[1])
                    && let Ok(Some((_, b))) = ray.clip_to_bounds(
                        Point3::new(min.x().min(max.x()), min.y().min(max.y()), -1.0e9)
                            .unwrap_or(min),
                        Point3::new(min.x().max(max.x()), min.y().max(max.y()), 1.0e9)
                            .unwrap_or(max),
                    )
                    && let (Ok(pa), Ok(pb)) = (ray.point_at(0.0), ray.point_at(b))
                    && let (Some(pa), Some(pb)) = (
                        self.camera.screen_point(pa, rect),
                        self.camera.screen_point(pb, rect),
                    )
                {
                    painter.line_segment([pa, pb], stroke);
                }
            }
            Entity::Image(image) => self.draw_image(painter, image, rect, stroke),
            Entity::PdfUnderlay(underlay) => {
                self.draw_pdf_underlay(painter, underlay, rect, stroke)
            }
            Entity::MLine(mline) => {
                if let Ok(paths) = mline.explode() {
                    for path in paths {
                        let points: Option<Vec<_>> = path
                            .into_iter()
                            .map(|point| self.camera.screen_point(point, rect))
                            .collect();
                        if let Some(points) = points {
                            painter.add(egui::Shape::line(points, stroke));
                        }
                    }
                }
                if let Ok(caps) = mline.cap_lines() {
                    for line in caps {
                        if let (Some(a), Some(b)) = (
                            self.camera.screen_point(line.start(), rect),
                            self.camera.screen_point(line.end(), rect),
                        ) {
                            painter.line_segment([a, b], stroke);
                        }
                    }
                }
            }
        }
    }

    fn draw_image(
        &self,
        painter: &Painter,
        image: &crate::RasterImage,
        rect: Rect,
        stroke: Stroke,
    ) {
        if !image.visible() {
            return;
        }
        let image_path = if image.path().is_relative() {
            self.path
                .as_deref()
                .and_then(std::path::Path::parent)
                .map_or_else(|| image.path().to_owned(), |base| base.join(image.path()))
        } else {
            image.path().to_owned()
        };
        let Ok(decoded) = image::open(&image_path) else {
            if let Some(at) = self.camera.screen_point(image.insertion(), rect) {
                painter.text(
                    at,
                    Align2::LEFT_TOP,
                    "[image unavailable]",
                    FontId::proportional(10.0),
                    stroke.color,
                );
            }
            return;
        };
        let rgba = decoded.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let texture = painter.ctx().load_texture(
            image_path.to_string_lossy().to_string(),
            color,
            egui::TextureOptions::LINEAR,
        );
        let Ok(triangles) = image.clipped_triangles() else {
            return;
        };
        let mut mesh = egui::Mesh::with_texture(texture.id());
        let tint = Color32::WHITE.gamma_multiply(image.opacity() as f32);
        for triangle in triangles {
            let base = mesh.vertices.len() as u32;
            for (position, uv) in triangle.positions.into_iter().zip(triangle.uvs) {
                if let Some(pos) = self.camera.screen_point(position, rect) {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos,
                        uv: egui::pos2(uv[0] as f32, uv[1] as f32),
                        color: tint,
                    });
                }
            }
            if mesh.vertices.len() as u32 >= base + 3 {
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        }
        if !mesh.vertices.is_empty() {
            painter.add(egui::Shape::mesh(mesh));
        }
        if let Some(boundary) = image.clip_boundary() {
            let points = boundary
                .iter()
                .filter_map(|p| self.camera.screen_point(*p, rect))
                .collect::<Vec<_>>();
            if points.len() > 2 {
                let mut closed = points.clone();
                closed.push(points[0]);
                painter.add(egui::Shape::line(closed, stroke));
            }
        }
    }

    fn draw_pdf_underlay(
        &self,
        painter: &Painter,
        underlay: &crate::PdfUnderlay,
        rect: Rect,
        stroke: Stroke,
    ) {
        if !underlay.display_enabled() {
            return;
        }
        let texture = match self.pdf_underlay_texture(painter, underlay) {
            Ok(texture) => texture,
            Err(message) => {
                self.draw_pdf_error(painter, underlay, rect, stroke, message);
                return;
            }
        };
        if !underlay.page_size_known() {
            self.draw_pdf_error(
                painter,
                underlay,
                rect,
                stroke,
                "[PDF page dimensions unresolved]",
            );
            return;
        }
        let polygon = match underlay.clipped_polygon() {
            Ok(polygon) => polygon,
            Err(error) => {
                self.draw_pdf_error(
                    painter,
                    underlay,
                    rect,
                    stroke,
                    &format!("[PDF geometry: {error}]"),
                );
                return;
            }
        };
        if polygon.len() < 3 {
            return;
        }
        let (u_vector, v_vector) = match underlay_plot_vectors(underlay) {
            Ok(vectors) => vectors,
            Err(_) => {
                self.draw_pdf_error(painter, underlay, rect, stroke, "[PDF placement invalid]");
                return;
            }
        };
        let clip = underlay.clip_enabled().then_some(polygon.as_slice());
        let Ok(triangles) = crate::raster_geometry::clipped_triangles(
            underlay.insertion(),
            u_vector,
            v_vector,
            clip,
        ) else {
            self.draw_pdf_error(painter, underlay, rect, stroke, "[PDF clip unsupported]");
            return;
        };
        let mut mesh = egui::Mesh::with_texture(texture.id());
        for triangle in triangles {
            let points = triangle.positions;
            let base = mesh.vertices.len() as u32;
            for (point, uv) in points.into_iter().zip(triangle.uvs) {
                if let Some(position) = self.camera.screen_point(point, rect) {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: position,
                        uv: egui::pos2(uv[0] as f32, 1.0 - uv[1] as f32),
                        color: Color32::WHITE,
                    });
                }
            }
            if mesh.vertices.len() as u32 >= base + 3 {
                mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
            }
        }
        if !mesh.vertices.is_empty() {
            painter.add(egui::Shape::mesh(mesh));
        }
        if underlay.frame_mode() != 0 {
            let points: Vec<_> = polygon
                .iter()
                .filter_map(|point| self.camera.screen_point(*point, rect))
                .collect();
            if points.len() > 2 {
                let mut closed = points.clone();
                closed.push(points[0]);
                painter.add(egui::Shape::line(closed, stroke));
            }
        }
    }

    fn pdf_underlay_texture(
        &self,
        painter: &Painter,
        underlay: &crate::PdfUnderlay,
    ) -> Result<egui::TextureHandle, &'static str> {
        let path = if underlay.path().is_relative() {
            self.path
                .as_deref()
                .and_then(std::path::Path::parent)
                .map_or_else(
                    || underlay.path().to_owned(),
                    |base| base.join(underlay.path()),
                )
        } else {
            underlay.path().to_owned()
        };
        let Ok(metadata) = std::fs::metadata(&path) else {
            return Err("[PDF underlay unavailable]");
        };
        if metadata.len() > 128 * 1024 * 1024 {
            return Err("[PDF underlay too large]");
        }
        let Ok(data) = std::fs::read(&path) else {
            return Err("[PDF underlay unavailable]");
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |value| value.as_nanos());
        let key = format!(
            "{}:{}:{}:{}:{}:{}:{}:{}",
            path.display(),
            modified,
            data.len(),
            underlay.page(),
            underlay.fade().to_bits(),
            underlay.contrast().to_bits(),
            underlay.monochrome(),
            underlay.display_enabled(),
        );
        if let Some(texture) = self
            .camera
            .pdf_cache
            .borrow()
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.texture.clone())
        {
            return Ok(texture);
        }

        let Ok(mut preview) = crate::pdf_render::render_pdf_page(&data, underlay.page() as usize)
        else {
            return Err("[PDF page unavailable]");
        };
        if crate::pdf_render::adjust_preview(
            &mut preview,
            underlay.fade(),
            underlay.contrast(),
            underlay.monochrome(),
        )
        .is_err()
        {
            return Err("[PDF display settings invalid]");
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [preview.width as usize, preview.height as usize],
            &preview.pixels,
        );
        let texture = painter.ctx().load_texture(
            format!("pdf-underlay:{}", key),
            image,
            egui::TextureOptions::LINEAR,
        );
        let mut cache = self.camera.pdf_cache.borrow_mut();
        cache.push(PdfTextureEntry {
            key,
            texture: texture.clone(),
        });
        if cache.len() > 8 {
            cache.remove(0);
        }
        Ok(texture)
    }

    fn draw_pdf_error(
        &self,
        painter: &Painter,
        underlay: &crate::PdfUnderlay,
        rect: Rect,
        stroke: Stroke,
        message: &str,
    ) {
        if let Some(point) = self.camera.screen_point(underlay.insertion(), rect) {
            painter.text(
                point,
                Align2::LEFT_TOP,
                message,
                FontId::proportional(10.0),
                stroke.color,
            );
        }
    }

    fn draw_mleader_geometry(
        &self,
        painter: &Painter,
        leader: &crate::MLeader,
        rect: Rect,
        stroke: Stroke,
    ) {
        let lines = leader.geometry().unwrap_or_else(|_| leader.segments());
        for line in lines {
            if let (Some(a), Some(b)) = (
                self.camera.screen_point(line.start(), rect),
                self.camera.screen_point(line.end(), rect),
            ) {
                painter.line_segment([a, b], stroke);
            }
        }
    }

    fn draw_insert_marker(
        &self,
        painter: &Painter,
        insert: &crate::Insert,
        rect: Rect,
        stroke: Stroke,
    ) {
        let Some(at) = self.camera.screen_point(insert.insertion(), rect) else {
            return;
        };
        painter.circle_stroke(at, 4.0, stroke);
        painter.line_segment(
            [at + egui::vec2(-6.0, 0.0), at + egui::vec2(6.0, 0.0)],
            stroke,
        );
        painter.line_segment(
            [at + egui::vec2(0.0, -6.0), at + egui::vec2(0.0, 6.0)],
            stroke,
        );
        painter.text(
            at + egui::vec2(7.0, -7.0),
            Align2::LEFT_TOP,
            insert.block_name(),
            FontId::proportional(11.0),
            stroke.color,
        );
    }

    fn draw_text(&self, painter: &Painter, text: &crate::Text, rect: Rect, stroke: Stroke) {
        if let Some(at) = self.camera.screen_point(text.location(), rect) {
            let size = (text.height() * self.camera.scale).clamp(7.0, 96.0) as f32;
            painter.text(
                at,
                Align2::LEFT_TOP,
                text.content(),
                FontId::proportional(size),
                stroke.color,
            );
        }
    }

    fn draw_preview(&self, painter: &Painter, rect: Rect) {
        let stroke = Stroke::new(1.3, ACCENT);
        if self.pedit_range_start().is_some() {
            if let Ok(polylines) = self.pedit_range_preview() {
                for polyline in polylines {
                    self.draw_entity(painter, &Entity::Polyline(polyline), rect, stroke);
                }
            }
            return;
        }
        let Some(cursor) = self.cursor else {
            return;
        };
        if self.pedit_awaits_point() {
            if let Ok(polyline) = self.pedit_vertex_preview(cursor) {
                self.draw_entity(painter, &Entity::Polyline(polyline), rect, stroke);
            }
            return;
        }
        if self.tool == Tool::Lengthen {
            let hit = self
                .camera
                .screen(cursor.x(), cursor.y(), rect)
                .and_then(|position| self.hit(position, rect));
            if let Some((_, entity)) = self.lengthen_preview(hit, cursor) {
                self.draw_entity(painter, &entity, rect, stroke);
            }
            return;
        }
        if matches!(self.tool, Tool::Break | Tool::BreakAtPoint) {
            if let Some(parts) = self.break_preview(cursor) {
                for part in parts {
                    self.draw_entity(painter, &part, rect, stroke);
                    let ends = match part {
                        Entity::Line(line) => Some([line.start(), line.end()]),
                        Entity::Arc(arc) => arc
                            .start_point()
                            .ok()
                            .zip(arc.end_point().ok())
                            .map(|(start, end)| [start, end]),
                        _ => None,
                    };
                    if let Some(ends) = ends {
                        for end in ends {
                            if let Some(position) = self.camera.screen_point(end, rect) {
                                painter.rect_filled(
                                    Rect::from_center_size(position, egui::vec2(4.0, 4.0)),
                                    0.0,
                                    ACCENT,
                                );
                            }
                        }
                    }
                }
            }
            return;
        }
        if matches!(self.tool, Tool::Fillet | Tool::Chamfer) {
            if let Some(position) = self.camera.screen_point(cursor, rect)
                && let Some(source) = self.hit(position, rect)
                && let Some(parts) =
                    self.corner_preview(source, cursor, painter.ctx().input(|i| i.modifiers.shift))
            {
                for part in parts {
                    self.draw_entity(painter, &part, rect, stroke);
                }
            }
            return;
        }
        if self.tool == Tool::Offset {
            if let Some(entity) = self.offset_preview(cursor) {
                self.draw_entity(painter, &entity, rect, stroke);
            }
            return;
        }
        if matches!(self.tool, Tool::Trim | Tool::Extend) {
            if let Some(position) = self.camera.screen_point(cursor, rect)
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
                if let Ok(polyline) = self
                    .pline_candidate(Some(cursor), false)
                    .or_else(|_| self.pline_candidate(None, false))
                {
                    self.draw_entity(painter, &Entity::Polyline(polyline), rect, stroke);
                }
                if let Some(guide) = self.pline_arc_guide(cursor) {
                    self.draw_entity(painter, &Entity::Line(guide), rect, stroke);
                }
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
        if let Some(p) = self.camera.screen_point(cursor, rect) {
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
                if !self.editor.is_entity_visible(index) || !self.entity_in_active_space(index) {
                    continue;
                }
                if let Ok(geometry) =
                    crate::block_expansion::expand_entity(self.editor.document(), entity)
                {
                    for candidate in geometry.iter().flat_map(entity_points) {
                        let d = (candidate.x() - point.x()).hypot(candidate.y() - point.y())
                            * self.camera.scale;
                        if d < distance {
                            distance = d;
                            nearest = Some(candidate);
                        }
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
            self.camera.screen_point(base, rect),
            self.camera.screen_point(cursor, rect),
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
                self.camera.screen_point(line.start(), rect),
                self.camera.screen_point(line.end(), rect),
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
            let start = self.camera.screen_point(start, rect)?;
            let end = self.camera.screen_point(end, rect)?;
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
            if !self.editor.is_entity_visible(index) || !self.entity_in_active_space(index) {
                continue;
            }
            let d = match entity {
                Entity::Line(line) => line_distance(*line).unwrap_or(f32::INFINITY),
                Entity::Polyline(polyline) => {
                    let mut nearest = polyline
                        .segments()
                        .filter_map(|segment| match segment.ok()? {
                            crate::PolylineSegment::Line(line) => line_distance(line),
                            crate::PolylineSegment::Arc { arc, .. } => arc_distance(arc),
                        })
                        .fold(f32::INFINITY, f32::min);
                    if let Ok(quads) = polyline.width_quads_xy() {
                        for quad in quads {
                            let points: Option<Vec<_>> = quad
                                .iter()
                                .map(|point| self.camera.screen_point(*point, rect))
                                .collect();
                            if let Some(points) = points {
                                for indices in [[0, 1, 2], [0, 2, 3]] {
                                    let [a, b, c] = indices.map(|index| points[index]);
                                    if point_in_triangle(position, a, b, c) {
                                        nearest = 0.0;
                                    }
                                }
                            }
                        }
                    }
                    nearest
                }
                Entity::Polyline3(polyline) => polyline
                    .segments()
                    .filter_map(&line_distance)
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
                Entity::MLeader(leader) => leader
                    .geometry()
                    .unwrap_or_else(|_| leader.segments())
                    .into_iter()
                    .filter_map(&line_distance)
                    .chain(match leader.content() {
                        crate::MLeaderContent::MText(text) => text
                            .bounds()
                            .ok()
                            .and_then(|bounds| self.camera.screen_point(bounds.min, rect))
                            .into_iter()
                            .map(|point| position.distance(point))
                            .collect::<Vec<_>>(),
                        crate::MLeaderContent::Block(insert) => self
                            .camera
                            .screen_point(insert.insertion(), rect)
                            .into_iter()
                            .map(|point| position.distance(point))
                            .collect::<Vec<_>>(),
                    })
                    .chain(match leader.content() {
                        crate::MLeaderContent::MText(_) => Vec::new(),
                        crate::MLeaderContent::Block(insert) => {
                            vec![self.expanded_entity_distance(
                                &Entity::Insert(insert.clone()),
                                position,
                                rect,
                                &line_distance,
                                &arc_distance,
                            )]
                        }
                    })
                    .fold(f32::INFINITY, f32::min),
                Entity::Insert(_) => self.expanded_entity_distance(
                    entity,
                    position,
                    rect,
                    &line_distance,
                    &arc_distance,
                ),
                Entity::Point(point) => self
                    .camera
                    .screen_point(point.position(), rect)
                    .map_or(f32::INFINITY, |p| position.distance(p)),
                Entity::Ellipse(ellipse) => ellipse
                    .tessellate_for_svg()
                    .iter()
                    .filter_map(|p| self.camera.screen_point(*p, rect))
                    .map(|p| position.distance(p))
                    .fold(f32::INFINITY, f32::min),
                Entity::Spline(spline) => spline
                    .tessellate(64)
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| self.camera.screen_point(p, rect))
                    .map(|p| position.distance(p))
                    .fold(f32::INFINITY, f32::min),
                Entity::XLine(line) => line_distance(Line3::new(
                    line.origin(),
                    line.point_at(1_000_000.0).unwrap_or(line.origin()),
                ))
                .unwrap_or(f32::INFINITY),
                Entity::Ray(ray) => line_distance(Line3::new(
                    ray.origin(),
                    ray.point_at(1_000_000.0).unwrap_or(ray.origin()),
                ))
                .unwrap_or(f32::INFINITY),
                Entity::Image(image) => image
                    .bounds()
                    .ok()
                    .and_then(|bounds| {
                        let a = self.camera.screen_point(bounds.min, rect)?;
                        let b = self.camera.screen_point(bounds.max, rect)?;
                        Some(Rect::from_two_pos(a, b).distance_to_pos(position))
                    })
                    .unwrap_or(f32::INFINITY),
                Entity::PdfUnderlay(underlay) => underlay
                    .bounds()
                    .ok()
                    .and_then(|bounds| {
                        let a = self.camera.screen_point(bounds.min, rect)?;
                        let b = self.camera.screen_point(bounds.max, rect)?;
                        Some(Rect::from_two_pos(a, b).distance_to_pos(position))
                    })
                    .unwrap_or(f32::INFINITY),
                Entity::MLine(mline) => mline
                    .explode()
                    .ok()
                    .into_iter()
                    .flatten()
                    .flat_map(|path| path.into_iter())
                    .filter_map(|point| self.camera.screen_point(point, rect))
                    .map(|point| position.distance(point))
                    .fold(f32::INFINITY, f32::min),
                Entity::Mesh(mesh) => mesh
                    .faces()
                    .iter()
                    .flat_map(|face| {
                        let (vertices, count) = face.vertices();
                        vertices[..count].to_vec()
                    })
                    .filter_map(|point| self.camera.screen_point(point, rect))
                    .map(|point| position.distance(point))
                    .fold(f32::INFINITY, f32::min),
                _ => f32::INFINITY,
            };
            if d < distance {
                distance = d;
                closest = Some(index);
            }
        }
        closest
    }

    fn expanded_entity_distance(
        &self,
        entity: &Entity,
        position: Pos2,
        rect: Rect,
        line_distance: &impl Fn(Line3) -> Option<f32>,
        arc_distance: &impl Fn(Arc3) -> Option<f32>,
    ) -> f32 {
        let Ok(entities) = crate::block_expansion::expand_entity(self.editor.document(), entity)
        else {
            return f32::INFINITY;
        };

        entities
            .iter()
            .map(|entity| match entity {
                Entity::Line(line) => line_distance(*line).unwrap_or(f32::INFINITY),
                Entity::Polyline(polyline) => polyline
                    .segments()
                    .filter_map(|segment| match segment.ok()? {
                        crate::PolylineSegment::Line(line) => line_distance(line),
                        crate::PolylineSegment::Arc { arc, .. } => arc_distance(arc),
                    })
                    .fold(f32::INFINITY, f32::min),
                Entity::Polyline3(polyline) => polyline
                    .segments()
                    .filter_map(line_distance)
                    .fold(f32::INFINITY, f32::min),
                Entity::Circle(circle) => self
                    .camera
                    .screen(circle.center().x(), circle.center().y(), rect)
                    .map_or(f32::INFINITY, |center| {
                        (position.distance(center) - (circle.radius() * self.camera.scale) as f32)
                            .abs()
                    }),
                Entity::Arc(arc) => arc_distance(*arc).unwrap_or(f32::INFINITY),
                _ => crate::svg::entity_bounds(std::slice::from_ref(entity))
                    .ok()
                    .and_then(|[x1, y1, x2, y2]| {
                        Some(
                            Rect::from_two_pos(
                                self.camera.screen(x1, y1, rect)?,
                                self.camera.screen(x2, y2, rect)?,
                            )
                            .distance_to_pos(position),
                        )
                    })
                    .unwrap_or(f32::INFINITY),
            })
            .fold(f32::INFINITY, f32::min)
    }
}

fn requires_block_expansion(entity: &Entity) -> bool {
    matches!(entity, Entity::Insert(_))
        || matches!(
            entity,
            Entity::MLeader(leader) if matches!(leader.content(), crate::MLeaderContent::Block(_))
        )
}

fn underlay_plot_vectors(
    underlay: &crate::PdfUnderlay,
) -> Result<(Point3, Point3), crate::GeometryError> {
    let [page_width, page_height] = underlay.resolved_page_size()?;
    let [scale_x, scale_y] = underlay.scale();
    let (sin, cos) = underlay.rotation().sin_cos();
    let u_vector = Point3::new(cos * page_width * scale_x, sin * page_width * scale_x, 0.0)?;
    let v_vector = Point3::new(
        -sin * page_height * scale_y,
        cos * page_height * scale_y,
        0.0,
    )?;
    Ok((u_vector, v_vector))
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

fn point_in_triangle(point: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
    let cross = |u: egui::Vec2, v: egui::Vec2| u.x * v.y - u.y * v.x;
    let area = cross(b - a, c - a);
    if area == 0.0 || !area.is_finite() {
        return false;
    }
    let sides = [
        cross(b - a, point - a),
        cross(c - b, point - b),
        cross(a - c, point - c),
    ];
    if area > 0.0 {
        sides.iter().all(|side| *side >= 0.0)
    } else {
        sides.iter().all(|side| *side <= 0.0)
    }
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
            .centerline_extrema()
            .unwrap_or_else(|_| polyline.vertices().to_vec()),
        Entity::Polyline3(polyline) => polyline.vertices().to_vec(),
        Entity::Point(point) => vec![point.position()],
        Entity::Ellipse(ellipse) => ellipse.tessellate_for_svg(),
        Entity::Spline(spline) => spline.tessellate(64).unwrap_or_default(),
        Entity::Leader(leader) => leader.vertices().to_vec(),
        Entity::MLeader(leader) => leader
            .geometry()
            .unwrap_or_else(|_| leader.segments())
            .into_iter()
            .flat_map(|line| [line.start(), line.end()])
            .collect(),
        Entity::Table(table) => table
            .bounds()
            .map(|b| vec![b.min, b.max])
            .unwrap_or_default(),
        Entity::Dimension(dimension) => dimension
            .bounds()
            .map(|b| vec![b.min, b.max])
            .unwrap_or_default(),
        Entity::Hatch(hatch) => hatch.boundaries().iter().flatten().copied().collect(),
        Entity::Text(text) => text
            .bounds()
            .map(|b| vec![b.min, b.max])
            .unwrap_or_default(),
        Entity::MText(text) => text
            .bounds()
            .map(|b| vec![b.min, b.max])
            .unwrap_or_default(),
        Entity::Insert(insert) => vec![insert.insertion()],
        Entity::Face3(face) => {
            let (vertices, count) = face.vertices();
            vertices[..count].to_vec()
        }
        Entity::XLine(line) => vec![line.origin()],
        Entity::Ray(ray) => vec![ray.origin()],
        Entity::Image(image) => image
            .bounds()
            .map(|b| vec![b.min, b.max])
            .unwrap_or_default(),
        Entity::PdfUnderlay(underlay) => underlay
            .bounds()
            .map(|b| vec![b.min, b.max])
            .unwrap_or_default(),
        Entity::MLine(mline) => mline
            .explode()
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .collect(),
        Entity::Mesh(mesh) => mesh
            .faces()
            .iter()
            .flat_map(|face| {
                let (vertices, count) = face.vertices();
                vertices[..count].to_vec()
            })
            .collect(),
    }
}
