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
