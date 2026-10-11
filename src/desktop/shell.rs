use super::toolsets::{PanelEditor, PanelKey, ToolButton, UserPanel};
use super::*;
use egui::{Align, Align2, Button, FontId, RichText, Stroke};

/// The recorded Mac menu hierarchy. Empty captions are separators.
pub struct MenuItem {
    pub label: &'static str,
    pub action: &'static str,
    pub children: &'static [MenuItem],
}

pub(super) struct ActionInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub image: Option<&'static [u8]>,
}

pub(super) struct ToolbarGroup {
    pub label: &'static str,
    pub actions: &'static [&'static str],
}

pub(super) struct ToolGroup {
    pub label: &'static str,
    pub layout: &'static str,
    pub items: &'static [ToolItem],
}

pub(super) struct ToolItem {
    pub label: &'static str,
    pub visible: bool,
    pub actions: &'static [&'static str],
}

pub(super) struct StatusItem {
    pub id: &'static str,
    pub label: &'static str,
    pub off: &'static str,
    pub on: &'static str,
}

pub(super) struct Workspace {
    pub start: bool,
    pub has_drawing: bool,
    pub toolbar: bool,
    pub file_tabs: bool,
    pub tools: bool,
    pub layers: bool,
    pub properties: bool,
    pub command: bool,
    pub history: bool,
    pub status: bool,
    pub view_cube: bool,
    pub ucs_icon: bool,
    pub connect: bool,
    pub hidden_status: HashSet<&'static str>,
    pub slim_tools: bool,
    pub(super) tool_visibility: HashMap<(&'static str, usize), bool>,
    pub(super) collapsed_tools: HashSet<&'static str>,
    pub(super) tool_order: Vec<PanelKey>,
    pub(super) user_panels: Vec<UserPanel>,
    pub(super) next_panel_id: u64,
    pub(super) panel_editor: Option<PanelEditor>,
    pub(super) scroll_to_panel: Option<u64>,
    pub(super) delete_panel: Option<u64>,
    pub(super) reorder_selection: Option<PanelKey>,
    pub(super) save_preferences_requested: bool,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            start: true,
            has_drawing: false,
            toolbar: true,
            file_tabs: true,
            tools: true,
            layers: true,
            properties: true,
            command: true,
            history: false,
            status: true,
            view_cube: true,
            ucs_icon: true,
            connect: true,
            hidden_status: HashSet::new(),
            slim_tools: false,
            tool_visibility: HashMap::new(),
            collapsed_tools: HashSet::new(),
            tool_order: catalog::TOOL_GROUPS
                .iter()
                .map(|group| PanelKey::System(group.label))
                .collect(),
            user_panels: Vec::new(),
            next_panel_id: 1,
            panel_editor: None,
            scroll_to_panel: None,
            delete_panel: None,
            reorder_selection: None,
            save_preferences_requested: false,
        }
    }
}

impl Workspace {
    fn toolset_width(&self) -> f32 {
        if !self.tools {
            0.0
        } else if self.slim_tools {
            90.0
        } else {
            184.0
        }
    }

    fn tool_visible(&self, group: &ToolGroup, index: usize) -> bool {
        self.tool_visibility
            .get(&(group.label, index))
            .copied()
            .unwrap_or(group.items[index].visible)
    }

    fn move_tool_group(&mut self, key: PanelKey, destination: usize) {
        let Some(source) = self.tool_order.iter().position(|known| *known == key) else {
            return;
        };
        let destination = destination.min(self.tool_order.len());
        let destination = destination.saturating_sub(usize::from(source < destination));
        self.reorder_selection = Some(key);
        if source != destination {
            let group = self.tool_order.remove(source);
            self.tool_order.insert(destination, group);
            self.save_preferences_requested = true;
        }
    }
}

struct PanelDrag {
    table: egui::Id,
    key: PanelKey,
}

#[derive(Clone, Default)]
struct MenuSnapshot {
    blocked: bool,
    editing_text: bool,
    drawing: bool,
    undo: bool,
    redo: bool,
    selection: bool,
    tools: bool,
    layers: bool,
    properties: bool,
    command: bool,
    toolbar: bool,
    file_tabs: bool,
    status: bool,
    view_cube: bool,
    ucs_icon: bool,
}

/// Availability is read by AppKit when it validates native menu items.
pub struct MenuStatus {
    pub enabled: bool,
    pub checked: Option<bool>,
}

pub fn native_menus() -> &'static [MenuItem] {
    catalog::MENUS
}

/// Native callbacks enqueue work for the editor event loop.
pub fn queue_menu_action(ctx: &egui::Context, action: &str) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Vec<String>>(egui::Id::new("native_menu_queue"))
            .push(action.to_owned());
    });
    ctx.request_repaint();
}

fn command_for_action(action: &str) -> Option<&'static str> {
    Some(match action {
        "ID_New" => "NEW",
        "ID_Open" => "OPEN",
        "ID_Save" => "SAVE",
        "ID_Saveas" => "SAVEAS",
        "ID_Export" => "EXPORTSVG",
        "ID_DWG_CLOSE" => "CLOSEDRAWING",
        "ID_U" => "UNDO",
        "ID_Redo" => "REDO",
        "ID_SelAll" => "SELECT ALL",
        "ID_Erase" | "MM_1561" => "ERASE",
        "ID_Line" => "LINE",
        "ID_Pline" => "PLINE",
        "ID_CircleRad" => "CIRCLE",
        "ID_ArcCeStEn" => "ARC",
        "ID_Rectang" => "RECTANG",
        "ID_Move" => "MOVE",
        "ID_Copy" => "COPY",
        "ID_Pan" | "ID_PanPick" => "PAN",
        "ID_ZoomExten" | "ID_ZoomAll" => "ZOOM EXTENTS",
        "ID_ZoomIn" => "ZOOM 2X",
        "ID_ZoomOut" => "ZOOM 0.5X",
        "ID_PanLeft" => "PAN LEFT",
        "ID_PanRight" => "PAN RIGHT",
        "ID_PanUp" => "PAN UP",
        "ID_PanDown" => "PAN DOWN",
        "ID_ToolSets" => "TOOLSETS",
        "ID_Layer" => "LAYERPALETTE",
        "ID_Properties" => "PROPERTIESTOGGLE",
        "ID_CmdLine" => "COMMANDLINETOGGLE",
        "ID_Toolbar" => "TOOLBAR",
        "ID_FileTab" => "FILETAB",
        "ID_StatusBar" => "STATUSBAR",
        "ID_ViewcubeOn" => "VIEWCUBE",
        "ID_UcsiconOn" => "UCSICON",
        "ID_ResetPalettes" => "RESETPALETTES",
        "MM_1904" => "PALETTES",
        "ID_Redrawall" | "ID_Regen" | "ID_Regenall" => "REGEN",
        "ID_Help" => "HELP",
        _ => return None,
    })
}

pub fn menu_action_status(ctx: &egui::Context, action: &str) -> MenuStatus {
    let state = ctx
        .data(|data| data.get_temp::<MenuSnapshot>(egui::Id::new("native_menu_state")))
        .unwrap_or_default();
    let checked = match action {
        "ID_ToolSets" => Some(state.tools),
        "ID_Layer" => Some(state.layers),
        "ID_Properties" => Some(state.properties),
        "ID_CmdLine" => Some(state.command),
        "ID_Toolbar" => Some(state.toolbar),
        "ID_FileTab" => Some(state.file_tabs),
        "ID_StatusBar" => Some(state.status),
        "ID_ViewcubeOn" => Some(state.view_cube),
        "ID_UcsiconOn" => Some(state.ucs_icon),
        _ => None,
    };
    let enabled = command_for_action(action).is_some()
        && !state.blocked
        && match action {
            "ID_New" | "ID_Open" | "ID_Help" | "ID_Toolbar" | "ID_FileTab" | "ID_CmdLine"
            | "ID_ResetPalettes" => true,
            "ID_U" => state.drawing && state.undo && !state.editing_text,
            "ID_Redo" => state.drawing && state.redo && !state.editing_text,
            "ID_SelAll" => state.drawing && !state.editing_text,
            "ID_Erase" | "MM_1561" | "ID_Move" | "ID_Copy" => state.drawing && state.selection,
            _ => state.drawing,
        };
    MenuStatus { enabled, checked }
}

pub(super) fn install_system_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    #[cfg(target_os = "macos")]
    {
        for (name, path, index) in [
            ("Mac UI", "/System/Library/Fonts/SFNS.ttf", 0),
            ("Mac Symbols", "/System/Library/Fonts/Apple Symbols.ttf", 0),
            ("Collapse", "/System/Library/Fonts/LucidaGrande.ttc", 0),
            ("Toolset", "/System/Library/Fonts/HelveticaNeue.ttc", 0),
            ("Caption", "/System/Library/Fonts/HelveticaNeue.ttc", 1),
            ("Command", "/System/Library/Fonts/SFNS.ttf", 0),
            ("History", "/System/Library/Fonts/SFNS.ttf", 0),
            ("Prompt", "/System/Library/Fonts/SFNS.ttf", 0),
        ] {
            if let Ok(bytes) = fs::read(path) {
                let mut data = egui::FontData::from_owned(bytes);
                data.index = index;
                fonts
                    .font_data
                    .insert(name.into(), std::sync::Arc::new(data));
            }
        }
        if fonts.font_data.contains_key("Mac UI") {
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .expect("default proportional fonts")
                .insert(0, "Mac UI".into());
        }
        if fonts.font_data.contains_key("Mac Symbols") {
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .expect("default proportional fonts")
                .push("Mac Symbols".into());
        }
    }
    let fallback = fonts.families[&egui::FontFamily::Proportional].clone();
    for name in [
        "Toolset",
        "Caption",
        "Command",
        "History",
        "Prompt",
        "Mac Symbols",
        "Collapse",
    ] {
        let mut family = fallback.clone();
        if fonts.font_data.contains_key(name) {
            family.insert(0, name.into());
        }
        fonts
            .families
            .insert(egui::FontFamily::Name(name.into()), family);
    }
    ctx.set_fonts(fonts);
}

pub(super) fn action_info(id: &str) -> Option<&'static ActionInfo> {
    catalog::ACTIONS
        .binary_search_by_key(&id, |action| action.id)
        .ok()
        .map(|index| &catalog::ACTIONS[index])
}

impl OpenCadApp {
    pub(super) fn text_owns_edit_shortcuts(&self, ctx: &egui::Context) -> bool {
        let command_id =
            ctx.data(|data| data.get_temp::<egui::Id>(egui::Id::new("command_field_id")));
        ctx.text_edit_focused()
            && (ctx.memory(|memory| memory.focused()) != command_id || !self.command.is_empty())
    }

    pub(super) fn publish_menu_state(&self, ctx: &egui::Context) {
        let editing_text = self.text_owns_edit_shortcuts(ctx);
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("native_menu_state"),
                MenuSnapshot {
                    blocked: self.pending.is_some() || self.show_help,
                    editing_text,
                    drawing: self.workspace.has_drawing && !self.workspace.start,
                    undo: self.editor.can_undo(),
                    redo: self.editor.can_redo(),
                    selection: !self.editor.selection().is_empty(),
                    tools: self.workspace.tools,
                    layers: self.workspace.layers,
                    properties: self.workspace.properties,
                    command: self.workspace.command,
                    toolbar: self.workspace.toolbar,
                    file_tabs: self.workspace.file_tabs,
                    status: self.workspace.status,
                    view_cube: self.workspace.view_cube,
                    ucs_icon: self.workspace.ucs_icon,
                },
            );
        });
    }

    pub(super) fn drain_menu_actions(&mut self, ctx: &egui::Context) {
        let actions = ctx.data_mut(|data| {
            std::mem::take(
                data.get_temp_mut_or_default::<Vec<String>>(egui::Id::new("native_menu_queue")),
            )
        });
        for action in actions {
            if menu_action_status(ctx, &action).enabled
                && let Some(command) = command_for_action(&action)
            {
                self.execute(command);
                self.publish_menu_state(ctx);
            }
        }
    }

    pub(super) fn load_icon(&mut self, ctx: &egui::Context, id: &'static str) {
        if !self.icons.contains_key(id)
            && let Some(bytes) = action_info(id).and_then(|action| action.image)
            && let Ok(image) = image::load_from_memory(bytes)
        {
            let image = image.to_rgba8();
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            self.icons.insert(
                id,
                ctx.load_texture(id, pixels, egui::TextureOptions::LINEAR),
            );
        }
    }

    pub(super) fn icon_button(
        &mut self,
        ui: &mut egui::Ui,
        id: &'static str,
        size: f32,
        icon_size: f32,
        enabled: bool,
    ) -> egui::Response {
        self.load_icon(ui.ctx(), id);
        let action = action_info(id);
        let label = match id {
            "ID_New" => "New",
            "ID_Open" => "Open DXF",
            "ID_Saveas" => "Save as",
            _ => action.map_or(id, |action| action.label),
        };
        let selected = match id {
            "ID_Line" => self.tool == Tool::Line,
            "ID_Pline" => self.tool == Tool::Polyline,
            "ID_CircleRad" => self.tool == Tool::Circle,
            "ID_ArcCeStEn" | "ID_Arc3point" => self.tool == Tool::Arc,
            "ID_Rectang" => self.tool == Tool::Rectangle,
            "ID_Move" => self.tool == Tool::Move,
            "ID_Copy" => self.tool == Tool::Copy,
            "ID_Pan" => self.tool == Tool::Pan,
            "UI_Palette_layers" => self.workspace.layers,
            "UI_Toolset_drafting" => true,
            _ => id.starts_with("UI_Status_") && id.ends_with("_on"),
        };
        let button = if let Some(texture) = self.icons.get(id) {
            Button::new(
                egui::Image::from_texture(texture)
                    .fit_to_exact_size(egui::vec2(icon_size, icon_size))
                    .alt_text(label),
            )
        } else {
            Button::new(RichText::new(label.chars().next().unwrap_or('·')).size(icon_size))
        }
        .frame_when_inactive(false)
        .selected(selected);
        let response = ui
            .scope(|ui| {
                // Fractional padding rounds outward in egui; the explicit minimum
                // size supplies the remaining pixel without enlarging the cell.
                ui.spacing_mut().button_padding =
                    egui::Vec2::splat(((size - icon_size) / 2.0).floor());
                ui.spacing_mut().interact_size = egui::Vec2::splat(size);
                ui.add_enabled(enabled, button.min_size(egui::vec2(size, size)))
            })
            .inner;
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
        response.on_hover_text(if enabled {
            action.map_or(label, |action| action.help)
        } else {
            "This command is not available in the current workspace."
        })
    }

    pub(super) fn header(&mut self, ui: &mut egui::Ui) {
        #[cfg(not(target_os = "macos"))]
        egui::Panel::top("menu_bar")
            .exact_size(24.0)
            .show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    for menu in native_menus() {
                        ui.menu_button(menu.label, |ui| self.menu_items(ui, menu.children));
                    }
                });
            });
        if self.workspace.toolbar {
            egui::Panel::top("toolbar")
                .exact_size(34.0)
                .frame(
                    egui::Frame::new()
                        .fill(PANEL)
                        .inner_margin(egui::Margin::symmetric(4, 3)),
                )
                .show(ui, |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("toolbar_scroll")
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 11.0;
                                let width = catalog::TOOLBARS
                                    .iter()
                                    .map(|group| group.actions.len() as f32 * 33.0)
                                    .sum::<f32>()
                                    + (catalog::TOOLBARS.len() - 1) as f32 * 16.0
                                    - 11.0;
                                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                                for group in catalog::TOOLBARS {
                                    ui.push_id(group.label, |ui| {
                                        for &id in group.actions {
                                            if self
                                                .icon_button(
                                                    ui,
                                                    id,
                                                    22.0,
                                                    16.0,
                                                    menu_action_status(ui.ctx(), id).enabled,
                                                )
                                                .clicked()
                                            {
                                                queue_menu_action(ui.ctx(), id);
                                            }
                                        }
                                    });
                                    if group.label
                                        != catalog::TOOLBARS
                                            .last()
                                            .expect("default toolbar groups")
                                            .label
                                    {
                                        ui.add_space(5.0);
                                    }
                                }
                            });
                        });
                });
        }
        if self.workspace.file_tabs {
            egui::Panel::top("file_tabs")
                .exact_size(24.0)
                .frame(
                    egui::Frame::new()
                        .fill(PANEL)
                        .inner_margin(egui::Margin::ZERO),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(24.0, 23.0), egui::Sense::click());
                        for (index, offset) in [(-3.5, -3.5), (3.5, -3.5), (-3.5, 3.5), (3.5, 3.5)]
                            .into_iter()
                            .enumerate()
                        {
                            ui.painter().rect_filled(
                                egui::Rect::from_center_size(
                                    rect.center() + egui::vec2(offset.0, offset.1),
                                    egui::vec2(5.0, 5.0),
                                ),
                                0.0,
                                if index == 0 { ACCENT } else { MUTED },
                            );
                        }
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Start page")
                        });
                        if response.on_hover_text("Start page").clicked() {
                            self.workspace.start = true;
                        }
                        if ui
                            .add_sized([24.0, 23.0], Button::new("+").frame(false))
                            .on_hover_text("New drawing")
                            .clicked()
                        {
                            self.request(PendingAction::New);
                        }
                        if ui
                            .add_sized(
                                [130.0, 23.0],
                                Button::selectable(self.workspace.start, "Start"),
                            )
                            .clicked()
                        {
                            self.workspace.start = true;
                        }
                        if self.workspace.has_drawing {
                            let name = format!(
                                "{}{}",
                                self.name,
                                if self.editor.is_dirty() { "*" } else { "" }
                            );
                            if ui
                                .add_sized(
                                    [160.0, 23.0],
                                    Button::selectable(!self.workspace.start, name),
                                )
                                .clicked()
                            {
                                self.workspace.start = false;
                            }
                        }
                    });
                });
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn menu_items(&mut self, ui: &mut egui::Ui, items: &'static [MenuItem]) {
        for item in items {
            if item.label.is_empty() {
                ui.separator();
            } else if !item.children.is_empty() {
                ui.menu_button(item.label, |ui| self.menu_items(ui, item.children));
            } else if ui
                .add_enabled(
                    menu_action_status(ui.ctx(), item.action).enabled,
                    Button::new(item.label),
                )
                .clicked()
            {
                queue_menu_action(ui.ctx(), item.action);
                ui.close();
            }
        }
    }

    pub(super) fn tools_panel(&mut self, ui: &mut egui::Ui) {
        if !self.workspace.tools {
            return;
        }
        egui::Panel::left("tools")
            .exact_size(self.workspace.toolset_width())
            .frame(egui::Frame::new().fill(Color32::from_rgb(60, 68, 82)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                self.toolset_selector(ui);
                if !self.workspace.status {
                    egui::Panel::bottom("toolset_bottom")
                        .exact_size(24.0)
                        .frame(egui::Frame::NONE)
                        .show(ui, |ui| self.toolset_bottom_bar(ui));
                }
                egui::ScrollArea::vertical()
                    .id_salt("toolsets_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for key in self.workspace.tool_order.clone() {
                            match key {
                                PanelKey::System(label) => {
                                    if let Some(group) = catalog::TOOL_GROUPS
                                        .iter()
                                        .find(|group| group.label == label)
                                    {
                                        self.tool_group_header(ui, group);
                                    }
                                }
                                PanelKey::User(id) => self.user_panel_header(ui, id),
                            }
                        }
                    });
            });
    }

    fn toolset_bottom_bar(&mut self, ui: &mut egui::Ui) {
        let (_, bar) = ui.allocate_space(egui::vec2(self.workspace.toolset_width(), 24.0));
        ui.painter()
            .rect_filled(bar, 0.0, Color32::from_rgb(60, 68, 82));
        ui.painter().hline(
            bar.x_range(),
            bar.top(),
            Stroke::new(1.5, Color32::from_rgb(47, 52, 63)),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("toolset_bottom_bar")
                .max_rect(bar)
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 0.0;
        child.spacing_mut().button_padding = egui::Vec2::ZERO;
        child.spacing_mut().interact_size = egui::Vec2::splat(24.0);
        let add = child.add(
            Button::new("")
                .frame(false)
                .min_size(egui::Vec2::splat(24.0)),
        );
        add.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add panel"));
        let center = add.rect.center();
        child.painter().hline(
            center.x - 7.0..=center.x + 7.0,
            center.y,
            Stroke::new(1.0, MUTED),
        );
        child.painter().vline(
            center.x,
            center.y - 7.0..=center.y + 7.0,
            Stroke::new(1.0, MUTED),
        );
        if add.clicked() && !self.workspace.add_user_panel() {
            self.message("Unable to create another panel.", true);
        }
        add.on_hover_text("Create a new panel");
        let reorder = child.add(
            Button::new("")
                .frame(false)
                .min_size(egui::Vec2::splat(24.0)),
        );
        reorder.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Reorder panels")
        });
        let center = reorder.rect.center();
        for offset in [-4.0, 0.0, 4.0] {
            child.painter().hline(
                center.x - 7.0..=center.x + 4.0,
                center.y + offset,
                Stroke::new(1.0, MUTED),
            );
        }
        child.painter().line_segment(
            [
                center + egui::vec2(6.0, -7.0),
                center + egui::vec2(6.0, 7.0),
            ],
            Stroke::new(1.0, MUTED),
        );
        if reorder.clicked() {
            self.workspace.reorder_selection = None;
        }
        // DragAndDrop consumes Escape before popovers process it. The same
        // press must both abandon the drag and leave the reorder interface.
        if child.input(|input| {
            input.raw.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: Key::Escape,
                        pressed: true,
                        ..
                    }
                )
            })
        }) {
            egui::Popup::close_id(child.ctx(), egui::Popup::default_response_id(&reorder));
        }
        egui::Popup::menu(&reorder.on_hover_text("Reorder Panels"))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(egui::Frame::popup(child.style()).inner_margin(egui::Margin::ZERO))
            .width(200.0)
            .show(|ui| self.toolset_reorder(ui));
    }

    fn toolset_reorder(&mut self, ui: &mut egui::Ui) {
        use egui::accesskit::{Action, ActionData, CustomAction};
        const MOVE_UP: i32 = 1;
        const MOVE_DOWN: i32 = 2;

        let (_, content) = ui.allocate_space(egui::vec2(200.0, 330.0));
        let table_rect = egui::Rect::from_min_size(
            content.min + egui::vec2(12.0, 0.0),
            egui::vec2(176.0, 300.0),
        );
        let table = ui.make_persistent_id("panel_order");
        let drop_area = ui.interact(table_rect, table, egui::Sense::hover());
        let mut list = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(table)
                .max_rect(table_rect)
                .layout(egui::Layout::top_down(Align::Min)),
        );
        list.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
        let mut movement = None;
        let mut last_row_bottom = table_rect.top();
        let groups = self.workspace.tool_order.clone();
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .min_scrolled_height(300.0)
            .auto_shrink([false, false])
            .show(&mut list, |ui| {
                for (index, &key) in groups.iter().enumerate() {
                    let Some(label) = self.workspace.panel_name(key).map(str::to_owned) else {
                        continue;
                    };
                    let id = table.with(key);
                    let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), 17.0));
                    last_row_bottom = rect.bottom();
                    let row = ui
                        .interact(rect, id, egui::Sense::click_and_drag())
                        .on_hover_cursor(egui::CursorIcon::Grab);
                    if row.clicked() {
                        self.workspace.reorder_selection = Some(key);
                    }
                    let selected = self.workspace.reorder_selection == Some(key);
                    row.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            true,
                            selected,
                            format!("Reorder panel: {label}"),
                        )
                    });
                    ui.ctx().accesskit_node_builder(id, |node| {
                        node.set_selected(selected);
                        let mut actions = Vec::new();
                        if index > 0 {
                            actions.push(CustomAction {
                                id: MOVE_UP,
                                description: "Move panel up".into(),
                            });
                        }
                        if index + 1 < groups.len() {
                            actions.push(CustomAction {
                                id: MOVE_DOWN,
                                description: "Move panel down".into(),
                            });
                        }
                        node.set_custom_actions(actions);
                    });
                    let action = ui.input(|input| {
                        input
                            .accesskit_action_requests(id, Action::CustomAction)
                            .find_map(|request| match request.data {
                                Some(ActionData::CustomAction(MOVE_UP)) if index > 0 => {
                                    Some(index - 1)
                                }
                                Some(ActionData::CustomAction(MOVE_DOWN))
                                    if index + 1 < groups.len() =>
                                {
                                    Some(index + 2)
                                }
                                _ => None,
                            })
                    });
                    if let Some(destination) = action {
                        movement = Some((key, destination));
                    }
                    if selected || row.has_focus() {
                        ui.painter()
                            .rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
                    }
                    ui.painter().text(
                        rect.left_center() + egui::vec2(3.0, 0.0),
                        Align2::LEFT_CENTER,
                        &label,
                        FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                    row.dnd_set_drag_payload(PanelDrag { table, key });
                    if let Some(payload) = row.dnd_hover_payload::<PanelDrag>()
                        && payload.table == table
                    {
                        let below = ui.input(|input| {
                            input
                                .pointer
                                .interact_pos()
                                .is_some_and(|pos| pos.y > rect.center().y)
                        });
                        ui.painter().hline(
                            rect.x_range(),
                            if below { rect.bottom() } else { rect.top() },
                            Stroke::new(1.5, ACCENT),
                        );
                        if let Some(payload) = row.dnd_release_payload::<PanelDrag>() {
                            movement = Some((payload.key, index + usize::from(below)));
                        }
                    }
                }
            });
        if drop_area.contains_pointer()
            && ui.input(|input| {
                input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| pos.y >= last_row_bottom)
            })
            && let Some(payload) = drop_area.dnd_hover_payload::<PanelDrag>()
            && payload.table == table
        {
            ui.painter().hline(
                table_rect.x_range(),
                last_row_bottom,
                Stroke::new(1.5, ACCENT),
            );
            if let Some(payload) = drop_area.dnd_release_payload::<PanelDrag>() {
                movement = Some((payload.key, groups.len()));
            }
        }
        if let Some((key, destination)) = movement {
            self.workspace.move_tool_group(key, destination);
            ui.ctx().request_repaint();
        }
        let mut instruction =
            ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                content.min + egui::vec2(10.0, 312.0),
                egui::vec2(180.0, 16.0),
            )));
        instruction
            .add(egui::Label::new(RichText::new("Drag to reorder.").size(12.0)).selectable(false));
    }

    fn toolset_selector(&mut self, ui: &mut egui::Ui) {
        let (_, row) = ui.allocate_space(egui::vec2(self.workspace.toolset_width(), 23.0));
        let cell_width = (row.width() - 23.0) / 2.0;
        // Place the cells from fixed edges so fractional widths cannot accumulate
        // rounding error and push the toggle outside the 90-point palette.
        for (index, caption, id) in [
            (0, "Drafting", "UI_Toolset_drafting"),
            (1, "Modeling", "UI_Toolset_modeling"),
        ] {
            let cell = egui::Rect::from_min_size(
                row.min + egui::vec2(index as f32 * cell_width, 0.0),
                egui::vec2(cell_width, 23.0),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("toolset_selector", index))
                    .max_rect(cell)
                    .layout(egui::Layout::centered_and_justified(
                        egui::Direction::LeftToRight,
                    )),
            );
            child.spacing_mut().button_padding = egui::Vec2::ZERO;
            child.spacing_mut().interact_size.y = 23.0;
            if self.workspace.slim_tools {
                self.icon_button(&mut child, id, 23.0, 16.0, index == 0);
            } else {
                child.add_enabled(index == 0, Button::selectable(index == 0, caption));
            }
        }
        let toggle_rect = egui::Rect::from_min_size(
            egui::pos2(row.right() - 23.0, row.top()),
            egui::Vec2::splat(23.0),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("toolset_width")
                .max_rect(toggle_rect)
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::LeftToRight,
                )),
        );
        let toggle = if self.workspace.slim_tools {
            "UI_Toolset_expand"
        } else {
            "UI_Toolset_collapse"
        };
        if self
            .icon_button(&mut child, toggle, 23.0, 8.0, true)
            .clicked()
        {
            self.workspace.slim_tools = !self.workspace.slim_tools;
        }
    }

    fn tool_group_header(&mut self, ui: &mut egui::Ui, group: &'static ToolGroup) {
        let id = ui.make_persistent_id(("toolset", group.label));
        let mut collapsed = self.workspace.collapsed_tools.contains(group.label);
        let (mut state, gear, _) =
            super::toolsets::toolset_header(ui, id, group.label, &mut collapsed, false);
        if collapsed {
            self.workspace.collapsed_tools.insert(group.label);
        } else {
            self.workspace.collapsed_tools.remove(group.label);
        }
        let label = format!("Customize {} panel", group.label);
        egui::Popup::menu(&gear.on_hover_text(label))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.tool_group_settings(ui, group));
        state.show_body_unindented(ui, |ui| self.tool_group(ui, group));
        let (_, separator) = ui.allocate_space(egui::vec2(ui.available_width(), 1.0));
        ui.painter().hline(
            separator.left() + 4.0..=separator.right() - 4.0,
            separator.center().y,
            Stroke::new(1.0, Color32::from_rgb(47, 52, 63)),
        );
    }

    fn tool_group_settings(&mut self, ui: &mut egui::Ui, group: &'static ToolGroup) {
        ui.label(format!("Customize {} panel", group.label));
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
        egui::ScrollArea::vertical()
            .id_salt(("toolset_settings", group.label))
            .max_height(360.0)
            .show(ui, |ui| {
                for (index, item) in group.items.iter().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            let mut visible = self.workspace.tool_visible(group, index);
                            let checkbox = ui.checkbox(&mut visible, "");
                            checkbox.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::Checkbox,
                                    true,
                                    visible,
                                    format!("Show {}: {}", group.label, item.label),
                                )
                            });
                            if checkbox.changed() {
                                if visible == item.visible {
                                    self.workspace.tool_visibility.remove(&(group.label, index));
                                } else {
                                    self.workspace
                                        .tool_visibility
                                        .insert((group.label, index), visible);
                                }
                            }
                            if let Some(&id) = item.actions.first() {
                                self.load_icon(ui.ctx(), id);
                                if let Some(texture) = self.icons.get(id) {
                                    ui.add(
                                        egui::Image::from_texture(texture)
                                            .fit_to_exact_size(egui::Vec2::splat(16.0))
                                            .alt_text(item.label),
                                    );
                                } else {
                                    ui.allocate_space(egui::Vec2::splat(16.0));
                                }
                            }
                            let caption = match item.actions {
                                [id] => action_info(id).map_or(item.label, |action| action.label),
                                _ => item.label,
                            };
                            ui.label(caption);
                        });
                    });
                }
            });
        if ui.button("Restore default panel commands").clicked() {
            self.workspace
                .tool_visibility
                .retain(|(label, _), _| *label != group.label);
        }
    }

    fn tool_group(&mut self, ui: &mut egui::Ui, group: &'static ToolGroup) {
        let items: Vec<_> = group
            .items
            .iter()
            .enumerate()
            .filter(|(index, _)| self.workspace.tool_visible(group, *index))
            .map(|(index, item)| ToolButton {
                key: index as u64,
                label: item.label,
                actions: item.actions.to_vec(),
                dropdown_name: None,
            })
            .collect();
        let layout = group
            .layout
            .strip_prefix("Layout")
            .and_then(|number| number.parse().ok())
            .unwrap_or(6);
        self.tool_buttons(ui, layout, &items);
    }

    pub(super) fn tool_buttons(&mut self, ui: &mut egui::Ui, layout: u8, items: &[ToolButton<'_>]) {
        if items.is_empty() {
            return;
        }
        if self.workspace.slim_tools {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            for row in items.chunks(3) {
                ui.horizontal(|ui| {
                    for item in row {
                        egui::Frame::new()
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| self.tool_item(ui, item, 22.0, 16.0));
                    }
                });
            }
            return;
        }
        let columns = super::toolsets::layout_columns(layout);
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let mut index = 0;
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            for &column in columns {
                if index == items.len() {
                    break;
                }
                if column == 22 {
                    ui.allocate_ui_with_layout(
                        egui::vec2(22.0, 44.0),
                        egui::Layout::top_down(Align::Min),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for _ in 0..2 {
                                if let Some(item) = items.get(index) {
                                    self.tool_item(ui, item, 22.0, 16.0);
                                    index += 1;
                                }
                            }
                        },
                    );
                } else {
                    let size = if column == 1 { 44.0 } else { 22.0 };
                    self.tool_item(
                        ui,
                        &items[index],
                        size,
                        if column == 1 { 32.0 } else { 16.0 },
                    );
                    index += 1;
                }
            }
            ui.add_space(4.0);
        });
        ui.add_space(2.0);
        for row in items[index..].chunks(8) {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                for item in row {
                    self.tool_item(ui, item, 22.0, 16.0);
                }
                ui.add_space(4.0);
            });
            ui.add_space(2.0);
        }
    }

    fn tool_item(&mut self, ui: &mut egui::Ui, item: &ToolButton<'_>, size: f32, icon_size: f32) {
        ui.push_id(item.key, |ui| {
            self.tool_item_inner(ui, item, size, icon_size)
        });
    }

    fn tool_item_inner(
        &mut self,
        ui: &mut egui::Ui,
        item: &ToolButton<'_>,
        size: f32,
        icon_size: f32,
    ) {
        let Some(&id) = item.actions.first() else {
            ui.add_enabled(false, Button::new("▾").min_size(egui::Vec2::splat(size)))
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, false, item.label)
                });
            return;
        };
        let enabled = item
            .actions
            .iter()
            .any(|id| menu_action_status(ui.ctx(), id).enabled);
        let response = self
            .icon_button(ui, id, size, icon_size, enabled)
            .on_hover_text(item.label);
        if let Some(name) = item.dropdown_name {
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
        }
        let dropdown = item.dropdown_name.is_some() || item.actions.len() > 1;
        if dropdown {
            let corner = response.rect.right_bottom() - egui::vec2(3.0, 3.0);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    corner,
                    corner - egui::vec2(4.0, 0.0),
                    corner - egui::vec2(0.0, 4.0),
                ],
                MUTED,
                Stroke::NONE,
            ));
        }
        let accessible_menu = dropdown
            && ui.input(|input| {
                input
                    .accesskit_action_requests(response.id, egui::accesskit::Action::CustomAction)
                    .any(|request| {
                        matches!(
                            request.data,
                            Some(egui::accesskit::ActionData::CustomAction(1))
                        )
                    })
            });
        if dropdown {
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_custom_actions(vec![egui::accesskit::CustomAction {
                    id: 1,
                    description: "Open command menu".into(),
                }])
            });
        }
        let open_menu = accessible_menu
            || response.secondary_clicked()
            || (response.clicked()
                && dropdown
                && (response.interact_pointer_pos().is_some_and(|position| {
                    position.x > response.rect.right() - 10.0
                        && position.y > response.rect.bottom() - 10.0
                }) || !menu_action_status(ui.ctx(), id).enabled));
        if response.clicked() && !open_menu {
            queue_menu_action(ui.ctx(), id);
        }
        if dropdown {
            egui::Popup::menu(&response)
                .open_memory(open_menu.then_some(egui::SetOpenCommand::Toggle))
                .show(|ui| {
                    for &action in &item.actions {
                        let label = action_info(action).map_or(action, |action| action.label);
                        if ui
                            .add_enabled(
                                menu_action_status(ui.ctx(), action).enabled,
                                Button::new(label),
                            )
                            .clicked()
                        {
                            queue_menu_action(ui.ctx(), action);
                            ui.close();
                        }
                    }
                });
        }
    }

    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        if !self.workspace.status || self.workspace.start {
            return;
        }
        egui::Panel::bottom("status")
            .exact_size(24.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if self.workspace.tools {
                        self.toolset_bottom_bar(ui);
                    }
                    ui.add_sized([58.0, 23.0], Button::selectable(true, "Model"));
                    ui.add_enabled(false, Button::new("+").min_size(egui::vec2(22.0, 23.0)));
                    ui.add_enabled(
                        false,
                        Button::new("Layout1").min_size(egui::vec2(60.0, 23.0)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let settings = ui
                            .menu_button("⚙", |ui| {
                                ui.with_layout(egui::Layout::top_down(Align::Min), |ui| {
                                    ui.label("Drafting settings");
                                    ui.checkbox(&mut self.grid, "Grid display");
                                    ui.checkbox(&mut self.grid_snap, "Snap mode");
                                    ui.checkbox(&mut self.object_snap, "Object snap mode");
                                    let mut ortho = self.ortho;
                                    if ui.checkbox(&mut ortho, "Ortho mode").changed() {
                                        self.set_ortho(ortho);
                                    }
                                    let mut polar = self.polar_tracking;
                                    if ui.checkbox(&mut polar, "Polar tracking mode").changed() {
                                        self.set_polar_tracking(polar);
                                    }
                                    let angle_label = format!("Polar angle: {}°", self.polar_angle);
                                    let angle_menu = ui
                                        .menu_button(&angle_label, |ui| {
                                            for angle in
                                                [90.0, 45.0, 30.0, 22.5, 18.0, 15.0, 10.0, 5.0]
                                            {
                                                ui.selectable_value(
                                                    &mut self.polar_angle,
                                                    angle,
                                                    format!("{angle}°"),
                                                );
                                            }
                                        })
                                        .response
                                        .on_hover_text("Custom increment: POLARANG <degrees>");
                                    angle_menu.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            angle_label.as_str(),
                                        )
                                    });
                                    ui.separator();
                                    let customization = ui
                                        .menu_button("Status bar customization", |ui| {
                                            ui.with_layout(
                                                egui::Layout::top_down(Align::Min),
                                                |ui| {
                                                    for item in catalog::STATUS_ITEMS {
                                                        let mut visible = !self
                                                            .workspace
                                                            .hidden_status
                                                            .contains(item.id);
                                                        if ui
                                                            .checkbox(
                                                                &mut visible,
                                                                format!("Show {}", item.label),
                                                            )
                                                            .changed()
                                                        {
                                                            if visible {
                                                                self.workspace
                                                                    .hidden_status
                                                                    .remove(item.id);
                                                            } else {
                                                                self.workspace
                                                                    .hidden_status
                                                                    .insert(item.id);
                                                            }
                                                        }
                                                    }
                                                    if ui
                                                        .button("Restore default status controls")
                                                        .clicked()
                                                    {
                                                        self.workspace.hidden_status.clear();
                                                    }
                                                },
                                            );
                                        })
                                        .response;
                                    customization.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            "Status bar customization",
                                        )
                                    });
                                });
                            })
                            .response
                            .on_hover_text("Drafting settings");
                        settings.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                "Drafting settings",
                            )
                        });
                        for item in catalog::STATUS_ITEMS.iter().rev() {
                            if !self.workspace.hidden_status.contains(item.id) {
                                self.status_item(ui, item);
                            }
                        }
                    });
                });
            });
    }

    fn status_item(&mut self, ui: &mut egui::Ui, item: &StatusItem) {
        match item.id {
            "ID_CursorCoordinates" => {
                let point = self.cursor.unwrap_or_else(|| {
                    Point3::new(0.0, 0.0, self.elevation).expect("finite drawing elevation")
                });
                let coordinates = format!("{:.4}, {:.4}, {:.4}", point.x(), point.y(), point.z());
                let response = ui
                    .add_sized(
                        [137.0, 23.0],
                        egui::Label::new(RichText::new(&coordinates).size(10.0)).truncate(),
                    )
                    .on_hover_text(&coordinates);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Cursor coordinates")
                });
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    node.set_label("Cursor coordinates");
                    node.set_value(coordinates);
                });
            }
            "ID_ModelSpace" | "ID_AnnotationScale" => {
                let caption = if item.id == "ID_ModelSpace" {
                    "Model"
                } else {
                    "1:1"
                };
                let response = ui.add_enabled(
                    false,
                    Button::new(RichText::new(caption).size(10.0))
                        .frame(false)
                        .min_size(egui::vec2(32.0, 23.0)),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, false, item.label)
                });
            }
            _ => {
                let state = match item.id {
                    "ID_GridMode" => Some((self.grid, "Grid", "Grid display · F7")),
                    "ID_SnapMode" => Some((self.grid_snap, "Grid snap", "Snap mode · F9")),
                    "ID_OrthoMode" => Some((self.ortho, "Ortho", "Ortho mode · F8")),
                    "ID_PolarTracking" => Some((
                        self.polar_tracking,
                        "Polar tracking",
                        "Polar tracking · F10",
                    )),
                    "ID_ObjectSnap" => Some((self.object_snap, "Object snap", "Object snap · F3")),
                    _ => None,
                };
                if let Some((value, label, help)) = state {
                    if self.status_toggle(ui, label, item.off, item.on, value, help) {
                        match item.id {
                            "ID_GridMode" => self.grid = !value,
                            "ID_SnapMode" => self.grid_snap = !value,
                            "ID_OrthoMode" => self.set_ortho(!value),
                            "ID_PolarTracking" => self.set_polar_tracking(!value),
                            _ => self.object_snap = !value,
                        }
                    }
                } else {
                    self.icon_button(ui, item.off, 22.0, 16.0, false);
                }
            }
        }
    }

    fn status_toggle(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        off: &'static str,
        on: &'static str,
        value: bool,
        help: &str,
    ) -> bool {
        let response = self
            .icon_button(ui, if value { on } else { off }, 22.0, 16.0, true)
            .on_hover_text(help);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, value, label)
        });
        response.clicked()
    }

    pub(super) fn command_panel(&mut self, ctx: &egui::Context) {
        if !self.workspace.command {
            return;
        }
        let left = if !self.workspace.start {
            self.workspace.toolset_width()
        } else {
            0.0
        };
        let right = if !self.workspace.start && (self.workspace.layers || self.workspace.properties)
        {
            236.0
        } else {
            0.0
        };
        let width = 500.0_f32.min((ctx.content_rect().width() - left - right - 40.0).max(240.0));
        egui::Area::new(egui::Id::new("command_overlay"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .anchor(
                Align2::CENTER_BOTTOM,
                egui::vec2(
                    (left - right) / 2.0,
                    if self.workspace.status && !self.workspace.start {
                        -24.0
                    } else {
                        0.0
                    },
                ),
            )
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(5, 2))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        if self.workspace.history {
                            egui::ScrollArea::vertical()
                                .id_salt("history")
                                .max_height(142.0)
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for (message, error) in &self.history {
                                        ui.label(
                                            RichText::new(message)
                                                .font(FontId::new(
                                                    10.0,
                                                    egui::FontFamily::Name("History".into()),
                                                ))
                                                .color(if *error {
                                                    SELECTED
                                                } else {
                                                    Color32::from_gray(217)
                                                }),
                                        );
                                    }
                                });
                        } else if let Some((message, error)) = self.history.back() {
                            ui.label(
                                RichText::new(message)
                                    .font(FontId::new(
                                        10.0,
                                        egui::FontFamily::Name("History".into()),
                                    ))
                                    .color(if *error {
                                        SELECTED
                                    } else {
                                        Color32::from_gray(217)
                                    }),
                            );
                        }
                        if matches!(self.tool, Tool::Line | Tool::Polyline)
                            && !self.points.is_empty()
                        {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(self.prompt()).font(FontId::new(
                                    11.0,
                                    egui::FontFamily::Name("Prompt".into()),
                                )));
                                if ui.small_button("Undo point").clicked() {
                                    self.undo_line_point();
                                }
                                if ui.small_button("Close chain").clicked() {
                                    self.close_line();
                                }
                            });
                        }
                        let mut run = false;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            let submit = ui
                                .add_sized([23.0, 20.0], Button::new(">_").frame(false))
                                .on_hover_text("Run command · Return");
                            submit.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Run")
                            });
                            run |= submit.clicked();
                            let response = ui.add_sized(
                                [width - 54.0, 20.0],
                                egui::TextEdit::singleline(&mut self.command)
                                    .id_salt("command_input")
                                    .font(FontId::new(
                                        11.0,
                                        egui::FontFamily::Name("Command".into()),
                                    ))
                                    .frame(egui::Frame::NONE)
                                    .hint_text("Type a command"),
                            );
                            ctx.data_mut(|data| {
                                data.insert_temp(egui::Id::new("command_field_id"), response.id)
                            });
                            ctx.accesskit_node_builder(response.id, |node| {
                                node.set_label("Command")
                            });
                            if self.command_focus {
                                response.request_focus();
                                self.command_focus = false;
                            }
                            run |= response.lost_focus()
                                && ui.input(|input| input.key_pressed(Key::Enter));
                            let response = ui
                                .add_sized(
                                    [20.0, 20.0],
                                    Button::new(if self.workspace.history { "⌄" } else { "⌃" })
                                        .frame(false),
                                )
                                .on_hover_text("Command history · F2");
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    "Command history",
                                )
                            });
                            if response.clicked() {
                                self.workspace.history = !self.workspace.history;
                            }
                        });
                        if run {
                            let command = std::mem::take(&mut self.command);
                            self.execute(&command);
                            self.command_focus = true;
                        }
                    });
            });
    }

    pub(super) fn start_page(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("start_navigation")
            .exact_size(240.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(47, 52, 63))
                    .inner_margin(egui::Margin::symmetric(32, 32)),
            )
            .show(ui, |ui| {
                ui.add_space(28.0);
                ui.label(RichText::new("OpenCad").size(24.0).strong());
                ui.add_space(20.0);
                let navigation_button = |label| {
                    Button::new(label)
                        .fill(Color32::from_rgb(47, 52, 63))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(110, 126, 147)))
                        .corner_radius(1)
                };
                if ui
                    .add_sized([176.0, 36.0], navigation_button("Open..."))
                    .clicked()
                {
                    self.request(PendingAction::Open(None));
                }
                if ui
                    .add_sized([176.0, 36.0], navigation_button("New"))
                    .clicked()
                {
                    self.request(PendingAction::New);
                }
                ui.add_space(32.0);
                let (recent, response) =
                    ui.allocate_exact_size(egui::vec2(176.0, 44.0), egui::Sense::click());
                ui.painter().rect_filled(
                    recent.expand2(egui::vec2(32.0, 0.0)),
                    0.0,
                    Color32::from_rgb(71, 79, 95),
                );
                ui.painter().text(
                    recent.left_center(),
                    Align2::LEFT_CENTER,
                    "Recent",
                    FontId::new(14.0, egui::FontFamily::Name("Caption".into())),
                    ui.visuals().text_color(),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, true, "Recent")
                });
                let (learning, response) =
                    ui.allocate_exact_size(egui::vec2(176.0, 44.0), egui::Sense::click());
                ui.painter().text(
                    learning.left_center(),
                    Align2::LEFT_CENTER,
                    "Learning",
                    FontId::proportional(14.0),
                    ui.visuals().text_color(),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Learning")
                });
                if response.clicked() {
                    self.show_help = true;
                }
                ui.with_layout(egui::Layout::bottom_up(Align::Min), |ui| {
                    if ui.link("Load courtyard study").clicked() {
                        self.request(PendingAction::Study);
                    }
                    if ui.link("Online help").clicked() {
                        self.show_help = true;
                    }
                });
            });
        if self.workspace.connect {
            egui::Panel::right("start_connect")
                .exact_size(350.0)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(61, 68, 82))
                        .inner_margin(egui::Margin::same(32)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            let close = ui.small_button("×").on_hover_text("Close Connect");
                            close.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    "Close Connect",
                                )
                            });
                            if close.clicked() {
                                self.workspace.connect = false;
                            }
                        });
                    });
                    ui.add_space(32.0);
                    ui.label(RichText::new("Connect").size(22.0));
                    ui.add_space(64.0);
                    ui.add_enabled(false, Button::new("Sign in"));
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(71, 79, 95))
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
                ui.add_space(30.0);
                ui.label(RichText::new("Recent").size(22.0));
                ui.add_space(32.0);
                ui.horizontal(|ui| {
                    if Self::recent_view_button(ui, true, self.recent_list).clicked() {
                        self.recent_list = true;
                    }
                    if Self::recent_view_button(ui, false, !self.recent_list).clicked() {
                        self.recent_list = false;
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.add_sized(
                            [ui.available_width().min(400.0), 36.0],
                            egui::TextEdit::singleline(&mut self.recent_search)
                                .font(FontId::proportional(14.0))
                                .margin(egui::Margin::symmetric(12, 8))
                                .vertical_align(Align::Center)
                                .hint_text("Search for recent files"),
                        );
                    });
                });
                let recent: Vec<_> = self
                    .recent_files
                    .iter()
                    .filter(|path| {
                        path.to_string_lossy()
                            .to_lowercase()
                            .contains(&self.recent_search.to_lowercase())
                    })
                    .cloned()
                    .collect();
                if recent.is_empty() {
                    let rect = ui.available_rect_before_wrap();
                    let center = rect.center() - egui::vec2(0.0, 20.0);
                    Self::empty_recents(ui.painter(), center - egui::vec2(0.0, 50.0));
                    ui.put(
                        egui::Rect::from_center_size(
                            center + egui::vec2(0.0, 78.0),
                            egui::vec2(rect.width(), 28.0),
                        ),
                        egui::Label::new(
                            RichText::new("There is nothing here yet.")
                                .size(20.0)
                                .color(Color32::from_gray(230)),
                        ),
                    );
                    ui.put(
                        egui::Rect::from_center_size(
                            center + egui::vec2(0.0, 108.0),
                            egui::vec2(rect.width(), 24.0),
                        ),
                        egui::Label::new(
                            RichText::new("To get started, create or open a document.")
                                .size(14.0)
                                .color(MUTED),
                        ),
                    );
                } else {
                    ui.add_space(24.0);
                    for path in recent {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        let label = if self.recent_list {
                            format!("{name}\n{}", path.display())
                        } else {
                            name.to_string()
                        };
                        if ui
                            .add_sized(
                                [
                                    ui.available_width(),
                                    if self.recent_list { 48.0 } else { 64.0 },
                                ],
                                Button::new(label),
                            )
                            .clicked()
                        {
                            self.request(PendingAction::Open(Some(path)));
                        }
                    }
                }
            });
    }

    fn recent_view_button(ui: &mut egui::Ui, list: bool, selected: bool) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
        if selected || response.hovered() {
            ui.painter().rect_filled(
                rect,
                0.0,
                if selected {
                    Color32::from_rgb(60, 87, 109)
                } else {
                    Color32::from_rgb(80, 89, 109)
                },
            );
            ui.painter().rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let center = rect.center();
        if list {
            for y in [-5.0, 0.0, 5.0] {
                ui.painter().rect_filled(
                    egui::Rect::from_center_size(
                        center + egui::vec2(-6.0, y),
                        egui::vec2(2.0, 2.0),
                    ),
                    0.0,
                    MUTED,
                );
                ui.painter().line_segment(
                    [center + egui::vec2(-2.0, y), center + egui::vec2(7.0, y)],
                    Stroke::new(2.0, MUTED),
                );
            }
        } else {
            for x in [-4.0, 4.0] {
                for y in [-4.0, 4.0] {
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(
                            center + egui::vec2(x, y),
                            egui::vec2(6.0, 6.0),
                        ),
                        0.0,
                        MUTED,
                    );
                }
            }
        }
        let label = if list { "List view" } else { "Grid view" };
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
        });
        response.on_hover_text(label)
    }

    fn empty_recents(painter: &egui::Painter, center: egui::Pos2) {
        let stroke = Stroke::new(1.4, Color32::from_rgb(40, 47, 60));
        let p = |x: f32, y: f32| center + egui::vec2(x, y);
        for (points, color) in [
            (
                vec![
                    p(-48.0, -48.0),
                    p(0.0, -76.0),
                    p(48.0, -48.0),
                    p(0.0, -20.0),
                ],
                Color32::from_rgb(157, 176, 214),
            ),
            (
                vec![p(-48.0, -48.0), p(0.0, -20.0), p(0.0, 70.0), p(-48.0, 42.0)],
                Color32::from_rgb(126, 142, 176),
            ),
            (
                vec![p(0.0, -20.0), p(48.0, -48.0), p(48.0, 42.0), p(0.0, 70.0)],
                Color32::from_rgb(88, 105, 137),
            ),
            (
                vec![p(0.0, 22.0), p(48.0, -6.0), p(76.0, 12.0), p(28.0, 40.0)],
                Color32::from_rgb(117, 135, 167),
            ),
            (
                vec![p(28.0, 40.0), p(76.0, 12.0), p(76.0, 54.0), p(28.0, 82.0)],
                Color32::from_rgb(112, 131, 164),
            ),
        ] {
            painter.add(egui::Shape::convex_polygon(points, color, stroke));
        }
        painter.line_segment(
            [p(38.0, 44.0), p(59.0, 32.0)],
            Stroke::new(4.0, Color32::from_rgb(174, 192, 225)),
        );
        painter.line_segment(
            [p(13.0, -7.0), p(34.0, -19.0)],
            Stroke::new(4.0, Color32::from_rgb(174, 192, 225)),
        );
    }
}
