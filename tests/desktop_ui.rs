#![cfg(feature = "desktop")]

use eframe::App;
use eframe::egui::{self, Event, accesskit};
use opencad::{DxfDocument, Entity, Point3};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "support/ui_snapshot.rs"]
mod ui_snapshot;

// These checks exercise the shipped widgets through accessibility actions and
// keyboard events. Core tests cannot catch a missing accessible command field,
// a disconnected Run button, focus stealing, or a destructive modal action.
// eframe supplies the test context/frame; no production test seam is needed.
struct DrawingUi {
    ctx: egui::Context,
    app: opencad::desktop::OpenCadApp,
    frame: eframe::Frame,
    nodes: HashMap<accesskit::NodeId, accesskit::Node>,
    viewport_commands: Vec<egui::ViewportCommand>,
    screen_size: egui::Vec2,
    paint: Vec<egui::epaint::ClippedShape>,
    snapshot: Option<ui_snapshot::Snapshot>,
}

impl DrawingUi {
    fn new() -> Self {
        Self::with_storage(None)
    }

    fn with_storage(storage: Option<&dyn eframe::Storage>) -> Self {
        Self::with_storage_and_path(storage, None)
    }

    fn with_storage_and_path(storage: Option<&dyn eframe::Storage>, path: Option<PathBuf>) -> Self {
        let ctx = egui::Context::default();
        let mut cc = eframe::CreationContext::_new_kittest(ctx.clone());
        cc.storage = storage;
        let app = opencad::desktop::OpenCadApp::new(&cc, path);
        let mut ui = Self {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            nodes: HashMap::new(),
            viewport_commands: Vec::new(),
            screen_size: egui::vec2(1440.0, 920.0),
            paint: Vec::new(),
            snapshot: ui_snapshot::Snapshot::from_environment(),
        };
        ui.step(Vec::new());
        ui
    }

    fn step(&mut self, events: Vec<Event>) {
        self.step_with_close(events, false, true);
    }

    fn step_with_close(&mut self, events: Vec<Event>, close_requested: bool, visible: bool) {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                self.screen_size,
            )),
            focused: true,
            modifiers: events
                .iter()
                .rev()
                .find_map(|event| match event {
                    Event::Key { modifiers, .. } | Event::PointerButton { modifiers, .. } => {
                        Some(*modifiers)
                    }
                    _ => None,
                })
                .unwrap_or_default(),
            events,
            ..Default::default()
        };
        if close_requested {
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .events
                .push(egui::ViewportEvent::Close);
        }
        let output = self.ctx.run_ui(input, |ui| {
            self.app.logic(ui.ctx(), &mut self.frame);
            if visible {
                self.app.ui(ui, &mut self.frame);
            }
        });
        self.viewport_commands = output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .clone();
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.update(&output);
        }
        self.paint = output.shapes;
        if visible {
            let update = output
                .platform_output
                .accesskit_update
                .expect("the editor must publish accessible controls");
            self.nodes = update.nodes.into_iter().collect();
        }
    }

    fn action(&mut self, target: accesskit::NodeId, action: accesskit::Action) {
        self.step(vec![Event::AccessKitActionRequest(
            accesskit::ActionRequest {
                action,
                target_tree: accesskit::TreeId::ROOT,
                target_node: target,
                data: None,
            },
        )]);
        self.step(Vec::new());
    }

    fn click(&mut self, label: &str) {
        eprintln!("UI click: {label}");
        let id = self
            .nodes
            .iter()
            .find(|(_, node)| node.role() == accesskit::Role::Button && node.label() == Some(label))
            .map(|(id, _)| *id)
            .unwrap_or_else(|| {
                panic!(
                    "missing accessible button {label}; nearby labels: {:?}",
                    self.nodes
                        .values()
                        .filter(|node| node.label().is_some_and(|name| name.contains(label)))
                        .map(|node| (node.role(), node.label()))
                        .collect::<Vec<_>>()
                )
            });
        self.action(id, accesskit::Action::Click);
    }

    fn toggle(&mut self, label: &str) {
        let id = self
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == accesskit::Role::CheckBox && node.label() == Some(label)
            })
            .map(|(id, _)| *id)
            .unwrap_or_else(|| panic!("missing accessible checkbox {label}"));
        self.action(id, accesskit::Action::Click);
    }

    fn custom_action(&mut self, label: &str, description: &str) {
        let (target, node) = self
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .unwrap_or_else(|| panic!("missing accessible control {label}"));
        let id = node
            .custom_actions()
            .iter()
            .find(|action| action.description.as_ref() == description)
            .unwrap_or_else(|| panic!("missing action {description} on {label}"))
            .id;
        self.step(vec![Event::AccessKitActionRequest(
            accesskit::ActionRequest {
                action: accesskit::Action::CustomAction,
                target_tree: accesskit::TreeId::ROOT,
                target_node: *target,
                data: Some(accesskit::ActionData::CustomAction(id)),
            },
        )]);
        self.step(Vec::new());
    }

    fn toolset_header(&self, label: &str) -> accesskit::NodeId {
        // Other platforms also have a Draw menu; select the palette header.
        self.nodes
            .iter()
            .find(|(_, node)| {
                node.role() == accesskit::Role::Button
                    && node.label() == Some(label)
                    && node.is_expanded().is_some()
                    && node.bounds().is_some_and(|bounds| {
                        bounds.x0 >= 0.0 && bounds.x1 <= 184.0 && bounds.y0 >= 58.0
                    })
            })
            .map(|(id, _)| *id)
            .expect("accessible toolset header")
    }

    fn dismiss_popup(&mut self) {
        self.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
        self.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
        for _ in 0..12 {
            self.step(Vec::new());
        }
    }

    fn checked(&self, label: &str) -> bool {
        self.nodes
            .values()
            .find(|node| node.role() == accesskit::Role::CheckBox && node.label() == Some(label))
            .unwrap_or_else(|| panic!("missing accessible checkbox {label}"))
            .toggled()
            == Some(accesskit::Toggled::True)
    }

    fn field(&self, label: &str) -> accesskit::NodeId {
        self.nodes
            .iter()
            .find(|(_, node)| {
                node.role() == accesskit::Role::TextInput
                    && (node.label() == Some(label)
                        || node.labelled_by().iter().any(|id| {
                            self.nodes.get(id).and_then(|label| label.value()) == Some(label)
                        }))
            })
            .map(|(id, _)| *id)
            .unwrap_or_else(|| panic!("missing labelled input {label}"))
    }

    fn combo(&self, label: &str) -> accesskit::NodeId {
        let mut matches = self.nodes.iter().filter(|(_, node)| {
            node.role() == accesskit::Role::ComboBox
                && (node.label() == Some(label)
                    || node.labelled_by().iter().any(|id| {
                        self.nodes.get(id).and_then(|label| label.value()) == Some(label)
                    }))
        });
        let id = *matches
            .next()
            .unwrap_or_else(|| panic!("missing labelled combo {label}"))
            .0;
        assert!(matches.next().is_none(), "ambiguous labelled combo {label}");
        id
    }

    fn command(&mut self, command: &str) {
        eprintln!("UI command: {command}");
        let field = self.field("Command");
        self.action(field, accesskit::Action::Focus);
        self.step(vec![Event::Text(command.into())]);
        assert_eq!(self.nodes[&field].value(), Some(command));
        self.click("Run");
        assert_eq!(self.nodes[&field].value(), Some(""));
    }

    fn command_with_enter(&mut self, command: &str) {
        let field = self.field("Command");
        self.action(field, accesskit::Action::Focus);
        self.step(vec![Event::Text(command.into())]);
        self.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, true)]);
        self.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, false)]);
        assert_eq!(self.nodes[&field].value(), Some(""));
    }

    fn replace_field(&mut self, label: &str, value: &str) {
        let field = self.field(label);
        self.action(field, accesskit::Action::Focus);
        self.step(vec![key(egui::Key::A, egui::Modifiers::COMMAND, true)]);
        self.step(vec![
            key(egui::Key::A, egui::Modifiers::COMMAND, false),
            Event::Text(value.into()),
        ]);
        assert_eq!(self.nodes[&field].value(), Some(value));
    }

    fn canvas_click(&mut self, x: f64, y: f64) {
        let bounds = self
            .nodes
            .values()
            .find(|node| {
                node.label()
                    .is_some_and(|label| label.starts_with("Drawing canvas,"))
            })
            .and_then(|node| node.bounds())
            .expect("drawing canvas must have accessible bounds");
        let pos = egui::pos2(
            (bounds.x0 + x * (bounds.x1 - bounds.x0)) as f32,
            (bounds.y0 + y * (bounds.y1 - bounds.y0)) as f32,
        );
        self.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        self.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        self.step(Vec::new());
    }

    fn has_text(&self, text: &str) -> bool {
        self.nodes.values().any(|node| {
            node.value().is_some_and(|value| value.contains(text))
                || node.label().is_some_and(|label| label.contains(text))
                || node
                    .description()
                    .is_some_and(|description| description.contains(text))
        })
    }

    fn snapshot(&self, name: &str) {
        if let Some(snapshot) = &self.snapshot {
            snapshot.save(&self.ctx, &self.paint, self.screen_size, name);
        }
    }

    fn bounds(&self, label: &str) -> Option<egui::Rect> {
        self.nodes
            .values()
            .find(|node| node.label() == Some(label) || node.value() == Some(label))
            .and_then(|node| node.bounds())
            .map(|bounds| {
                egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                )
            })
    }

    fn unselected_line_colors(&self) -> Vec<egui::Color32> {
        fn collect(shape: &egui::Shape, canvas: egui::Rect, colors: &mut Vec<egui::Color32>) {
            match shape {
                egui::Shape::LineSegment { points, stroke }
                    if (stroke.width - 1.3).abs() < f32::EPSILON
                        && points.iter().all(|point| canvas.contains(*point)) =>
                {
                    colors.push(stroke.color);
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, canvas, colors);
                    }
                }
                _ => {}
            }
        }
        let canvas = self
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .expect("canvas bounds");
        let mut colors = Vec::new();
        for clipped in &self.paint {
            if clipped.clip_rect.intersects(canvas) {
                collect(&clipped.shape, canvas, &mut colors);
            }
        }
        colors
    }
}

#[derive(Default)]
struct PreferenceStorage(HashMap<String, String>);

impl eframe::Storage for PreferenceStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn flush(&mut self) {}
}

fn key(key: egui::Key, modifiers: egui::Modifiers, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    }
}

#[test]
fn reference_start_page_preserves_dirty_drawing_and_reopens_recent_files() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("recent.dxf");
    let mut ui = DrawingUi::new();
    ui.step(Vec::new());
    assert!(ui.has_text("There is nothing here yet."));
    assert!(ui.bounds("Connect").unwrap().max.y < 200.0);
    assert!(
        ui.bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .is_none()
    );
    ui.snapshot("start");
    ui.click("New");
    ui.command("LINE 0,0,3 10,20,3");
    ui.command("SELECT");
    ui.snapshot("drafting");
    ui.click("Start");
    assert!(!ui.has_text("Save changes"));
    assert!(ui.has_text("Drawing1*"));
    ui.click("Drawing1*");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let saved = fs::read_to_string(&drawing).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    let Entity::Line(line) = document.entities()[0] else {
        panic!("line expected");
    };
    assert_eq!(line.start(), Point3::new(0.0, 0.0, 3.0).unwrap());
    assert_eq!(line.end(), Point3::new(10.0, 20.0, 3.0).unwrap());
    ui.command("CLOSEDRAWING");
    assert!(ui.has_text("Recent"));
    assert!(!ui.has_text("There is nothing here yet."));
    ui.click("recent.dxf");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(drawing).unwrap(), saved);
}

#[test]
fn workspace_preferences_and_recent_files_survive_restart() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("restart.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE 0,0 10,20");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    ui.click("Customize Draw panel");
    ui.toggle("Show Draw: Line");
    ui.toggle("Show Draw: 3D Polyline");
    ui.dismiss_popup();
    ui.action(ui.toolset_header("Draw"), accesskit::Action::Click);
    ui.click("Collapse toolsets");
    ui.click("Drafting settings");
    ui.click("Status bar customization");
    ui.toggle("Show Transparency display");
    ui.dismiss_popup();
    ui.step(vec![key(egui::Key::Num1, egui::Modifiers::COMMAND, true)]);
    ui.step(vec![key(egui::Key::Num1, egui::Modifiers::COMMAND, false)]);
    ui.click("Start");
    ui.click("List view");
    ui.click("restart.dxf");
    ui.command("LINE 30,40 50,60");

    // Save through eframe's real lifecycle hook and reconstruct with a fresh
    // egui Context: neither in-memory widgets nor the drawing may leak across.
    let mut storage = PreferenceStorage::default();
    ui.app.save(&mut storage);
    drop(ui);
    let mut restored = DrawingUi::with_storage(Some(&storage));
    assert!(restored.has_text("Recent"));
    assert!(!restored.has_text("Drawing1"));
    assert!(restored.has_text("restart.dxf"));
    let recent_label = format!("restart.dxf\n{}", drawing.display());
    let recent = restored.bounds(&recent_label).unwrap();
    assert_eq!(recent.height(), 48.0, "recent list presentation restored");
    restored.click(&recent_label);
    assert_eq!(
        restored
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap()
            .left(),
        90.0
    );
    assert!(
        restored.bounds("My").is_none(),
        "Properties palette stays hidden"
    );
    assert_eq!(
        restored.nodes[&restored.toolset_header("Draw")].is_expanded(),
        Some(false)
    );
    assert!(restored.bounds("Transparency display").is_none());
    restored.action(restored.toolset_header("Draw"), accesskit::Action::Click);
    for _ in 0..12 {
        restored.step(Vec::new());
    }
    restored.click("Customize Draw panel");
    assert!(!restored.checked("Show Draw: Line"));
    assert!(restored.checked("Show Draw: 3D Polyline"));
    restored.dismiss_popup();
    restored.command("SAVE");
    assert_eq!(
        DxfDocument::parse(&fs::read_to_string(&drawing).unwrap())
            .unwrap()
            .entities()
            .len(),
        1,
        "workspace persistence must not restore an unsaved drawing"
    );
}

#[test]
fn workspace_preferences_recover_from_invalid_and_outdated_data() {
    let cases = [
        ("not a preferences record", 184.0, true),
        (
            r#"(
                panels: {"retired_palette": false},
                slim_tools: true,
                collapsed_toolsets: ["Draw", "retired_panel"],
                tool_visibility: {
                    "Draw": {"ID_Line": false, "retired_command": true},
                    "retired_panel": {"ID_Line": false},
                },
                hidden_status: ["retired_status"],
                recent_files: [Text("relative-file.dxf")],
                future_option: true,
            )"#,
            90.0,
            false,
        ),
    ];
    for (stored, width, line_visible) in cases {
        let mut storage = PreferenceStorage::default();
        storage
            .0
            .insert("opencad.workspace.v1".into(), stored.into());
        let mut ui = DrawingUi::with_storage(Some(&storage));
        assert!(ui.has_text("There is nothing here yet."));
        ui.click("New");
        assert_eq!(
            ui.bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
                .unwrap()
                .left(),
            width
        );
        assert!(ui.bounds("Transparency display").is_some());
        assert!(
            ui.bounds("My").is_some(),
            "missing preferences retain the Properties palette"
        );
        if !line_visible {
            assert_eq!(
                ui.nodes[&ui.toolset_header("Draw")].is_expanded(),
                Some(false)
            );
            ui.action(ui.toolset_header("Draw"), accesskit::Action::Click);
        }
        ui.click("Customize Draw panel");
        assert_eq!(ui.checked("Show Draw: Line"), line_visible);
        ui.dismiss_popup();
        ui.command("LINE 0,0 10,20");
        assert!(ui.has_text("1 selected"));
        assert!(ui.bounds("Apply properties").is_some());
    }
}

// Linux permits arbitrary filename bytes; macOS filesystems require Unicode.
#[cfg(target_os = "linux")]
#[test]
fn recent_files_retain_non_utf8_native_paths() {
    use std::os::unix::ffi::OsStringExt;
    let scratch = Scratch::new();
    let drawing = scratch
        .0
        .join(std::ffi::OsString::from_vec(b"native-\xff.dxf".to_vec()));
    let source = fs::read("examples/courtyard.dxf").unwrap();
    fs::write(&drawing, &source).unwrap();
    let mut ui = DrawingUi::with_storage_and_path(None, Some(drawing.clone()));
    let mut storage = PreferenceStorage::default();
    ui.app.save(&mut storage);
    drop(ui);
    let mut restored = DrawingUi::with_storage(Some(&storage));
    restored.click(&drawing.file_name().unwrap().to_string_lossy());
    restored.command("SAVE");
    assert_eq!(fs::read(&drawing).unwrap(), source);
    assert!(
        !scratch.0.join("native-�.dxf").exists(),
        "native path was not replaced by display text"
    );
}

#[test]
fn reference_native_menu_queue_respects_history_text_focus_and_dirty_modal() {
    use opencad::desktop::{menu_action_status, queue_menu_action};
    let mut ui = DrawingUi::new();
    assert!(menu_action_status(&ui.ctx, "ID_New").enabled);
    assert!(!menu_action_status(&ui.ctx, "ID_Line").enabled);
    assert!(!menu_action_status(&ui.ctx, "ID_Spline").enabled);
    queue_menu_action(&ui.ctx, "ID_New");
    ui.step(Vec::new());
    ui.step(Vec::new());
    ui.command("LINE 0,0 10,0");
    assert!(menu_action_status(&ui.ctx, "ID_U").enabled);
    queue_menu_action(&ui.ctx, "ID_U");
    ui.step(Vec::new());
    assert!(!menu_action_status(&ui.ctx, "ID_U").enabled);
    assert!(menu_action_status(&ui.ctx, "ID_Redo").enabled);
    queue_menu_action(&ui.ctx, "ID_Redo");
    ui.step(Vec::new());
    queue_menu_action(&ui.ctx, "ID_SelAll");
    ui.step(Vec::new());
    assert!(ui.has_text("1 selected"));

    ui.replace_field("Command", "unfinished command");
    assert!(!menu_action_status(&ui.ctx, "ID_U").enabled);
    assert!(!menu_action_status(&ui.ctx, "ID_SelAll").enabled);
    queue_menu_action(&ui.ctx, "ID_U");
    ui.step(Vec::new());
    ui.replace_field("Command", "SELECT");
    ui.click("Run");
    assert!(ui.has_text("0 selected"));
    assert!(menu_action_status(&ui.ctx, "ID_U").enabled);
    queue_menu_action(&ui.ctx, "ID_New");
    ui.step(Vec::new());
    assert!(ui.has_text("Save changes"));
    assert!(!menu_action_status(&ui.ctx, "ID_New").enabled);
    queue_menu_action(&ui.ctx, "ID_U");
    ui.step(Vec::new());
    ui.click("Cancel");
    ui.command("SELECT ALL");
    assert!(ui.has_text("1 selected"));
}

#[test]
fn reference_palette_shortcuts_preserve_property_edits_and_compact_command_bounds() {
    use opencad::desktop::menu_action_status;
    let scratch = Scratch::new();
    let drawing = scratch.0.join("palette-edit.dxf");
    let mut ui = DrawingUi::new();
    ui.screen_size = egui::vec2(980.0, 640.0);
    ui.click("New");
    ui.command("LINE 0,0,3 10,20,3");
    ui.replace_field("End Z", "9");
    assert!(!menu_action_status(&ui.ctx, "ID_U").enabled);
    for shortcut in [
        egui::Key::Num1,
        egui::Key::Num1,
        egui::Key::Num5,
        egui::Key::Num5,
    ] {
        ui.step(vec![key(shortcut, egui::Modifiers::COMMAND, true)]);
        ui.step(vec![key(shortcut, egui::Modifiers::COMMAND, false)]);
    }
    assert_eq!(ui.nodes[&ui.field("End Z")].value(), Some("9"));
    assert_eq!(
        menu_action_status(&ui.ctx, "ID_Properties").checked,
        Some(true)
    );
    assert_eq!(
        menu_action_status(&ui.ctx, "ID_ToolSets").checked,
        Some(true)
    );
    ui.click("Apply properties");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let document = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    let Entity::Line(line) = document.entities()[0] else {
        panic!("line expected");
    };
    assert_eq!(line.end().z(), 9.0);
    for size in [egui::vec2(980.0, 640.0), egui::vec2(1440.0, 560.0)] {
        ui.screen_size = size;
        ui.step(Vec::new());
        ui.step(Vec::new());
        let canvas = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        let command = ui.bounds("Command").unwrap();
        assert!(
            canvas.contains_rect(command),
            "command: {command:?}; canvas: {canvas:?}"
        );
        assert!(command.max.y <= size.y - 24.0);
    }
    ui.click("Drafting settings");
    ui.toggle("Ortho mode");
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.click("Viewport controls");
    ui.toggle("ViewCube");
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    assert_eq!(
        menu_action_status(&ui.ctx, "ID_ViewcubeOn").checked,
        Some(false)
    );
    ui.step(vec![key(egui::Key::F2, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::F2, egui::Modifiers::NONE, false)]);
    assert!(ui.bounds("Command").is_some());
    ui.snapshot("compact-drafting");
}

#[test]
fn panel_reordering_supports_drag_accessibility_and_restart() {
    fn row(ui: &DrawingUi, panel: &str) -> (accesskit::NodeId, egui::Rect) {
        let label = format!("Reorder panel: {panel}");
        let (&id, node) = ui
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label.as_str()))
            .unwrap_or_else(|| panic!("missing reorder row {panel}"));
        let bounds = node.bounds().expect("reorder row bounds");
        (
            id,
            egui::Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            ),
        )
    }
    fn drag(ui: &mut DrawingUi, source: egui::Pos2, destination: egui::Pos2, cancel: bool) {
        ui.step(vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        ui.step(vec![Event::PointerMoved(source + egui::vec2(0.0, 8.0))]);
        ui.step(vec![Event::PointerMoved(destination)]);
        if cancel {
            ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
            ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
        }
        ui.step(vec![Event::PointerButton {
            pos: destination,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        ui.step(Vec::new());
    }
    fn move_accessibly(ui: &mut DrawingUi, panel: &str, description: &str) {
        let (target, _) = row(ui, panel);
        let action = ui.nodes[&target]
            .custom_actions()
            .iter()
            .find(|action| action.description.as_ref() == description)
            .expect("accessible move action")
            .id;
        ui.step(vec![Event::AccessKitActionRequest(
            accesskit::ActionRequest {
                action: accesskit::Action::CustomAction,
                target_tree: accesskit::TreeId::ROOT,
                target_node: target,
                data: Some(accesskit::ActionData::CustomAction(action)),
            },
        )]);
        ui.step(Vec::new());
    }
    fn before(ui: &DrawingUi, first: &str, second: &str) {
        let first = ui.nodes[&ui.toolset_header(first)].bounds().unwrap();
        let second = ui.nodes[&ui.toolset_header(second)].bounds().unwrap();
        assert!(first.y0 < second.y0);
    }

    let scratch = Scratch::new();
    let drawing = scratch.0.join("reordered.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE 0,0 10,0");
    ui.replace_field("End X", "15");
    before(&ui, "Draw", "Hatch");
    ui.click("Reorder panels");
    assert!(ui.has_text("Drag to reorder."));
    let (_, source) = row(&ui, "Draw");
    let (_, target) = row(&ui, "Block");
    drag(
        &mut ui,
        source.center(),
        egui::pos2(target.center().x, target.bottom() - 2.0),
        false,
    );
    assert!(row(&ui, "Hatch").1.top() < row(&ui, "Block").1.top());
    assert!(row(&ui, "Block").1.top() < row(&ui, "Draw").1.top());
    assert_eq!(ui.nodes[&row(&ui, "Draw").0].is_selected(), Some(true));
    // Capture the settled popover rather than a frame from its opening fade.
    for _ in 0..20 {
        ui.step(Vec::new());
    }
    ui.snapshot("toolsets-reorder");
    ui.dismiss_popup();
    before(&ui, "Hatch", "Block");
    before(&ui, "Block", "Draw");
    assert_eq!(ui.nodes[&ui.field("End X")].value(), Some("15"));
    ui.click("Apply properties");
    ui.command(&format!("SAVEAS {}", drawing.display()));

    ui.click("Reorder panels");
    let (_, source) = row(&ui, "Draw");
    let (_, target) = row(&ui, "Hatch");
    drag(
        &mut ui,
        source.center(),
        egui::pos2(target.center().x, target.top() + 2.0),
        false,
    );
    assert!(row(&ui, "Draw").1.top() < row(&ui, "Hatch").1.top());
    assert!(
        ui.nodes[&row(&ui, "Draw").0]
            .custom_actions()
            .iter()
            .all(|action| action.description.as_ref() != "Move panel up")
    );
    assert!(
        ui.nodes[&row(&ui, "Parametric").0]
            .custom_actions()
            .iter()
            .all(|action| action.description.as_ref() != "Move panel down")
    );
    move_accessibly(&mut ui, "Draw", "Move panel down");
    assert!(row(&ui, "Hatch").1.top() < row(&ui, "Draw").1.top());
    move_accessibly(&mut ui, "Draw", "Move panel up");
    assert!(row(&ui, "Draw").1.top() < row(&ui, "Hatch").1.top());
    move_accessibly(&mut ui, "Draw", "Move panel down");

    let (_, source) = row(&ui, "Draw");
    let (_, target) = row(&ui, "Parametric");
    drag(&mut ui, source.center(), target.center(), true);
    assert!(!ui.has_text("Reorder panel:"));
    before(&ui, "Hatch", "Draw");
    before(&ui, "Draw", "Block");
    ui.click("Collapse toolsets");
    ui.click("Reorder panels");
    assert!(row(&ui, "Hatch").1.top() < row(&ui, "Draw").1.top());
    let (_, source) = row(&ui, "Draw");
    let (_, last) = row(&ui, "Parametric");
    drag(
        &mut ui,
        source.center(),
        egui::pos2(last.center().x, last.bottom() + 20.0),
        false,
    );
    assert!(row(&ui, "Parametric").1.top() < row(&ui, "Draw").1.top());
    let (_, source) = row(&ui, "Draw");
    let (_, first) = row(&ui, "Hatch");
    drag(
        &mut ui,
        source.center(),
        egui::pos2(first.center().x, first.top() + 2.0),
        false,
    );
    assert!(row(&ui, "Draw").1.top() < row(&ui, "Hatch").1.top());
    move_accessibly(&mut ui, "Draw", "Move panel down");
    ui.dismiss_popup();
    let mut storage = PreferenceStorage::default();
    ui.app.save(&mut storage);
    drop(ui);

    let mut restored = DrawingUi::with_storage(Some(&storage));
    restored.click("reordered.dxf");
    before(&restored, "Hatch", "Draw");
    before(&restored, "Draw", "Block");
    restored.click("Reorder panels");
    assert!(row(&restored, "Hatch").1.top() < row(&restored, "Draw").1.top());
    restored.dismiss_popup();
    restored.command("SAVE");
    let doc = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    assert_eq!(doc.entities().len(), 1);
    let Entity::Line(line) = &doc.entities()[0] else {
        panic!("drawing must retain its line");
    };
    assert_eq!(line.end().x(), 15.0);
}

#[test]
fn custom_panels_edit_drag_execute_persist_and_delete() {
    fn drag(ui: &mut DrawingUi, source: egui::Pos2, destination: egui::Pos2, cancel: bool) {
        ui.step(vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        ui.step(vec![Event::PointerMoved(source + egui::vec2(0.0, 8.0))]);
        ui.step(vec![Event::PointerMoved(destination)]);
        if cancel {
            ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
            ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
        }
        ui.step(vec![Event::PointerButton {
            pos: destination,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        ui.step(Vec::new());
    }
    fn add(ui: &mut DrawingUi, id: &str, label: &str) {
        ui.replace_field("Search panel commands", id);
        ui.custom_action(
            &format!("Available command: {label}"),
            "Add command to panel",
        );
        assert!(ui.bounds(&format!("Panel item: {label}")).is_some());
    }
    fn select(ui: &mut DrawingUi, label: &str) {
        let id = *ui
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .expect("outline item")
            .0;
        ui.action(id, accesskit::Action::Click);
    }

    let scratch = Scratch::new();
    let drawing = scratch.0.join("custom-panels.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.click("Add panel");
    assert!(ui.bounds("New Panel").is_some());
    assert!(ui.bounds("Rename Panel").is_none());
    ui.click("Customize New Panel panel");
    ui.replace_field("Rename Panel", "Fabrication");
    for layout in 2..=6 {
        ui.click("Next Layout");
        assert!(ui.has_text(&format!("{layout}/6")));
    }
    assert!(
        ui.nodes
            .values()
            .any(|node| node.label() == Some("Next Layout") && node.is_disabled())
    );
    for _ in 0..4 {
        ui.click("Previous Layout");
    }
    assert!(ui.has_text("2/6"));
    ui.replace_field("Search panel commands", "ID_Line");
    let source = ui.bounds("Available command: Line").unwrap().center();
    let target = ui.bounds("Panel command drop area").unwrap().center();
    drag(&mut ui, source, target, false);
    assert!(ui.bounds("Panel item: Line").is_some());

    ui.click("Add Drop-down");
    ui.replace_field("Rename Drop-down", "Primitives");
    add(&mut ui, "ID_CircleRad", "Circle, Center, Radius");
    add(&mut ui, "ID_Rectang", "Rectangle");
    // Moving a child out of a drop-down retains it as an independent command.
    let source = ui.bounds("Panel item: Rectangle").unwrap().center();
    let target = ui.bounds("Panel item: Line").unwrap().left_top() + egui::vec2(50.0, 2.0);
    drag(&mut ui, source, target, false);
    assert!(
        ui.bounds("Panel item: Rectangle").unwrap().top()
            < ui.bounds("Panel item: Line").unwrap().top()
    );
    // Place the command back inside using the drop-down's center target.
    let source = ui.bounds("Panel item: Rectangle").unwrap().center();
    let target = ui.bounds("Panel item: Primitives").unwrap().center();
    drag(&mut ui, source, target, false);
    assert!(
        ui.bounds("Panel item: Rectangle").unwrap().left()
            == ui
                .bounds("Panel item: Circle, Center, Radius")
                .unwrap()
                .left()
    );

    select(&mut ui, "Panel item: Primitives");
    add(&mut ui, "ID_Save", "Save");
    select(&mut ui, "Panel item: Save");
    ui.click("Delete selected panel item");
    assert!(ui.bounds("Panel item: Save").is_none());
    for _ in 0..20 {
        ui.step(Vec::new());
    }
    ui.snapshot("custom-panel-editor");
    // Canceling an active drag closes the editor and adds nothing.
    ui.replace_field("Search panel commands", "ID_Save");
    let source = ui.bounds("Available command: Save").unwrap().center();
    let target = ui.bounds("Panel item: Line").unwrap().center();
    drag(&mut ui, source, target, true);
    assert!(ui.bounds("Rename Panel").is_none());
    ui.click("Customize Fabrication panel");
    assert!(ui.bounds("Panel item: Save").is_none());
    ui.dismiss_popup();
    // Hide the system copies so these actions exercise only the new panel.
    ui.action(ui.toolset_header("Draw"), accesskit::Action::Click);
    for _ in 0..12 {
        ui.step(Vec::new());
    }
    ui.click("Line");
    ui.canvas_click(0.1, 0.2);
    ui.canvas_click(0.4, 0.3);
    assert!(
        ui.bounds("Rectangle").is_none(),
        "Rectangle is inside the custom drop-down"
    );
    ui.custom_action("Primitives", "Open command menu");
    ui.click("Circle, Center, Radius");
    ui.canvas_click(0.7, 0.4);
    ui.canvas_click(0.8, 0.5);
    ui.custom_action("Primitives", "Open command menu");
    ui.click("Rectangle");
    ui.canvas_click(0.2, 0.6);
    ui.canvas_click(0.5, 0.8);
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let before = fs::read_to_string(&drawing).unwrap();
    let document = DxfDocument::parse(&before).unwrap();
    assert_eq!(document.entities().len(), 6);
    assert_eq!(
        document
            .entities()
            .iter()
            .filter(|entity| matches!(entity, Entity::Circle(_)))
            .count(),
        1
    );

    ui.click("Reorder panels");
    let source = ui.bounds("Reorder panel: Fabrication").unwrap().center();
    let target = ui.bounds("Reorder panel: Draw").unwrap().left_top() + egui::vec2(50.0, 2.0);
    drag(&mut ui, source, target, false);
    ui.dismiss_popup();
    ui.click("Customize Fabrication panel");
    ui.replace_field("Rename Panel", "Workshop");
    ui.dismiss_popup();
    ui.action(ui.toolset_header("Workshop"), accesskit::Action::Click);
    let mut storage = PreferenceStorage::default();
    ui.app.save(&mut storage);
    drop(ui);

    let mut ui = DrawingUi::with_storage(Some(&storage));
    ui.click("custom-panels.dxf");
    let workshop = ui.toolset_header("Workshop");
    assert_eq!(ui.nodes[&workshop].is_expanded(), Some(false));
    assert!(
        ui.nodes[&workshop].bounds().unwrap().y0
            < ui.nodes[&ui.toolset_header("Draw")].bounds().unwrap().y0
    );
    ui.click("Customize Workshop panel");
    assert!(ui.has_text("2/6"));
    assert!(ui.bounds("Panel item: Line").is_some());
    assert!(ui.bounds("Panel item: Primitives").is_some());
    assert!(ui.bounds("Panel item: Circle, Center, Radius").is_some());
    assert!(ui.bounds("Panel item: Rectangle").is_some());
    assert!(ui.bounds("Panel item: Save").is_none());
    ui.dismiss_popup();
    ui.click("Delete Workshop panel");
    ui.click("Cancel panel deletion");
    assert!(ui.bounds("Workshop").is_some());
    ui.click("Delete Workshop panel");
    ui.click("Confirm delete Workshop panel");
    assert!(ui.bounds("Workshop").is_none());
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&drawing).unwrap(), before);
    ui.app.save(&mut storage);
    drop(ui);
    let mut ui = DrawingUi::with_storage(Some(&storage));
    ui.click("custom-panels.dxf");
    ui.click("Reorder panels");
    assert!(ui.bounds("Reorder panel: Workshop").is_none());
    assert!(ui.bounds("Reorder panel: Draw").is_some());
    ui.dismiss_popup();
    ui.click("Add panel");
    assert!(ui.bounds("New Panel").is_some());
}

#[test]
fn saved_custom_panels_validate_identities_and_catalog_references() {
    let storage = PreferenceStorage(HashMap::from([(
        "opencad.workspace.v1".into(),
        r#"(
        next_panel_id: 1,
        panel_order: [User(7), User(7), User(99), System("obsolete"), System("Hatch")],
        user_panels: [
            (id: 0, name: "Zero"),
            (id: 7, name: "Recovered", layout: 99, next_item_id: 1, items: [
                Command(id: 2, action: "ID_Line"),
                Command(id: 2, action: "ID_Save"),
                Command(id: 0, action: "ID_Save"),
                Command(id: 3, action: "obsolete"),
                DropDown(id: 4, name: "", commands: [
                    (id: 2, action: "ID_Save"),
                    (id: 5, action: "obsolete"),
                    (id: 6, action: "ID_Rectang"),
                ]),
            ]),
            (id: 7, name: "Duplicate"),
        ],
    )"#
        .into(),
    )]));
    let mut ui = DrawingUi::with_storage(Some(&storage));
    ui.click("New");
    assert!(ui.bounds("Zero").is_none());
    assert!(ui.bounds("Duplicate").is_none());
    assert!(
        ui.nodes[&ui.toolset_header("Recovered")]
            .bounds()
            .unwrap()
            .y0
            < ui.nodes[&ui.toolset_header("Hatch")].bounds().unwrap().y0
    );
    ui.click("Customize Recovered panel");
    assert!(ui.has_text("1/6"));
    assert!(ui.bounds("Panel item: Line").is_some());
    assert!(ui.bounds("Panel item: New Drop-down").is_some());
    assert!(ui.bounds("Panel item: Rectangle").is_some());
    assert!(ui.bounds("Panel item: Save").is_none());
    assert!(!ui.has_text("obsolete"));
    ui.replace_field("Search panel commands", "ID_Save");
    ui.custom_action("Available command: Save", "Add command to panel");
    assert!(ui.bounds("Panel item: Save").is_some());
    ui.dismiss_popup();
    ui.click("Add panel");
    ui.click("Customize New Panel panel");
    ui.replace_field("Rename Panel", "Second");
    ui.dismiss_popup();
    let mut normalized = PreferenceStorage::default();
    ui.app.save(&mut normalized);
    let mut ui = DrawingUi::with_storage(Some(&normalized));
    ui.click("New");
    ui.click("Reorder panels");
    assert_eq!(
        ui.nodes
            .values()
            .filter(|node| node.label() == Some("Reorder panel: Recovered"))
            .count(),
        1
    );
    assert_eq!(
        ui.nodes
            .values()
            .filter(|node| node.label() == Some("Reorder panel: Second"))
            .count(),
        1
    );
    assert_eq!(
        ui.nodes
            .values()
            .filter(|node| node
                .label()
                .is_some_and(|label| label.starts_with("Reorder panel:")))
            .count(),
        11
    );
    ui.dismiss_popup();
    ui.click("Customize Recovered panel");
    assert!(ui.bounds("Panel item: Save").is_some());
}

#[test]
fn saved_panel_order_ignores_removed_and_duplicate_panels() {
    let storage = PreferenceStorage(HashMap::from([(
        "opencad.workspace.v1".into(),
        r#"(toolset_order: ["Parametric", "Hatch", "Parametric", "removed-panel"])"#.into(),
    )]));
    let mut ui = DrawingUi::with_storage(Some(&storage));
    ui.click("New");
    let expected = [
        "Parametric",
        "Hatch",
        "Draw",
        "Block",
        "Modify",
        "Text",
        "Dimension",
        "Leader",
        "Table",
    ];
    let positions: Vec<_> = expected
        .iter()
        .map(|label| ui.nodes[&ui.toolset_header(label)].bounds().unwrap().y0)
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    ui.click("Reorder panels");
    let rows = ui
        .nodes
        .values()
        .filter(|node| {
            node.label()
                .is_some_and(|label| label.starts_with("Reorder panel:"))
        })
        .count();
    assert_eq!(rows, expected.len());
    assert!(!ui.has_text("removed-panel"));
    ui.dismiss_popup();
    // The toolbar remains usable when the drawing status bar is hidden.
    ui.command("STATUSBAR");
    assert!(ui.bounds("Model").is_none());
    ui.click("Reorder panels");
    assert!(ui.has_text("Drag to reorder."));
    ui.dismiss_popup();
    ui.command("LINE 0,0 5,5");
    assert_eq!(ui.nodes[&ui.field("End X")].value(), Some("5"));
}

#[test]
fn toolset_gears_and_slim_palette_preserve_drafting_and_property_edits() {
    fn settle(ui: &mut DrawingUi) {
        for _ in 0..12 {
            ui.step(Vec::new());
        }
    }
    fn dismiss(ui: &mut DrawingUi) {
        ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
        settle(ui);
    }
    fn panel_button(ui: &DrawingUi, label: &str) -> Option<egui::Rect> {
        ui.nodes
            .values()
            .filter(|node| node.role() == accesskit::Role::Button && node.label() == Some(label))
            .filter_map(|node| node.bounds())
            .find(|bounds| bounds.x0 >= 0.0 && bounds.x1 <= 184.0 && bounds.y0 >= 58.0)
            .map(|bounds| {
                egui::Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                )
            })
    }
    fn expanded(ui: &DrawingUi) -> bool {
        ui.nodes[&draw_header(ui)]
            .is_expanded()
            .expect("section expansion state")
    }
    fn draw_header(ui: &DrawingUi) -> accesskit::NodeId {
        ui.toolset_header("Draw")
    }
    fn click_draw_header(ui: &mut DrawingUi) {
        ui.action(draw_header(ui), accesskit::Action::Click);
    }
    fn canvas(ui: &DrawingUi) -> egui::Rect {
        ui.bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .expect("drawing canvas")
    }

    let scratch = Scratch::new();
    let drawing = scratch.0.join("toolset-edit.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE 0,0,3 10,20,3");
    ui.replace_field("End Z", "9");
    settle(&mut ui);
    assert_eq!(canvas(&ui).left(), 184.0);
    let header = panel_button(&ui, "Draw").unwrap();
    let gear = panel_button(&ui, "Customize Draw panel").unwrap();
    assert_eq!(header.height(), 20.0);
    assert_eq!(gear.size(), egui::Vec2::splat(12.0));
    assert!(header.right() <= gear.left());
    assert_eq!(
        panel_button(&ui, "Line").unwrap().size(),
        egui::Vec2::splat(44.0)
    );
    ui.snapshot("toolsets-full");

    ui.click("Customize Draw panel");
    assert!(expanded(&ui));
    assert!(ui.checked("Show Draw: Line"));
    assert!(!ui.checked("Show Draw: 3D Polyline"));
    ui.toggle("Show Draw: Line");
    ui.toggle("Show Draw: 3D Polyline");
    assert!(!ui.checked("Show Draw: Line"));
    assert!(ui.checked("Show Draw: 3D Polyline"));
    settle(&mut ui);
    ui.snapshot("toolsets-customization");
    dismiss(&mut ui);
    assert!(panel_button(&ui, "Line").is_none());
    let three_d = ui
        .nodes
        .values()
        .find(|node| node.role() == accesskit::Role::Button && node.label() == Some("3D Polyline"))
        .expect("newly visible command");
    assert!(three_d.is_disabled());
    click_draw_header(&mut ui);
    settle(&mut ui);
    assert!(!expanded(&ui));
    assert!(panel_button(&ui, "Polyline").is_none());
    assert!(panel_button(&ui, "Customize Draw panel").is_some());
    click_draw_header(&mut ui);
    settle(&mut ui);
    assert!(panel_button(&ui, "Polyline").is_some());

    ui.click("Start");
    ui.click("Drawing1*");
    for _ in 0..2 {
        ui.step(vec![key(egui::Key::Num5, egui::Modifiers::COMMAND, true)]);
        ui.step(vec![key(egui::Key::Num5, egui::Modifiers::COMMAND, false)]);
    }
    assert!(panel_button(&ui, "Line").is_none());
    assert_eq!(ui.nodes[&ui.field("End Z")].value(), Some("9"));
    ui.click("Apply properties");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    ui.click("Customize Draw panel");
    ui.click("Restore default panel commands");
    assert!(ui.checked("Show Draw: Line"));
    assert!(!ui.checked("Show Draw: 3D Polyline"));
    dismiss(&mut ui);

    ui.click("Collapse toolsets");
    settle(&mut ui);
    assert_eq!(canvas(&ui).left(), 90.0);
    assert_eq!(ui.bounds("Model").unwrap().left(), 90.0);
    assert_eq!(
        panel_button(&ui, "Line").unwrap().size(),
        egui::Vec2::splat(22.0)
    );
    let expand = panel_button(&ui, "Expand toolsets").unwrap();
    assert!(
        expand.right() <= 90.0,
        "slim toggle outside palette: {expand:?}"
    );
    assert!(
        ui.nodes
            .values()
            .find(|node| {
                node.role() == accesskit::Role::Button && node.label() == Some("Modeling")
            })
            .unwrap()
            .is_disabled()
    );
    ui.snapshot("toolsets-slim");
    click_draw_header(&mut ui);
    settle(&mut ui);
    assert!(panel_button(&ui, "Line").is_none());
    click_draw_header(&mut ui);
    settle(&mut ui);
    ui.click("Line");
    ui.command("30,40,3");
    ui.command("50,60,3");
    ui.command("SELECT");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 2);
    let Entity::Line(original) = document.entities()[0] else {
        panic!("line expected");
    };
    assert_eq!(original.end().z(), 9.0);
    let Entity::Line(added) = document.entities()[1] else {
        panic!("line expected");
    };
    assert_eq!(added.start(), Point3::new(30.0, 40.0, 3.0).unwrap());
    assert_eq!(added.end(), Point3::new(50.0, 60.0, 3.0).unwrap());

    for size in [egui::vec2(980.0, 640.0), egui::vec2(1440.0, 560.0)] {
        ui.screen_size = size;
        settle(&mut ui);
        assert!(canvas(&ui).contains_rect(ui.bounds("Command").unwrap()));
    }
    ui.snapshot("toolsets-slim-compact");
    ui.click("Expand toolsets");
    settle(&mut ui);
    assert_eq!(canvas(&ui).left(), 184.0);
    assert_eq!(ui.bounds("Model").unwrap().left(), 184.0);
    assert_eq!(
        panel_button(&ui, "Line").unwrap().size(),
        egui::Vec2::splat(44.0)
    );
}

// The status inventory is recovered metadata, but the actual widgets still
// need checks for ordering, clipping, disabled actions, and visibility changes.
#[test]
fn reference_status_controls_fit_in_order_and_can_be_customized() {
    let mut ui = DrawingUi::new();
    ui.screen_size = egui::vec2(980.0, 640.0);
    ui.click("New");
    ui.step(Vec::new());
    let labels = [
        "Cursor coordinates",
        "Model / paper space",
        "Grid",
        "Grid snap",
        "Ortho",
        "Polar tracking",
        "Dynamic input",
        "Lineweight display",
        "Transparency display",
        "Group selection",
        "Object snap tracking",
        "Object snap",
        "Viewport maximize",
        "Annotation visibility",
        "Annotation auto scale",
        "Lock viewport",
        "Annotation scale",
        "Viewport scale",
        "Annotation scale sync",
        "Isolate objects",
        "Drafting settings",
    ];
    for size in [egui::vec2(980.0, 640.0), egui::vec2(1440.0, 920.0)] {
        ui.screen_size = size;
        ui.step(Vec::new());
        ui.step(Vec::new());
        let mut right = ui.bounds("Layout1").unwrap().right();
        for label in labels {
            let bounds = ui
                .nodes
                .values()
                .filter(|node| node.label() == Some(label) || node.value() == Some(label))
                .filter_map(|node| node.bounds())
                .find(|bounds| bounds.y0 >= f64::from(size.y - 24.0))
                .map(|bounds| {
                    egui::Rect::from_min_max(
                        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                    )
                })
                .unwrap_or_else(|| panic!("missing status control {label}"));
            assert!(
                bounds.left() >= right - 0.1,
                "{label} overlaps its predecessor: {bounds:?}, {right}"
            );
            assert!(
                bounds.right() <= size.x
                    && bounds.bottom() <= size.y
                    && bounds.top() >= size.y - 24.0,
                "{label} is outside the status strip: {bounds:?}"
            );
            right = bounds.right();
        }
        if size.x == 980.0 {
            ui.snapshot("compact-status-drafting");
        }
    }
    for label in [
        "Dynamic input",
        "Object snap tracking",
        "Lock viewport",
        "Isolate objects",
    ] {
        assert!(
            ui.nodes
                .values()
                .find(|node| node.label() == Some(label))
                .unwrap()
                .is_disabled()
        );
    }
    ui.snapshot("status-drafting");
    ui.toggle("Polar tracking");
    ui.click("Drafting settings");
    ui.click("Status bar customization");
    ui.toggle("Show Transparency display");
    // Retain the fully opened popup instead of its short fade-in transition.
    for _ in 0..12 {
        ui.step(Vec::new());
    }
    ui.snapshot("status-customization");
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    assert!(ui.bounds("Transparency display").is_none());
    assert!(ui.checked("Polar tracking"));
    ui.click("Drafting settings");
    ui.click("Status bar customization");
    ui.click("Restore default status controls");
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    assert!(ui.bounds("Transparency display").is_some());
    assert!(ui.checked("Polar tracking"));
}

#[test]
fn polar_modes_share_state_across_keys_commands_and_drafting_settings() {
    let mut ui = DrawingUi::new();
    ui.click("New");
    assert!(!ui.checked("Polar tracking"));
    ui.toggle("Ortho");
    ui.toggle("Polar tracking");
    assert!(!ui.checked("Ortho"));
    assert!(ui.checked("Polar tracking"));
    ui.command("ORTHO ON");
    assert!(ui.checked("Ortho"));
    assert!(!ui.checked("Polar tracking"));
    ui.command("POLAR ON");
    assert!(!ui.checked("Ortho"));
    assert!(ui.checked("Polar tracking"));
    ui.canvas_click(0.5, 0.5);
    for (key_code, polar, ortho) in [
        (egui::Key::F8, false, true),
        (egui::Key::F10, true, false),
        (egui::Key::F10, false, false),
    ] {
        ui.step(vec![key(key_code, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(key_code, egui::Modifiers::NONE, false)]);
        assert_eq!(ui.checked("Ortho"), ortho);
        assert_eq!(ui.checked("Polar tracking"), polar);
    }
    ui.click("Drafting settings");
    ui.toggle("Polar tracking mode");
    ui.toggle("Ortho mode");
    assert!(ui.checked("Ortho mode"));
    assert!(!ui.checked("Polar tracking mode"));
    ui.toggle("Polar tracking mode");
    assert!(!ui.checked("Ortho mode"));
    ui.click("Polar angle: 90°");
    ui.click("45°");
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    for invalid in ["0", "-5", "361", "NaN", "inf", "1e-320"] {
        ui.command(&format!("POLARANG {invalid}"));
        ui.click("Drafting settings");
        assert!(
            ui.bounds("Polar angle: 45°").is_some(),
            "invalid increment {invalid} changed the setting"
        );
        ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    }
    ui.command("POLAR OFF");
    assert!(!ui.checked("Ortho"));
    assert!(!ui.checked("Polar tracking"));
}

#[test]
fn polar_pointer_guides_constrain_angles_and_yield_to_exact_object_points() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("polar-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("OSNAP OFF");
    ui.command("SNAP OFF");
    ui.command("ELEVATION 7");
    ui.command("POLARANG 45");
    ui.command("POLAR ON");
    ui.command("LINE");
    ui.command("0,0,7");
    let screen = |ui: &DrawingUi, x: f32, y: f32, scale: f32| {
        let rect = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        (rect, rect.center() + egui::vec2(x * scale, -y * scale))
    };
    let click_world = |ui: &mut DrawingUi, x: f32, y: f32| {
        let (rect, pos) = screen(ui, x, y, 4.0);
        ui.canvas_click(
            f64::from((pos.x - rect.left()) / rect.width()),
            f64::from((pos.y - rect.top()) / rect.height()),
        );
    };
    let (_, near) = screen(&ui, 25.0, 24.5, 4.0);
    ui.step(vec![Event::PointerMoved(near)]);
    assert!(ui.has_text("Polar guide: 45.00 degrees"));
    assert!(ui.paint.iter().any(|shape| matches!(&shape.shape, egui::Shape::LineSegment {stroke, ..} if stroke.color == egui::Color32::from_rgb(95, 170, 110))));
    ui.snapshot("polar-guide");
    let (_, far) = screen(&ui, 25.0, 19.0, 4.0);
    ui.step(vec![Event::PointerMoved(far)]);
    assert!(!ui.has_text("Polar guide:"));
    click_world(&mut ui, 25.0, 24.5);
    ui.command("");
    ui.command("LINE");
    ui.command("0,0,7");
    click_world(&mut ui, 25.0, 19.0);
    ui.command("");
    ui.command("LINE 25,24.5,7 30,24.5,7");
    ui.command("OSNAP ON");
    ui.command("LINE");
    ui.command("0,0,7");
    ui.step(vec![Event::PointerMoved(near)]);
    assert!(!ui.has_text("Polar guide:"));
    click_world(&mut ui, 25.0, 24.5);
    ui.command("");
    ui.command("OSNAP OFF");
    ui.command("POLARANG 100");
    ui.command("LINE");
    ui.command("0,0,7");
    click_world(&mut ui, 25.0, -1.0);
    ui.command("");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let document = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    let lines: Vec<_> = document
        .entities()
        .iter()
        .map(|entity| match entity {
            Entity::Line(line) => *line,
            _ => panic!("expected line"),
        })
        .collect();
    assert_eq!(lines.len(), 5);
    assert!((lines[0].end().x() - 24.75).abs() < 1e-9);
    assert!((lines[0].end().y() - 24.75).abs() < 1e-9);
    assert_eq!(lines[1].end(), Point3::new(25.0, 19.0, 7.0).unwrap());
    assert_eq!(lines[2].start(), Point3::new(25.0, 24.5, 7.0).unwrap());
    assert_eq!(lines[2].end(), Point3::new(30.0, 24.5, 7.0).unwrap());
    assert_eq!(lines[3].end(), lines[2].start());
    assert_eq!(lines[4].end(), Point3::new(25.0, 0.0, 7.0).unwrap());
    assert!(
        lines
            .iter()
            .all(|line| line.start().z() == 7.0 && line.end().z() == 7.0)
    );
}

// Straight polyline properties cannot catch a lost outgoing bulge during
// inspector edits, reversal, duplication, or invalid finite geometry input.
#[test]
fn bulged_polyline_widgets_edit_reverse_copy_and_save_real_curves() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("bulged-polylines-widget.dxf");
    let appended = scratch.0.join("bulged-appended.dxf");
    let svg = scratch.0.join("bulged-polylines-widget.svg");
    let mut ui = DrawingUi::new();
    ui.screen_size = egui::vec2(1440.0, 1400.0);
    ui.click("New");
    ui.command("LINETYPE DASHED 1,-0.5");
    ui.command("LAYER NEW DETAIL");
    ui.command("LAYER COLOR DETAIL 3");
    ui.command("LAYER SET DETAIL");
    ui.command("PLINE 0,0,3 10,0,3 10,10,3 0,10,3 CLOSE");
    ui.replace_field("Vertex 1 bulge", "1");
    ui.replace_field("Vertex 2 bulge", "-0.4142135623730951");
    ui.replace_field("Vertex 4 bulge", "2");
    ui.click("Apply properties");
    assert!(ui.has_text("Properties applied."));
    ui.command("ENTITY COLOR 4");
    ui.command("ENTITY LINETYPE DASHED");
    ui.command("ENTITY LTSCALE 0.5");
    ui.command("LTSCALE 2");
    ui.command("COPY 20,0,2");
    ui.click("Reverse vertices");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let saved = fs::read_to_string(&drawing).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.entities().len(), 2);
    let Entity::Polyline(first) = &document.entities()[0] else {
        panic!("first curve")
    };
    let Entity::Polyline(second) = &document.entities()[1] else {
        panic!("copied curve")
    };
    assert_eq!(first.bulges(), &[1.0, -0.4142135623730951, 0.0, 2.0]);
    assert_eq!(second.bulges(), &[0.0, 0.4142135623730951, -1.0, -2.0]);
    assert!(first.is_closed() && second.is_closed());
    assert_eq!(first.elevation(), 3.0);
    assert_eq!(second.elevation(), 5.0);
    assert_eq!(
        second
            .vertices()
            .iter()
            .map(|point| (point.x(), point.y()))
            .collect::<Vec<_>>(),
        vec![(20.0, 10.0), (30.0, 10.0), (30.0, 0.0), (20.0, 0.0)]
    );
    assert!((first.length().unwrap() - second.length().unwrap()).abs() < 1e-10);
    for invalid in ["NaN", "1e308"] {
        ui.replace_field("Vertex 1 bulge", invalid);
        ui.click("Apply properties");
        assert!(ui.has_text(if invalid == "NaN" {
            "bulges must be finite"
        } else {
            "bulged segment exceeds representable geometry"
        }));
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&drawing).unwrap(), saved);
    }
    ui.replace_field("Vertex 1 bulge", "0");
    ui.click("Add vertex");
    ui.command(&format!("SAVEAS {}", appended.display()));
    let added = DxfDocument::parse(&fs::read_to_string(&appended).unwrap()).unwrap();
    let Entity::Polyline(added) = &added.entities()[1] else {
        unreachable!()
    };
    assert_eq!(added.vertices().len(), 5);
    assert_eq!(added.bulges(), &[0.0, 0.4142135623730951, -1.0, 0.0, -2.0]);
    assert!((added.length().unwrap() - second.length().unwrap()).abs() < 1e-10);
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&appended).unwrap(), saved);
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    let exported = fs::read_to_string(&svg).unwrap();
    assert!(exported.contains(" A "));
    assert!(exported.contains("stroke=\"#00ffff\""));
    ui.command(&format!("OPEN {}", drawing.display()));
    ui.command(&format!("SAVEAS {}", drawing.display()));
    assert_eq!(fs::read_to_string(&drawing).unwrap(), saved);
}

// Chord-only hit, snap and fit logic misses the curved locus and its extrema.
// Drive the actual canvas instead of testing another geometry helper.
#[test]
fn bulged_polyline_mouse_selection_snap_and_fit_use_arc_geometry() {
    fn collect(shape: &egui::Shape, paths: &mut Vec<Vec<egui::Pos2>>) {
        match shape {
            egui::Shape::Path(path) if path.points.len() > 8 => paths.push(path.points.clone()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, paths);
                }
            }
            _ => {}
        }
    }
    let curve = |ui: &DrawingUi| {
        let mut paths = Vec::new();
        for clipped in &ui.paint {
            collect(&clipped.shape, &mut paths);
        }
        paths.into_iter().max_by_key(Vec::len).expect("painted arc")
    };
    let canvas = |ui: &DrawingUi| {
        let bounds = ui
            .nodes
            .values()
            .find(|node| {
                node.label()
                    .is_some_and(|label| label.starts_with("Drawing canvas,"))
            })
            .and_then(|node| node.bounds())
            .unwrap();
        egui::Rect::from_min_max(
            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
        )
    };
    let click_painted = |ui: &mut DrawingUi, point: egui::Pos2| {
        let canvas = canvas(ui);
        ui.canvas_click(
            f64::from((point.x - canvas.left()) / canvas.width()),
            f64::from((point.y - canvas.top()) / canvas.height()),
        );
    };
    let bottom = |points: &[egui::Pos2]| *points.iter().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    let scratch = Scratch::new();
    let drawing = scratch.0.join("bulged-mouse.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("OSNAP OFF");
    ui.command("SNAP OFF");
    ui.command("ELEVATION 3");
    ui.command("PLINE");
    ui.canvas_click(0.4, 0.5);
    ui.canvas_click(0.6, 0.5);
    ui.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, false)]);
    ui.replace_field("Vertex 1 bulge", "1");
    ui.click("Apply properties");
    assert!(ui.has_text("Properties applied."));
    ui.command(&format!("SAVEAS {}", drawing.display()));
    ui.command("SELECT");
    let points = curve(&ui);
    let chord = points[0].lerp(*points.last().unwrap(), 0.5);
    assert!(bottom(&points).y > chord.y + 50.0);
    click_painted(&mut ui, bottom(&points));
    assert!(ui.has_text("1 selected"));
    ui.command("SELECT");
    let points = curve(&ui);
    click_painted(&mut ui, points[0].lerp(*points.last().unwrap(), 0.5));
    assert!(ui.has_text("0 selected"));
    ui.command("OSNAP ON");
    ui.command("LINE");
    let points = curve(&ui);
    click_painted(&mut ui, bottom(&points) + egui::vec2(0.5, 0.5));
    ui.canvas_click(0.8, 0.3);
    ui.command("");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let document = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    let Entity::Polyline(polyline) = &document.entities()[0] else {
        unreachable!()
    };
    let Entity::Line(line) = &document.entities()[1] else {
        panic!("snapped line")
    };
    let a = polyline.vertices()[0];
    let b = polyline.vertices()[1];
    assert!((line.start().x() - (a.x() + b.x()) / 2.0).abs() < 1e-6);
    assert!((line.start().y() - (a.y() - (b.x() - a.x()) / 2.0)).abs() < 1e-6);
    assert_eq!(line.start().z(), 3.0);
    ui.command("UNDO");
    ui.command("SELECT ALL");
    ui.command("FIT");
    let canvas = canvas(&ui);
    let mut paths = Vec::new();
    for clipped in &ui.paint {
        if clipped.clip_rect.contains(canvas.center()) {
            collect(&clipped.shape, &mut paths);
        }
    }
    assert!(!paths.is_empty(), "the fitted curve must paint");
    assert!(
        paths.iter().flatten().all(|point| canvas.contains(*point)),
        "fitting must include the arc extrema"
    );
}

struct Scratch(PathBuf, bool);

impl Scratch {
    fn new() -> Self {
        let evidence = std::env::var_os("OPENCAD_UI_EVIDENCE_DIR");
        let parent = evidence
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let directory = parent.join(format!(
            "opencad-ui-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            }
        ));
        fs::create_dir_all(&parent).unwrap();
        fs::create_dir(&directory).expect("UI scratch directory must be fresh");
        if evidence.is_some() {
            eprintln!("UI evidence: {}", directory.display());
        }
        Self(directory, evidence.is_some())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.1 {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn accessible_drawing_commands_save_and_reopen_real_geometry() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("mixed.dxf");
    let svg = scratch.0.join("mixed.svg");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command_with_enter("LINE 1,2,3 4,6,15 8,10,20");
    ui.command("CIRCLE 30,40,5 3");
    ui.command("RECTANG 0,0,1 10,20,99");
    ui.command("SELECT ALL");
    ui.command("MOVE 10,20,30");
    ui.command("COPY 1,2,3");
    ui.command("UNDO");
    ui.command("REDO");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    assert!(drawing.exists(), "save must create an actual drawing");
    let saved = fs::read_to_string(&drawing).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.entities().len(), 14);
    let Entity::Line(line) = document.entities()[0] else {
        panic!("first entity must remain a line");
    };
    assert_eq!(line.start(), Point3::new(11.0, 22.0, 33.0).unwrap());
    assert_eq!(line.end(), Point3::new(14.0, 26.0, 45.0).unwrap());
    let Entity::Circle(circle) = document.entities()[2] else {
        panic!("third entity must remain a circle");
    };
    assert_eq!(circle.center(), Point3::new(40.0, 60.0, 35.0).unwrap());
    assert_eq!(circle.radius(), 3.0);
    let Entity::Circle(copy) = document.entities()[9] else {
        panic!("the copied circle must survive redo");
    };
    assert_eq!(copy.center(), Point3::new(41.0, 62.0, 38.0).unwrap());
    assert_eq!(copy.radius(), 3.0);
    ui.click("New");
    ui.command(&format!("OPEN {}", drawing.display()));
    assert!(ui.has_text("Opened mixed.dxf"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&drawing).unwrap(), saved);
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    let exported = fs::read_to_string(svg).unwrap();
    assert_eq!(exported.matches("<line ").count(), 12);
    assert_eq!(exported.matches("<circle ").count(), 2);
}

#[test]
fn transform_commands_reference_copies_confirmation_and_history_save_geometry() {
    use opencad::desktop::{menu_action_status, queue_menu_action};
    fn near_point(actual: Point3, expected: [f64; 3]) {
        for (actual, expected) in [actual.x(), actual.y(), actual.z()]
            .into_iter()
            .zip(expected)
        {
            assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
        }
    }
    let scratch = Scratch::new();
    let path = scratch.0.join("transformed.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    for action in ["ID_Rotate", "ID_Scale", "ID_Mirror"] {
        assert!(!menu_action_status(&ui.ctx, action).enabled);
    }
    ui.command("LINE 1,2,3 4,2,5");
    ui.command("CIRCLE 10,20,7 2");
    ui.command("ARC 10,20,7 2 0 90");
    ui.command("PLINE 0,0,7 10,0,7 10,10,7 CLOSE");
    ui.command("SELECT 1");
    for action in ["ID_Rotate", "ID_Scale", "ID_Mirror"] {
        assert!(menu_action_status(&ui.ctx, action).enabled);
    }
    queue_menu_action(&ui.ctx, "ID_Rotate");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Rotate"));
    ui.command("1,2,3");
    ui.command("Copy");
    ui.command("Reference");
    ui.command("0,0");
    ui.command("1,0");
    ui.command("90");
    assert!(ui.has_text("Transformed copy created."));
    queue_menu_action(&ui.ctx, "ID_Scale");
    ui.step(Vec::new());
    ui.command("1,2,3");
    ui.command("R");
    ui.command("0");
    assert!(ui.has_text("The reference length must be positive."));
    ui.command("3");
    ui.command("6");
    assert!(ui.has_text("Selection transformed."));
    ui.command(&format!("SAVEAS {}", path.display()));
    let before_mirror = fs::read_to_string(&path).unwrap();
    queue_menu_action(&ui.ctx, "ID_Mirror");
    ui.step(Vec::new());
    ui.command("0,0");
    ui.command("0,0,10");
    assert!(ui.has_text("a coincident line"));
    ui.command("0,10");
    assert!(ui.has_text("Erase source objects?"));
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), before_mirror);
    ui.command("MIRROR");
    ui.command("0,0");
    ui.command("0,10");
    ui.command_with_enter(""); // OpenCad's explicit default retains the source.
    assert!(ui.has_text("Current tool: Select"));
    ui.command("SAVE");
    assert_eq!(
        DxfDocument::parse(&fs::read_to_string(&path).unwrap())
            .unwrap()
            .entities()
            .len(),
        6,
        "default mirror must create a copy before undo/redo"
    );
    ui.command("UNDO");
    ui.command("REDO");
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.entities().len(), 6);
    let Entity::Line(original) = document.entities()[0] else {
        panic!("expected line")
    };
    near_point(original.start(), [1.0, 2.0, 3.0]);
    near_point(original.end(), [4.0, 2.0, 5.0]);
    let Entity::Line(scaled) = document.entities()[4] else {
        panic!("expected scaled copy")
    };
    near_point(scaled.start(), [1.0, 2.0, 3.0]);
    near_point(scaled.end(), [1.0, 8.0, 7.0]);
    let Entity::Line(mirrored) = document.entities()[5] else {
        panic!("expected reflected copy")
    };
    near_point(mirrored.start(), [-1.0, 2.0, 3.0]);
    near_point(mirrored.end(), [-1.0, 8.0, 7.0]);
    ui.command("MI 0,0 0,10 YES");
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("SELECT 1");
    ui.command("SCALE 0,0 -1");
    assert!(ui.has_text("the scale factor must be positive"));
    ui.command("ROTATE 0,0 NaN");
    assert!(ui.has_text("Numbers must be finite."));
    ui.command("ROTATE 0,0 360");
    assert!(ui.has_text("Selection unchanged."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("RO 1,2,3 -90 COPY");
    ui.command("SC 1,2,3 0.5");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 7);
    let Entity::Line(copy) = document.entities()[6] else {
        panic!("expected direct transform copy")
    };
    near_point(copy.start(), [1.0, 2.0, 3.0]);
    near_point(copy.end(), [1.0, 0.5, 4.0]);
}

#[test]
fn transform_pointer_tools_reference_lengths_and_keyboard_options_work() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pointer-transforms.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("OSNAP OFF");
    ui.command("LINE 0,0,7 10,0,7");
    ui.command("SELECT ALL");
    ui.click("Rotate");
    ui.canvas_click(0.5, 0.5); // Empty-drawing camera keeps the origin at center.
    ui.step(vec![key(egui::Key::C, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::C, egui::Modifiers::NONE, false)]);
    assert!(ui.has_text("Copy enabled."));
    ui.canvas_click(0.5, 0.25);
    ui.click("Scale");
    ui.canvas_click(0.5, 0.5);
    ui.canvas_click(0.6, 0.5);
    assert!(ui.has_text("Enter a positive scale factor, or Reference"));
    ui.step(vec![key(egui::Key::R, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::R, egui::Modifiers::NONE, false)]);
    ui.canvas_click(0.5, 0.5);
    ui.canvas_click(0.6, 0.5);
    ui.canvas_click(0.7, 0.5);
    ui.click("Mirror");
    ui.canvas_click(0.5, 0.4);
    ui.canvas_click(0.5, 0.6);
    ui.snapshot("mirror-preview");
    ui.step(vec![key(egui::Key::Y, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Y, egui::Modifiers::NONE, false)]);
    ui.command(&format!("SAVEAS {}", path.display()));
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        document.entities().len(),
        2,
        "Copy survives Scale and Mirror's erase-source choice"
    );
    let Entity::Line(original) = document.entities()[0] else {
        panic!("expected line")
    };
    assert_eq!(original.end(), Point3::new(10.0, 0.0, 7.0).unwrap());
    let Entity::Line(copy) = document.entities()[1] else {
        panic!("expected transformed line")
    };
    assert!(copy.start().x().abs() < 1e-6);
    assert!(copy.end().x().abs() < 1e-6);
    assert!(copy.start().y().abs() < 1e-6);
    assert!((copy.end().y() - 20.0).abs() < 1e-3);
    assert_eq!(copy.start().z(), 14.0);
    assert_eq!(copy.end().z(), 14.0);
}

#[test]
fn offset_commands_options_repetition_history_and_direct_forms_save_geometry() {
    use opencad::desktop::{menu_action_status, queue_menu_action};
    let scratch = Scratch::new();
    let path = scratch.0.join("offsets.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    assert!(menu_action_status(&ui.ctx, "ID_Offset").enabled);
    ui.command("LAYER NEW DETAIL");
    ui.command("LINE 0,0,7 10,0,7");
    ui.command("ENTITY COLOR 3");
    ui.command("CIRCLE 20,20,7 5");
    ui.command("ARC 40,20,7 3 0 90");
    ui.command("PLINE 50,0 60,0");
    ui.command("LAYER SET 0");
    ui.command("SELECT");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    queue_menu_action(&ui.ctx, "ID_Offset");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Offset"));
    ui.command("0");
    assert!(ui.has_text("the offset distance must be positive"));
    ui.command("2");
    ui.command("0");
    assert!(ui.has_text("Entity number is outside this drawing"));
    ui.command("4");
    assert!(ui.has_text("offset currently supports XY lines, circles and arcs"));
    ui.command("1");
    ui.command("0,3");
    ui.command("Erase");
    ui.command("Yes");
    ui.command("Layer");
    ui.command("Current");
    ui.command("1");
    ui.command("0,-5");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 5);
    for (index, y, layer) in [(3, 2.0, "DETAIL"), (4, -2.0, "0")] {
        let Entity::Line(line) = document.entities()[index] else {
            panic!("expected offset line")
        };
        assert_eq!(line.start(), Point3::new(0.0, y, 7.0).unwrap());
        assert_eq!(line.end(), Point3::new(10.0, y, 7.0).unwrap());
        assert_eq!(document.entity_layer(index), Some(layer));
        assert_eq!(document.entity_style(index).unwrap().color_index(), 3);
    }
    ui.command("Undo");
    ui.command("U");
    ui.command("Undo");
    assert!(ui.has_text("There are no offsets to undo in this command."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Erase");
    ui.command("No");
    ui.command("Layer");
    ui.command("Source");
    ui.command("Through");
    ui.command("2");
    ui.command("20,20");
    assert!(ui.has_text("the offset would collapse the curve's radius"));
    ui.command("24,20");
    ui.command("3");
    ui.command("40,28");
    ui.command_with_enter("");
    assert!(ui.has_text("Current tool: Select"));
    ui.command("OFFSET 5 1 0,20 CURRENT");
    ui.command("OFFSET THROUGH 1 0,-3 ERASE SOURCE");
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.entities().len(), 7);
    let Entity::Circle(circle) = document.entities()[3] else {
        panic!("expected concentric circle")
    };
    assert_eq!(circle.radius(), 4.0);
    assert_eq!(circle.center(), Point3::new(20.0, 20.0, 7.0).unwrap());
    let Entity::Arc(arc) = document.entities()[4] else {
        panic!("expected concentric arc")
    };
    assert_eq!(arc.radius(), 8.0);
    assert!((arc.sweep() - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    for (index, y, layer) in [(5, 5.0, "0"), (6, -3.0, "DETAIL")] {
        let Entity::Line(line) = document.entities()[index] else {
            panic!("expected direct offset line")
        };
        assert_eq!(line.start(), Point3::new(0.0, y, 7.0).unwrap());
        assert_eq!(document.entity_layer(index), Some(layer));
        assert_eq!(document.entity_style(index).unwrap().color_index(), 3);
    }
    ui.command("OFFSET -1 1 0,20");
    ui.command("OFFSET 1 1 0,20 CURRENT SOURCE");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
}

#[test]
fn offset_pointer_picking_preview_through_keyboard_undo_and_cancel_work() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pointer-offsets.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("OSNAP OFF");
    ui.command("LINE -40,0,7 40,0,7");
    ui.click("Offset");
    ui.command("10");
    ui.canvas_click(0.5, 0.5);
    let bounds = ui
        .nodes
        .values()
        .find(|node| {
            node.label()
                .is_some_and(|label| label.starts_with("Drawing canvas,"))
        })
        .and_then(|node| node.bounds())
        .unwrap();
    ui.step(vec![Event::PointerMoved(egui::pos2(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        (bounds.y0 + (bounds.y1 - bounds.y0) * 0.4) as f32,
    ))]);
    ui.snapshot("offset-preview");
    ui.canvas_click(0.5, 0.4);
    ui.canvas_click(0.5, 0.5);
    ui.canvas_click(0.5, 0.6);
    ui.step(vec![key(egui::Key::U, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::U, egui::Modifiers::NONE, false)]);
    assert!(ui.has_text("Offset undone."));
    ui.command("Through");
    ui.command("1");
    let bounds = ui
        .nodes
        .values()
        .find(|node| {
            node.label()
                .is_some_and(|label| label.starts_with("Drawing canvas,"))
        })
        .and_then(|node| node.bounds())
        .unwrap();
    let expected_y = (bounds.y1 - bounds.y0) * 0.2 / 4.0;
    ui.canvas_click(0.5, 0.3);
    ui.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, false)]);
    assert!(ui.has_text("Current tool: Select"));
    ui.command(&format!("SAVEAS {}", path.display()));
    let saved = fs::read_to_string(&path).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.entities().len(), 3);
    for (index, expected_y) in [(1, 10.0), (2, expected_y)] {
        let Entity::Line(line) = document.entities()[index] else {
            panic!("expected pointer offset")
        };
        assert!((line.start().y() - expected_y).abs() < 1e-4);
        assert_eq!(line.start().x(), -40.0);
        assert_eq!(line.end().x(), 40.0);
        assert_eq!(line.start().z(), 7.0);
        assert_eq!(line.end().z(), 7.0);
    }
    ui.command("O");
    ui.command_with_enter(""); // OpenCad's initial Enter selects Through.
    ui.command("1");
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(path).unwrap(), saved);
}

#[test]
fn trim_extend_commands_boundaries_conversion_history_and_direct_forms_work() {
    use opencad::desktop::{menu_action_status, queue_menu_action};
    let scratch = Scratch::new();
    let path = scratch.0.join("trim-extend.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    assert!(menu_action_status(&ui.ctx, "ID_Trim").enabled);
    assert!(menu_action_status(&ui.ctx, "ID_Extend").enabled);
    ui.command("LAYER NEW DETAIL");
    ui.command("LINE 0,0,7 10,0,7");
    ui.command("ENTITY COLOR 3");
    ui.command("LINE 3,-5,7 3,5,7");
    ui.command("LINE 7,-5,7 7,5,7");
    ui.command("CIRCLE 20,0,7 5");
    ui.command("LINE 20,-10,7 20,10,7");
    ui.command("CIRCLE 20,0,7 8");
    ui.command("ARC 40,0,7 5 0 90");
    ui.command("LINE 37,-10,7 37,10,7");
    ui.command("LAYER SET 0");
    ui.command("SELECT 2 3");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    queue_menu_action(&ui.ctx, "ID_Trim");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Trim"));
    ui.command_with_enter("");
    ui.command("1 5,0");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 9);
    for (index, start, end) in [(0, 0.0, 3.0), (8, 7.0, 10.0)] {
        let Entity::Line(line) = document.entities()[index] else {
            panic!("expected trimmed line")
        };
        assert!((line.start().x() - start).abs() < 1e-10);
        assert!((line.end().x() - end).abs() < 1e-10);
        assert_eq!(line.start().z(), 7.0);
        assert_eq!(document.entity_layer(index), Some("DETAIL"));
        assert_eq!(document.entity_style(index).unwrap().color_index(), 3);
    }
    ui.command("Undo");
    ui.command("U");
    assert!(ui.has_text("There are no trim/extend operations to undo in this command."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Cutting edges");
    ui.command("0");
    assert!(ui.has_text("Entity number is outside this drawing"));
    ui.command("5");
    ui.command_with_enter("");
    ui.command("4 25,0");
    // The explicit diameter moves from number 5 to 4 after circle conversion.
    ui.command("5 28,0");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 8);
    for (index, radius) in [(6, 5.0), (7, 8.0)] {
        let Entity::Arc(arc) = document.entities()[index] else {
            panic!("expected circle converted to arc")
        };
        assert_eq!(arc.center(), Point3::new(20.0, 0.0, 7.0).unwrap());
        assert_eq!(arc.radius(), radius);
        assert!((arc.start_angle() - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
        assert!((arc.sweep() - std::f64::consts::PI).abs() < 1e-10);
    }
    ui.command("Undo");
    ui.command("Undo");
    // Undo also restores the original explicit boundary number.
    ui.command("4 25,0");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Exit");
    ui.command("TRIM 1 5,0 2,3");
    ui.command("UNDO");
    ui.command("EXTEND 7 40,5 8");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let Entity::Arc(arc) = document.entities()[6] else {
        panic!("expected extended arc")
    };
    assert!((arc.end_point().unwrap().x() - 37.0).abs() < 1e-10);
    assert!((arc.end_point().unwrap().y() - 4.0).abs() < 1e-10);
    ui.command("UNDO");
    ui.command("LINE 15,5,7 15,10,7");
    ui.command("EXTEND 1 10,0 9");
    assert!(ui.has_text("no usable boundary intersection"));
    ui.command("EXTEND 1 10,0 9 EDGEEXTEND");
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    let Entity::Line(line) = document.entities()[0] else {
        panic!("expected extended line")
    };
    assert_eq!(line.end(), Point3::new(15.0, 0.0, 7.0).unwrap());
    ui.command("TRIM 0 5,0 ALL");
    ui.command("TRIM 1 3,0 ALL");
    ui.command("TRIM 1 5,0 ALL UNKNOWN");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(path).unwrap(), saved);
}

#[test]
fn trim_extend_pointer_preview_shift_edge_options_and_cancel_work() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pointer-trim-extend.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE -40,0,7 40,0,7");
    ui.command("LINE -20,-30,7 -20,30,7");
    ui.command("LINE 20,-30,7 20,30,7");
    ui.command("SELECT");
    ui.click("Trim");
    let position = |ui: &DrawingUi, x: f64, y: f64| {
        let bounds = ui
            .nodes
            .values()
            .find(|node| {
                node.label()
                    .is_some_and(|label| label.starts_with("Drawing canvas,"))
            })
            .and_then(|node| node.bounds())
            .unwrap();
        egui::pos2(
            ((bounds.x0 + bounds.x1) * 0.5 + 4.0 * x) as f32,
            ((bounds.y0 + bounds.y1) * 0.5 - 4.0 * y) as f32,
        )
    };
    let click = |ui: &mut DrawingUi, x: f64, y: f64, modifiers| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    click(&mut ui, -20.0, 20.0, egui::Modifiers::NONE);
    click(&mut ui, 20.0, 20.0, egui::Modifiers::NONE);
    ui.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, false)]);
    // Object snap would choose the boundary midpoint; trimming uses raw picks.
    let pos = position(&ui, 19.0, 0.0);
    ui.step(vec![Event::PointerMoved(pos)]);
    ui.snapshot("trim-preview");
    click(&mut ui, 19.0, 0.0, egui::Modifiers::NONE);
    assert!(ui.has_text("Curve trimmed."));
    ui.step(vec![key(egui::Key::U, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::U, egui::Modifiers::NONE, false)]);
    assert!(ui.has_text("Trim/Extend undone."));
    click(&mut ui, 30.0, 0.0, egui::Modifiers::NONE);
    // Shift in TRIM attempts extension rather than removing another portion.
    click(&mut ui, 19.0, 0.0, egui::Modifiers::SHIFT);
    // No change: the endpoint already lies on this boundary.
    assert!(ui.has_text("no usable boundary intersection"));
    ui.command("Undo");
    ui.command_with_enter("");
    assert!(ui.has_text("Current tool: Select"));
    ui.command("LINE 60,10,7 60,20,7");
    ui.command("SELECT");
    ui.custom_action("Trim", "Open command menu");
    ui.click("Extend");
    ui.command("4");
    ui.command_with_enter("");
    click(&mut ui, 39.0, 0.0, egui::Modifiers::NONE);
    assert!(ui.has_text("no usable boundary intersection"));
    ui.command("Edge");
    ui.command("Invalid");
    assert!(ui.has_text("Boundary Edge must be Extend or NoExtend"));
    ui.command("Extend");
    click(&mut ui, 39.0, 0.0, egui::Modifiers::NONE);
    assert!(ui.has_text("Curve extended."));
    ui.command("Cutting edges");
    ui.command("2 3");
    ui.command_with_enter("");
    click(&mut ui, 0.0, 0.0, egui::Modifiers::SHIFT);
    assert!(ui.has_text("Curve trimmed."));
    ui.command("U");
    ui.command("Undo");
    ui.command("Cutting edges");
    ui.command_with_enter(""); // An empty edge selection uses all visible curves.
    ui.command("1 0,0");
    ui.command("Undo");
    ui.command("All");
    ui.command("1 30,0");
    ui.command("Undo");
    ui.command("Edge");
    ui.command("NoExtend");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let document = DxfDocument::parse(&original).unwrap();
    assert_eq!(document.entities().len(), 4);
    let Entity::Line(line) = document.entities()[0] else {
        panic!("expected restored source line")
    };
    assert_eq!(line.start(), Point3::new(-40.0, 0.0, 7.0).unwrap());
    assert_eq!(line.end(), Point3::new(40.0, 0.0, 7.0).unwrap());
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn fillet_chamfer_commands_persist_dimensions_and_undo_complete_corner_edits() {
    use opencad::CornerParameter::{ChamferFirst, ChamferSecond, FilletRadius};
    use opencad::desktop::{menu_action_status, queue_menu_action};
    let scratch = Scratch::new();
    let path = scratch.0.join("corners.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE 5,0,7 40,0,7");
    ui.command("LINE 0,40,7 0,5,7");
    ui.command("FILLETRAD 6");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    assert!(menu_action_status(&ui.ctx, "ID_Fillet").enabled);
    assert!(menu_action_status(&ui.ctx, "ID_Chamfer").enabled);
    queue_menu_action(&ui.ctx, "ID_Fillet");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Fillet"));
    ui.command("Multiple");
    ui.command("Radius");
    ui.command("-1");
    assert!(ui.has_text("corner dimensions must be finite and nonnegative"));
    ui.command("8");
    ui.command("1 20,0");
    ui.command("1 30,0");
    assert!(ui.has_text("corners currently require two distinct XY lines"));
    ui.command("2 0,20");
    assert!(ui.has_text("Fillet created."));
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let Entity::Arc(arc) = document.entities()[2] else {
        panic!("expected fillet")
    };
    assert_eq!(arc.radius(), 8.0);
    assert!((arc.center().x() - 8.0).abs() < 1e-9);
    assert!((arc.center().y() - 8.0).abs() < 1e-9);
    assert_eq!(arc.center().z(), 7.0);
    ui.command("Undo");
    ui.command("Undo");
    ui.command("Undo");
    assert!(ui.has_text("There are no drawing edits to undo in this corner command."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Exit");
    ui.command("CHAMFER 2 4 1 20,0 2 0,20 NOTRIM");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        &document.entities()[..2],
        DxfDocument::parse(&original).unwrap().entities()
    );
    let Entity::Line(chamfer) = document.entities()[2] else {
        panic!("expected chamfer")
    };
    assert!((chamfer.start().x() - 2.0).abs() < 1e-9);
    assert!((chamfer.end().y() - 4.0).abs() < 1e-9);
    ui.command("UNDO");
    ui.command("CHA");
    ui.command("Distance");
    ui.command("3");
    ui.command("-5");
    assert!(ui.has_text("corner dimensions must be finite and nonnegative"));
    // The distance pair is staged until both values validate.
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("CHA");
    ui.command("Multiple");
    ui.command("Distance");
    ui.command("3");
    ui.command("5");
    ui.command("1 20,0");
    ui.command("2 0,20");
    ui.command("Undo");
    ui.command("Undo");
    ui.command("Exit");
    ui.command("FILLET 100 1 20,0 2 0,20 TRIM");
    ui.command("FILLET 2 1 20,0 1 30,0 TRIM");
    ui.command("CHAMFER -1 2 1 20,0 2 0,20");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("CHAMFERA 3");
    ui.command("CHAMFERB 5");
    ui.command("FILLETRAD 4");
    ui.command("SAVE");
    ui.command("CLOSEDRAWING");
    ui.click("corners.dxf");
    for (name, value) in [("FILLETRAD", 4), ("CHAMFERA", 3), ("CHAMFERB", 5)] {
        ui.command(name);
        assert!(ui.has_text(&format!("{name} = {value}")));
    }
    let saved = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved.corner_value(FilletRadius), 4.0);
    assert_eq!(saved.corner_value(ChamferFirst), 3.0);
    assert_eq!(saved.corner_value(ChamferSecond), 5.0);
    ui.command("FILLET");
    ui.command("Trim");
    ui.command("NoTrim");
    ui.command("Exit");
    let mut storage = PreferenceStorage::default();
    ui.app.save(&mut storage);
    drop(ui);
    let mut ui = DrawingUi::with_storage(Some(&storage));
    ui.click("New");
    ui.command("FILLETRAD");
    assert!(ui.has_text("FILLETRAD = 0"));
    ui.command("CHAMFERA");
    assert!(ui.has_text("CHAMFERA = 0"));
    ui.command("FILLET");
    assert!(ui.has_text("Radius 0; NoTrim"));
}

#[test]
fn fillet_chamfer_canvas_picks_preview_shift_keyboard_and_cancel_work() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pointer-corners.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE -50,0,7 50,0,7");
    ui.command("LINE 0,-40,7 0,40,7");
    ui.command("FILLETRAD 12");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let position = |ui: &DrawingUi, x: f64, y: f64| {
        let bounds = ui
            .nodes
            .values()
            .find(|node| {
                node.label()
                    .is_some_and(|label| label.starts_with("Drawing canvas,"))
            })
            .and_then(|node| node.bounds())
            .unwrap();
        egui::pos2(
            ((bounds.x0 + bounds.x1) * 0.5 + 4.0 * x) as f32,
            ((bounds.y0 + bounds.y1) * 0.5 - 4.0 * y) as f32,
        )
    };
    let click = |ui: &mut DrawingUi, x: f64, y: f64, modifiers| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    ui.click("Fillet");
    // Both picks lie near the lines' midpoint snap. Corner picking uses raw positions.
    click(&mut ui, 2.0, 0.0, egui::Modifiers::NONE);
    ui.step(vec![key(egui::Key::M, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::M, egui::Modifiers::NONE, false)]);
    let pos = position(&ui, 0.0, 2.0);
    ui.step(vec![Event::PointerMoved(pos)]);
    ui.snapshot("fillet-preview");
    click(&mut ui, 0.0, 2.0, egui::Modifiers::SHIFT);
    assert!(ui.has_text("Fillet created."));
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 2);
    assert_eq!(
        document.corner_value(opencad::CornerParameter::FilletRadius),
        12.0
    );
    for entity in document.entities() {
        let Entity::Line(line) = entity else {
            panic!("expected sharp sources")
        };
        assert_eq!(line.start(), Point3::new(0.0, 0.0, 7.0).unwrap());
    }
    // Return focus from the command field without picking another source.
    click(&mut ui, 30.0, 20.0, egui::Modifiers::NONE);
    ui.step(vec![key(egui::Key::U, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::U, egui::Modifiers::NONE, false)]);
    assert!(ui.has_text("Corner edit undone."));
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.custom_action("Fillet", "Open command menu");
    ui.click("Chamfer");
    click(&mut ui, 30.0, 20.0, egui::Modifiers::NONE);
    ui.step(vec![key(egui::Key::D, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::D, egui::Modifiers::NONE, false)]);
    ui.command("3");
    ui.command("6");
    ui.command("Trim");
    ui.command("invalid");
    assert!(ui.has_text("Enter Trim or NoTrim."));
    ui.command("NoTrim");
    click(&mut ui, 30.0, 0.0, egui::Modifiers::NONE);
    let pos = position(&ui, 0.0, 30.0);
    ui.step(vec![Event::PointerMoved(pos)]);
    ui.snapshot("chamfer-preview");
    click(&mut ui, 0.0, 30.0, egui::Modifiers::NONE);
    assert!(ui.has_text("Chamfer created."));
    assert!(ui.has_text("Current tool: Select"));
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(
        &document.entities()[..2],
        DxfDocument::parse(&original).unwrap().entities()
    );
    let Entity::Line(chamfer) = document.entities()[2] else {
        panic!("expected chamfer")
    };
    assert_eq!(chamfer.start(), Point3::new(3.0, 0.0, 7.0).unwrap());
    assert_eq!(chamfer.end(), Point3::new(0.0, 6.0, 7.0).unwrap());
    ui.command("FILLET");
    click(&mut ui, 30.0, 0.0, egui::Modifiers::NONE);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
}

#[test]
fn break_commands_override_points_split_convert_preserve_style_and_undo_atomically() {
    use opencad::desktop::{menu_action_status, queue_menu_action};
    let scratch = Scratch::new();
    let path = scratch.0.join("break-commands.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    for action in ["ID_Break", "ID_BreakSele"] {
        assert!(menu_action_status(&ui.ctx, action).enabled);
    }
    ui.command("LAYER NEW DETAIL");
    ui.command("LINE 0,0,7 40,0,7");
    ui.command("ENTITY COLOR 3");
    ui.command("CIRCLE 80,0,7 10");
    ui.command("ARC 120,0,7 10 270 90");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    queue_menu_action(&ui.ctx, "ID_Break");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Break"));
    ui.command("1 8,20");
    ui.command("First");
    ui.command("12,20");
    ui.command("28,-30");
    assert!(ui.has_text("Curve broken."));
    assert!(ui.has_text("Current tool: Select"));
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.entities().len(), 4);
    for (index, start, end) in [(0, 0.0, 12.0), (3, 28.0, 40.0)] {
        let Entity::Line(line) = document.entities()[index] else {
            panic!("expected broken line");
        };
        assert!((line.start().x() - start).abs() < 1e-9);
        assert!((line.end().x() - end).abs() < 1e-9);
        assert_eq!(line.start().z(), 7.0);
        assert_eq!(document.entity_layer(index), Some("DETAIL"));
        assert_eq!(document.entity_style(index).unwrap().color_index(), 3);
    }
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("REDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("UNDO");
    queue_menu_action(&ui.ctx, "ID_BreakSele");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Break at Point"));
    ui.command("1");
    ui.command("@");
    assert!(ui.has_text("BREAKATPOINT requires an absolute break point."));
    ui.command("20,15");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let (Entity::Line(first), Entity::Line(second)) =
        (&document.entities()[0], &document.entities()[3])
    else {
        panic!("expected split lines");
    };
    assert_eq!(first.end(), second.start());
    assert_eq!(first.end(), Point3::new(20.0, 0.0, 7.0).unwrap());
    ui.command("UNDO");
    for command in [
        "BREAK 1 10,30 @",
        "BREAK 1 10,30 @0,0",
        "BREAKATPOINT 1 10,30",
    ] {
        ui.command(command);
        ui.command("SAVE");
        let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(document.entities().len(), 4);
        let Entity::Line(line) = document.entities()[0] else {
            panic!("expected split line");
        };
        assert_eq!(line.end(), Point3::new(10.0, 0.0, 7.0).unwrap());
        ui.command("UNDO");
    }
    ui.command("BREAK 2 90,0 80,10");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let Entity::Arc(arc) = document.entities()[2] else {
        panic!("expected converted circle");
    };
    assert_eq!(arc.center(), Point3::new(80.0, 0.0, 7.0).unwrap());
    assert!((arc.start_angle() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    assert!((arc.sweep() - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    ui.command("UNDO");
    ui.command("BREAK 3 130,0 @0,0");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 4);
    for index in [2, 3] {
        let Entity::Arc(arc) = document.entities()[index] else {
            panic!("expected split arc");
        };
        assert!((arc.sweep() - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }
    ui.command("UNDO");
    for command in [
        "BREAKATPOINT 2 90,0",
        "BREAKATPOINT 1 0,0",
        "BREAK 0 10,0 20,0",
        "BREAK 1 NaN,0 20,0",
        "BREAK 1 10,0",
        "BREAK 2 90,0 @",
    ] {
        ui.command(command);
    }
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("LAYER LOCK DETAIL");
    ui.command("BREAK 1 10,0 20,0");
    assert!(ui.has_text("unlock layer DETAIL before editing it"));
    ui.command("UNDO");
    ui.command("LAYER HIDE DETAIL");
    ui.command("BREAK 1 10,0 20,0");
    assert!(ui.has_text("Break sources must be visible."));
    ui.command("UNDO");
    ui.command("BREAK");
    ui.command("First");
    assert!(ui.has_text("Select a curve before overriding the first point."));
    ui.command("1");
    ui.command("10,0");
    ui.command_with_enter("");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn break_canvas_raw_picks_first_override_preview_dropdown_and_cancel_work() {
    let scratch = Scratch::new();
    let path = scratch.0.join("break-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE -40,0,7 40,0,7");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let position = |ui: &DrawingUi, x: f64, y: f64| {
        let bounds = ui
            .nodes
            .values()
            .find(|node| {
                node.label()
                    .is_some_and(|label| label.starts_with("Drawing canvas,"))
            })
            .and_then(|node| node.bounds())
            .unwrap();
        egui::pos2(
            ((bounds.x0 + bounds.x1) * 0.5 + 4.0 * x) as f32,
            ((bounds.y0 + bounds.y1) * 0.5 - 4.0 * y) as f32,
        )
    };
    let click = |ui: &mut DrawingUi, x: f64, y: f64| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        ui.step(Vec::new());
    };
    // The native dropdown defaults to Break at Point; choose Break via its accessible menu.
    ui.custom_action("Break at Point", "Open command menu");
    ui.click("Break");
    click(&mut ui, 2.0, 0.0); // Near the midpoint snap; retain the raw source pick.
    let pos = position(&ui, 20.0, 15.0);
    ui.step(vec![Event::PointerMoved(pos)]);
    ui.snapshot("break-gap-preview");
    click(&mut ui, 20.0, 15.0); // No entity here; project onto the selected source.
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 2);
    let (Entity::Line(first), Entity::Line(second)) =
        (&document.entities()[0], &document.entities()[1])
    else {
        panic!("expected broken lines");
    };
    assert!((first.end().x() - 2.0).abs() < 1e-4);
    assert!((second.start().x() - 20.0).abs() < 1e-4);
    ui.command("UNDO");
    ui.command("BREAK");
    click(&mut ui, -20.0, 0.0);
    ui.step(vec![key(egui::Key::F, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::F, egui::Modifiers::NONE, false)]);
    click(&mut ui, -10.0, 20.0);
    ui.command("@0,0");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let (Entity::Line(first), Entity::Line(second)) =
        (&document.entities()[0], &document.entities()[1])
    else {
        panic!("expected split lines");
    };
    assert_eq!(first.end(), second.start());
    assert!((first.end().x() + 10.0).abs() < 1e-4);
    ui.command("UNDO");
    ui.custom_action("Break at Point", "Open command menu");
    ui.click("Break at Point");
    click(&mut ui, -25.0, 0.0);
    let pos = position(&ui, 10.0, 20.0);
    ui.step(vec![Event::PointerMoved(pos)]);
    ui.snapshot("break-at-point-preview");
    click(&mut ui, 10.0, 20.0);
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let (Entity::Line(first), Entity::Line(second)) =
        (&document.entities()[0], &document.entities()[1])
    else {
        panic!("expected split lines");
    };
    assert_eq!(first.end(), second.start());
    assert!((first.end().x() - 10.0).abs() < 1e-4);
    ui.command("UNDO");
    ui.command("BREAK");
    click(&mut ui, -30.0, 0.0);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn lengthen_commands_measure_change_repeat_and_undo_without_losing_source_style() {
    use opencad::desktop::{menu_action_status, queue_menu_action};
    let scratch = Scratch::new();
    let path = scratch.0.join("lengthen-commands.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LAYER NEW DETAIL");
    ui.command("LINE 0,0,7 40,0,7");
    ui.command("ENTITY COLOR 3");
    ui.command("CIRCLE 80,0,7 10");
    ui.command("ARC 120,0,7 10 270 90");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(menu_action_status(&ui.ctx, "ID_Lengthen").enabled);
    queue_menu_action(&ui.ctx, "ID_Lengthen");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Lengthen"));
    ui.command("1 40,0");
    assert!(ui.has_text("Length = 40.0000"));
    ui.command("3 130,0");
    assert!(ui.has_text("included angle = 180.0000 degrees"));
    ui.command("DE");
    ui.command("NaN");
    assert!(ui.has_text("Numbers must be finite."));
    ui.command("-10");
    ui.command("1 40,0");
    ui.command("1 30,0");
    ui.command("SAVE");
    let Entity::Line(line) = read().entities()[0] else {
        panic!("expected line")
    };
    assert_eq!(line.end(), Point3::new(20.0, 0.0, 7.0).unwrap());
    ui.command("U");
    ui.command("U");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("P");
    ui.command("A");
    assert!(ui.has_text("Angle is available with Delta or Total."));
    ui.command("50");
    ui.command("1 40,0");
    ui.command("T");
    ui.command("5");
    ui.command("1 0,0");
    ui.command("SAVE");
    let document = read();
    let Entity::Line(line) = document.entities()[0] else {
        panic!("expected line")
    };
    assert_eq!(line.start(), Point3::new(15.0, 0.0, 7.0).unwrap());
    assert_eq!(line.end(), Point3::new(20.0, 0.0, 7.0).unwrap());
    assert_eq!(document.entity_style(0).unwrap().color_index(), 3);
    assert_eq!(document.entity_layer(0), Some("DETAIL"));
    ui.command("Undo");
    ui.command("Undo");
    ui.command("Total");
    ui.command("Angle");
    ui.command("90");
    ui.command("3 120,-10");
    ui.command("Delta");
    ui.command("Angle");
    ui.command("45");
    ui.command("3 130,0");
    ui.command("SAVE");
    let Entity::Arc(arc) = read().entities()[2] else {
        panic!("expected arc")
    };
    assert!((arc.sweep().to_degrees() - 135.0).abs() < 1e-9);
    assert_eq!(arc.end_angle(), std::f64::consts::FRAC_PI_2);
    assert_eq!(arc.center().z(), 7.0);
    ui.command("U");
    ui.command("U");
    ui.command("Exit");
    for (command, start, end) in [
        ("LEN DELTA 5 1 40,0", 0.0, 45.0),
        ("LENGTHEN PERCENT 50 1 0,0", 20.0, 40.0),
        ("LENGTHEN TOTAL 10 1 40,0", 0.0, 10.0),
        ("LENGTHEN DYNAMIC 1 0,0 -10,15,-8", -10.0, 40.0),
    ] {
        ui.command(command);
        ui.command("SAVE");
        let Entity::Line(line) = read().entities()[0] else {
            panic!("expected line")
        };
        assert_eq!(line.start(), Point3::new(start, 0.0, 7.0).unwrap());
        assert_eq!(line.end(), Point3::new(end, 0.0, 7.0).unwrap());
        ui.command("UNDO");
    }
    for (command, sweep) in [
        ("LENGTHEN DELTA ANGLE -90 3 120,-10", 90.0),
        ("LENGTHEN TOTAL ANGLE 270 3 120,10", 270.0),
    ] {
        ui.command(command);
        ui.command("SAVE");
        let Entity::Arc(arc) = read().entities()[2] else {
            panic!("expected arc")
        };
        assert!((arc.sweep().to_degrees() - sweep).abs() < 1e-9);
        ui.command("UNDO");
    }
    for command in [
        "LENGTHEN PERCENT 200 2 90,0", // Closed circles are a no-op.
        "LENGTHEN DELTA -40 1 40,0",
        "LENGTHEN PERCENT 0 1 40,0",
        "LENGTHEN TOTAL inf 1 40,0",
        "LENGTHEN TOTAL ANGLE 90 1 40,0",
        "LENGTHEN DYNAMIC 1 40,0 -10,0",
        "LENGTHEN TOTAL 10 0 40,0",
        "LENGTHEN DELTA 5 1",
        "LENGTHEN TOTAL ANGLE 360 3 120,10",
    ] {
        ui.command(command);
    }
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("LAYER LOCK DETAIL");
    ui.command("LENGTHEN DELTA 5 1 40,0");
    assert!(ui.has_text("unlock layer DETAIL before editing it"));
    ui.command("UNDO");
    ui.command("LAYER HIDE DETAIL");
    ui.command("LENGTHEN DELTA 5 1 40,0");
    assert!(ui.has_text("Lengthen sources must be visible."));
    ui.command("UNDO");
    ui.command("LEN");
    ui.command("DE");
    ui.command("SAVE"); // A complete single-word command must leave the value prompt available.
    ui.command("LINE");
    assert!(ui.has_text("Current tool: Line"));
    ui.command("LEN");
    assert!(ui.has_text("Select a curve to measure"));
    ui.command("DE");
    ui.command("5");
    ui.command("1 40,0");
    ui.command("ENTITY COLOR 2");
    assert!(ui.has_text("Current tool: Select"));
    ui.command("LEN");
    ui.command("U");
    assert!(ui.has_text("There are no lengthen edits to undo in this command."));
    ui.command_with_enter("");
    assert!(ui.has_text("Current tool: Select"));
    ui.command("UNDO"); // Revert the color; retain the length edit for reopening.
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    ui.command(&format!("OPEN {}", path.display()));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    let Entity::Line(line) = read().entities()[0] else {
        panic!("expected line")
    };
    assert_eq!(line.end().x(), 45.0);
}

#[test]
fn lengthen_canvas_uses_raw_picks_keyboard_options_and_dynamic_clicks_and_dragging() {
    let scratch = Scratch::new();
    let path = scratch.0.join("lengthen-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE -40,0,7 40,0,7");
    ui.command("ARC 60,40,7 15 270 90");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let position = |ui: &DrawingUi, x: f64, y: f64| {
        let bounds = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        egui::pos2(
            bounds.center().x + 4.0 * x as f32,
            bounds.center().y - 4.0 * y as f32,
        )
    };
    let click = |ui: &mut DrawingUi, x: f64, y: f64| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        ui.step(Vec::new());
    };
    let press = |ui: &mut DrawingUi, value: egui::Key| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    ui.command("LEN");
    click(&mut ui, 2.0, 0.0);
    assert!(ui.has_text("Length = 80.0000"));
    press(&mut ui, egui::Key::D);
    assert!(ui.has_text("Specify signed delta length"));
    ui.command("10");
    let pos = position(&ui, 2.0, 0.0);
    ui.step(vec![Event::PointerMoved(pos)]);
    ui.snapshot("lengthen-delta-preview");
    click(&mut ui, 2.0, 0.0); // A midpoint snap here would incorrectly make this pick ambiguous.
    ui.command("SAVE");
    let Entity::Line(line) = read().entities()[0] else {
        panic!("expected line")
    };
    assert!((line.end().x() - 50.0).abs() < 1e-4);
    ui.command("U");
    ui.command("LEN");
    click(&mut ui, 2.0, 0.0);
    press(&mut ui, egui::Key::P);
    assert!(ui.has_text("Specify a positive percentage"));
    ui.command("50");
    click(&mut ui, 2.0, 0.0);
    ui.command("SAVE");
    let Entity::Line(line) = read().entities()[0] else {
        panic!("expected line")
    };
    assert!((line.end().x()).abs() < 1e-4);
    click(&mut ui, -30.0, 0.0); // Restore canvas keyboard focus without changing the requested mode.
    press(&mut ui, egui::Key::U);
    press(&mut ui, egui::Key::U);
    press(&mut ui, egui::Key::T);
    assert!(ui.has_text("Specify a positive total length"));
    press(&mut ui, egui::Key::A);
    assert!(ui.has_text("Specify the angle in degrees"));
    ui.command("90");
    click(&mut ui, 60.0, 25.0);
    ui.command("SAVE");
    let Entity::Arc(arc) = read().entities()[1] else {
        panic!("expected arc")
    };
    assert!((arc.sweep().to_degrees() - 90.0).abs() < 1e-9);
    ui.command("U");
    ui.command("DY");
    click(&mut ui, 35.0, 0.0);
    let pos = position(&ui, 55.0, 18.0);
    ui.step(vec![Event::PointerMoved(pos)]);
    ui.snapshot("lengthen-dynamic-line-preview");
    click(&mut ui, 55.0, 18.0);
    ui.command("SAVE");
    let Entity::Line(line) = read().entities()[0] else {
        panic!("expected line")
    };
    assert!((line.end().x() - 55.0).abs() < 1e-4);
    assert_eq!(line.end().y(), 0.0);
    assert_eq!(line.end().z(), 7.0);
    ui.command("U");
    // Dynamic also supports a single pointer drag, using the original press for endpoint selection.
    let from = position(&ui, -35.0, 0.0);
    ui.step(vec![
        Event::PointerMoved(from),
        Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    let to = position(&ui, -60.0, -15.0);
    ui.step(vec![Event::PointerMoved(to)]);
    ui.step(Vec::new());
    ui.snapshot("lengthen-dynamic-drag-preview");
    ui.step(vec![Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    ui.step(Vec::new());
    ui.command("SAVE");
    let Entity::Line(line) = read().entities()[0] else {
        panic!("expected line")
    };
    assert!((line.start().x() + 60.0).abs() < 1e-4);
    assert_eq!(line.end(), Point3::new(40.0, 0.0, 7.0).unwrap());
    ui.command("U");
    click(&mut ui, 60.0, 25.0);
    let pos = position(&ui, 90.0, 40.0);
    ui.step(vec![Event::PointerMoved(pos)]);
    ui.snapshot("lengthen-dynamic-arc-preview");
    click(&mut ui, 90.0, 40.0);
    ui.command("SAVE");
    let Entity::Arc(arc) = read().entities()[1] else {
        panic!("expected arc")
    };
    assert!((arc.sweep().to_degrees() - 90.0).abs() < 1e-4);
    assert_eq!(arc.end_angle(), std::f64::consts::FRAC_PI_2);
    ui.command("U");
    click(&mut ui, -35.0, 0.0);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn explode_commands_convert_selected_polylines_atomically_and_preserve_styles_on_reload() {
    use opencad::desktop::{menu_action_status, queue_menu_action};
    use opencad::{Circle3, Line3, Polyline2};
    let scratch = Scratch::new();
    let path = scratch.0.join("explode-commands.dxf");
    let p = |x, y| Point3::new(x, y, 7.0).unwrap();
    let mut document = DxfDocument::default();
    document.push(Entity::Line(Line3::new(p(-100.0, -30.0), p(-80.0, -30.0))));
    document.push(Entity::Polyline(
        Polyline2::with_bulges(
            vec![p(-40.0, 0.0), p(-10.0, 0.0), p(-10.0, 20.0)],
            vec![1.0, 0.0, 0.0],
            false,
        )
        .unwrap(),
    ));
    document.push(Entity::Polyline(
        Polyline2::with_bulges(
            vec![p(20.0, 0.0), p(40.0, 0.0), p(40.0, 20.0), p(20.0, 20.0)],
            vec![0.0, (std::f64::consts::PI / 8.0).tan(), 0.0, 0.0],
            true,
        )
        .unwrap(),
    ));
    document.push(Entity::Circle(Circle3::new(p(80.0, 0.0), 5.0).unwrap()));
    for (index, layer, color, scale) in [(1, "DETAIL", 3, 2.0), (2, "FRAME", 5, 0.5)] {
        document.add_layer(layer).unwrap();
        document.set_entity_layer(index, layer).unwrap();
        document.set_entity_color(index, color).unwrap();
        document.set_entity_linetype_scale(index, scale).unwrap();
    }
    let original = document.to_dxf();
    fs::write(&path, &original).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command(&format!("OPEN {}", path.display()));
    assert!(menu_action_status(&ui.ctx, "ID_Explode").enabled);
    queue_menu_action(&ui.ctx, "ID_Explode");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Explode"));
    ui.command("2 3");
    assert!(ui.has_text("2 selected"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command_with_enter("");
    assert!(ui.has_text("2 polylines exploded into 6 components."));
    assert!(ui.has_text("Current tool: Select"));
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    let exploded = read();
    assert_eq!(exploded.entities().len(), 8);
    assert!(
        exploded
            .entities()
            .iter()
            .all(|entity| !matches!(entity, Entity::Polyline(_)))
    );
    assert_eq!(
        &exploded.entities()[..2],
        &[
            document.entities()[0].clone(),
            document.entities()[3].clone()
        ]
    );
    assert_eq!(
        exploded
            .entities()
            .iter()
            .filter(|entity| matches!(entity, Entity::Arc(_)))
            .count(),
        2
    );
    for index in 2..8 {
        let source = if index < 4 { 1 } else { 2 };
        assert_eq!(exploded.entity_layer(index), document.entity_layer(source));
        assert_eq!(exploded.entity_style(index), document.entity_style(source));
    }
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("REDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("UNDO");
    ui.command("SELECT 2");
    ui.command("X"); // Existing selection executes immediately.
    assert!(ui.has_text("1 polylines exploded into 2 components."));
    ui.command("SAVE");
    assert_eq!(read().entities().len(), 5);
    ui.command("UNDO");
    ui.command("EXPLODE 3 2 3 1"); // Deduplicate sources; leave the simple LINE unchanged.
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("UNDO");
    for command in [
        "EXPLODE 0 2",
        "EXPLODE 2 99",
        "EXPLODE 2 not-an-index",
        "EXPLODE 4",
    ] {
        ui.command(command);
    }
    assert!(ui.has_text("No polylines selected; geometry unchanged."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    for (action, layer) in [("LOCK", "FRAME"), ("HIDE", "DETAIL")] {
        ui.command(&format!("LAYER {action} {layer}"));
        ui.command("SAVE");
        let protected = fs::read_to_string(&path).unwrap();
        ui.command("EXPLODE 2 3");
        assert!(ui.has_text(if action == "LOCK" {
            "unlock layer FRAME before editing it"
        } else {
            "Explode sources must be visible."
        }));
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&path).unwrap(), protected);
        ui.command("UNDO");
    }
    ui.command("SELECT");
    ui.command("EXPLODE");
    ui.command("ALL");
    assert!(ui.has_text("4 selected"));
    ui.command_with_enter("");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command(&format!("OPEN {}", path.display()));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
}

#[test]
fn explode_canvas_selects_curves_and_windows_with_shift_removal_enter_and_escape() {
    let scratch = Scratch::new();
    let path = scratch.0.join("explode-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.screen_size = egui::vec2(1440.0, 1120.0);
    ui.click("New");
    ui.command("PLINE -40,0,7 -10,0,7 -10,20,7");
    ui.replace_field("Vertex 1 bulge", "1");
    ui.click("Apply properties");
    ui.command("PLINE 20,0,7 40,0,7 40,20,7 CLOSE");
    ui.command("CIRCLE 60,-30,7 5");
    ui.command("SELECT");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let position = |ui: &DrawingUi, x: f64, y: f64| {
        let bounds = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        egui::pos2(
            bounds.center().x + 4.0 * x as f32,
            bounds.center().y - 4.0 * y as f32,
        )
    };
    let click = |ui: &mut DrawingUi, x: f64, y: f64, modifiers| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    let drag = |ui: &mut DrawingUi, from: (f64, f64), to: (f64, f64), modifiers| {
        let from = position(ui, from.0, from.1);
        let to = position(ui, to.0, to.1);
        ui.step(vec![
            Event::PointerMoved(from),
            Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerMoved(to)]);
        ui.step(vec![Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    let press = |ui: &mut DrawingUi, value: egui::Key| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    ui.click("Explode");
    click(&mut ui, -25.0, -15.0, egui::Modifiers::NONE); // The curved locus, away from its chord.
    click(&mut ui, 30.0, 0.0, egui::Modifiers::NONE);
    click(&mut ui, 65.0, -30.0, egui::Modifiers::NONE);
    assert!(ui.has_text("3 selected"));
    click(&mut ui, 65.0, -30.0, egui::Modifiers::SHIFT);
    click(&mut ui, -25.0, -15.0, egui::Modifiers::NONE); // Repeated picks add only once.
    assert!(ui.has_text("2 selected"));
    click(&mut ui, -25.0, -15.0, egui::Modifiers::SHIFT);
    assert!(ui.has_text("1 selected"));
    click(&mut ui, -25.0, -15.0, egui::Modifiers::NONE);
    ui.snapshot("explode-selected-curves");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    click(&mut ui, -25.0, -15.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Enter);
    ui.snapshot("explode-components");
    assert!(ui.has_text("2 polylines exploded into 5 components."));
    ui.command("SAVE");
    assert_eq!(read().entities().len(), 6);
    assert_eq!(
        read()
            .entities()
            .iter()
            .filter(|entity| matches!(entity, Entity::Arc(_)))
            .count(),
        1
    );
    ui.command("UNDO");
    ui.command("SELECT");
    ui.click("Explode");
    drag(&mut ui, (-50.0, 30.0), (45.0, -25.0), egui::Modifiers::NONE);
    assert!(ui.has_text("2 selected"));
    drag(
        &mut ui,
        (-45.0, 25.0),
        (-5.0, -20.0),
        egui::Modifiers::SHIFT,
    );
    assert!(ui.has_text("1 selected"));
    press(&mut ui, egui::Key::Enter);
    ui.command("SAVE");
    let result = read();
    assert_eq!(result.entities().len(), 5);
    assert!(matches!(result.entities()[0], Entity::Polyline(_)));
    ui.command("UNDO");
    ui.command("SELECT");
    ui.click("Explode");
    click(&mut ui, -25.0, -15.0, egui::Modifiers::NONE);
    click(&mut ui, 0.0, -50.0, egui::Modifiers::NONE);
    assert!(ui.has_text("1 selected"));
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("SELECT");
    ui.command("EXPLODE");
    ui.command("1 2");
    ui.command("Exit");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn join_commands_stage_sources_filter_candidates_preserve_style_and_close_arcs() {
    use opencad::desktop::{menu_action_status, queue_menu_action};
    use opencad::{Arc3, Circle3, Line3};
    let scratch = Scratch::new();
    let path = scratch.0.join("join-commands.dxf");
    let p = |x, y| Point3::new(x, y, 7.0).unwrap();
    let mut document = DxfDocument::default();
    for (a, b) in [
        ((-5.0, 5.0), (0.0, 5.0)),
        ((10.0, 5.0), (20.0, 5.0)),
        ((0.0, 10.0), (5.0, 10.0)),
        ((40.0, 5.0), (30.0, 5.0)),
    ] {
        if document.entities().len() == 1 {
            document.push(Entity::Circle(Circle3::new(p(70.0, 0.0), 5.0).unwrap()));
        }
        document.push(Entity::Line(Line3::new(p(a.0, a.1), p(b.0, b.1))));
    }
    for (start, end) in [(300_f64, 330_f64), (350.0, 20.0), (20.0, 300.0)] {
        document.push(Entity::Arc(
            Arc3::new(p(50.0, -20.0), 10.0, start.to_radians(), end.to_radians()).unwrap(),
        ));
    }
    document.add_layer("DETAIL").unwrap();
    document.add_layer("TARGET").unwrap();
    for (index, layer, color, scale) in [
        (2, "DETAIL", 3, 2.0),
        (5, "DETAIL", 5, 0.5),
        (0, "TARGET", 1, 1.0),
        (4, "TARGET", 1, 1.0),
    ] {
        document.set_entity_layer(index, layer).unwrap();
        document.set_entity_color(index, color).unwrap();
        document.set_entity_linetype_scale(index, scale).unwrap();
    }
    let original = document.to_dxf();
    fs::write(&path, &original).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command(&format!("OPEN {}", path.display()));
    assert!(menu_action_status(&ui.ctx, "ID_Join").enabled);
    queue_menu_action(&ui.ctx, "ID_Join");
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Join"));
    ui.command("3");
    ui.command_with_enter("");
    assert!(ui.has_text("Source stays selected"));
    ui.command("1 5 2 4 1");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command_with_enter("");
    assert!(ui.has_text("2 objects joined to source."));
    assert!(ui.has_text("Current tool: Select"));
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    let joined = read();
    assert_eq!(joined.entities().len(), 6);
    assert_eq!(
        joined.entities()[1],
        Entity::Line(Line3::new(p(-5.0, 5.0), p(40.0, 5.0)))
    );
    assert_eq!(joined.entity_layer(1), document.entity_layer(2));
    assert_eq!(joined.entity_style(1), document.entity_style(2));
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("REDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("UNDO");
    ui.command("J 3 5 1 5 2");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("UNDO");
    for command in [
        "JOIN 0 3",
        "JOIN 3 99",
        "JOIN 3 invalid",
        "JOIN 3 CLOSE",
        "JOIN 3 1 CLOSE",
        "JOIN 6 7 8",
    ] {
        ui.command(command);
        if command == "JOIN 3 CLOSE" {
            assert!(ui.has_text("Join cLose requires an arc"));
        }
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
    assert!(ui.has_text("this arc join requires a full turn"));
    for action in ["LOCK", "HIDE"] {
        ui.command(&format!("LAYER {action} TARGET"));
        ui.command("SAVE");
        let protected = fs::read_to_string(&path).unwrap();
        ui.command("JOIN 3 1 5");
        assert!(ui.has_text(if action == "LOCK" {
            "unlock layer TARGET before editing it"
        } else {
            "Join objects must be visible."
        }));
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&path).unwrap(), protected);
        ui.command("UNDO");
    }
    ui.command("SELECT 3");
    ui.command("JOIN"); // One preselected object starts source mode.
    assert!(ui.has_text("Source stays selected"));
    ui.command("1 5");
    ui.command_with_enter("");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("UNDO");
    ui.command("SELECT 3 1 5");
    ui.command("J"); // Multiple preselection uses the lowest supported source index.
    ui.command("SAVE");
    assert_eq!(read().entities().len(), 6);
    assert_eq!(read().entity_style(0), document.entity_style(0));
    ui.command("UNDO");
    ui.command("SELECT");
    ui.command("JOIN");
    ui.command("ALL");
    ui.command_with_enter(""); // Only the source's compatible group is consumed.
    ui.command("SAVE");
    assert_eq!(read().entities().len(), 6);
    ui.command("UNDO");
    ui.command("JOIN 6 7");
    ui.command("SAVE");
    let arcs = read();
    let Entity::Arc(joined_arc) = arcs.entities()[5] else {
        panic!("arc")
    };
    assert!((joined_arc.sweep().to_degrees() - 80.0).abs() < 1e-9);
    assert_eq!(arcs.entity_style(5), document.entity_style(5));
    ui.command("UNDO");
    ui.command("JOIN 6 CLOSE");
    ui.command("SAVE");
    let closed = fs::read_to_string(&path).unwrap();
    assert!(ui.has_text("Arc closed into a circle."));
    assert!(matches!(read().entities()[7], Entity::Circle(_)));
    assert_eq!(read().entity_style(7), document.entity_style(5));
    assert_eq!(read().entity_layer(7), document.entity_layer(5));
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("REDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), closed);
    ui.command(&format!("OPEN {}", path.display()));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), closed);
}

#[test]
fn join_canvas_confirms_source_selects_curves_and_windows_and_handles_close_and_cancel_keys() {
    let scratch = Scratch::new();
    let path = scratch.0.join("join-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.screen_size = egui::vec2(1440.0, 1120.0);
    ui.click("New");
    for command in [
        "LINE -60,-10,7 -40,-10,7",
        "LINE -20,-10,7 0,-10,7",
        "LINE 20,-10,7 40,-10,7",
        "LINE 60,-10,7 60,10,7",
        "ARC 0,35,7 10 0 90",
        "ARC 0,35,7 10 180 270",
    ] {
        ui.command(command);
    }
    ui.command("SELECT");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(read().entities().len(), 6);
    let position = |ui: &DrawingUi, x: f64, y: f64| {
        let bounds = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        egui::pos2(
            bounds.center().x + 4.0 * x as f32,
            bounds.center().y - 4.0 * y as f32,
        )
    };
    let click = |ui: &mut DrawingUi, x: f64, y: f64, modifiers| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    let drag = |ui: &mut DrawingUi, from: (f64, f64), to: (f64, f64), modifiers| {
        let from = position(ui, from.0, from.1);
        let to = position(ui, to.0, to.1);
        ui.step(vec![
            Event::PointerMoved(from),
            Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerMoved(to)]);
        ui.step(vec![Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    ui.click("Join");
    click(&mut ui, -10.0, -10.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Enter);
    assert!(ui.has_text("Source stays selected"));
    click(&mut ui, -10.0, -10.0, egui::Modifiers::SHIFT);
    assert!(ui.has_text("1 selected")); // A confirmed source cannot be removed.
    for (x, y) in [(30.0, -10.0), (-50.0, -10.0), (60.0, 0.0)] {
        click(&mut ui, x, y, egui::Modifiers::NONE);
    }
    assert!(ui.has_text("4 selected"));
    click(&mut ui, 60.0, 0.0, egui::Modifiers::SHIFT);
    ui.snapshot("join-selected-gapped-lines");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    click(&mut ui, -10.0, -10.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Enter);
    assert!(ui.has_text("2 objects joined to source."));
    ui.snapshot("join-line-envelope");
    ui.command("SAVE");
    assert_eq!(read().entities().len(), 4);
    let Entity::Line(joined) = read().entities()[0] else {
        panic!("line")
    };
    assert_eq!(joined.start(), Point3::new(-60.0, -10.0, 7.0).unwrap());
    assert_eq!(joined.end(), Point3::new(40.0, -10.0, 7.0).unwrap());
    ui.command("UNDO");
    ui.command("SELECT");
    ui.click("Join");
    drag(&mut ui, (-70.0, 0.0), (50.0, -20.0), egui::Modifiers::NONE);
    assert!(ui.has_text("3 selected"));
    drag(
        &mut ui,
        (-65.0, -5.0),
        (-35.0, -15.0),
        egui::Modifiers::SHIFT,
    );
    assert!(ui.has_text("2 selected"));
    press(&mut ui, egui::Key::Enter);
    ui.command("SAVE");
    assert_eq!(read().entities().len(), 5);
    let Entity::Line(joined) = read().entities()[1] else {
        panic!("line")
    };
    assert_eq!(joined.start(), Point3::new(-20.0, -10.0, 7.0).unwrap());
    assert_eq!(joined.end(), Point3::new(40.0, -10.0, 7.0).unwrap());
    ui.command("UNDO");
    ui.command("SELECT");
    ui.click("Join");
    click(&mut ui, 7.0710678, 42.0710678, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Enter);
    click(&mut ui, -7.0710678, 27.9289322, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Enter);
    ui.snapshot("join-arc-envelope");
    ui.command("SAVE");
    let Entity::Arc(joined) = read().entities()[4] else {
        panic!("arc")
    };
    assert!((joined.sweep().to_degrees() - 270.0).abs() < 1e-5);
    ui.command("UNDO");
    ui.command("JOIN 5");
    click(&mut ui, 7.0710678, 42.0710678, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::L);
    assert!(ui.has_text("Arc closed into a circle."));
    ui.snapshot("join-arc-close");
    ui.command("SAVE");
    assert!(matches!(read().entities()[5], Entity::Circle(_)));
    ui.command("UNDO");
    ui.command("SELECT");
    ui.click("Join");
    click(&mut ui, -10.0, -10.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Enter);
    click(&mut ui, 30.0, -10.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("SELECT");
    ui.command("JOIN");
    ui.command("1 2 3");
    ui.command("Exit");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn pedit_commands_convert_curves_preserve_properties_and_bound_session_undo() {
    use opencad::{Arc3, Circle3, Line3, Polyline2};
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit.dxf");
    let p = |x, y| Point3::new(x, y, 7.0).unwrap();
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document
        .push_on_layer(
            Entity::Line(Line3::new(p(-20.0, -10.0), p(20.0, -10.0))),
            "DETAIL",
        )
        .unwrap();
    document
        .push_on_layer(
            Entity::Arc(Arc3::new(p(0.0, 25.0), 10.0, 0.0, std::f64::consts::PI * 1.5).unwrap()),
            "DETAIL",
        )
        .unwrap();
    document.push(Entity::Polyline(
        Polyline2::with_bulges(
            vec![p(-30.0, 0.0), p(-10.0, 0.0), p(-20.0, 15.0)],
            vec![1.0, 0.0, -0.25],
            false,
        )
        .unwrap(),
    ));
    document.push(Entity::Circle(Circle3::new(p(40.0, 0.0), 5.0).unwrap()));
    document.push(Entity::Line(Line3::new(
        p(0.0, 0.0),
        Point3::new(10.0, 0.0, 8.0).unwrap(),
    )));
    document.set_entity_color(0, 3).unwrap();
    document.set_entity_linetype_scale(0, 2.0).unwrap();
    fs::write(&path, document.to_dxf()).unwrap();
    let original = fs::read_to_string(&path).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", path.display()));
    assert!(opencad::desktop::menu_action_status(&ui.ctx, "ID_Pedit").enabled);
    opencad::desktop::queue_menu_action(&ui.ctx, "ID_Pedit");
    ui.step(Vec::new());
    ui.step(Vec::new());
    assert!(ui.has_text("Current tool: Polyline Edit"));
    ui.command("1");
    assert!(ui.has_text("Convert it? Yes/No <Yes>"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("No");
    assert!(ui.has_text("Select one XY polyline"));
    ui.command("1");
    ui.command_with_enter("");
    ui.command("SAVE");
    let converted = read();
    assert!(matches!(converted.entities()[4], Entity::Polyline(_)));
    assert_eq!(converted.entity_layer(4), Some("DETAIL"));
    assert_eq!(converted.entity_style(4), document.entity_style(0));
    let Entity::Polyline(line) = &converted.entities()[4] else {
        panic!("converted line")
    };
    assert_eq!(line.vertices(), &[p(-20.0, -10.0), p(20.0, -10.0)]);
    assert_eq!(line.bulges(), &[0.0, 0.0]);
    ui.command("Close");
    ui.command("Reverse");
    ui.command("SAVE");
    let Entity::Polyline(closed) = read().entities()[4].clone() else {
        panic!("polyline")
    };
    assert!(closed.is_closed());
    assert_eq!(closed.vertices(), &[p(20.0, -10.0), p(-20.0, -10.0)]);
    ui.command("Undo");
    ui.command("Undo");
    ui.command("Undo");
    assert!(ui.has_text("Convert it? Yes/No <Yes>"));
    ui.command("Undo");
    assert!(ui.has_text("No more operations in this PEDIT session."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("No");
    ui.command("Exit");
    ui.command("PE 3 Close");
    ui.command("Close"); // A no-op must not add a session history step.
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Close");
    ui.command("Open");
    ui.command("Reverse");
    ui.command("Undo");
    ui.command("Exit");
    ui.command("UNDO");
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[2].clone() else {
        panic!("polyline")
    };
    assert!(polyline.is_closed());
    assert_eq!(polyline.bulges(), &[1.0, 0.0, -0.25]);
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    for command in [
        "PE 0",
        "PE 99",
        "PE 3 invalid",
        "PE 1 Close",
        "PE 3 Close extra",
        "PE 4",
        "PE 5",
    ] {
        ui.command(command);
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&path).unwrap(), original, "{command}");
    }
    for state in ["LOCK", "HIDE"] {
        ui.command(&format!("LAYER {state} DETAIL"));
        ui.command("SAVE");
        let protected = fs::read_to_string(&path).unwrap();
        ui.command("PE 1");
        if state == "LOCK" {
            ui.command("Yes");
        }
        assert!(ui.has_text(if state == "LOCK" {
            "unlock layer DETAIL before editing it"
        } else {
            "PEDIT objects must be visible"
        }));
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&path).unwrap(), protected);
        ui.command("Exit");
        ui.command("UNDO");
    }
    ui.command("SELECT 2");
    ui.command("PE");
    ui.command("Yes");
    ui.command("SAVE");
    let Entity::Polyline(arc) = read().entities()[4].clone() else {
        panic!("converted arc")
    };
    assert!((arc.bulges()[0] - (std::f64::consts::PI * 3.0 / 8.0).tan()).abs() < 1e-9);
    assert!((arc.length().unwrap() - std::f64::consts::PI * 15.0).abs() < 1e-9);
    assert_eq!(arc.elevation(), 7.0);
    ui.command("Exit");
    ui.command("PEDITACCEPT 1");
    let mut storage = PreferenceStorage::default();
    ui.app.save(&mut storage);
    let mut restored = DrawingUi::with_storage_and_path(Some(&storage), Some(path.clone()));
    restored.command("PEDITACCEPT");
    assert!(restored.has_text("PEDITACCEPT = 1"));
    restored.command("PE 1"); // Original line is still first after arc conversion.
    assert!(restored.has_text("PEDIT · Close / Reverse / Width / Undo"));
    restored.command("SAVE");
    assert!(matches!(read().entities()[4], Entity::Polyline(_)));
    restored.command("Undo");
    restored.command("No");
    restored.command("Exit");
    restored.command("PEDITACCEPT 2");
    assert!(restored.has_text("PEDITACCEPT must be 0 or 1"));
    restored.command("PEDITACCEPT");
    assert!(restored.has_text("PEDITACCEPT = 1"));
}

#[test]
fn pedit_canvas_picks_and_keyboard_options_edit_curved_polylines_and_cancel_conversion() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE -50,-20,7 -10,-20,7");
    ui.command("ARC 10,20,7 10 0 270");
    ui.command("PLINE 30,-20,7 50,-20,7 50,0,7");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let click = |ui: &mut DrawingUi, x: f32, y: f32| {
        let canvas = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        let position = egui::pos2(canvas.center().x + x * 4.0, canvas.center().y - y * 4.0);
        ui.step(vec![
            Event::PointerMoved(position),
            Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        ui.step(Vec::new());
    };
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    ui.command("SELECT");
    ui.command("PE");
    click(&mut ui, -30.0, -20.0);
    assert!(ui.has_text("Convert it? Yes/No <Yes>"));
    ui.snapshot("pedit-conversion-prompt");
    press(&mut ui, egui::Key::N);
    assert!(ui.has_text("Select one XY polyline"));
    click(&mut ui, -30.0, -20.0);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("SELECT");
    ui.command("PE");
    click(&mut ui, 17.071_068, 27.071_068);
    press(&mut ui, egui::Key::Y);
    assert!(ui.has_text("PEDIT · Close / Reverse / Width / Undo"));
    press(&mut ui, egui::Key::R);
    ui.command("SAVE");
    let Entity::Polyline(arc) = read().entities()[2].clone() else {
        panic!("arc polyline")
    };
    assert!(arc.bulges()[0] < -1.0);
    assert!((arc.length().unwrap() - std::f64::consts::PI * 15.0).abs() < 1e-9);
    click(&mut ui, -80.0, -50.0); // Restore canvas focus without changing the source.
    press(&mut ui, egui::Key::C);
    ui.snapshot("pedit-reversed-closed-arc");
    ui.command("SAVE");
    let Entity::Polyline(closed) = read().entities()[2].clone() else {
        panic!("closed arc")
    };
    assert!(closed.is_closed());
    click(&mut ui, -80.0, -50.0);
    press(&mut ui, egui::Key::O);
    press(&mut ui, egui::Key::U);
    ui.command("SAVE");
    let Entity::Polyline(closed) = read().entities()[2].clone() else {
        panic!("closed arc")
    };
    assert!(closed.is_closed());
    click(&mut ui, -80.0, -50.0);
    press(&mut ui, egui::Key::Enter);
    assert!(ui.has_text("Current tool: Select"));
    ui.command("UNDO");
    ui.command("UNDO");
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("SELECT");
    ui.command("PE");
    click(&mut ui, 40.0, -20.0);
    press(&mut ui, egui::Key::C);
    press(&mut ui, egui::Key::Escape); // Completed PEDIT edits survive cancellation.
    ui.command("SAVE");
    let Entity::Polyline(closed) = read().entities()[2].clone() else {
        panic!("polyline")
    };
    assert!(closed.is_closed());
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn pedit_join_commands_merge_chains_preserve_records_and_restore_conversion_history() {
    use opencad::{Arc3, Circle3, Line3, Polyline2};
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-join.dxf");
    let p = |x, y| Point3::new(x, y, 7.0).unwrap();
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document.push(Entity::Line(Line3::new(p(40.0, 10.0), p(30.0, 10.0))));
    document.push(Entity::Circle(Circle3::new(p(70.0, 20.0), 5.0).unwrap()));
    document
        .push_on_layer(
            Entity::Polyline(Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false).unwrap()),
            "DETAIL",
        )
        .unwrap();
    document.set_entity_color(2, 3).unwrap();
    document.set_entity_linetype_scale(2, 2.0).unwrap();
    document.push(Entity::Line(Line3::new(p(20.0, 0.0), p(10.0, 0.0))));
    document.push(Entity::Arc(
        Arc3::new(p(20.0, 10.0), 10.0, 270_f64.to_radians(), 0.0).unwrap(),
    ));
    document.push(Entity::Line(Line3::new(p(70.0, 0.0), p(80.0, 0.0))));
    let original = document.to_dxf().replacen(
        "POLYLINE\n",
        "POLYLINE\n5\nB4\n999\npedit-source-record\n",
        1,
    );
    fs::write(&path, &original).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", path.display()));
    ui.command("PEDIT 3 Join 1 4 5");
    ui.command("SAVE");
    let joined = read();
    assert_eq!(joined.entities().len(), 3);
    let Entity::Polyline(polyline) = &joined.entities()[1] else {
        panic!("joined polyline")
    };
    assert_eq!(polyline.vertices().len(), 5);
    assert!((polyline.length().unwrap() - 30.0 - 5.0 * std::f64::consts::PI).abs() < 1e-9);
    assert_eq!(joined.entity_layer(1), Some("DETAIL"));
    assert_eq!(joined.entity_style(1), document.entity_style(2));
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("pedit-source-record")
    );
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Undo");
    assert!(ui.has_text("No more operations in this PEDIT session."));
    ui.command("Close");
    ui.command("Join 1");
    assert!(ui.has_text("open the source polyline before joining objects"));
    ui.command("Undo");
    ui.command("Join 1 999");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Join");
    ui.command("1 4 5");
    assert!(ui.has_text("4 selected"));
    ui.command("Reverse");
    assert!(ui.has_text("Press Enter to finish selecting Join objects first"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command_with_enter("");
    ui.command("SAVE");
    assert_eq!(read().entities(), joined.entities());
    ui.command("Undo");
    ui.command("Join 2"); // Incompatible input adds no history.
    ui.command("Undo");
    assert!(ui.has_text("No more operations in this PEDIT session."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Exit");

    let converted_path = scratch.0.join("pedit-join-conversion.dxf");
    let mut converted = DrawingUi::new();
    converted.click("New");
    converted.command("LINE 0,0,7 10,0,7");
    converted.command("LINE 10,0,7 20,0,7");
    converted.command(&format!("SAVEAS {}", converted_path.display()));
    let before_conversion = fs::read_to_string(&converted_path).unwrap();
    converted.command("PE 1");
    converted.command("Yes");
    converted.command("Join 1"); // Conversion appended the source after this line.
    converted.command("SAVE");
    let result = DxfDocument::parse(&fs::read_to_string(&converted_path).unwrap()).unwrap();
    let Entity::Polyline(result) = &result.entities()[0] else {
        panic!("converted and joined")
    };
    assert_eq!(
        result.vertices(),
        &[p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)]
    );
    converted.command("Undo");
    converted.command("SAVE");
    let result = DxfDocument::parse(&fs::read_to_string(&converted_path).unwrap()).unwrap();
    assert_eq!(result.entities().len(), 2);
    assert!(matches!(result.entities()[1], Entity::Polyline(_)));
    converted.command("Undo");
    assert!(converted.has_text("Convert it? Yes/No <Yes>"));
    converted.command("SAVE");
    assert_eq!(
        fs::read_to_string(&converted_path).unwrap(),
        before_conversion
    );

    let protected_path = scratch.0.join("pedit-join-protected.dxf");
    document.add_layer("PROTECTED").unwrap();
    document.set_entity_layer(4, "PROTECTED").unwrap();
    document.set_layer_locked("PROTECTED", true).unwrap();
    fs::write(&protected_path, document.to_dxf()).unwrap();
    let protected_original = fs::read_to_string(&protected_path).unwrap();
    let mut protected = DrawingUi::new();
    protected.command(&format!("OPEN {}", protected_path.display()));
    protected.command("PEDIT 3 Join 1 4 5");
    assert!(protected.has_text("unlock layer PROTECTED before editing it"));
    protected.command("SAVE");
    assert_eq!(
        fs::read_to_string(&protected_path).unwrap(),
        protected_original
    );
    protected.command("Exit");
    document.set_layer_visible("PROTECTED", false).unwrap();
    fs::write(&protected_path, document.to_dxf()).unwrap();
    protected.command(&format!("OPEN {}", protected_path.display()));
    protected.command("PEDIT 3 Join 1 4 5");
    assert!(protected.has_text("Join objects must be visible"));
}

#[test]
fn pedit_join_canvas_selects_windows_preserves_anchor_and_handles_keyboard_and_cancel() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-join-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    for command in [
        "PLINE -50,-20,7 -30,-20,7",
        "LINE -30,-20,7 -10,-20,7",
        "ARC -10,-10,7 10 270 0",
        "LINE 0,-10,7 20,-10,7",
        "CIRCLE 40,20,7 6",
        "LINE 40,-20,7 60,-20,7",
    ] {
        ui.command(command);
    }
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let position = |ui: &DrawingUi, x: f32, y: f32| {
        let bounds = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        egui::pos2(bounds.center().x + 4.0 * x, bounds.center().y - 4.0 * y)
    };
    let click = |ui: &mut DrawingUi, x, y, modifiers| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    let drag = |ui: &mut DrawingUi, from: (f32, f32), to: (f32, f32), modifiers| {
        let from = position(ui, from.0, from.1);
        let to = position(ui, to.0, to.1);
        ui.step(vec![
            Event::PointerMoved(from),
            Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerMoved(to)]);
        ui.step(vec![Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    ui.command("SELECT");
    ui.command("PE");
    click(&mut ui, -40.0, -20.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::J);
    assert!(ui.has_text("PEDIT Join · Select touching XY"));
    click(&mut ui, -40.0, -20.0, egui::Modifiers::SHIFT);
    assert!(ui.has_text("1 selected"));
    drag(&mut ui, (-35.0, 5.0), (25.0, -30.0), egui::Modifiers::NONE);
    assert!(ui.has_text("4 selected"));
    drag(
        &mut ui,
        (-35.0, -15.0),
        (-5.0, -25.0),
        egui::Modifiers::SHIFT,
    );
    assert!(ui.has_text("3 selected"));
    click(&mut ui, -20.0, -20.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::A);
    assert!(ui.has_text("6 selected"));
    click(&mut ui, 40.0, 26.0, egui::Modifiers::SHIFT);
    click(&mut ui, 50.0, -20.0, egui::Modifiers::SHIFT);
    assert!(ui.has_text("4 selected"));
    ui.snapshot("pedit-join-candidate-selection");
    press(&mut ui, egui::Key::Enter);
    assert!(ui.has_text("3 objects joined to polyline"));
    ui.snapshot("pedit-join-curved-chain");
    ui.command("SAVE");
    let merged = read();
    assert_eq!(merged.entities().len(), 3);
    let Entity::Polyline(polyline) = &merged.entities()[0] else {
        panic!("joined polyline")
    };
    assert_eq!(polyline.vertices().len(), 5);
    assert!((polyline.length().unwrap() - 60.0 - 5.0 * std::f64::consts::PI).abs() < 1e-9);
    click(&mut ui, -40.0, -20.0, egui::Modifiers::NONE); // Focus canvas.
    press(&mut ui, egui::Key::R);
    press(&mut ui, egui::Key::U);
    press(&mut ui, egui::Key::U);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Join");
    click(&mut ui, -20.0, -20.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("PE 1 Join");
    ui.command("All");
    ui.command_with_enter("");
    ui.command("SAVE");
    assert_eq!(read().entities(), merged.entities());
    ui.command("Exit"); // Completed join survives ending the session.
    ui.command("SAVE");
    assert_eq!(read().entities(), merged.entities());
}

#[test]
fn pedit_vertex_commands_traverse_closed_curves_move_with_validation_and_preserve_history() {
    use opencad::{Circle3, Polyline2};
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-vertices.dxf");
    let p = |x, y| Point3::new(x, y, 7.0).unwrap();
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document.push(Entity::Circle(Circle3::new(p(60.0, 20.0), 5.0).unwrap()));
    document
        .push_on_layer(
            Entity::Polyline(
                Polyline2::with_bulges(
                    vec![p(-20.0, 0.0), p(0.0, 0.0), p(20.0, 0.0)],
                    vec![1.0, -0.5, 0.25],
                    true,
                )
                .unwrap(),
            ),
            "DETAIL",
        )
        .unwrap();
    document.set_entity_color(1, 3).unwrap();
    let original = document.to_dxf().replacen(
        "POLYLINE\n",
        "POLYLINE\n5\nC1\n999\nvertex-source-identity\n",
        1,
    );
    fs::write(&path, &original).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", path.display()));
    ui.command("PEDIT 2 Edit vertex");
    assert!(ui.has_text("PEDIT vertex 1 of 3: -20,0,7"));
    ui.command("Previous");
    assert!(ui.has_text("PEDIT vertex 1 of 3: -20,0,7"));
    for _ in 0..3 {
        ui.command("Next");
    }
    assert!(ui.has_text("PEDIT vertex 3 of 3: 20,0,7")); // Closed does not wrap.
    ui.command("Previous");
    ui.command("Move 0,10");
    ui.command("SAVE");
    let moved = read();
    let Entity::Polyline(result) = &moved.entities()[1] else {
        panic!("moved polyline")
    };
    assert_eq!(
        result.vertices(),
        &[p(-20.0, 0.0), p(0.0, 10.0), p(20.0, 0.0)]
    );
    assert_eq!(result.bulges(), &[1.0, -0.5, 0.25]);
    assert!(result.is_closed());
    assert_eq!(moved.entity_style(1), document.entity_style(1));
    assert_eq!(moved.entity_layer(1), Some("DETAIL"));
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("vertex-source-identity")
    );
    for (index, segment) in result.segments().enumerate() {
        let segment = segment.unwrap();
        for (fraction, vertex) in [(0.0, index), (1.0, (index + 1) % 3)] {
            let actual = segment.point_at_fraction(fraction).unwrap();
            let expected = result.vertices()[vertex];
            assert!((actual.x() - expected.x()).abs() < 1e-9);
            assert!((actual.y() - expected.y()).abs() < 1e-9);
        }
    }
    ui.command("Move 0,10"); // No-op does not add another session checkpoint.
    ui.command("Undo");
    assert!(ui.has_text("Exit Edit vertex before using a PEDIT operation"));
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Undo");
    assert!(ui.has_text("No more operations in this PEDIT session."));
    ui.command("Edit vertex");
    ui.command("Next");
    ui.command("Move");
    for input in [
        "NaN,0",
        "0,10,8",
        "1e308,1e308",
        "Next",
        "Tangent",
        "-20,0",
        "20,0",
    ] {
        ui.command(input);
    }
    assert!(ui.has_text("Moving this vertex would collapse a curved segment."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command_with_enter(""); // Cancel only the pending move.
    assert!(ui.has_text("PEDIT vertex 2 of 3: 0,0,7"));
    ui.command("Previous");
    ui.command("Move 20,0"); // Also protect the closed polyline's final arc.
    assert!(ui.has_text("Moving this vertex would collapse a curved segment."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command_with_enter("");
    ui.command_with_enter(""); // Leave Edit vertex.
    ui.command("Undo");
    assert!(ui.has_text("No more operations in this PEDIT session."));
    ui.command("Edit");
    ui.command("Move");
    ui.command("@5,10");
    ui.command("SAVE");
    let Entity::Polyline(result) = read().entities()[1].clone() else {
        panic!("relative vertex move")
    };
    assert_eq!(result.vertices()[0], p(-15.0, 10.0));
    assert_eq!(result.bulges(), &[1.0, -0.5, 0.25]);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Exit");

    let protected_path = scratch.0.join("pedit-vertices-protected.dxf");
    document.set_layer_locked("DETAIL", true).unwrap();
    fs::write(&protected_path, document.to_dxf()).unwrap();
    let protected_original = fs::read_to_string(&protected_path).unwrap();
    let mut protected = DrawingUi::new();
    protected.command(&format!("OPEN {}", protected_path.display()));
    protected.command("PEDIT 2 E");
    protected.command("Next");
    protected.command("Move 0,10");
    assert!(protected.has_text("unlock layer DETAIL before editing it"));
    protected.command("SAVE");
    assert_eq!(
        fs::read_to_string(&protected_path).unwrap(),
        protected_original
    );

    let conversion_path = scratch.0.join("pedit-vertices-conversion.dxf");
    let mut converted = DrawingUi::new();
    converted.click("New");
    converted.command("LINE 0,0,7 10,0,7");
    converted.command(&format!("SAVEAS {}", conversion_path.display()));
    let before_conversion = fs::read_to_string(&conversion_path).unwrap();
    converted.command("PE 1");
    converted.command("Yes");
    converted.command("E");
    converted.command("Next");
    converted.command("Move 10,10");
    converted.command("Exit");
    converted.command("Undo");
    converted.command("SAVE");
    let result = DxfDocument::parse(&fs::read_to_string(&conversion_path).unwrap()).unwrap();
    assert!(matches!(result.entities()[0], Entity::Polyline(_)));
    converted.command("Undo");
    assert!(converted.has_text("Convert it? Yes/No <Yes>"));
    converted.command("SAVE");
    assert_eq!(
        fs::read_to_string(&conversion_path).unwrap(),
        before_conversion
    );
}

#[test]
fn pedit_vertex_canvas_marks_traverses_previews_snaps_moves_and_cancels() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-vertices-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("PLINE -40,-20,7 -20,0,7 20,0,7");
    ui.replace_field("Vertex 1 bulge", "1");
    ui.click("Apply properties");
    ui.command("CIRCLE 40,20,7 6");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let position = |ui: &DrawingUi, x: f32, y: f32| {
        let bounds = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        egui::pos2(bounds.center().x + x * 4.0, bounds.center().y - y * 4.0)
    };
    let click = |ui: &mut DrawingUi, x, y, modifiers| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    ui.command("PE 1");
    click(&mut ui, -40.0, -20.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::E);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::P);
    press(&mut ui, egui::Key::P);
    assert!(ui.has_text("PEDIT vertex 1 of 3: -40,-20,7"));
    press(&mut ui, egui::Key::N);
    assert!(ui.has_text("PEDIT vertex 2 of 3: -20,0,7"));
    ui.snapshot("pedit-marked-vertex");
    press(&mut ui, egui::Key::M);
    let preview = position(&ui, 5.0, 15.0);
    ui.step(vec![Event::PointerMoved(preview)]);
    ui.snapshot("pedit-move-vertex-preview");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    click(&mut ui, 5.0, 15.0, egui::Modifiers::NONE);
    assert!(ui.has_text("PEDIT vertex 2 of 3: 5,15,7"));
    ui.snapshot("pedit-moved-vertex");
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[0].clone() else {
        panic!("pointer move")
    };
    assert_eq!(polyline.vertices()[1], Point3::new(5.0, 15.0, 7.0).unwrap());
    assert_eq!(polyline.bulges(), &[1.0, 0.0, 0.0]);
    click(&mut ui, 5.0, 15.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::M);
    click(&mut ui, 20.0, 20.0, egui::Modifiers::SHIFT);
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[0].clone() else {
        panic!("ortho move")
    };
    assert_eq!(
        polyline.vertices()[1],
        Point3::new(20.0, 15.0, 7.0).unwrap()
    );
    click(&mut ui, 20.0, 15.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::M);
    click(&mut ui, 40.4, 20.4, egui::Modifiers::NONE);
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[0].clone() else {
        panic!("object snap move")
    };
    assert_eq!(
        polyline.vertices()[1],
        Point3::new(40.0, 20.0, 7.0).unwrap()
    );
    ui.command("Exit");
    for _ in 0..3 {
        ui.command("Undo");
    }
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Edit");
    click(&mut ui, -40.0, -20.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::M);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("PE 1 Edit");
    ui.command("Move -35,-25");
    click(&mut ui, -35.0, -25.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[0].clone() else {
        panic!("completed move")
    };
    assert_eq!(
        polyline.vertices()[0],
        Point3::new(-35.0, -25.0, 7.0).unwrap()
    );
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn pedit_insert_commands_preserve_segment_mapping_records_and_atomic_history() {
    use opencad::{Circle3, Polyline2};

    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-insert.dxf");
    let p = |x, y| Point3::new(x, y, 7.0).unwrap();
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document.push(Entity::Circle(Circle3::new(p(60.0, 20.0), 5.0).unwrap()));
    document
        .push_on_layer(
            Entity::Polyline(
                Polyline2::with_bulges(
                    vec![p(-20.0, 0.0), p(0.0, 0.0), p(20.0, 0.0)],
                    vec![0.5, -1.0, 9.0],
                    false,
                )
                .unwrap(),
            ),
            "DETAIL",
        )
        .unwrap();
    document.set_entity_color(1, 3).unwrap();
    let original = document.to_dxf().replacen(
        "POLYLINE\n",
        "POLYLINE\n5\nC2\n999\ninsert-source-identity\n",
        1,
    );
    fs::write(&path, &original).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", path.display()));
    ui.command("PE 2 Edit vertex");
    ui.command("Insert -10,10");
    assert!(ui.has_text("PEDIT vertex 2 of 4: -10,10,7"));
    ui.command("SAVE");
    let first_insert = fs::read_to_string(&path).unwrap();
    let result = read();
    let Entity::Polyline(polyline) = &result.entities()[1] else {
        panic!("inserted polyline")
    };
    assert_eq!(
        polyline.vertices(),
        &[p(-20.0, 0.0), p(-10.0, 10.0), p(0.0, 0.0), p(20.0, 0.0)]
    );
    assert_eq!(polyline.bulges(), &[0.5, 0.0, -1.0, 9.0]);
    assert!(!polyline.is_closed());
    assert_eq!(result.entity_style(1), document.entity_style(1));
    assert_eq!(result.entity_layer(1), Some("DETAIL"));
    assert_eq!(result.entities()[0], document.entities()[0]);
    assert!(first_insert.contains("insert-source-identity"));
    let Entity::Polyline(before) = &document.entities()[1] else {
        unreachable!()
    };
    let unchanged_arc = before.segments().nth(1).unwrap().unwrap();
    let shifted_arc = polyline.segments().nth(2).unwrap().unwrap();
    for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
        assert_eq!(
            unchanged_arc.point_at_fraction(fraction).unwrap(),
            shifted_arc.point_at_fraction(fraction).unwrap()
        );
    }
    ui.command("I");
    ui.command("@5,-5");
    assert!(ui.has_text("PEDIT vertex 3 of 5: -5,5,7"));
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[1].clone() else {
        panic!("relative insertion")
    };
    assert_eq!(polyline.vertices()[2], p(-5.0, 5.0));
    assert_eq!(polyline.bulges(), &[0.5, 0.0, 0.0, -1.0, 9.0]);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), first_insert);
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    ui.command("Edit");
    ui.command("Next");
    ui.command("Next");
    ui.command("Insert 30,10");
    assert!(ui.has_text("PEDIT vertex 4 of 4: 30,10,7"));
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[1].clone() else {
        panic!("open append")
    };
    assert_eq!(polyline.bulges(), &[0.5, -1.0, 0.0, 9.0]);
    assert_eq!(polyline.vertices()[3], p(30.0, 10.0));
    ui.command("Exit");
    ui.command("Undo");
    ui.command("Close");
    ui.command("SAVE");
    let closed = fs::read_to_string(&path).unwrap();
    ui.command("Edit");
    ui.command("Next");
    ui.command("Next");
    ui.command("Insert 30,10");
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[1].clone() else {
        panic!("closed insertion")
    };
    assert!(polyline.is_closed());
    assert_eq!(polyline.bulges(), &[0.5, -1.0, 9.0, 0.0]);
    assert_eq!(polyline.vertices()[3], p(30.0, 10.0));
    ui.command("Next");
    assert!(ui.has_text("PEDIT vertex 4 of 4: 30,10,7"));
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), closed);
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    ui.command("Edit");
    ui.command("Insert");
    for input in [
        "-20,0",
        "0,0",
        "1,2,8",
        "NaN,0",
        "1e308,1e308",
        "Next",
        "Undo",
        "Insert 1,2 extra",
    ] {
        ui.command(input);
    }
    assert!(ui.has_text("Insert a point distinct from the adjacent vertices."));
    assert!(ui.has_text("Exit to cancel point entry first."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command_with_enter("");
    assert!(ui.has_text("PEDIT vertex 1 of 3: -20,0,7"));
    ui.command_with_enter("");
    ui.command("Undo");
    assert!(ui.has_text("No more operations in this PEDIT session."));
    ui.command("Exit");

    let protected_path = scratch.0.join("pedit-insert-protected.dxf");
    document.set_layer_locked("DETAIL", true).unwrap();
    fs::write(&protected_path, document.to_dxf()).unwrap();
    let protected_original = fs::read_to_string(&protected_path).unwrap();
    let mut protected = DrawingUi::new();
    protected.command(&format!("OPEN {}", protected_path.display()));
    protected.command("PE 2 E");
    protected.command("Insert -10,10");
    assert!(protected.has_text("unlock layer DETAIL before editing it"));
    protected.command("SAVE");
    assert_eq!(
        fs::read_to_string(&protected_path).unwrap(),
        protected_original
    );

    let conversion_path = scratch.0.join("pedit-insert-conversion.dxf");
    let mut converted = DrawingUi::new();
    converted.click("New");
    converted.command("LINE 0,0,7 10,0,7");
    converted.command(&format!("SAVEAS {}", conversion_path.display()));
    let before_conversion = fs::read_to_string(&conversion_path).unwrap();
    converted.command("PE 1");
    converted.command("Yes");
    converted.command("E");
    converted.command("Insert 5,10");
    converted.command("SAVE");
    let inserted = DxfDocument::parse(&fs::read_to_string(&conversion_path).unwrap()).unwrap();
    let Entity::Polyline(polyline) = &inserted.entities()[0] else {
        panic!("converted insertion")
    };
    assert_eq!(polyline.vertices().len(), 3);
    assert_eq!(polyline.vertices()[1], p(5.0, 10.0));
    converted.command("Exit");
    converted.command("Undo");
    converted.command("SAVE");
    let reverted = DxfDocument::parse(&fs::read_to_string(&conversion_path).unwrap()).unwrap();
    let Entity::Polyline(polyline) = &reverted.entities()[0] else {
        panic!("reverted insertion")
    };
    assert_eq!(polyline.vertices().len(), 2);
    converted.command("Undo");
    converted.command("SAVE");
    assert_eq!(
        fs::read_to_string(&conversion_path).unwrap(),
        before_conversion
    );
}

#[test]
fn pedit_insert_canvas_previews_snaps_marks_new_vertex_and_cancels() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-insert-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("PLINE -40,-20,7 -20,0,7 20,0,7");
    ui.replace_field("Vertex 1 bulge", "1");
    ui.click("Apply properties");
    ui.command("CIRCLE 40,20,7 6");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let position = |ui: &DrawingUi, x: f32, y: f32| {
        let bounds = ui
            .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
            .unwrap();
        egui::pos2(bounds.center().x + x * 4.0, bounds.center().y - y * 4.0)
    };
    let click = |ui: &mut DrawingUi, x, y, modifiers| {
        let pos = position(ui, x, y);
        ui.step(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        ui.step(vec![Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        }]);
        ui.step(Vec::new());
    };
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    ui.command("PE 1 E");
    click(&mut ui, -40.0, -20.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::I);
    let preview = position(&ui, -25.0, -10.0);
    ui.step(vec![Event::PointerMoved(preview)]);
    ui.snapshot("pedit-insert-preview");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    click(&mut ui, -25.0, -10.0, egui::Modifiers::NONE);
    assert!(ui.has_text("PEDIT vertex 2 of 4: -25,-10,7"));
    ui.snapshot("pedit-inserted-vertex");
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[0].clone() else {
        panic!("pointer insertion")
    };
    assert_eq!(
        polyline.vertices()[1],
        Point3::new(-25.0, -10.0, 7.0).unwrap()
    );
    assert_eq!(polyline.bulges(), &[1.0, 0.0, 0.0, 0.0]);
    click(&mut ui, -25.0, -10.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::I);
    click(&mut ui, -5.0, -5.0, egui::Modifiers::SHIFT);
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[0].clone() else {
        panic!("ortho insertion")
    };
    assert_eq!(
        polyline.vertices()[2],
        Point3::new(-5.0, -10.0, 7.0).unwrap()
    );
    click(&mut ui, -5.0, -10.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::I);
    click(&mut ui, 40.4, 20.4, egui::Modifiers::NONE);
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[0].clone() else {
        panic!("object snap insertion")
    };
    assert_eq!(
        polyline.vertices()[3],
        Point3::new(40.0, 20.0, 7.0).unwrap()
    );
    ui.command("Exit");
    for _ in 0..3 {
        ui.command("Undo");
    }
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Edit");
    click(&mut ui, -40.0, -20.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::I);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("PE 1 Edit");
    ui.command("Insert -30,-10");
    click(&mut ui, -30.0, -10.0, egui::Modifiers::NONE);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[0].clone() else {
        panic!("completed insertion")
    };
    assert_eq!(
        polyline.vertices()[1],
        Point3::new(-30.0, -10.0, 7.0).unwrap()
    );
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn pedit_straighten_commands_remove_ranges_convert_arcs_and_preserve_history() {
    use opencad::{Circle3, Polyline2};

    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-straighten.dxf");
    let p = |x, y| Point3::new(x, y, 7.0).unwrap();
    let vertices = vec![
        p(-40.0, -20.0),
        p(-20.0, 10.0),
        p(0.0, 0.0),
        p(20.0, 10.0),
        p(40.0, -20.0),
    ];
    let bulges = vec![0.5, -0.25, 1.0, -0.5, 9.0];
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document.push(Entity::Circle(Circle3::new(p(60.0, 20.0), 5.0).unwrap()));
    document
        .push_on_layer(
            Entity::Polyline(
                Polyline2::with_bulges(vertices.clone(), bulges.clone(), false).unwrap(),
            ),
            "DETAIL",
        )
        .unwrap();
    document.set_entity_color(1, 3).unwrap();
    let original = document.to_dxf().replacen(
        "POLYLINE\n",
        "POLYLINE\n5\nC3\n999\nstraighten-source-identity\n",
        1,
    );
    fs::write(&path, &original).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", path.display()));
    ui.command("PE 2 E");
    ui.command("Straighten");
    ui.command("Next");
    ui.command("Next");
    assert!(ui.has_text("Straighten start vertex 1"));
    assert!(ui.has_text("PEDIT vertex 3 of 5: 0,0,7"));
    for input in ["Move 10,10", "Insert 10,10", "Straighten", "Undo"] {
        ui.command(input);
    }
    assert!(ui.has_text("Use Go or Exit to finish Straighten first."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Go");
    assert!(ui.has_text("PEDIT vertex 1 of 4: -40,-20,7"));
    assert!(!ui.has_text("Straighten start vertex"));
    ui.command("SAVE");
    let first_straighten = fs::read_to_string(&path).unwrap();
    let result = read();
    let Entity::Polyline(polyline) = &result.entities()[1] else {
        panic!("straightened range")
    };
    assert_eq!(
        polyline.vertices(),
        &[vertices[0], vertices[2], vertices[3], vertices[4]]
    );
    assert_eq!(polyline.bulges(), &[0.0, 1.0, -0.5, 9.0]);
    assert!(!polyline.is_closed());
    assert_eq!(result.entity_style(1), document.entity_style(1));
    assert_eq!(result.entity_layer(1), Some("DETAIL"));
    assert_eq!(result.entities()[0], document.entities()[0]);
    assert!(first_straighten.contains("straighten-source-identity"));
    let Entity::Polyline(before) = &document.entities()[1] else {
        unreachable!()
    };
    for index in 2..4 {
        let unchanged = before.segments().nth(index).unwrap().unwrap();
        let shifted = polyline.segments().nth(index - 1).unwrap().unwrap();
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert_eq!(
                unchanged.point_at_fraction(fraction).unwrap(),
                shifted.point_at_fraction(fraction).unwrap()
            );
        }
    }
    ui.command("Go"); // Cleared start cannot be reused against shifted indices.
    assert!(ui.has_text("Choose Straighten or Break in Edit vertex first."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), first_straighten);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    ui.command("Edit");
    for _ in 0..4 {
        ui.command("Next");
    }
    ui.command("S");
    ui.command("Previous");
    ui.command("Previous");
    ui.command("G");
    assert!(ui.has_text("PEDIT vertex 3 of 4: 0,0,7"));
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[1].clone() else {
        panic!("backward range")
    };
    assert_eq!(
        polyline.vertices(),
        &[vertices[0], vertices[1], vertices[2], vertices[4]]
    );
    assert_eq!(polyline.bulges(), &[0.5, -0.25, 0.0, 9.0]);
    ui.command("Exit");
    ui.command("Undo");

    ui.command("Edit");
    for _ in 0..2 {
        ui.command("Straighten");
        ui.command("Go");
    }
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[1].clone() else {
        panic!("same marker arc")
    };
    assert_eq!(polyline.vertices(), vertices);
    assert_eq!(polyline.bulges(), &[0.0, -0.25, 1.0, -0.5, 9.0]);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("Undo");
    assert!(ui.has_text("No more operations in this PEDIT session."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    ui.command("Edit");
    for _ in 0..4 {
        ui.command("Next");
    }
    ui.command("Straighten");
    ui.command("Go"); // Open final edge is dormant and must stay unchanged.
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Edit");
    ui.command("Straighten");
    for _ in 0..4 {
        ui.command("Next");
    }
    ui.command("Go");
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[1].clone() else {
        panic!("whole open range")
    };
    assert_eq!(polyline.vertices(), &[vertices[0], vertices[4]]);
    assert_eq!(polyline.bulges(), &[0.0, 9.0]);
    assert!(!polyline.is_closed());
    ui.command("Exit");
    ui.command("Undo");

    ui.command("Close");
    ui.command("SAVE");
    let closed = fs::read_to_string(&path).unwrap();
    ui.command("Edit");
    for _ in 0..4 {
        ui.command("Next");
    }
    ui.command("Straighten");
    ui.command("Go");
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[1].clone() else {
        panic!("closed final edge")
    };
    assert!(polyline.is_closed());
    assert_eq!(polyline.vertices(), vertices);
    assert_eq!(polyline.bulges(), &[0.5, -0.25, 1.0, -0.5, 0.0]);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), closed);
    ui.command("Edit");
    for _ in 0..4 {
        ui.command("Next");
    }
    ui.command("Straighten");
    for _ in 0..4 {
        ui.command("Previous");
    }
    ui.command("Go");
    ui.command("SAVE");
    let Entity::Polyline(polyline) = read().entities()[1].clone() else {
        panic!("closed stored-order range")
    };
    assert!(polyline.is_closed());
    assert_eq!(polyline.vertices(), &[vertices[0], vertices[4]]);
    assert_eq!(polyline.bulges(), &[0.0, 9.0]); // Closure lies outside the range.
    ui.command("Exit");
    ui.command("Undo");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    ui.command("Edit");
    ui.command("Straighten");
    ui.command("Next");
    ui.command_with_enter("");
    assert!(!ui.has_text("Straighten start vertex"));
    assert!(ui.has_text("PEDIT vertex 2 of 5: -20,10,7"));
    ui.command("Straighten");
    ui.command("Next");
    ui.command("Exit");
    assert!(!ui.has_text("Straighten start vertex"));
    ui.command("Move");
    ui.command("Straighten");
    assert!(ui.has_text("Exit point entry before choosing Straighten."));
    ui.command("Exit");
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    let protected_path = scratch.0.join("pedit-straighten-protected.dxf");
    document.set_layer_locked("DETAIL", true).unwrap();
    fs::write(&protected_path, document.to_dxf()).unwrap();
    let protected_original = fs::read_to_string(&protected_path).unwrap();
    let mut protected = DrawingUi::new();
    protected.command(&format!("OPEN {}", protected_path.display()));
    protected.command("PE 2 E");
    protected.command("Straighten");
    protected.command("Next");
    protected.command("Next");
    protected.command("Go");
    assert!(protected.has_text("unlock layer DETAIL before editing it"));
    assert!(protected.has_text("Straighten start vertex 1"));
    protected.command("SAVE");
    assert_eq!(
        fs::read_to_string(&protected_path).unwrap(),
        protected_original
    );
}

#[test]
fn pedit_straighten_canvas_previews_marks_ranges_and_cancels_with_keys() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-straighten-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("PLINE -40,-20,7 -20,20,7 0,0,7 20,20,7 40,-20,7");
    ui.replace_field("Vertex 1 bulge", "1");
    ui.click("Apply properties");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let focus = |ui: &mut DrawingUi| ui.canvas_click(0.5, 0.5);
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    ui.command("PE 1 E");
    focus(&mut ui);
    press(&mut ui, egui::Key::S);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::N);
    assert!(ui.has_text("Straighten start vertex 1"));
    assert!(ui.has_text("PEDIT vertex 3 of 5: 0,0,7"));
    ui.snapshot("pedit-straighten-range-preview");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    focus(&mut ui);
    press(&mut ui, egui::Key::G);
    assert!(!ui.has_text("Straighten start vertex"));
    ui.snapshot("pedit-straightened-range");
    ui.command("SAVE");
    let saved = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let Entity::Polyline(polyline) = &saved.entities()[0] else {
        panic!("keyboard straightening")
    };
    assert_eq!(polyline.vertices().len(), 4);
    assert_eq!(polyline.vertices()[1], Point3::new(0.0, 0.0, 7.0).unwrap());
    assert_eq!(polyline.bulges(), &[0.0, 0.0, 0.0, 0.0]);
    focus(&mut ui);
    press(&mut ui, egui::Key::Escape);
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    ui.command("PE 1 E");
    focus(&mut ui);
    press(&mut ui, egui::Key::S);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::Enter);
    assert!(!ui.has_text("Straighten start vertex"));
    assert!(ui.has_text("PEDIT vertex 2 of 5: -20,20,7"));
    press(&mut ui, egui::Key::S);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

// Width data crosses commands, staged prompts, history, source-backed DXF,
// transforms and vertex reconstruction. Geometry-only tests cannot exercise
// prompt routing, cancellation, Save during staged input or keyboard focus.
#[test]
fn pedit_width_commands_stage_values_preserve_history_and_reject_protected_edits() {
    use opencad::{Polyline2, PolylineWidth};
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-width.dxf");
    let p = |x, y| Point3::new(x, y, 7.0).unwrap();
    let w = |start, end| PolylineWidth::new(start, end).unwrap();
    let mut polyline = Polyline2::with_bulges(
        vec![p(-30.0, 0.0), p(-10.0, 0.0), p(10.0, 0.0), p(30.0, 0.0)],
        vec![0.0, -1.0, 0.0, 9.0],
        false,
    )
    .unwrap();
    let widths = vec![w(2.0, 6.0), w(4.0, 8.0), w(6.0, 10.0), w(9.0, 11.0)];
    polyline.set_widths(widths.clone()).unwrap();
    let mut document = DxfDocument::default();
    document.push(Entity::Polyline(polyline.clone()));
    let original =
        document
            .to_dxf()
            .replacen("POLYLINE\n", "POLYLINE\n5\nE1\n999\nwidth-source\n", 1);
    fs::write(&path, &original).unwrap();
    let read = || {
        let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let Entity::Polyline(polyline) = &document.entities()[0] else {
            panic!("polyline")
        };
        polyline.clone()
    };
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", path.display()));
    ui.command("PE 1 Width");
    assert!(ui.has_text("PEDIT Width · Enter a finite nonnegative uniform width"));
    for invalid in ["-1", "NaN", "inf", "Next"] {
        ui.command(invalid);
    }
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("2");
    ui.command("SAVE");
    assert!(read().widths().iter().all(|width| *width == w(2.0, 2.0)));
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("E");
    ui.command("W");
    ui.command("1");
    assert!(ui.has_text("PEDIT Vertex Width · Enter the outgoing segment's ending width"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("-3");
    ui.command("3");
    ui.command("SAVE");
    let first = fs::read_to_string(&path).unwrap();
    assert_eq!(
        read().widths(),
        &[w(1.0, 3.0), widths[1], widths[2], widths[3]]
    );
    ui.command("Regen");
    assert!(ui.has_text("Polyline regenerated"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), first);
    ui.command("W");
    ui.command("7");
    ui.command_with_enter(""); // cancels both staged values
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), first);
    ui.command("Exit"); // returns to main PEDIT
    ui.command("Undo"); // Regen and cancelled width created no extra history
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("E");
    ui.command("Width 1 3");
    ui.command("Next");
    ui.command("Width 5 7");
    ui.command("SAVE");
    assert_eq!(
        read().widths(),
        &[w(1.0, 3.0), w(5.0, 7.0), widths[2], widths[3]]
    );
    ui.snapshot("pedit-variable-widths");
    ui.command("Exit");
    ui.command("Undo");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Exit");
    ui.command("PE 1 Width 4");
    ui.command("SAVE");
    assert!(read().widths().iter().all(|width| *width == w(4.0, 4.0)));
    ui.command("Undo");
    ui.command("Exit");
    ui.command("PLINEREVERSEWIDTHS");
    assert!(ui.has_text("PLINEREVERSEWIDTHS = 0"));
    ui.command("PLINEREVERSEWIDTHS 2");
    assert!(ui.has_text("PLINEREVERSEWIDTHS must be 0 or 1"));
    ui.command("PE 1 Reverse");
    ui.command("SAVE");
    assert_eq!(read().widths(), widths);
    ui.command("Undo");
    ui.command("Exit");
    ui.command("PLINEREVERSEWIDTHS 1");
    ui.command("PE 1 Reverse");
    ui.command("SAVE");
    assert_eq!(
        read().widths(),
        &[w(10.0, 6.0), w(8.0, 4.0), w(6.0, 2.0), w(11.0, 9.0)]
    );
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Exit");

    // Insert halves the taper; Straighten retains the outer endpoint widths.
    ui.command("PE 1 E");
    ui.command("Insert -20,5");
    ui.command("SAVE");
    assert_eq!(
        read().widths(),
        &[w(2.0, 4.0), w(4.0, 6.0), widths[1], widths[2], widths[3]]
    );
    ui.command("Exit");
    ui.command("Undo");
    ui.command("E");
    ui.command("Straighten");
    ui.command("Next");
    ui.command("Next");
    ui.command("Go");
    ui.command("SAVE");
    assert_eq!(read().widths(), &[w(2.0, 8.0), widths[2], widths[3]]);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Exit");
    assert!(fs::read_to_string(&path).unwrap().contains("width-source"));
    document.set_layer_locked("0", true).unwrap();
    let protected_path = scratch.0.join("protected-width.dxf");
    let protected = document.to_dxf();
    fs::write(&protected_path, &protected).unwrap();
    ui.command(&format!("OPEN {}", protected_path.display()));
    ui.command("PE 1 Width 8");
    assert!(ui.has_text("unlock layer 0 before editing it"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&protected_path).unwrap(), protected);
}

#[test]
fn wide_polyline_canvas_selection_and_width_keyboard_cancel_work() {
    let scratch = Scratch::new();
    let path = scratch.0.join("wide-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("PLINE -30,0 30,0");
    ui.command("PE 1 Width 20");
    ui.command("Exit");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    ui.command("SELECT");
    // New drawing camera is centered at world origin, scale 4. Clicking 24 px
    // away from its centerline is inside the 80 px wide filled strip.
    let canvas = ui
        .nodes
        .values()
        .find(|node| {
            node.label()
                .is_some_and(|label| label.starts_with("Drawing canvas,"))
        })
        .and_then(|node| node.bounds())
        .unwrap();
    let y = 0.5 + 24.0 / (canvas.y1 - canvas.y0);
    ui.canvas_click(0.5, y);
    ui.command("PE"); // uses the object selected through its filled width
    assert!(ui.has_text("PEDIT · Close / Reverse / Width"));
    ui.canvas_click(0.5, y);
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    press(&mut ui, egui::Key::W);
    assert!(ui.has_text("PEDIT Width · Enter a finite nonnegative uniform width"));
    press(&mut ui, egui::Key::Enter);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.snapshot("pedit-wide-polyline-selected");
    ui.command("Width");
    ui.canvas_click(0.5, y);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn pedit_break_commands_split_truncate_open_loops_and_restore_session_history() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-vertex-break.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("PLINE -40,-20,7 -20,20,7 0,0,7 20,20,7 40,-20,7");
    ui.replace_field("Vertex 1 bulge", "0.5");
    ui.replace_field("Vertex 4 bulge", "-0.5");
    ui.click("Apply properties");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let read = || DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let original_doc = read();
    let Entity::Polyline(original_polyline) = &original_doc.entities()[0] else {
        panic!("polyline")
    };

    ui.command("PE 1 E");
    ui.command("Break");
    ui.command("Go");
    assert!(ui.has_text("choose at least one interior vertex"));
    assert!(ui.has_text("Break start vertex 1"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Next");
    ui.command("Next");
    ui.command("Go");
    ui.command("SAVE");
    let truncated = read();
    assert_eq!(truncated.entities().len(), 1);
    let Entity::Polyline(polyline) = &truncated.entities()[0] else {
        panic!("truncated")
    };
    assert_eq!(polyline.vertices(), &original_polyline.vertices()[2..]);
    assert_eq!(polyline.bulges(), &original_polyline.bulges()[2..]);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    ui.command("Edit");
    ui.command("Next");
    ui.command("Break");
    ui.command("Next");
    ui.command("Next");
    for option in ["Move 10,10", "Insert 10,10", "Straighten", "Break", "Undo"] {
        ui.command(option);
    }
    assert!(ui.has_text("Use Go or Exit to finish Break first."));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Go");
    assert!(!ui.has_text("Break start vertex"));
    assert!(ui.has_text("PEDIT vertex 1 of 2: -40,-20,7"));
    ui.command("SAVE");
    let split = read();
    assert_eq!(split.entities().len(), 2);
    let Entity::Polyline(prefix) = &split.entities()[0] else {
        panic!("prefix")
    };
    let Entity::Polyline(suffix) = &split.entities()[1] else {
        panic!("suffix")
    };
    assert_eq!(prefix.vertices(), &original_polyline.vertices()[..2]);
    assert_eq!(suffix.vertices(), &original_polyline.vertices()[3..]);
    assert_eq!(prefix.bulges(), &original_polyline.bulges()[..2]);
    assert_eq!(suffix.bulges(), &original_polyline.bulges()[3..]);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    ui.command("Close");
    ui.command("Edit");
    ui.command("Next");
    ui.command("Next");
    ui.command("Break");
    ui.command("Go");
    ui.command("SAVE");
    let opened = read();
    assert_eq!(opened.entities().len(), 1);
    let Entity::Polyline(polyline) = &opened.entities()[0] else {
        panic!("opened loop")
    };
    assert!(!polyline.is_closed());
    assert_eq!(polyline.vertices().len(), 6);
    assert_eq!(polyline.vertices()[0], original_polyline.vertices()[2]);
    assert_eq!(polyline.vertices()[5], original_polyline.vertices()[2]);
    ui.command("Exit");
    ui.command("Undo");
    ui.command("Undo");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("Edit");
    ui.command("Move");
    ui.command("Break");
    assert!(ui.has_text("Exit point entry before choosing Break."));
    ui.command("Exit");
    ui.command("Next");
    ui.command("Break");
    ui.command("Next");
    ui.command_with_enter("");
    assert!(!ui.has_text("Break start vertex"));
    assert!(ui.has_text("PEDIT vertex 3 of 5: 0,0,7"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn pedit_break_canvas_marks_previews_and_cancels_ranges_with_keyboard_options() {
    let scratch = Scratch::new();
    let path = scratch.0.join("pedit-vertex-break-pointer.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("PLINE -40,-20,7 -20,20,7 0,0,7 20,20,7 40,-20,7");
    ui.replace_field("Vertex 1 bulge", "1");
    ui.click("Apply properties");
    ui.command(&format!("SAVEAS {}", path.display()));
    let original = fs::read_to_string(&path).unwrap();
    let focus = |ui: &mut DrawingUi| ui.canvas_click(0.5, 0.5);
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    ui.command("PE 1 E");
    focus(&mut ui);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::B);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::N);
    assert!(ui.has_text("Break start vertex 2"));
    assert!(ui.has_text("PEDIT vertex 4 of 5: 20,20,7"));
    ui.snapshot("pedit-break-range-preview");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    focus(&mut ui);
    press(&mut ui, egui::Key::G);
    assert!(!ui.has_text("Break start vertex"));
    assert!(ui.has_text("PEDIT vertex 1 of 2: -40,-20,7"));
    ui.snapshot("pedit-broken-range");
    ui.command("SAVE");
    let split = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(split.entities().len(), 2);
    focus(&mut ui);
    press(&mut ui, egui::Key::Escape);
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    ui.command("PE 1 E");
    focus(&mut ui);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::B);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::Enter);
    assert!(!ui.has_text("Break start vertex"));
    assert!(ui.has_text("PEDIT vertex 3 of 5: 0,0,7"));
    press(&mut ui, egui::Key::B);
    press(&mut ui, egui::Key::N);
    press(&mut ui, egui::Key::Escape);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn closing_a_hidden_dirty_drawing_waits_for_cancel_or_save() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("quit.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE 1,2,3 4,6,15");
    // eframe still calls logic() while ui() is skipped for a hidden window.
    ui.step_with_close(Vec::new(), true, false);
    assert!(
        ui.viewport_commands
            .contains(&egui::ViewportCommand::CancelClose)
    );
    assert!(!ui.viewport_commands.contains(&egui::ViewportCommand::Close));
    ui.step(Vec::new());
    assert!(ui.has_text("Save changes"));
    ui.click("Cancel");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let saved = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    assert_eq!(saved.entities().len(), 1);
    ui.command("SELECT ALL");
    ui.command("MOVE 10,20,30");
    ui.step_with_close(Vec::new(), true, true);
    assert!(
        ui.viewport_commands
            .contains(&egui::ViewportCommand::CancelClose)
    );
    ui.click("Save changes");
    assert!(ui.viewport_commands.contains(&egui::ViewportCommand::Close));
    let saved = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    let Entity::Line(line) = saved.entities()[0] else {
        panic!("saved geometry must remain a line");
    };
    assert_eq!(line.start(), Point3::new(11.0, 22.0, 33.0).unwrap());
    assert_eq!(line.end(), Point3::new(14.0, 26.0, 45.0).unwrap());
}

#[test]
fn cancel_and_failed_open_retain_the_drawing_and_source_records() {
    let scratch = Scratch::new();
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/autocad-r12-lines.dxf");
    let drawing = scratch.0.join("preserved.dxf");
    let invalid = scratch.0.join("invalid.dxf");
    fs::write(&invalid, "not a drawing").unwrap();
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", source.display()));
    ui.command("SELECT 1");
    ui.replace_field("End Z", "NaN");
    ui.click("Apply properties");
    assert!(ui.has_text("finite"));
    ui.command("MOVE 10,20,30");
    ui.click("New");
    assert!(ui.has_text("Save changes"));
    ui.click("Cancel");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let before = fs::read_to_string(&drawing).unwrap();
    ui.command(&format!("OPEN {}", invalid.display()));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&drawing).unwrap(), before);
    let original = include_str!("fixtures/autocad-r12-lines.dxf");
    let expected = original
        .replacen("1.0\n 20\n2.0\n 30\n3.0", "11\n 20\n22\n 30\n33", 1)
        .replacen("4.0\n 21\n6.0\n 31\n15.0", "14\n 21\n26\n 31\n45", 1);
    assert_eq!(before, expected);
}

#[test]
fn mouse_tools_create_lines_circles_arcs_and_rectangles_at_the_active_elevation() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("mouse-tools.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("ELEVATION 7");
    ui.click("Line");
    ui.canvas_click(0.1, 0.2);
    ui.canvas_click(0.4, 0.3);
    ui.click("Circle, Center, Radius");
    ui.canvas_click(0.7, 0.4);
    ui.canvas_click(0.8, 0.5);
    ui.click("Arc, 3 Points");
    ui.click("Arc, Center, Start, End");
    ui.canvas_click(0.4, 0.7);
    ui.canvas_click(0.5, 0.7);
    ui.canvas_click(0.4, 0.8);
    ui.click("Rectangle");
    ui.canvas_click(0.2, 0.6);
    ui.canvas_click(0.5, 0.8);
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let document = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 7);
    assert_eq!(
        document
            .entities()
            .iter()
            .filter(|entity| matches!(entity, Entity::Circle(_)))
            .count(),
        1
    );
    for entity in document.entities() {
        match entity {
            Entity::Line(line) => {
                assert_eq!(line.start().z(), 7.0);
                assert_eq!(line.end().z(), 7.0);
                assert!(line.length().unwrap() > 0.0);
            }
            Entity::Circle(circle) => {
                assert_eq!(circle.center().z(), 7.0);
                assert!(circle.radius() > 0.0);
            }
            Entity::Arc(arc) => {
                assert_eq!(arc.center().z(), 7.0);
                assert!(arc.radius() > 0.0);
                assert!(arc.sweep() > 0.0);
            }
            Entity::Polyline(_) => panic!("these tools create individual entities"),
        }
    }
}

#[test]
fn arc_selection_follows_the_visible_sweep() {
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.click("Arc, 3 Points");
    ui.click("Arc, Center, Start, End");
    ui.canvas_click(0.5, 0.5);
    ui.canvas_click(0.6, 0.5);
    ui.canvas_click(0.5, 0.4);
    ui.command("SELECT");
    ui.canvas_click(0.6, 0.5);
    assert!(ui.has_text("1 selected"));
    assert!(ui.has_text("Start angle (°)"));
    // The opposite point lies on the same circle, outside this quarter arc.
    ui.canvas_click(0.4, 0.5);
    assert!(ui.has_text("0 selected"));
}

#[test]
fn layer_panel_keeps_controls_clear_of_canvas_and_inside_window() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/layers.dxf");
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", source.display()));
    ui.command("SELECT 1");
    ui.click("Show Layer List");
    ui.step(Vec::new());
    let bounds = |label: &str| {
        ui.nodes
            .values()
            .find(|node| node.label() == Some(label) || node.value() == Some(label))
            .and_then(|node| node.bounds())
            .unwrap_or_else(|| panic!("missing bounds for {label}"))
    };
    let canvas =
        bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.");
    for label in [
        "Properties",
        "Color index",
        "Apply color",
        "New layer",
        "Show BYLAYER_COLOR",
        "Lock BYLAYER_COLOR",
    ] {
        let control = bounds(label);
        assert!(
            control.x0 >= canvas.x1 && control.x1 <= 1440.0,
            "{label}: {control:?}; canvas: {canvas:?}"
        );
    }
}

#[test]
fn inspector_scroll_keeps_property_edits_reachable_with_layers_expanded() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("scrolled.dxf");
    let mut ui = DrawingUi::new();
    ui.screen_size = egui::vec2(1440.0, 560.0);
    ui.click("New");
    ui.command("LINE 0,0,3 10,0,3");
    ui.click("Show Layer List");
    for _ in 0..30 {
        ui.step(Vec::new());
    }
    let before = ui.bounds("Apply properties");
    let canvas = ui
        .bounds("Drawing canvas, XY top view. Use the command field for exact coordinates.")
        .expect("canvas bounds");
    ui.step(vec![
        Event::PointerMoved(egui::pos2(1350.0, canvas.min.y + 16.0)),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -600.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    for _ in 0..90 {
        ui.step(Vec::new());
    }
    let after = ui.bounds("Apply properties").expect("visible apply button");
    assert!(
        after.min.y >= canvas.min.y && after.max.y <= canvas.max.y,
        "apply: {after:?}; canvas: {canvas:?}; before: {before:?}"
    );
    ui.replace_field("End Z", "9");
    ui.click("Apply properties");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let document = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    let Entity::Line(line) = document.entities()[0] else {
        panic!("line expected");
    };
    assert_eq!(line.start(), Point3::new(0.0, 0.0, 3.0).unwrap());
    assert_eq!(line.end(), Point3::new(10.0, 0.0, 9.0).unwrap());
}

#[test]
fn layer_color_widgets_validate_sync_history_and_preserve_imported_state() {
    let scratch = Scratch::new();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/layers.dxf");
    let drawing = scratch.0.join("colors.dxf");
    let svg = scratch.0.join("colors.svg");
    let original = include_str!("fixtures/layers.dxf");
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", source.display()));
    ui.command(&format!("SAVEAS {}", drawing.display()));
    ui.click("Show Layer List");
    ui.command("LAYER SET walls");
    assert_eq!(ui.nodes[&ui.field("Color index")].value(), Some("1"));
    for invalid in ["0", "256", "not a color"] {
        ui.replace_field("Color index", invalid);
        ui.click("Apply color");
        assert!(ui.has_text("1 to 255"));
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&drawing).unwrap(), original);
    }
    ui.replace_field("Color index", "3");
    ui.click("Apply color");
    ui.command("UNDO");
    assert_eq!(ui.nodes[&ui.field("Color index")].value(), Some("1"));
    ui.command("REDO");
    assert_eq!(ui.nodes[&ui.field("Color index")].value(), Some("3"));
    for (name, imported, color) in [
        ("HIDDEN", "3", "6"),
        ("FROZEN", "5", "12"),
        ("LOCKED", "2", "255"),
        ("ZERO_COLOR", "0", "1"),
        ("BYLAYER_COLOR", "256", "7"),
    ] {
        ui.command(&format!("LAYER SET {name}"));
        assert_eq!(ui.nodes[&ui.field("Color index")].value(), Some(imported));
        ui.replace_field("Color index", color);
        ui.click("Apply color");
    }
    ui.command("LAYER COLOR \"walls\" 4");
    ui.command("LAYER SET WALLS");
    assert_eq!(ui.nodes[&ui.field("Color index")].value(), Some("4"));
    ui.command("UNDO");
    assert_eq!(ui.nodes[&ui.field("Color index")].value(), Some("3"));
    let green = ui.unselected_line_colors();
    assert_eq!(green.len(), 3);
    assert!(green.contains(&egui::Color32::from_rgb(0, 255, 0)));
    ui.command("LAYER COLOR WALLS 12");
    assert_eq!(ui.nodes[&ui.field("Color index")].value(), Some("12"));
    let shaded = ui.unselected_line_colors();
    assert_eq!(shaded.len(), 3);
    assert!(shaded.contains(&egui::Color32::from_rgb(204, 0, 0)));
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    let exported = fs::read_to_string(svg).unwrap();
    assert_eq!(exported.matches("<line ").count(), 3);
    assert!(exported.contains("stroke=\"#a50000\""));
    assert!(exported.contains("stroke=\"#000000\""));
    assert!(!exported.contains("<circle "));
    assert!(!exported.contains("<path "));
    ui.command("SAVE");
    let saved = fs::read_to_string(&drawing).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(
        document.entities(),
        DxfDocument::parse(original).unwrap().entities()
    );
    for (name, flags, color) in [
        ("WALLS", 64, 12),
        ("HIDDEN", 0, -6),
        ("FROZEN", 3, 12),
        ("LOCKED", 68, 255),
        ("ZERO_COLOR", 0, 1),
        ("BYLAYER_COLOR", 0, 7),
    ] {
        let layer = document.layer(name).unwrap();
        assert_eq!(layer.flags(), flags);
        assert_eq!(layer.color_index(), color);
    }
    let expected = original
        .replace("WALLS\n70\n64\n62\n1\n", "WALLS\n70\n64\n62\n12\n")
        .replace("HIDDEN\n70\n0\n62\n-3\n", "HIDDEN\n70\n0\n62\n-6\n")
        .replace("FROZEN\n70\n3\n62\n5\n", "FROZEN\n70\n3\n62\n12\n")
        .replace("LOCKED\n70\n68\n62\n2\n", "LOCKED\n70\n68\n62\n255\n")
        .replace("ZERO_COLOR\n70\n0\n62\n0\n", "ZERO_COLOR\n70\n0\n62\n1\n")
        .replace(
            "BYLAYER_COLOR\n70\n0\n62\n256\n",
            "BYLAYER_COLOR\n70\n0\n62\n7\n",
        );
    assert_eq!(saved, expected);
}

// Core tests cannot catch a disconnected pattern editor, inaccessible combo,
// stale scale after undo, or a canvas that ignores the document's new style.
#[test]
fn linetype_widgets_assign_validate_render_and_save_with_undo() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("linetypes.dxf");
    let svg = scratch.0.join("linetypes.svg");
    let mut ui = DrawingUi::new();
    ui.screen_size = egui::vec2(1440.0, 1200.0);
    ui.click("New");
    ui.command("LINE 0,0,3 20,0,3");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let original = fs::read_to_string(&drawing).unwrap();
    ui.click("Linetypes");
    ui.replace_field("New linetype", "DASHDOT");
    for pattern in ["1", "-1,1", "0,0"] {
        ui.replace_field("Pattern lengths", pattern);
        ui.click("Add linetype");
        assert!(ui.has_text("2–12 finite lengths"));
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&drawing).unwrap(), original);
    }
    ui.replace_field("Pattern lengths", "2,-1,0,-1");
    ui.click("Add linetype");
    assert_eq!(ui.nodes[&ui.field("New linetype")].value(), Some(""));
    ui.command("UNDO");
    ui.command("LAYER LINETYPE 0 DASHDOT");
    assert!(ui.has_text("Unknown linetype DASHDOT"));
    ui.command("REDO");
    ui.click("Linetypes");
    ui.click("Show Layer List");
    let combo = ui
        .nodes
        .iter()
        .find(|(_, node)| {
            node.labelled_by().iter().any(|id| {
                ui.nodes.get(id).and_then(|label| label.value()) == Some("Layer linetype")
            })
        })
        .map(|(id, _)| *id)
        .expect("labelled linetype combo");
    ui.action(combo, accesskit::Action::Click);
    ui.click("DASHDOT");
    assert!(ui.has_text("Layer linetype applied"));
    ui.command("SELECT");
    ui.command("FIT");
    assert_eq!(ui.unselected_line_colors().len(), 5);
    ui.click("Show Layer List");
    ui.click("Linetypes");
    ui.replace_field("Linetype scale", "0");
    ui.click("Apply scale");
    assert!(ui.has_text("greater than zero"));
    ui.replace_field("Linetype scale", "2");
    ui.click("Apply scale");
    assert_eq!(ui.unselected_line_colors().len(), 3);
    ui.command("UNDO");
    assert_eq!(ui.nodes[&ui.field("Linetype scale")].value(), Some("1"));
    assert_eq!(ui.unselected_line_colors().len(), 5);
    ui.command("REDO");
    assert_eq!(ui.nodes[&ui.field("Linetype scale")].value(), Some("2"));
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    let exported = fs::read_to_string(svg).unwrap();
    assert_eq!(exported.matches("<line ").count(), 3);
    assert_eq!(
        exported.matches("fill=\"#000000\" stroke=\"none\"").count(),
        2
    );
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    assert_eq!(document.layer("0").unwrap().linetype(), "DASHDOT");
    assert_eq!(document.linetype_scale(), 2.0);
    assert_eq!(
        document.linetype("DASHDOT").unwrap().pattern(),
        &[2.0, -1.0, 0.0, -1.0]
    );
    assert_eq!(
        document.entities(),
        DxfDocument::parse(&original).unwrap().entities()
    );
    ui.command("LINETYPE DOTTED 0,-1");
    ui.command("LAYER LINETYPE 0 DOTTED");
    ui.command("LTSCALE 1");
    ui.command("SAVE");
    let commands = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    assert_eq!(commands.layer("0").unwrap().linetype(), "DOTTED");
    assert_eq!(commands.linetype("DOTTED").unwrap().pattern(), &[0.0, -1.0]);
    assert_eq!(commands.linetype_scale(), 1.0);
}

#[test]
fn layer_widgets_assign_protect_and_hide_geometry_through_save_and_undo() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("layers.dxf");
    let svg = scratch.0.join("layers.svg");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("LINE 0,0,3 10,0,3");
    ui.click("Show Layer List");
    ui.replace_field("New layer", "Walls");
    ui.click("Add layer");
    ui.click("Assign to current layer");
    assert!(ui.has_text("Selection assigned to current layer"));
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let saved = fs::read_to_string(&drawing).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.entity_layer(0), Some("Walls"));
    assert_eq!(document.layers().len(), 2);
    fs::copy(&drawing, scratch.0.join("assigned.dxf")).unwrap();
    ui.replace_field("New layer", "wALLS");
    ui.click("Add layer");
    assert!(ui.has_text("already exists"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&drawing).unwrap(), saved);
    ui.toggle("Lock Walls");
    ui.command("MOVE 10 20 30");
    assert!(ui.has_text("unlock layer Walls"));
    ui.command("DELETE");
    assert!(ui.has_text("unlock layer Walls"));
    ui.command("SAVE");
    let locked = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    assert_eq!(locked.entities(), document.entities());
    assert!(locked.layer("Walls").unwrap().is_locked());
    fs::copy(&drawing, scratch.0.join("locked.dxf")).unwrap();
    ui.toggle("Lock Walls");
    ui.toggle("Show Walls");
    assert!(ui.has_text("0 selected"));
    ui.command("SELECT ALL");
    assert!(ui.has_text("0 selected"));
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    assert!(ui.has_text("no visible entities"));
    assert!(!svg.exists());
    ui.command("LINE 20,0 30,0");
    assert!(ui.has_text("show layer Walls before drawing"));
    ui.command("SAVE");
    let hidden = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    assert_eq!(hidden.entities(), document.entities());
    assert!(!hidden.layer("Walls").unwrap().is_visible());
    fs::copy(&drawing, scratch.0.join("hidden.dxf")).unwrap();
    ui.command("UNDO");
    ui.command("COPY 20 0 0");
    ui.command("SAVE");
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    assert_eq!(
        fs::read_to_string(&svg).unwrap().matches("<line ").count(),
        2
    );
    let copied = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    assert_eq!(copied.entities().len(), 2);
    assert_eq!(copied.entity_layer(1), Some("Walls"));
    let Entity::Line(line) = copied.entities()[1] else {
        panic!("line expected");
    };
    assert_eq!(line.start(), Point3::new(20.0, 0.0, 3.0).unwrap());
    assert_eq!(line.end(), Point3::new(30.0, 0.0, 3.0).unwrap());
    fs::copy(&drawing, scratch.0.join("copied.dxf")).unwrap();
    ui.command(&format!("OPEN {}", drawing.display()));
    ui.command("SELECT ALL");
    ui.command("LAYER ASSIGN 0");
    ui.command("SAVE");
    let reassigned = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    assert_eq!(reassigned.entity_layer(0), Some("0"));
    assert_eq!(reassigned.entity_layer(1), Some("0"));
    assert_eq!(reassigned.entities(), copied.entities());
}

// Document tests cannot catch mixed-selection fields retaining a stale value,
// inaccessible style controls, disconnected commands, or canvas painting that
// still uses only layer properties. Exercise the actual shipped widgets.
#[test]
fn entity_style_widgets_handle_mixed_selection_undo_and_resolved_paint() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("entity-styles.dxf");
    let svg = scratch.0.join("entity-styles.svg");
    let mut ui = DrawingUi::new();
    ui.screen_size = egui::vec2(1440.0, 1200.0);
    ui.click("New");
    ui.command("ENTITY COLOR BYBLOCK");
    assert!(ui.has_text("Select entities before applying"));
    ui.command("LINETYPE DASHED 1,-0.5");
    ui.command("LAYER NEW DETAIL");
    ui.command("LAYER COLOR DETAIL 3");
    ui.command("LAYER LINETYPE DETAIL DASHED");
    ui.command("LTSCALE 2");
    ui.command("LINE 0,0,3 10,0,3");
    ui.command("LINE 0,4,3 10,4,3");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let original = fs::read_to_string(&drawing).unwrap();
    assert_eq!(ui.nodes[&ui.field("Entity color")].value(), Some("BYLAYER"));
    for color in ["257", "-3"] {
        ui.replace_field("Entity color", color);
        ui.click("Apply entity color");
        assert!(ui.has_text("from 1 to 255"));
        ui.command("SAVE");
        assert_eq!(fs::read_to_string(&drawing).unwrap(), original);
    }
    ui.replace_field("Entity color", "1");
    ui.click("Apply entity color");
    ui.command("UNDO");
    assert_eq!(ui.nodes[&ui.field("Entity color")].value(), Some("BYLAYER"));
    ui.command("REDO");
    assert_eq!(ui.nodes[&ui.field("Entity color")].value(), Some("1"));
    let combo = ui.combo("Entity linetype");
    ui.action(combo, accesskit::Action::Click);
    ui.click("CONTINUOUS");
    ui.replace_field("Entity scale", "-1");
    ui.click("Apply entity scale");
    assert!(ui.has_text("nonnegative"));
    ui.replace_field("Entity scale", "0.5");
    ui.click("Apply entity scale");
    ui.replace_field("Entity scale", "0");
    ui.click("Apply entity scale");
    assert_eq!(ui.nodes[&ui.field("Entity scale")].value(), Some("1"));
    ui.command("UNDO");
    assert_eq!(ui.nodes[&ui.field("Entity scale")].value(), Some("0.5"));
    ui.command("SELECT ALL");
    assert_eq!(ui.nodes[&ui.field("Entity color")].value(), Some(""));
    assert_eq!(ui.nodes[&ui.field("Entity scale")].value(), Some(""));
    ui.replace_field("Entity color", "BYBLOCK");
    ui.click("Apply entity color");
    ui.command("UNDO");
    assert_eq!(ui.nodes[&ui.field("Entity color")].value(), Some(""));
    ui.command("REDO");
    assert_eq!(ui.nodes[&ui.field("Entity color")].value(), Some("BYBLOCK"));
    let combo = ui.combo("Entity linetype");
    ui.action(combo, accesskit::Action::Click);
    ui.click("BYBLOCK");
    ui.replace_field("Entity scale", "0.5");
    ui.click("Apply entity scale");
    ui.command("SELECT 1");
    ui.command("ENTITY COLOR BYLAYER");
    ui.command("ENTITY LINETYPE BYLAYER");
    ui.command("ENTITY LTSCALE 0.5");
    ui.command("SELECT");
    ui.command("FIT");
    let colors = ui.unselected_line_colors();
    assert_eq!(colors.len(), 8);
    assert_eq!(
        colors
            .iter()
            .filter(|color| **color == egui::Color32::GREEN)
            .count(),
        7
    );
    assert_eq!(
        colors
            .iter()
            .filter(|color| **color == egui::Color32::WHITE)
            .count(),
        1
    );
    ui.command("SAVE");
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    let exported = fs::read_to_string(svg).unwrap();
    assert_eq!(exported.matches("stroke=\"#00ff00\"").count(), 7);
    assert_eq!(exported.matches("stroke=\"#000000\"").count(), 1);
    let document = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    assert_eq!(
        document.entities(),
        DxfDocument::parse(&original).unwrap().entities()
    );
    assert_eq!(document.layer("DETAIL").unwrap().color_index(), 3);
    assert_eq!(document.layer("DETAIL").unwrap().linetype(), "DASHED");
    assert_eq!(document.linetype_scale(), 2.0);
    assert_eq!(document.entity_style(0).unwrap().color_index(), 256);
    assert_eq!(document.entity_style(0).unwrap().linetype(), "BYLAYER");
    assert_eq!(document.entity_style(1).unwrap().color_index(), 0);
    assert_eq!(document.entity_style(1).unwrap().linetype(), "BYBLOCK");
    for index in 0..2 {
        assert_eq!(document.entity_style(index).unwrap().linetype_scale(), 0.5);
    }
}

#[test]
fn arc_properties_keep_angle_units_and_source_text_through_widget_editing() {
    let scratch = Scratch::new();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/arcs.dxf");
    let drawing = scratch.0.join("arcs.dxf");
    let svg = scratch.0.join("arcs.svg");
    let mut ui = DrawingUi::new();
    ui.command(&format!("OPEN {}", source.display()));
    assert!(ui.has_text("Opened arcs.dxf"));
    ui.command("SELECT 1");
    ui.replace_field("Center X", "4");
    ui.click("Apply properties");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let expected = include_str!("fixtures/arcs.dxf").replacen("10\n1e0\n", "10\n4\n", 1);
    assert_eq!(fs::read_to_string(&drawing).unwrap(), expected);
    ui.replace_field("Radius", "-1");
    ui.click("Apply properties");
    assert!(ui.has_text("nonnegative"));
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&drawing).unwrap(), expected);
    ui.replace_field("Radius", "7");
    ui.replace_field("Start angle (°)", "0");
    ui.click("Apply properties");
    ui.command("MOVE 10 20 30");
    ui.command("COPY -10 -20 -30");
    ui.command("UNDO");
    ui.command("REDO");
    ui.command("SAVE");
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    let document = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    assert_eq!(document.entities().len(), 3);
    for (index, center) in [(0, [14.0, 22.0, 33.0]), (2, [4.0, 2.0, 3.0])] {
        let Entity::Arc(arc) = document.entities()[index] else {
            panic!("ARC expected")
        };
        assert_eq!(
            arc.center(),
            Point3::new(center[0], center[1], center[2]).unwrap()
        );
        assert_eq!(arc.radius(), 7.0);
        assert_eq!(arc.start_angle(), 0.0);
        assert!((arc.end_angle().to_degrees() - 90.0).abs() < 1e-12);
    }
    assert_eq!(
        fs::read_to_string(svg).unwrap().matches("<path ").count(),
        3
    );
    ui.command(&format!("OPEN {}", drawing.display()));
    ui.command("ARC 20,30,4 2 270 90");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    let Entity::Arc(arc) = document.entities()[3] else {
        panic!("command must create an ARC")
    };
    assert_eq!(arc.center(), Point3::new(20.0, 30.0, 4.0).unwrap());
    assert!((arc.sweep().to_degrees() - 180.0).abs() < 1e-12);
}

// Polyline drafts commit as one entity and one undo action. Earlier LINE tests
// commit each segment immediately and cannot catch draft loss or C switching
// to the circle tool. Closing-edge hit tests and later-vertex snaps use real paint.
#[test]
fn polyline_mouse_drafts_finish_cancel_select_and_snap_as_whole_objects() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("polyline-mouse.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("OSNAP OFF");
    ui.command("SNAP OFF");
    ui.command("ELEVATION 7");
    ui.click("Polyline");
    ui.canvas_click(0.4, 0.5);
    ui.canvas_click(0.6, 0.5);
    ui.canvas_click(0.6, 0.3);
    ui.click("Undo point");
    ui.canvas_click(0.6, 0.3);
    ui.step(vec![key(egui::Key::C, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::C, egui::Modifiers::NONE, false)]);
    ui.command(&format!("SAVEAS {}", drawing.display()));
    let closed = fs::read_to_string(&drawing).unwrap();
    let document = DxfDocument::parse(&closed).unwrap();
    let Entity::Polyline(shape) = &document.entities()[0] else {
        panic!("C must close the polyline")
    };
    assert_eq!(document.entities().len(), 1);
    assert_eq!(shape.vertices().len(), 3);
    assert!(shape.is_closed());
    assert!(shape.vertices().iter().all(|point| point.z() == 7.0));
    ui.command("UNDO");
    ui.command("SAVE");
    assert!(
        DxfDocument::parse(&fs::read_to_string(&drawing).unwrap())
            .unwrap()
            .entities()
            .is_empty()
    );
    ui.command("REDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&drawing).unwrap(), closed);
    ui.click("Polyline");
    ui.canvas_click(0.3, 0.3);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&drawing).unwrap(), closed);
    ui.command("PLINE 0,0,3 4,0,4");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&drawing).unwrap(), closed);
    ui.command("SELECT");
    ui.canvas_click(0.5, 0.4);
    assert!(ui.has_text("1 selected"));
    ui.canvas_click(0.5, 0.45);
    assert!(ui.has_text("0 selected"));
    ui.command("OSNAP ON");
    ui.click("Line");
    ui.canvas_click(0.6005, 0.3005);
    ui.canvas_click(0.7, 0.3);
    ui.command("");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&drawing).unwrap()).unwrap();
    let Entity::Line(line) = document.entities()[1] else {
        panic!("line snapped to a polyline vertex")
    };
    assert_eq!(line.start(), shape.vertices()[2]);
    ui.click("Polyline");
    ui.canvas_click(0.3, 0.7);
    ui.canvas_click(0.4, 0.7);
    ui.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Enter, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(drawing).unwrap()).unwrap();
    let Entity::Polyline(shape) = &document.entities()[2] else {
        panic!("Enter must finish a polyline")
    };
    assert_eq!(shape.vertices().len(), 2);
    assert!(!shape.is_closed());
}

// Dynamic coordinate fields, closure and vertex controls were not exercised by
// the fixed-size LINE/CIRCLE/ARC inspector checks. The saved artifact is also
// retained for independent AutoCAD import, ENTGET and R12 reimport verification.
#[test]
fn polyline_properties_vertices_styles_and_copy_survive_real_widget_save() {
    let scratch = Scratch::new();
    let drawing = scratch.0.join("polylines-widget.dxf");
    let svg = scratch.0.join("polylines-widget.svg");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("PLINE");
    ui.command("0,0,3");
    ui.command("4,0,4");
    assert!(ui.has_text("share one Z elevation"));
    ui.command("4,0");
    ui.command("@0,3");
    ui.command("C");
    ui.replace_field("Elevation", "5");
    ui.replace_field("Vertex 2 X", "6");
    ui.click("Apply properties");
    ui.toggle("Closed polyline");
    ui.command("UNDO");
    ui.click("Add vertex");
    ui.replace_field("Vertex 4 X", "-2");
    ui.replace_field("Vertex 4 Y", "1");
    ui.click("Apply properties");
    ui.click("Remove last vertex");
    ui.command("UNDO");
    ui.click("Reverse vertices");
    ui.command("UNDO");
    ui.command("LAYER NEW DETAIL");
    ui.command("LAYER ASSIGN DETAIL");
    ui.command("LAYER COLOR DETAIL 3");
    ui.command("LINETYPE DASHED 1,-0.5");
    ui.command("ENTITY LINETYPE DASHED");
    ui.command("ENTITY COLOR 4");
    ui.command("ENTITY LTSCALE 0.5");
    ui.command("LTSCALE 2");
    ui.command("MOVE 10 20 2");
    ui.command("COPY -10 -20 -2");
    ui.command("UNDO");
    ui.command("REDO");
    ui.command(&format!("SAVEAS {}", drawing.display()));
    ui.command(&format!("EXPORTSVG {}", svg.display()));
    let saved = fs::read_to_string(&drawing).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.entities().len(), 2);
    for (index, offset) in [(0, [10.0, 20.0, 2.0]), (1, [0.0, 0.0, 0.0])] {
        let Entity::Polyline(shape) = &document.entities()[index] else {
            panic!("one polyline per object")
        };
        assert!(shape.is_closed());
        let expected = [(0.0, 0.0), (6.0, 0.0), (4.0, 3.0), (-2.0, 1.0)]
            .map(|(x, y)| Point3::new(x + offset[0], y + offset[1], 5.0 + offset[2]).unwrap());
        assert_eq!(shape.vertices(), &expected);
        assert_eq!(document.entity_layer(index), Some("DETAIL"));
        assert_eq!(document.entity_style(index).unwrap().color_index(), 4);
        assert_eq!(document.entity_style(index).unwrap().linetype(), "DASHED");
        assert_eq!(document.entity_style(index).unwrap().linetype_scale(), 0.5);
    }
    assert_eq!(document.linetype_scale(), 2.0);
    let exported = fs::read_to_string(svg).unwrap();
    assert!(exported.matches("<line ").count() > 8);
    assert!(exported.contains("#00ffff"));
    ui.command("SELECT");
    let colors = ui.unselected_line_colors();
    assert!(colors.len() > 8);
    assert!(
        colors
            .iter()
            .all(|color| *color == egui::Color32::from_rgb(0, 255, 255))
    );
    ui.command(&format!("OPEN {}", drawing.display()));
    ui.command("SELECT 2");
    ui.replace_field("Vertex 2 X", "NaN");
    ui.click("Apply properties");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(drawing).unwrap(), saved);
}

#[test]
fn pline_creation_widths_stage_carry_close_and_undo_atomically() {
    use opencad::PolylineWidth;
    let scratch = Scratch::new();
    let path = scratch.0.join("pline-creation-widths.dxf");
    let mut ui = DrawingUi::new();
    ui.click("New");
    ui.command("PLINEWID");
    assert!(ui.has_text("PLINEWID = 0"));
    for invalid in ["-1", "NaN", "inf", "2 3"] {
        ui.command(&format!("PLINEWID {invalid}"));
    }
    ui.command("PLINEWID 2");
    ui.command(&format!("SAVEAS {}", path.display()));
    let baseline = fs::read_to_string(&path).unwrap();
    ui.command("PLINE -10,0 10,0");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let Entity::Polyline(polyline) = &document.entities()[0] else {
        panic!("polyline")
    };
    assert!(
        polyline
            .widths()
            .iter()
            .all(|width| *width == PolylineWidth::new(2.0, 2.0).unwrap())
    );
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), baseline);
    ui.command("PLINE -10,0 10,0 10,10 CLOSE");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let Entity::Polyline(polyline) = &document.entities()[0] else {
        panic!("polyline")
    };
    assert!(polyline.is_closed());
    assert!(
        polyline
            .widths()
            .iter()
            .all(|width| *width == PolylineWidth::new(2.0, 2.0).unwrap())
    );
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), baseline);

    ui.command("PLINE");
    ui.command("Width");
    assert!(ui.has_text("Specify the PLINE starting point"));
    ui.command("0,0,3");
    ui.command("Width 1 2 3");
    ui.command("Width");
    assert!(ui.has_text("PLINE Width · Starting width"));
    for invalid in ["-1", "NaN", "inf", "10,0"] {
        ui.command(invalid);
    }
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), baseline);
    ui.command(""); // starting default is the current width, 2
    assert!(ui.has_text("PLINE Width · Ending width"));
    ui.command(""); // ending default is the entered starting value
    ui.command("Width 2 6");
    ui.command("10,0");
    ui.command("20,0"); // ending width 6 carries forward uniformly
    ui.command("Halfwidth");
    ui.command("1");
    ui.command("SAVE"); // a draft and a half-entered pair are not serialized
    assert_eq!(fs::read_to_string(&path).unwrap(), baseline);
    ui.command("-1");
    ui.command("2"); // full widths are 2 -> 4
    ui.command("30,0");
    ui.command("U");
    ui.command("30,10"); // Undo restores the removed segment's taper
    ui.command("Width 9");
    ui.command("Exit"); // cancels both staged values, retaining carried width 4
    ui.step(vec![Event::PointerMoved(egui::pos2(1000.0, 450.0))]);
    ui.snapshot("pline-tapered-draft");
    ui.command("Close");
    ui.command("SAVE");
    let saved = fs::read_to_string(&path).unwrap();
    let document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.pline_width(), 2.0); // in-command choices don't alter PLINEWID
    let Entity::Polyline(polyline) = &document.entities()[0] else {
        panic!("polyline")
    };
    assert!(polyline.is_closed());
    assert_eq!(polyline.vertices().len(), 4);
    assert!(polyline.vertices().iter().all(|point| point.z() == 3.0));
    assert_eq!(
        polyline.widths(),
        &[(2.0, 6.0), (6.0, 6.0), (2.0, 4.0), (4.0, 4.0)]
            .map(|(start, end)| PolylineWidth::new(start, end).unwrap())
    );
    ui.snapshot("pline-tapered-closed");
    ui.command("UNDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), baseline);
    ui.command("REDO");
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    ui.command("PLINE");
    ui.command("0,0");
    ui.command("Width 8");
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, true)]);
    ui.step(vec![key(egui::Key::Escape, egui::Modifiers::NONE, false)]);
    ui.command("SAVE");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
}

#[test]
fn pline_width_defaults_follow_open_drawings_and_canvas_input() {
    use opencad::PolylineWidth;
    let scratch = Scratch::new();
    let path = scratch.0.join("pline-default.dxf");
    let invalid = scratch.0.join("invalid-default.dxf");
    let zero = scratch.0.join("missing-default.dxf");
    let minimal = "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
    let source = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$PLINEWID\n40\n7.00\n0\nENDSEC\n{minimal}"
    );
    fs::write(&path, &source).unwrap();
    fs::write(&invalid, source.replace("7.00", "NaN")).unwrap();
    fs::write(&zero, minimal).unwrap();
    let mut ui = DrawingUi::with_storage_and_path(None, Some(path.clone()));
    ui.command("PLINEWID");
    assert!(ui.has_text("PLINEWID = 7"));
    ui.command(&format!("OPEN {}", invalid.display()));
    ui.command("PLINEWID");
    assert!(ui.has_text("PLINEWID = 7"));
    ui.command("OSNAP OFF");
    ui.command("SNAP OFF");
    ui.command("PLINE");
    ui.canvas_click(0.4, 0.5);
    let press = |ui: &mut DrawingUi, value| {
        ui.step(vec![key(value, egui::Modifiers::NONE, true)]);
        ui.step(vec![key(value, egui::Modifiers::NONE, false)]);
    };
    press(&mut ui, egui::Key::W);
    ui.canvas_click(0.5, 0.4); // width prompt rejects adding a point
    press(&mut ui, egui::Key::Enter);
    press(&mut ui, egui::Key::Enter);
    ui.command("PLINEWID 9"); // the current draft retains its accepted width
    ui.canvas_click(0.6, 0.5);
    ui.command("Halfwidth 1e308 1"); // doubled halfwidth must be finite
    ui.canvas_click(0.6, 0.5); // focus canvas after the rejected option
    press(&mut ui, egui::Key::U); // remove this coincident draft vertex
    press(&mut ui, egui::Key::H);
    assert!(ui.has_text("PLINE Halfwidth · Starting halfwidth"));
    press(&mut ui, egui::Key::Enter);
    press(&mut ui, egui::Key::Enter);
    ui.canvas_click(0.7, 0.4);
    press(&mut ui, egui::Key::Enter);
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(document.pline_width(), 9.0);
    let Entity::Polyline(polyline) = &document.entities()[0] else {
        panic!("polyline")
    };
    assert_eq!(polyline.vertices().len(), 3);
    assert!(
        polyline
            .widths()
            .iter()
            .all(|width| *width == PolylineWidth::new(7.0, 7.0).unwrap())
    );
    ui.command("PLINE 0,0 10,0");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let Entity::Polyline(polyline) = &document.entities()[1] else {
        panic!("polyline")
    };
    assert!(
        polyline
            .widths()
            .iter()
            .all(|width| *width == PolylineWidth::new(9.0, 9.0).unwrap())
    );
    ui.command(&format!("OPEN {}", zero.display()));
    ui.command("PLINEWID");
    assert!(ui.has_text("PLINEWID = 0"));
    ui.command("PLINE 0,0 10,0");
    ui.command("SAVE");
    let document = DxfDocument::parse(&fs::read_to_string(&zero).unwrap()).unwrap();
    let Entity::Polyline(polyline) = &document.entities()[0] else {
        panic!("polyline")
    };
    assert!(
        polyline
            .widths()
            .iter()
            .all(|width| *width == PolylineWidth::default())
    );
}
