use super::*;
use egui::{Align, Align2, Button, FontId, RichText, Stroke};
use serde::{Deserialize, Serialize};
use shell::action_info;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum PanelKey {
    System(&'static str),
    User(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) enum UserItem {
    Command {
        id: u64,
        action: String,
    },
    DropDown {
        id: u64,
        name: String,
        commands: Vec<UserCommand>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) struct UserCommand {
    pub id: u64,
    pub action: String,
}

impl UserItem {
    fn id(&self) -> u64 {
        match self {
            Self::Command { id, .. } | Self::DropDown { id, .. } => *id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct UserPanel {
    pub id: u64,
    pub name: String,
    pub layout: u8,
    pub collapsed: bool,
    pub items: Vec<UserItem>,
    pub next_item_id: u64,
}

impl Default for UserPanel {
    fn default() -> Self {
        Self {
            id: 0,
            name: "New Panel".into(),
            layout: 1,
            collapsed: false,
            items: Vec::new(),
            next_item_id: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ItemKey {
    parent: Option<u64>,
    id: u64,
}

pub(super) struct PanelEditor {
    panel: u64,
    name: String,
    search: String,
    selected: Option<ItemKey>,
    dropdown_name: Option<(u64, String)>,
}

#[derive(Clone, Copy)]
enum ItemSource {
    Catalog(&'static str),
    Existing(ItemKey),
}

struct CommandDrag {
    panel: u64,
    source: ItemSource,
}

#[derive(Clone, Copy)]
struct ItemDestination {
    parent: Option<u64>,
    index: usize,
}

pub(super) struct ToolButton<'a> {
    pub key: u64,
    pub label: &'a str,
    pub actions: Vec<&'static str>,
    pub dropdown_name: Option<&'a str>,
}

pub(super) fn layout_columns(layout: u8) -> &'static [u8] {
    match layout {
        1 => &[1, 1, 1, 1],
        2 => &[1, 1, 1, 22, 22],
        3 => &[1, 22, 22, 1, 22, 22],
        4 => &[1, 1, 22, 22, 22, 22],
        5 => &[1, 22, 22, 22, 22, 22, 22],
        _ => &[2, 2, 2, 2, 2, 2, 2, 2],
    }
}

impl shell::Workspace {
    pub(super) fn panel_name(&self, key: PanelKey) -> Option<&str> {
        match key {
            PanelKey::System(label) => Some(label),
            PanelKey::User(id) => self
                .user_panels
                .iter()
                .find(|panel| panel.id == id)
                .map(|panel| panel.name.as_str()),
        }
    }

    pub(super) fn add_user_panel(&mut self) -> bool {
        let Some(next) = self.next_panel_id.checked_add(1) else {
            return false;
        };
        let id = self.next_panel_id;
        self.next_panel_id = next;
        self.user_panels.push(UserPanel {
            id,
            ..Default::default()
        });
        self.tool_order.push(PanelKey::User(id));
        self.scroll_to_panel = Some(id);
        self.save_preferences_requested = true;
        true
    }

    fn remove_user_panel(&mut self, id: u64) {
        self.user_panels.retain(|panel| panel.id != id);
        self.tool_order.retain(|key| *key != PanelKey::User(id));
        if self
            .panel_editor
            .as_ref()
            .is_some_and(|editor| editor.panel == id)
        {
            self.panel_editor = None;
        }
        if self.reorder_selection == Some(PanelKey::User(id)) {
            self.reorder_selection = None;
        }
        self.delete_panel = None;
        self.save_preferences_requested = true;
    }
}

impl UserPanel {
    /// Validate saved identities and catalog references without trusting names as keys.
    pub(super) fn normalize(&mut self) {
        if self.name.trim().is_empty() {
            self.name = "New Panel".into();
        }
        if !(1..=6).contains(&self.layout) {
            self.layout = 1;
        }
        let mut ids = HashSet::new();
        self.items.retain_mut(|item| {
            if item.id() == 0 || !ids.insert(item.id()) {
                return false;
            }
            match item {
                UserItem::Command { action, .. } => action_info(action).is_some(),
                UserItem::DropDown { name, commands, .. } => {
                    if name.trim().is_empty() {
                        *name = "New Drop-down".into();
                    }
                    commands.retain(|command| {
                        command.id != 0
                            && ids.insert(command.id)
                            && action_info(&command.action).is_some()
                    });
                    true
                }
            }
        });
        self.next_item_id = self
            .next_item_id
            .max(ids.into_iter().max().unwrap_or(0).saturating_add(1))
            .max(1);
    }

    fn allocate_item(&mut self) -> Option<u64> {
        let next = self.next_item_id.checked_add(1)?;
        let id = self.next_item_id;
        self.next_item_id = next;
        Some(id)
    }

    fn selected_destination(&self, selected: Option<ItemKey>) -> ItemDestination {
        if let Some(key) = selected {
            if let Some(parent) = key.parent {
                if let Some(UserItem::DropDown { commands, .. }) =
                    self.items.iter().find(|item| item.id() == parent)
                {
                    return ItemDestination {
                        parent: Some(parent),
                        index: commands
                            .iter()
                            .position(|command| command.id == key.id)
                            .map_or(commands.len(), |index| index + 1),
                    };
                }
            } else if let Some(index) = self.items.iter().position(|item| item.id() == key.id) {
                return match &self.items[index] {
                    UserItem::DropDown { commands, .. } => ItemDestination {
                        parent: Some(key.id),
                        index: commands.len(),
                    },
                    _ => ItemDestination {
                        parent: None,
                        index: index + 1,
                    },
                };
            }
        }
        ItemDestination {
            parent: None,
            index: self.items.len(),
        }
    }

    fn remove_item(&mut self, key: ItemKey) -> Option<UserItem> {
        if let Some(parent) = key.parent {
            let UserItem::DropDown { commands, .. } =
                self.items.iter_mut().find(|item| item.id() == parent)?
            else {
                return None;
            };
            let index = commands.iter().position(|command| command.id == key.id)?;
            let command = commands.remove(index);
            Some(UserItem::Command {
                id: command.id,
                action: command.action,
            })
        } else {
            let index = self.items.iter().position(|item| item.id() == key.id)?;
            Some(self.items.remove(index))
        }
    }

    fn transfer(
        &mut self,
        source: ItemSource,
        mut destination: ItemDestination,
    ) -> Option<ItemKey> {
        if let Some(parent) = destination.parent {
            if !self
                .items
                .iter()
                .any(|item| matches!(item, UserItem::DropDown { id, .. } if *id == parent))
            {
                return None;
            }
            if let ItemSource::Existing(key) = source
                && key.parent.is_none()
                && self
                    .items
                    .iter()
                    .any(|item| matches!(item, UserItem::DropDown { id, .. } if *id == key.id))
            {
                return None;
            }
        }
        let item = match source {
            ItemSource::Catalog(action) => UserItem::Command {
                id: self.allocate_item()?,
                action: action.into(),
            },
            ItemSource::Existing(key) => {
                let position = if let Some(parent) = key.parent {
                    let UserItem::DropDown { commands, .. } =
                        self.items.iter().find(|item| item.id() == parent)?
                    else {
                        return None;
                    };
                    commands.iter().position(|command| command.id == key.id)?
                } else {
                    self.items.iter().position(|item| item.id() == key.id)?
                };
                if key.parent == destination.parent && position < destination.index {
                    destination.index = destination.index.saturating_sub(1);
                }
                self.remove_item(key)?
            }
        };
        let key = ItemKey {
            parent: destination.parent,
            id: item.id(),
        };
        if let Some(parent) = destination.parent {
            let Some(UserItem::DropDown { commands, .. }) =
                self.items.iter_mut().find(|item| item.id() == parent)
            else {
                return None;
            };
            if let UserItem::Command { id, action } = item {
                commands.insert(
                    destination.index.min(commands.len()),
                    UserCommand { id, action },
                );
            }
        } else {
            self.items
                .insert(destination.index.min(self.items.len()), item);
        }
        Some(key)
    }
}

pub(super) fn toolset_header(
    ui: &mut egui::Ui,
    id: egui::Id,
    name: &str,
    collapsed: &mut bool,
    user: bool,
) -> (
    egui::collapsing_header::CollapsingState,
    egui::Response,
    Option<egui::Response>,
) {
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
    state.set_open(!*collapsed);
    let (_, header) = ui.allocate_space(egui::vec2(ui.available_width(), 20.0));
    let gear_rect = egui::Rect::from_center_size(
        egui::pos2(header.right() - 9.0, header.center().y),
        egui::Vec2::splat(12.0),
    );
    let delete_rect = gear_rect.translate(egui::vec2(-15.0, 0.0));
    let title_rect = egui::Rect::from_min_max(
        header.min,
        egui::pos2(
            if user {
                delete_rect.left()
            } else {
                gear_rect.left()
            } - 3.0,
            header.bottom(),
        ),
    );
    let title = ui.interact(title_rect, id, egui::Sense::click());
    if title.clicked() {
        state.toggle(ui);
        *collapsed = !state.is_open();
    }
    title.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::CollapsingHeader,
            true,
            state.is_open(),
            name,
        )
    });
    ui.ctx()
        .accesskit_node_builder(title.id, |node| node.set_expanded(state.is_open()));
    ui.painter().text(
        egui::pos2(header.left() + 9.0, header.center().y),
        Align2::CENTER_CENTER,
        if state.is_open() { "▾" } else { "▸" },
        FontId::new(14.0, egui::FontFamily::Name("Collapse".into())),
        ui.visuals().text_color(),
    );
    ui.painter().with_clip_rect(title_rect).text(
        egui::pos2(header.left() + 15.0, header.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::new(10.0, egui::FontFamily::Name("Toolset".into())),
        ui.visuals().text_color(),
    );
    let gear = ui.interact(gear_rect, id.with("settings"), egui::Sense::click());
    gear.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Customize {name} panel"),
        )
    });
    if gear.hovered() || gear.has_focus() {
        ui.painter()
            .rect_filled(gear_rect, 0.0, Color32::from_rgb(80, 89, 109));
    }
    ui.painter().text(
        gear_rect.center(),
        Align2::CENTER_CENTER,
        "⚙",
        FontId::new(16.0, egui::FontFamily::Name("Mac Symbols".into())),
        ui.visuals().text_color(),
    );
    let delete = user.then(|| {
        let delete = ui.interact(delete_rect, id.with("delete"), egui::Sense::click());
        delete.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Delete {name} panel"),
            )
        });
        if ui.rect_contains_pointer(header) || delete.has_focus() {
            ui.painter().text(
                delete_rect.center(),
                Align2::CENTER_CENTER,
                "×",
                FontId::proportional(14.0),
                ui.visuals().text_color(),
            );
        }
        delete.on_hover_text("Delete panel")
    });
    (state, gear, delete)
}

impl OpenCadApp {
    pub(super) fn user_panel_header(&mut self, ui: &mut egui::Ui, panel_id: u64) {
        let Some(index) = self
            .workspace
            .user_panels
            .iter()
            .position(|panel| panel.id == panel_id)
        else {
            return;
        };
        let mut panel = self.workspace.user_panels[index].clone();
        let id = ui.make_persistent_id(("user_toolset", panel_id));
        let (mut state, gear, delete) =
            toolset_header(ui, id, &panel.name, &mut panel.collapsed, true);
        if panel.collapsed != self.workspace.user_panels[index].collapsed {
            self.workspace.user_panels[index].collapsed = panel.collapsed;
            self.workspace.save_preferences_requested = true;
        }
        if delete.is_some_and(|response| response.clicked()) {
            self.workspace.delete_panel = Some(panel_id);
        }
        if self.workspace.delete_panel == Some(panel_id) {
            let mut confirmed = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Delete panel?").size(10.0));
                let delete = ui.small_button("Delete");
                delete.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("Confirm delete {} panel", panel.name),
                    )
                });
                confirmed = delete.clicked();
                let cancel = ui.small_button("Cancel");
                cancel.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Cancel panel deletion",
                    )
                });
                if cancel.clicked() {
                    self.workspace.delete_panel = None;
                }
            });
            if confirmed {
                self.workspace.remove_user_panel(panel_id);
                egui::Popup::close_id(ui.ctx(), egui::Popup::default_response_id(&gear));
                return;
            }
        }
        if ui.input(|input| {
            input.raw.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        ..
                    }
                )
            })
        }) {
            egui::Popup::close_id(ui.ctx(), egui::Popup::default_response_id(&gear));
        }
        egui::Popup::menu(&gear.on_hover_text(format!("Customize {} panel", panel.name)))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .frame(egui::Frame::popup(ui.style()).inner_margin(egui::Margin::ZERO))
            .width(432.0)
            .show(|ui| self.user_panel_editor(ui, panel_id));
        state.show_body_unindented(ui, |ui| {
            let items: Vec<_> = panel
                .items
                .iter()
                .map(|item| match item {
                    UserItem::Command { id, action } => ToolButton {
                        key: *id,
                        label: action_info(action).map_or(action.as_str(), |action| action.label),
                        actions: action_info(action)
                            .map(|action| action.id)
                            .into_iter()
                            .collect(),
                        dropdown_name: None,
                    },
                    UserItem::DropDown { id, name, commands } => ToolButton {
                        key: *id,
                        label: name,
                        actions: commands
                            .iter()
                            .filter_map(|command| {
                                action_info(&command.action).map(|action| action.id)
                            })
                            .collect(),
                        dropdown_name: Some(name),
                    },
                })
                .collect();
            self.tool_buttons(ui, panel.layout, &items);
        });
        let (_, separator) = ui.allocate_space(egui::vec2(ui.available_width(), 1.0));
        ui.painter().hline(
            separator.left() + 4.0..=separator.right() - 4.0,
            separator.center().y,
            Stroke::new(1.0, Color32::from_rgb(47, 52, 63)),
        );
        if self.workspace.scroll_to_panel == Some(panel_id) {
            ui.scroll_to_cursor(Some(Align::BOTTOM));
            self.workspace.scroll_to_panel = None;
        }
    }

    fn user_panel_editor(&mut self, ui: &mut egui::Ui, panel_id: u64) {
        let Some(index) = self
            .workspace
            .user_panels
            .iter()
            .position(|panel| panel.id == panel_id)
        else {
            return;
        };
        let original = self.workspace.user_panels[index].clone();
        let mut panel = original.clone();
        let mut editor = self
            .workspace
            .panel_editor
            .take()
            .filter(|editor| editor.panel == panel_id)
            .unwrap_or_else(|| PanelEditor {
                panel: panel_id,
                name: panel.name.clone(),
                search: String::new(),
                selected: None,
                dropdown_name: None,
            });
        let (_, content) = ui.allocate_space(egui::vec2(432.0, 454.0));
        let mut rename = editor_child(
            ui,
            content.min + egui::vec2(12.0, 12.0),
            egui::vec2(200.0, 22.0),
            "rename",
        );
        let response = rename.add_sized(
            [200.0, 22.0],
            egui::TextEdit::singleline(&mut editor.name).char_limit(80),
        );
        ui.ctx()
            .accesskit_node_builder(response.id, |node| node.set_label("Rename Panel"));
        if response.changed() && !editor.name.trim().is_empty() {
            panel.name = editor.name.trim().to_owned();
        }
        if response.lost_focus() && editor.name.trim().is_empty() {
            editor.name.clone_from(&panel.name);
        }
        if editor.name.trim().is_empty() {
            response.on_hover_text("Enter a panel name.");
        }
        let mut layout = editor_child(
            ui,
            content.min + egui::vec2(12.0, 42.0),
            egui::vec2(200.0, 90.0),
            "layout",
        );
        layout.horizontal(|ui| {
            ui.label("Layout:");
            let previous = ui.add_enabled(
                panel.layout > 1,
                Button::new("‹").min_size(egui::vec2(20.0, 18.0)),
            );
            previous.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    panel.layout > 1,
                    "Previous Layout",
                )
            });
            if previous.clicked() {
                panel.layout -= 1;
            }
            ui.label(format!("{}/6", panel.layout));
            let next = ui.add_enabled(
                panel.layout < 6,
                Button::new("›").min_size(egui::vec2(20.0, 18.0)),
            );
            next.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, panel.layout < 6, "Next Layout")
            });
            if next.clicked() {
                panel.layout += 1;
            }
        });
        let (_, preview) = layout.allocate_space(egui::vec2(200.0, 69.0));
        paint_layout(&layout, preview, panel.layout);
        let mut outline = editor_child(
            ui,
            content.min + egui::vec2(12.0, 140.0),
            egui::vec2(200.0, 272.0),
            "outline",
        );
        if panel_outline(&mut outline, &mut panel, &mut editor) {
            self.message("Unable to add another panel item.", true);
        }
        let mut controls = editor_child(
            ui,
            content.min + egui::vec2(12.0, 419.0),
            egui::vec2(200.0, 24.0),
            "controls",
        );
        controls.horizontal(|ui| {
            if ui
                .button("Add Drop-down")
                .on_hover_text("Add a new drop-down button.")
                .clicked()
            {
                if let Some(id) = panel.allocate_item() {
                    panel.items.push(UserItem::DropDown {
                        id,
                        name: "New Drop-down".into(),
                        commands: Vec::new(),
                    });
                    editor.selected = Some(ItemKey { parent: None, id });
                    editor.dropdown_name = Some((id, "New Drop-down".into()));
                } else {
                    self.message("Unable to add another panel item.", true);
                }
            }
            let delete = ui.add_enabled(editor.selected.is_some(), Button::new("Delete"));
            delete.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    editor.selected.is_some(),
                    "Delete selected panel item",
                )
            });
            if delete.clicked() {
                if let Some(key) = editor.selected.take() {
                    panel.remove_item(key);
                }
                editor.dropdown_name = None;
            }
        });
        let mut search = editor_child(
            ui,
            content.min + egui::vec2(220.0, 12.0),
            egui::vec2(200.0, 22.0),
            "search",
        );
        let search = search.add_sized(
            [200.0, 22.0],
            egui::TextEdit::singleline(&mut editor.search)
                .hint_text("Search commands")
                .char_limit(100),
        );
        ui.ctx()
            .accesskit_node_builder(search.id, |node| node.set_label("Search panel commands"));
        let mut browser = editor_child(
            ui,
            content.min + egui::vec2(220.0, 42.0),
            egui::vec2(200.0, 400.0),
            "browser",
        );
        self.panel_command_browser(&mut browser, &mut panel, &mut editor);
        if panel != original {
            self.workspace.user_panels[index] = panel;
            self.workspace.save_preferences_requested = true;
            ui.ctx().request_repaint();
        }
        self.workspace.panel_editor = Some(editor);
    }

    fn panel_command_browser(
        &mut self,
        ui: &mut egui::Ui,
        panel: &mut UserPanel,
        editor: &mut PanelEditor,
    ) {
        let query = editor.search.to_lowercase();
        let actions: Vec<_> = catalog::ACTIONS
            .iter()
            .filter(|action| {
                !action.id.starts_with("UI_")
                    && (action.label.to_lowercase().contains(&query)
                        || action.id.to_lowercase().contains(&query))
            })
            .collect();
        let mut addition = None;
        egui::ScrollArea::vertical()
            .id_salt("commands")
            .max_height(400.0)
            .auto_shrink([false, false])
            .show_rows(ui, 17.0, actions.len(), |ui, rows| {
                for index in rows {
                    let action = actions[index];
                    let id = ui.make_persistent_id(("catalog_command", action.id));
                    let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), 17.0));
                    let row = ui.interact(rect, id, egui::Sense::click_and_drag());
                    row.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::SelectableLabel,
                            true,
                            format!("Available command: {}", action.label),
                        )
                    });
                    ui.ctx().accesskit_node_builder(id, |node| {
                        node.set_custom_actions(vec![egui::accesskit::CustomAction {
                            id: 1,
                            description: "Add command to panel".into(),
                        }])
                    });
                    let add = ui.input(|input| {
                        input
                            .accesskit_action_requests(id, egui::accesskit::Action::CustomAction)
                            .any(|request| {
                                matches!(
                                    request.data,
                                    Some(egui::accesskit::ActionData::CustomAction(1))
                                )
                            })
                    });
                    if row.double_clicked() || add {
                        addition = Some(action.id);
                    }
                    row.dnd_set_drag_payload(CommandDrag {
                        panel: panel.id,
                        source: ItemSource::Catalog(action.id),
                    });
                    if row.hovered() {
                        ui.painter()
                            .rect_filled(rect, 0.0, ui.visuals().widgets.hovered.bg_fill);
                    }
                    self.load_panel_command_icon(ui, action.id, rect);
                    ui.painter().with_clip_rect(rect).text(
                        rect.left_center() + egui::vec2(22.0, 0.0),
                        Align2::LEFT_CENTER,
                        action.label,
                        FontId::proportional(12.0),
                        ui.visuals().text_color(),
                    );
                    let availability = if shell::menu_action_status(ui.ctx(), action.id).enabled {
                        ""
                    } else {
                        "\nThis command is not available in the current workspace."
                    };
                    row.on_hover_text(format!("{}\n{}{availability}", action.label, action.help));
                }
            });
        if let Some(action) = addition {
            let destination = panel.selected_destination(editor.selected);
            if let Some(key) = panel.transfer(ItemSource::Catalog(action), destination) {
                editor.selected = Some(key);
            } else {
                self.message("Unable to add another panel item.", true);
            }
        }
    }

    fn load_panel_command_icon(
        &mut self,
        ui: &mut egui::Ui,
        action: &'static str,
        rect: egui::Rect,
    ) {
        self.load_icon(ui.ctx(), action);
        if let Some(texture) = self.icons.get(action) {
            let icon = egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(8.5, 0.0),
                egui::Vec2::splat(14.0),
            );
            ui.painter().image(
                texture.id(),
                icon,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

fn editor_child(
    ui: &mut egui::Ui,
    origin: egui::Pos2,
    size: egui::Vec2,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> egui::Ui {
    let rect = egui::Rect::from_min_size(origin, size);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(salt)
            .max_rect(rect)
            .layout(egui::Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    child.spacing_mut().item_spacing = egui::Vec2::ZERO;
    child
}

fn paint_layout(ui: &egui::Ui, rect: egui::Rect, layout: u8) {
    let columns = layout_columns(layout);
    let widths: Vec<_> = columns
        .iter()
        .map(|column| if *column == 1 { 36.0 } else { 18.0 })
        .collect();
    let mut x = rect.center().x - widths.iter().sum::<f32>() / 2.0;
    for (&column, width) in columns.iter().zip(widths) {
        let count = if column == 22 { 2 } else { 1 };
        let size = if column == 1 { 30.0 } else { 13.0 };
        for row in 0..count {
            let top = rect.center().y
                - if column == 1 || column == 22 {
                    16.0
                } else {
                    6.5
                }
                + row as f32 * 18.0;
            let cell = egui::Rect::from_min_size(
                egui::pos2(x + (width - size) / 2.0, top),
                egui::Vec2::splat(size),
            );
            ui.painter()
                .rect_filled(cell, 1.0, Color32::from_rgb(77, 94, 113));
            ui.painter().rect_stroke(
                cell,
                1.0,
                Stroke::new(1.0, ACCENT),
                egui::StrokeKind::Inside,
            );
            ui.painter().line_segment(
                [
                    cell.left_bottom() + egui::vec2(3.0, -3.0),
                    cell.right_top() + egui::vec2(-3.0, 3.0),
                ],
                Stroke::new(1.0, MUTED),
            );
        }
        x += width;
    }
}

/// Returns true if a catalog insertion exhausted the identity allocator.
fn panel_outline(ui: &mut egui::Ui, panel: &mut UserPanel, editor: &mut PanelEditor) -> bool {
    let area = ui.available_rect_before_wrap();
    let background = ui.interact(
        area,
        ui.make_persistent_id("outline_drop"),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(area, 0.0, Color32::from_rgb(47, 54, 65));
    let mut movement = None;
    let mut bottom = area.top();
    egui::ScrollArea::vertical().id_salt("panel_items").max_height(272.0).min_scrolled_height(272.0).auto_shrink([false, false]).show(ui, |ui| {
        if panel.items.is_empty() {
            let hint = ui.add_sized([196.0, 72.0], egui::Label::new(RichText::new("To add a command to the panel, drag it from the list on the right and drop it here.").size(12.0)).wrap().selectable(false));
            hint.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Panel command drop area"));
        }
        for (index, item) in panel.items.iter_mut().enumerate() {
            let key = ItemKey { parent: None, id: item.id() };
            match item {
                UserItem::Command { action, .. } => {
                    let name = action_info(action).map_or(action.as_str(), |action| action.label);
                    let response = outline_row(ui, editor, key, name, false);
                    bottom = response.rect.bottom();
                    outline_drop(ui, &response, panel.id, ItemDestination { parent: None, index }, None, &mut movement);
                }
                UserItem::DropDown { id, name, commands } => {
                    let response = outline_row(ui, editor, key, name, true);
                    if editor.selected == Some(key) {
                        let (_, buffer) = editor.dropdown_name.get_or_insert_with(|| (*id, name.clone()));
                        let mut rename = editor_child(ui, response.rect.min + egui::vec2(16.0, 0.0), egui::vec2(response.rect.width() - 16.0, 17.0), ("dropdown_name", *id));
                        let rename = rename.add_sized([rename.available_width(), 17.0], egui::TextEdit::singleline(buffer).char_limit(80));
                        ui.ctx().accesskit_node_builder(rename.id, |node| node.set_label("Rename Drop-down"));
                        if rename.changed() && !buffer.trim().is_empty() { *name = buffer.trim().to_owned(); }
                        if rename.lost_focus() && buffer.trim().is_empty() { buffer.clone_from(name); }
                    }
                    bottom = response.rect.bottom();
                    outline_drop(ui, &response, panel.id, ItemDestination { parent: None, index }, Some((*id, commands.len())), &mut movement);
                    for (child_index, command) in commands.iter().enumerate() {
                        let child_key = ItemKey { parent: Some(*id), id: command.id };
                        let name = action_info(&command.action).map_or(command.action.as_str(), |action| action.label);
                        let response = outline_row(ui, editor, child_key, name, false);
                        bottom = response.rect.bottom();
                        outline_drop(ui, &response, panel.id, ItemDestination { parent: Some(*id), index: child_index }, None, &mut movement);
                    }
                    if commands.is_empty() {
                        let response = ui.add_sized([196.0, 17.0], egui::Label::new(RichText::new("    Drop commands here.").size(12.0)));
                        bottom = response.rect.bottom();
                        outline_drop(ui, &response, panel.id, ItemDestination { parent: Some(*id), index: 0 }, Some((*id, 0)), &mut movement);
                    }
                }
            }
        }
    });
    if background.contains_pointer()
        && ui.input(|input| {
            input
                .pointer
                .interact_pos()
                .is_some_and(|pos| pos.y >= bottom)
        })
        && let Some(payload) = background.dnd_hover_payload::<CommandDrag>()
        && payload.panel == panel.id
    {
        ui.painter().hline(
            area.x_range(),
            bottom.max(area.top()),
            Stroke::new(1.5, ACCENT),
        );
        if let Some(payload) = background.dnd_release_payload::<CommandDrag>() {
            movement = Some((
                payload.source,
                ItemDestination {
                    parent: None,
                    index: panel.items.len(),
                },
            ));
        }
    }
    if let Some((source, destination)) = movement {
        if let Some(key) = panel.transfer(source, destination) {
            editor.selected = Some(key);
            editor.dropdown_name = None;
        } else if matches!(source, ItemSource::Catalog(_)) {
            return true;
        }
    }
    false
}

fn outline_row(
    ui: &mut egui::Ui,
    editor: &mut PanelEditor,
    key: ItemKey,
    name: &str,
    dropdown: bool,
) -> egui::Response {
    let id = ui.make_persistent_id(("panel_item", key.id));
    let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), 17.0));
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    if response.clicked() {
        editor.selected = Some(key);
        editor.dropdown_name = dropdown.then(|| (key.id, name.to_owned()));
    }
    let selected = editor.selected == Some(key);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            format!("Panel item: {name}"),
        )
    });
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
    }
    ui.painter().with_clip_rect(rect).text(
        rect.left_center() + egui::vec2(if key.parent.is_some() { 20.0 } else { 3.0 }, 0.0),
        Align2::LEFT_CENTER,
        if dropdown {
            format!("▾ {name}")
        } else {
            name.to_owned()
        },
        FontId::proportional(12.0),
        ui.visuals().text_color(),
    );
    response.dnd_set_drag_payload(CommandDrag {
        panel: editor.panel,
        source: ItemSource::Existing(key),
    });
    response
}

fn outline_drop(
    ui: &egui::Ui,
    row: &egui::Response,
    panel_id: u64,
    destination: ItemDestination,
    dropdown: Option<(u64, usize)>,
    movement: &mut Option<(ItemSource, ItemDestination)>,
) {
    if let Some(payload) = row.dnd_hover_payload::<CommandDrag>()
        && payload.panel == panel_id
    {
        let pointer = ui
            .input(|input| input.pointer.interact_pos())
            .unwrap_or(row.rect.center());
        let below = pointer.y > row.rect.center().y;
        let inside = dropdown
            .filter(|_| pointer.y > row.rect.top() + 4.0 && pointer.y < row.rect.bottom() - 4.0);
        let target = if let Some((id, count)) = inside {
            ItemDestination {
                parent: Some(id),
                index: count,
            }
        } else {
            ItemDestination {
                index: destination.index + usize::from(below),
                ..destination
            }
        };
        if inside.is_some() {
            ui.painter().rect_stroke(
                row.rect,
                0.0,
                Stroke::new(1.5, ACCENT),
                egui::StrokeKind::Inside,
            );
        } else {
            ui.painter().hline(
                row.rect.x_range(),
                if below {
                    row.rect.bottom()
                } else {
                    row.rect.top()
                },
                Stroke::new(1.5, ACCENT),
            );
        }
        if let Some(payload) = row.dnd_release_payload::<CommandDrag>() {
            *movement = Some((payload.source, target));
        }
    }
}
