mod canvas;
mod catalog;
mod commands;
mod offsets;
mod panels;
mod preferences;
mod shell;
mod toolsets;
mod transforms;

pub use shell::{MenuItem, MenuStatus, menu_action_status, native_menus, queue_menu_action};

use crate::{
    Arc3, Circle3, DxfDocument, Editor, Entity, EntityStyle, Line3, Linetype, Offset, OffsetLayer,
    OffsetOptions, Point3, Polyline2, Transform,
};
use eframe::egui::{self, Color32, Key, Modifiers};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const ACCENT: Color32 = Color32::from_rgb(126, 194, 232);
const SELECTED: Color32 = Color32::from_rgb(143, 186, 226);
const MUTED: Color32 = Color32::from_rgb(180, 185, 195);
const PANEL: Color32 = Color32::from_rgb(59, 68, 83);
const CANVAS: Color32 = Color32::from_rgb(34, 40, 47);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Line,
    Polyline,
    Circle,
    Arc,
    Rectangle,
    Move,
    Copy,
    Rotate,
    Scale,
    Mirror,
    Offset,
    Pan,
}
impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Line => "Line",
            Self::Polyline => "Polyline",
            Self::Circle => "Circle",
            Self::Arc => "Arc",
            Self::Rectangle => "Rectangle",
            Self::Move => "Move",
            Self::Copy => "Copy",
            Self::Rotate => "Rotate",
            Self::Scale => "Scale",
            Self::Mirror => "Mirror",
            Self::Offset => "Offset",
            Self::Pan => "Pan",
        }
    }
}

enum PendingAction {
    New,
    Open(Option<PathBuf>),
    Study,
    CloseDrawing,
    Close,
}

#[derive(Default)]
struct Inspector {
    entity: Option<(usize, Entity)>,
    fields: Vec<String>,
    style: EntityStyleInput,
}

#[derive(Default)]
struct EntityStyleInput {
    state: Vec<(usize, EntityStyle)>,
    color: String,
    scale: String,
}

#[derive(Default)]
struct LayerColorInput {
    layer: Option<(String, i16)>,
    value: String,
}

#[derive(Default)]
struct LinetypeInput {
    name: String,
    pattern: String,
    scale_state: Option<f64>,
    scale: String,
}

pub struct OpenCadApp {
    editor: Editor,
    path: Option<PathBuf>,
    name: String,
    tool: Tool,
    points: Vec<Point3>,
    transform_input: transforms::TransformInput,
    offset_input: offsets::OffsetInput,
    camera: canvas::Camera,
    fit_pending: bool,
    grid: bool,
    grid_snap: bool,
    object_snap: bool,
    ortho: bool,
    polar_tracking: bool,
    polar_angle: f64,
    polar_guide: Option<(Point3, f64)>,
    grid_step: f64,
    elevation: f64,
    cursor: Option<Point3>,
    snap_label: Option<&'static str>,
    command: String,
    command_focus: bool,
    new_layer: String,
    layer_color: LayerColorInput,
    linetype_input: LinetypeInput,
    history: VecDeque<(String, bool)>,
    status: String,
    status_error: bool,
    inspector: Inspector,
    marquee: Option<egui::Pos2>,
    pending: Option<PendingAction>,
    allow_close: bool,
    show_help: bool,
    workspace: shell::Workspace,
    icons: HashMap<&'static str, egui::TextureHandle>,
    recent_files: Vec<PathBuf>,
    recent_search: String,
    recent_list: bool,
}

impl OpenCadApp {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        cc.egui_ctx.enable_accesskit();
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = PANEL;
        style.visuals.window_fill = PANEL;
        style.visuals.extreme_bg_color = Color32::from_rgb(47, 52, 63);
        style.visuals.selection.bg_fill = Color32::from_rgb(68, 103, 137);
        style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(68, 103, 137);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(80, 89, 109);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(80, 89, 109);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(80, 89, 109);
        style.visuals.widgets.noninteractive.bg_stroke.color = Color32::from_rgb(47, 52, 63);
        style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(217, 217, 217);
        style.visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(217, 217, 217);
        style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
        for widget in [
            &mut style.visuals.widgets.noninteractive,
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widget.corner_radius = egui::CornerRadius::ZERO;
        }
        style.spacing.item_spacing = egui::vec2(5.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 3.0);
        style.spacing.interact_size = egui::vec2(22.0, 20.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(12.0));
        cc.egui_ctx.set_style_of(egui::Theme::Dark, style);
        shell::install_system_font(&cc.egui_ctx);
        let mut app = Self {
            editor: Editor::default(),
            path: None,
            name: "Drawing1".into(),
            tool: Tool::Select,
            points: Vec::new(),
            transform_input: transforms::TransformInput::default(),
            offset_input: offsets::OffsetInput::default(),
            camera: canvas::Camera::default(),
            fit_pending: true,
            grid: false,
            grid_snap: false,
            object_snap: true,
            ortho: false,
            polar_tracking: false,
            polar_angle: 90.0,
            polar_guide: None,
            grid_step: 10.0,
            elevation: 0.0,
            cursor: None,
            snap_label: None,
            command: String::new(),
            command_focus: false,
            new_layer: String::new(),
            layer_color: LayerColorInput::default(),
            linetype_input: LinetypeInput::default(),
            history: VecDeque::new(),
            status: "Create or open a drawing.".into(),
            status_error: false,
            inspector: Inspector::default(),
            marquee: None,
            pending: None,
            allow_close: false,
            show_help: false,
            workspace: shell::Workspace::default(),
            icons: HashMap::new(),
            recent_files: Vec::new(),
            recent_search: String::new(),
            recent_list: false,
        };
        if let Some(storage) = cc.storage {
            app.restore_preferences(storage);
        }
        if let Some(path) = path {
            app.open_path(path);
        }
        app.publish_menu_state(&cc.egui_ctx);
        app
    }

    fn message(&mut self, message: impl Into<String>, error: bool) {
        let message = message.into();
        self.status.clone_from(&message);
        self.status_error = error;
        if error {
            self.workspace.history = true;
            self.workspace.command = true;
        }
        self.history.push_back((message, error));
        while self.history.len() > 80 {
            self.history.pop_front();
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        if matches!(
            tool,
            Tool::Move | Tool::Copy | Tool::Rotate | Tool::Scale | Tool::Mirror
        ) && self.editor.selection().is_empty()
        {
            self.message("Select entities before transforming them.", true);
            return;
        }
        self.tool = tool;
        self.points.clear();
        self.transform_input = transforms::TransformInput::default();
        self.offset_input = offsets::OffsetInput::default();
        self.marquee = None;
        self.message(self.prompt(), false);
    }

    fn prompt(&self) -> &'static str {
        match (self.tool, self.points.len()) {
            (Tool::Select, _) => {
                "Select an entity. Shift adds to selection; drag selects a window."
            }
            (Tool::Line, 0) => "LINE · Specify first point: x,y[,z]",
            (Tool::Line, _) => "LINE · Next point, U to undo, C to close, or Enter to finish.",
            (Tool::Polyline, 0) => "PLINE · Specify first point: x,y[,z]",
            (Tool::Polyline, _) => {
                "PLINE · Next vertex, U to undo, C to close, or Enter to finish."
            }
            (Tool::Circle, 0) => "CIRCLE · Specify center: x,y[,z]",
            (Tool::Circle, _) => "CIRCLE · Specify radius or a point on the circle.",
            (Tool::Arc, 0) => "ARC · Specify center: x,y[,z]",
            (Tool::Arc, 1) => "ARC · Specify start point in the center's XY plane.",
            (Tool::Arc, _) => "ARC · Specify end direction; the arc sweeps counterclockwise.",
            (Tool::Rectangle, 0) => "RECTANG · Specify first corner: x,y[,z]",
            (Tool::Rectangle, _) => "RECTANG · Specify opposite corner.",
            (Tool::Move | Tool::Copy, 0) => {
                "Specify base point, then destination; or enter MOVE/COPY dx dy [dz]."
            }
            (Tool::Move | Tool::Copy, _) => "Specify destination point.",
            (Tool::Rotate | Tool::Scale | Tool::Mirror, _) => self.transform_prompt(),
            (Tool::Offset, _) => self.offset_prompt(),
            (Tool::Pan, _) => "Drag to pan. Scroll to zoom; F fits the drawing.",
        }
    }

    fn finish(&mut self) {
        self.tool = Tool::Select;
        self.points.clear();
        self.transform_input = transforms::TransformInput::default();
        self.offset_input = offsets::OffsetInput::default();
        self.marquee = None;
        self.polar_guide = None;
    }

    fn set_ortho(&mut self, enabled: bool) {
        self.ortho = enabled;
        if enabled {
            self.polar_tracking = false;
        }
        self.polar_guide = None;
    }

    fn set_polar_tracking(&mut self, enabled: bool) {
        self.polar_tracking = enabled;
        if enabled {
            self.ortho = false;
        }
        self.polar_guide = None;
    }

    fn drawing_elevation(&self) -> f64 {
        if matches!(
            self.tool,
            Tool::Polyline | Tool::Rotate | Tool::Scale | Tool::Mirror
        ) {
            self.points
                .first()
                .map_or(self.elevation, |point| point.z())
        } else {
            self.elevation
        }
    }

    fn finish_polyline(&mut self, closed: bool) -> Result<(), String> {
        let polyline =
            Polyline2::new(self.points.clone(), closed).map_err(|error| error.to_string())?;
        self.add_entities(vec![Entity::Polyline(polyline)])?;
        self.finish();
        self.message("Polyline created.", false);
        Ok(())
    }

    fn point(&mut self, point: Point3) -> Result<(), String> {
        match self.tool {
            Tool::Rotate | Tool::Scale | Tool::Mirror => return self.transform_point(point),
            Tool::Offset => return self.offset_point(point),
            Tool::Polyline => {
                if self
                    .points
                    .first()
                    .is_some_and(|first| first.z() != point.z())
                {
                    return Err(crate::GeometryError::NonPlanarPolyline.to_string());
                }
                self.points.push(point);
            }
            Tool::Line => {
                if let Some(previous) = self.points.last().copied() {
                    self.add_entities(vec![Entity::Line(Line3::new(previous, point))])?;
                }
                self.points.push(point);
            }
            Tool::Circle if !self.points.is_empty() => {
                let center = self.points[0];
                let radius = (point.x() - center.x()).hypot(point.y() - center.y());
                self.add_entities(vec![Entity::Circle(
                    Circle3::new(center, radius).map_err(|e| e.to_string())?,
                )])?;
                self.finish();
            }
            Tool::Rectangle if !self.points.is_empty() => {
                self.add_entities(rectangle(self.points[0], point)?)?;
                self.finish();
            }
            Tool::Arc if self.points.len() == 2 => {
                let arc = arc_from_points(self.points[0], self.points[1], point)?;
                self.add_entities(vec![Entity::Arc(arc)])?;
                self.finish();
            }
            Tool::Move | Tool::Copy if !self.points.is_empty() => {
                let base = self.points[0];
                self.editor
                    .translate_selection(
                        point.x() - base.x(),
                        point.y() - base.y(),
                        point.z() - base.z(),
                        self.tool == Tool::Copy,
                    )
                    .map_err(|e| e.to_string())?;
                self.finish();
            }
            Tool::Circle | Tool::Arc | Tool::Rectangle | Tool::Move | Tool::Copy => {
                if self.tool == Tool::Arc && self.points.len() == 1 {
                    // Validate the start before advancing the three-point interaction.
                    arc_from_points(self.points[0], point, point)?;
                }
                self.points.push(point);
            }
            _ => return Err("Choose a drawing or movement tool before entering a point.".into()),
        }
        self.message(self.prompt(), false);
        Ok(())
    }

    fn add_entities(&mut self, entities: Vec<Entity>) -> Result<(), String> {
        self.editor
            .add(entities)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn request(&mut self, action: PendingAction) {
        if self.editor.is_dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }

    fn perform(&mut self, action: PendingAction) {
        match action {
            PendingAction::New | PendingAction::CloseDrawing => {
                let closing = matches!(action, PendingAction::CloseDrawing);
                self.workspace.has_drawing = !closing;
                self.workspace.start = closing;
                self.editor = Editor::default();
                self.layer_color = LayerColorInput::default();
                self.linetype_input = LinetypeInput::default();
                self.path = None;
                self.name = "Drawing1".into();
                self.finish();
                self.fit_pending = true;
                self.inspector = Inspector::default();
                self.message(
                    "New drawing. Coordinates are unitless; top view uses the XY plane.",
                    false,
                );
            }
            PendingAction::Study => {
                self.workspace.has_drawing = true;
                self.workspace.start = false;
                self.editor = Editor::new(
                    DxfDocument::parse(include_str!("../../examples/courtyard.dxf"))
                        .expect("validated study"),
                );
                self.layer_color = LayerColorInput::default();
                self.linetype_input = LinetypeInput::default();
                self.path = None;
                self.name = "Courtyard study".into();
                self.finish();
                self.fit_pending = true;
                self.inspector = Inspector::default();
                self.message("Courtyard study loaded.", false);
            }
            PendingAction::Open(path) => {
                let path = path.or_else(|| {
                    rfd::FileDialog::new()
                        .add_filter("R12 ASCII DXF", &["dxf"])
                        .pick_file()
                });
                if let Some(path) = path {
                    self.open_path(path);
                }
            }
            PendingAction::Close => self.allow_close = true,
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        let result = fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| DxfDocument::parse(&s).map_err(|e| e.to_string()));
        match result {
            Ok(document) => {
                self.workspace.has_drawing = true;
                self.workspace.start = false;
                self.remember_file(&path);
                self.editor = Editor::new(document);
                self.layer_color = LayerColorInput::default();
                self.linetype_input = LinetypeInput::default();
                self.name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.path = Some(path);
                self.finish();
                self.fit_pending = true;
                self.inspector = Inspector::default();
                self.message(
                    format!(
                        "Opened {} · {} entities.",
                        self.name,
                        self.editor.entities().len()
                    ),
                    false,
                );
            }
            Err(error) => self.message(format!("Open failed: {error}"), true),
        }
    }

    fn save(&mut self, save_as: bool) -> bool {
        let path = if !save_as { self.path.clone() } else { None };
        let path = path.or_else(|| {
            let mut dialog = rfd::FileDialog::new()
                .add_filter("R12 ASCII DXF", &["dxf"])
                .set_file_name(
                    self.path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .and_then(|s| s.to_str())
                        .unwrap_or("drawing.dxf"),
                );
            if let Some(parent) = self
                .path
                .as_ref()
                .and_then(|path| path.parent())
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                dialog = dialog.set_directory(parent);
            }
            dialog.save_file()
        });
        let Some(mut path) = path else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("dxf");
        }
        self.save_path(path)
    }

    fn save_path(&mut self, path: PathBuf) -> bool {
        match atomic_write(&path, self.editor.to_dxf().as_bytes()) {
            Ok(()) => {
                self.remember_file(&path);
                self.name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.path = Some(path);
                self.editor.mark_saved();
                self.message(
                    format!(
                        "Saved {} · {} entities.",
                        self.name,
                        self.editor.entities().len()
                    ),
                    false,
                );
                true
            }
            Err(error) => {
                self.message(format!("Save failed: {error}"), true);
                false
            }
        }
    }

    fn remember_file(&mut self, path: &Path) {
        // A later launch may use a different working directory.
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        self.recent_files.retain(|recent| recent != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(20);
    }

    fn export_svg(&mut self, path: Option<PathBuf>) {
        let svg = match self.editor.to_svg() {
            Ok(svg) => svg,
            Err(crate::SvgError::EmptyDrawing) => {
                self.message("There are no visible entities to export.", true);
                return;
            }
            Err(error) => {
                self.message(error.to_string(), true);
                return;
            }
        };
        let path = path.or_else(|| {
            rfd::FileDialog::new()
                .add_filter("SVG drawing", &["svg"])
                .set_file_name("drawing.svg")
                .save_file()
        });
        if let Some(mut path) = path {
            if path.extension().is_none() {
                path.set_extension("svg");
            }
            match atomic_write(&path, svg.as_bytes()) {
                Ok(()) => self.message(
                    format!("Exported XY projection to {}.", path.display()),
                    false,
                ),
                Err(error) => self.message(format!("Export failed: {error}"), true),
            }
        }
    }

    fn undo(&mut self, redo: bool) {
        self.finish();
        let changed = if redo {
            self.editor.redo()
        } else {
            self.editor.undo()
        };
        self.message(
            if changed {
                if redo { "Redone." } else { "Undone." }
            } else {
                "No more history."
            },
            false,
        );
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() || self.show_help {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)) {
            self.save(true);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save(false);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            self.request(PendingAction::Open(None));
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::N)) {
            self.request(PendingAction::New);
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.finish();
            self.command.clear();
            self.message("Command canceled.", false);
        }
        for (key, command) in [
            (Key::Num1, "PROPERTIESTOGGLE"),
            (Key::Num5, "TOOLSETS"),
            (Key::Num3, "COMMANDLINETOGGLE"),
        ] {
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, key)) {
                self.execute(command);
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F2)) {
            self.workspace.history = !self.workspace.history;
            self.workspace.command = true;
        }
        if self.workspace.start {
            return;
        }
        if !self.text_owns_edit_shortcuts(ctx) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z)) {
                self.undo(true);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z)) {
                self.undo(false);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A)) {
                self.editor
                    .select((0..self.editor.entities().len()).collect());
            }
        }
        if ctx.text_edit_focused() {
            return;
        }
        if self.tool == Tool::Mirror && self.points.len() == 2 {
            for (key, answer) in [(Key::Y, "Yes"), (Key::N, "No")] {
                if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                    self.execute(answer);
                }
            }
        }
        for (key, tool) in [
            (Key::V, Tool::Select),
            (Key::L, Tool::Line),
            (Key::C, Tool::Circle),
            (Key::A, Tool::Arc),
            (Key::R, Tool::Rectangle),
            (Key::M, Tool::Move),
            (Key::O, Tool::Offset),
            (Key::P, Tool::Pan),
        ] {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                if matches!(self.tool, Tool::Rotate | Tool::Scale)
                    && !self.points.is_empty()
                    && matches!(key, Key::C | Key::R)
                {
                    self.execute(if key == Key::C { "Copy" } else { "Reference" });
                } else if key == Key::C && matches!(self.tool, Tool::Line | Tool::Polyline) {
                    self.close_line();
                } else {
                    self.set_tool(tool);
                }
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::U)) {
            if self.tool == Tool::Offset {
                self.execute("Undo");
            } else {
                self.undo_line_point();
            }
        }
        if ctx.input(|i| i.key_pressed(Key::F)) {
            self.fit_pending = true;
        }
        if ctx.input(|i| i.key_pressed(Key::F7)) {
            self.grid = !self.grid;
        }
        if ctx.input(|i| i.key_pressed(Key::F8)) {
            self.set_ortho(!self.ortho);
        }
        if ctx.input(|i| i.key_pressed(Key::F9)) {
            self.grid_snap = !self.grid_snap;
        }
        if ctx.input(|i| i.key_pressed(Key::F10)) {
            self.set_polar_tracking(!self.polar_tracking);
        }
        if ctx.input(|i| i.key_pressed(Key::F3)) {
            self.object_snap = !self.object_snap;
        }
        if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
            self.finish();
            match self.editor.delete_selection() {
                Ok(true) => self.message("Selection deleted.", false),
                Err(error) => self.message(error.to_string(), true),
                Ok(false) => {}
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            if self.tool == Tool::Offset || (self.tool == Tool::Mirror && self.points.len() == 2) {
                self.execute("");
            } else if self.points.is_empty() {
                self.command_focus = true;
            } else if self.tool == Tool::Polyline {
                if let Err(error) = self.finish_polyline(false) {
                    self.message(error, true);
                }
            } else {
                self.finish();
                self.message("Command finished.", false);
            }
        }
    }
}

impl eframe::App for OpenCadApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_preferences(storage);
    }

    fn persist_egui_memory(&self) -> bool {
        // Persist the explicit workspace contract, not transient popup or
        // drawing-specific widget state. eframe saves native window geometry.
        false
    }

    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.drain_menu_actions(ctx);
        // eframe skips ui() while minimized or occluded. Protect dirty drawings
        // here so closing a hidden window still waits for the user's decision.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.editor.is_dirty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.pending = Some(PendingAction::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.pending.is_none()
            && let Some(path) = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .find_map(|file| file.path.clone())
            })
        {
            self.request(PendingAction::Open(Some(path)));
        }
        self.shortcuts(&ctx);
        self.header(ui);
        self.status_bar(ui);
        if self.workspace.start {
            self.start_page(ui);
        } else {
            self.inspector_panel(ui);
            self.tools_panel(ui);
            self.canvas(ui);
        }
        self.command_panel(&ctx);
        self.dialogs(&ctx);
        self.publish_menu_state(&ctx);
        if std::mem::take(&mut self.workspace.save_preferences_requested)
            && let Some(storage) = frame.storage_mut()
        {
            self.save_preferences(storage);
            storage.flush();
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{}{} — OpenCad",
            if self.workspace.start {
                "Start"
            } else {
                &self.name
            },
            if self.editor.is_dirty() && !self.workspace.start {
                " *"
            } else {
                ""
            }
        )));
    }
}

fn arc_from_points(center: Point3, start: Point3, end: Point3) -> Result<Arc3, String> {
    if start.z() != center.z() || end.z() != center.z() {
        return Err("Arc points must share the center's Z elevation.".into());
    }
    let sx = start.x() - center.x();
    let sy = start.y() - center.y();
    let ex = end.x() - center.x();
    let ey = end.y() - center.y();
    if [sx, sy, ex, ey].iter().any(|value| !value.is_finite()) {
        return Err("The arc direction exceeds the finite coordinate range.".into());
    }
    if (sx == 0.0 && sy == 0.0) || (ex == 0.0 && ey == 0.0) {
        return Err("Arc start and end directions must differ from the center.".into());
    }
    Arc3::new(center, sx.hypot(sy), sy.atan2(sx), ey.atan2(ex)).map_err(|e| e.to_string())
}

fn rectangle(a: Point3, b: Point3) -> Result<Vec<Entity>, String> {
    let c = Point3::new(b.x(), a.y(), a.z()).map_err(|e| e.to_string())?;
    let d = Point3::new(a.x(), b.y(), a.z()).map_err(|e| e.to_string())?;
    let b = Point3::new(b.x(), b.y(), a.z()).map_err(|e| e.to_string())?;
    Ok([(a, c), (c, b), (b, d), (d, a)]
        .into_iter()
        .map(|(s, e)| Entity::Line(Line3::new(s, e)))
        .collect())
}

/// A failed write leaves the existing destination intact.
fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("destination needs a filename"))?;
    for attempt in 0..100 {
        let temporary = parent.join(format!(
            ".{}.opencad-{}-{attempt}.tmp",
            name.to_string_lossy(),
            std::process::id()
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let result = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| {
                drop(file);
                fs::rename(&temporary, path)
            });
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return result;
    }
    Err(std::io::Error::other(
        "could not allocate a temporary save file",
    ))
}
