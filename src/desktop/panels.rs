use super::*;
use egui::{Button, RichText};

impl OpenCadApp {
    pub(super) fn inspector_panel(&mut self, ui: &mut egui::Ui) {
        if !self.workspace.layers && !self.workspace.properties {
            return;
        }
        let selected = if self.editor.selection().len() == 1 {
            let index = self.editor.selection()[0];
            Some((index, self.editor.entities()[index].clone()))
        } else {
            None
        };
        if self.inspector.entity != selected {
            self.inspector.entity = selected.clone();
            self.inspector.fields = match selected.as_ref() {
                Some((_, Entity::Line(line))) => vec![
                    line.start().x(),
                    line.start().y(),
                    line.start().z(),
                    line.end().x(),
                    line.end().y(),
                    line.end().z(),
                ],
                Some((_, Entity::Circle(circle))) => vec![
                    circle.center().x(),
                    circle.center().y(),
                    circle.center().z(),
                    circle.radius(),
                ],
                Some((_, Entity::Arc(arc))) => vec![
                    arc.center().x(),
                    arc.center().y(),
                    arc.center().z(),
                    arc.radius(),
                    arc.start_angle().to_degrees(),
                    arc.end_angle().to_degrees(),
                ],
                Some((_, Entity::Polyline(polyline))) => std::iter::once(polyline.elevation())
                    .chain(
                        polyline
                            .vertices()
                            .iter()
                            .zip(polyline.bulges())
                            .flat_map(|(point, bulge)| [point.x(), point.y(), *bulge]),
                    )
                    .collect(),
                None => Vec::new(),
            }
            .into_iter()
            .map(|n| n.to_string())
            .collect();
        }
        egui::Panel::right("inspector").exact_size(236.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(6, 0)))
            .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 3.0);
            ui.style_mut().text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(11.0));
            ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(11.0));
            egui::ScrollArea::vertical().id_salt("inspector_content").auto_shrink([false,false]).show(ui, |ui| {
            if self.workspace.layers {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 11.0;
                    self.icon_button(ui, "UI_Palette_layers", 23.0, 16.0, true);
                    self.icon_button(ui, "UI_Palette_content", 23.0, 16.0, false);
                    self.icon_button(ui, "UI_Palette_blocks", 23.0, 16.0, false);
                });
                ui.label(RichText::new("Layers").font(egui::FontId::new(11.0, egui::FontFamily::Name("Caption".into()))));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let selected_layer = (self.editor.selection().len() == 1).then(|| self.editor.entity_layer(self.editor.selection()[0]).expect("selected entity layer").to_owned());
                    for id in ["UI_Layer_makeCurrent", "UI_Layer_previous", "UI_Layer_isolate", "UI_Layer_unisolate", "UI_Layer_freeze", "UI_Layer_off", "UI_Layer_lock"] {
                        let enabled = selected_layer.is_some() && matches!(id, "UI_Layer_makeCurrent" | "UI_Layer_off" | "UI_Layer_lock");
                        if self.icon_button(ui, id, 28.0, 16.0, enabled).clicked() && let Some(layer) = selected_layer.as_ref() {
                            self.finish();
                            let result = match id {
                                "UI_Layer_makeCurrent" => self.editor.set_current_layer(layer),
                                "UI_Layer_off" => self.editor.set_layer_visible(layer, false).map(|_| ()),
                                _ => self.editor.set_layer_locked(layer, true).map(|_| ()),
                            };
                            match result {
                                Ok(()) => self.message("Layer updated.", false),
                                Err(error) => self.message(error.to_string(), true),
                            }
                        }
                    }
                });
                self.current_layer_control(ui);
                ui.add_enabled(false, Button::new("Unsaved Layer State     ▾").min_size(egui::vec2(ui.available_width(), 19.0)));
                egui::CollapsingHeader::new("Show Layer List").id_salt("layer_details").show(ui, |ui| self.layer_panel(ui));
                ui.separator();
            }
            if !self.workspace.properties { return; }
            ui.horizontal(|ui| {
                let caption = ui.label(RichText::new("Properties").font(egui::FontId::new(11.0, egui::FontFamily::Name("Caption".into()))));
                ui.ctx().accesskit_node_builder(caption.id, |node| node.set_description(format!("{} selected", self.editor.selection().len())));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled(false, Button::new("My").small());
                    ui.add_enabled(false, Button::new("All").small());
                });
            });
            egui::CollapsingHeader::new("Linetypes").show(ui, |ui| self.linetype_panel(ui));
            if let Some((index,entity))=selected {
                ui.horizontal(|ui| {ui.label(RichText::new(format!("{} {:03}",entity.name(),index+1)).monospace().color(SELECTED)); ui.label(RichText::new(format!("Layer {}",self.editor.entity_layer(index).unwrap_or("?"))).small().color(MUTED));});
                let labels: Vec<String> = match &entity {
                    Entity::Line(_) => ["Start X","Start Y","Start Z","End X","End Y","End Z"].into_iter().map(String::from).collect(),
                    Entity::Circle(_) => ["Center X","Center Y","Center Z","Radius"].into_iter().map(String::from).collect(),
                    Entity::Arc(_) => ["Center X","Center Y","Center Z","Radius","Start angle (°)","End angle (°)"].into_iter().map(String::from).collect(),
                    Entity::Polyline(polyline) => std::iter::once("Elevation".to_owned()).chain((1..=polyline.vertices().len()).flat_map(|index| [format!("Vertex {index} X"),format!("Vertex {index} Y"),format!("Vertex {index} bulge")])).collect(),
                };
                egui::Grid::new("coordinates").num_columns(2).spacing([4.0,2.0]).show(ui, |ui| {
                    for (position,label) in labels.iter().enumerate() {
                        let label=ui.label(label);
                        ui.add_sized([ui.available_width(),19.0],egui::TextEdit::singleline(&mut self.inspector.fields[position]).id_salt(position).font(egui::TextStyle::Monospace)).labelled_by(label.id);
                        ui.end_row();
                    }
                });
                if ui.add_sized([ui.available_width(),22.0],Button::new("Apply properties")).clicked() {
                    match self.inspector_entity(&entity) {
                        Ok(replacement) => {
                            self.finish();
                            match self.editor.update(index,replacement) {
                                Ok(_) => self.message("Properties applied.",false),
                                Err(error) => self.message(error.to_string(),true),
                            }
                        }
                        Err(error) => self.message(error,true),
                    }
                }
                match &entity {
                    Entity::Line(line) => {
                        ui.label(RichText::new(format!("Length  {}",line.length().map(|n|format!("{n:.4}")).unwrap_or_else(|_|"exceeds finite range".into()))).small().color(MUTED));
                        if ui.button("Reverse endpoints").clicked() {
                            self.finish();
                            if let Err(error)=self.editor.update(index,Entity::Line(Line3::new(line.end(),line.start()))) {self.message(error.to_string(),true);}
                        }
                    }
                    Entity::Circle(_) => {ui.label(RichText::new("Plane  World XY (+Z normal)").small().color(MUTED));}
                    Entity::Arc(arc) => {
                        ui.label(RichText::new(format!("Length  {}",arc.length().map(|n|format!("{n:.4}")).unwrap_or_else(|_|"exceeds finite range".into()))).small().color(MUTED));
                        ui.label(RichText::new(format!("CCW sweep  {:.4}°",arc.sweep().to_degrees())).small().color(MUTED));
                        ui.label(RichText::new("Plane  World XY (+Z normal)").small().color(MUTED));
                    }                    Entity::Polyline(polyline) => {
                        ui.label(RichText::new("Bulge → next vertex: 0 line, +1 CCW / −1 CW semicircle. Last bulge is used when closed.").small().color(MUTED));
                        ui.label(RichText::new(format!("{} vertices · Length {}",polyline.vertices().len(),polyline.length().map(|value|format!("{value:.4}")).unwrap_or_else(|_|"exceeds finite range".into()))).small().color(MUTED));
                        let mut closed = polyline.is_closed();
                        if ui.checkbox(&mut closed, "Closed polyline").changed() {
                            self.modify_polyline(index, &entity, |polyline| { polyline.set_closed(closed); Ok(()) });
                        }
                        if ui.button("Add vertex").clicked() {
                            self.modify_polyline(index, &entity, |polyline| {
                                let mut vertices = polyline.vertices().to_vec();
                                vertices.push(*vertices.last().expect("two vertices"));
                                let mut bulges = polyline.bulges().to_vec();
                                let outgoing = *bulges.last().expect("two vertices");
                                *bulges.last_mut().expect("two vertices") = 0.0;
                                bulges.push(outgoing);
                                *polyline = Polyline2::with_bulges(vertices, bulges, polyline.is_closed())?;
                                Ok(())
                            });
                        }
                        if ui.add_enabled(polyline.vertices().len() > 2, Button::new("Remove last vertex")).clicked() {
                            self.modify_polyline(index, &entity, |polyline| polyline.set_vertices(polyline.vertices()[..polyline.vertices().len()-1].to_vec()));
                        }
                        if ui.button("Reverse vertices").clicked() {
                            self.modify_polyline(index, &entity, |polyline| { polyline.reverse(); Ok(()) });
                        }
                    }
                }
            } else if self.editor.selection().is_empty() {
                ui.add_enabled(false, Button::new("Model Space     ▾").min_size(egui::vec2(ui.available_width(), 19.0)));
                ui.scope(|ui| {
                    ui.spacing_mut().interact_size.y = 17.0;
                    ui.spacing_mut().button_padding.y = 1.0;
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (label, value) in [("Color", "□  ByLayer"), ("Layer", self.editor.current_layer()), ("Linetype", "Continuous"), ("Linetype scale", "1.0000"), ("Lineweight", "ByLayer"), ("Transparency", "0"), ("Text style", "Standard"), ("Dimension style", "Standard"), ("Multileader style", "Standard"), ("Table style", "Standard"), ("Annotation scale", "1:1"), ("Text height", "0.2000"), ("Plot style", "ByColor"), ("Plot style table", "None"), ("Plot table type", "Not available")] {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(104.0, 17.0), egui::Sense::hover());
                            ui.put(rect, egui::Label::new(label).truncate().halign(egui::Align::Max)).on_hover_text(label);
                            ui.add_enabled(false, Button::new(value).min_size(egui::vec2(104.0, 17.0)));
                        });
                    }
                });
            } else {ui.label("Move, copy, or delete the selected entities together. Each action is one undo step.");}
            if !self.editor.selection().is_empty() {
                ui.separator();
                self.entity_style_panel(ui);
            }
            egui::CollapsingHeader::new("Entities").show(ui, |ui| {
            ui.add_space(6.0);ui.separator();
            ui.horizontal(|ui| {ui.label(RichText::new("ENTITIES").small().color(MUTED));ui.label(RichText::new(self.editor.entities().len().to_string()).small().color(ACCENT));});
            ui.horizontal(|ui| {
                if ui.small_button("Select all").clicked() {self.finish();self.editor.select((0..self.editor.entities().len()).collect());}
                if ui.small_button("Clear").clicked() {self.finish();self.editor.select(Vec::new());}
            });
            egui::ScrollArea::vertical().id_salt("entities").max_height(240.0).auto_shrink([false,false]).show(ui, |ui| {
                for index in 0..self.editor.entities().len() {
                    let entity=&self.editor.entities()[index];
                    let visible = self.editor.is_entity_visible(index);
                    let label = format!("{:03}   {}{}",index+1,entity.name(),if visible {""} else {" · hidden"});
                    if ui.add_enabled(visible, egui::Button::selectable(self.editor.selection().contains(&index),RichText::new(label).monospace())).clicked() {
                        self.finish();
                        if ui.input(|i|i.modifiers.shift) {self.editor.toggle_selection(index);} else {self.editor.select(vec![index]);}
                    }
                }
            });
            });
            });
        });
    }

    fn entity_style_panel(&mut self, ui: &mut egui::Ui) {
        let state: Vec<_> = self
            .editor
            .selection()
            .iter()
            .map(|index| {
                (
                    *index,
                    self.editor
                        .entity_style(*index)
                        .expect("selected entity style")
                        .clone(),
                )
            })
            .collect();
        let Some((_, first)) = state.first() else {
            return;
        };
        if self.inspector.style.state != state {
            self.inspector.style.color = if state
                .iter()
                .all(|(_, style)| style.color_index() == first.color_index())
            {
                match first.color_index() {
                    0 => "BYBLOCK".into(),
                    256 => "BYLAYER".into(),
                    index => index.to_string(),
                }
            } else {
                String::new()
            };
            self.inspector.style.scale = if state
                .iter()
                .all(|(_, style)| style.linetype_scale() == first.linetype_scale())
            {
                first.linetype_scale().to_string()
            } else {
                String::new()
            };
            self.inspector.style.state = state.clone();
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Entity color");
            ui.add(
                egui::TextEdit::singleline(&mut self.inspector.style.color)
                    .hint_text("Mixed")
                    .desired_width(80.0),
            )
            .labelled_by(label.id);
            if ui.button("Apply entity color").clicked() {
                self.finish();
                let result =
                    commands::parse_entity_color(&self.inspector.style.color).and_then(|color| {
                        self.editor
                            .assign_selection_color(color)
                            .map_err(|error| error.to_string())
                    });
                match result {
                    Ok(_) => {
                        self.inspector.style.state.clear();
                        self.message("Entity color applied.", false);
                    }
                    Err(error) => self.message(error, true),
                }
            }
        });
        ui.label(
            RichText::new("BYLAYER · BYBLOCK · index 1–255")
                .small()
                .color(MUTED),
        );
        let mut kind = if state
            .iter()
            .all(|(_, style)| style.linetype().eq_ignore_ascii_case(first.linetype()))
        {
            first.linetype().to_owned()
        } else {
            String::new()
        };
        let original = kind.clone();
        let label = ui.label("Entity linetype");
        egui::ComboBox::from_id_salt("entity_linetype")
            .selected_text(if kind.is_empty() { "Mixed" } else { &kind })
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                for name in ["BYLAYER", "BYBLOCK"] {
                    if ui
                        .selectable_value(&mut kind, name.to_owned(), name)
                        .clicked()
                    {
                        ui.close();
                    }
                }
                for item in self.editor.linetypes() {
                    if !item.name().eq_ignore_ascii_case("BYLAYER")
                        && !item.name().eq_ignore_ascii_case("BYBLOCK")
                        && ui
                            .selectable_value(&mut kind, item.name().to_owned(), item.name())
                            .clicked()
                    {
                        ui.close();
                    }
                }
            })
            .response
            .labelled_by(label.id);
        if kind != original {
            self.finish();
            match self.editor.assign_selection_linetype(&kind) {
                Ok(_) => self.message("Entity linetype applied.", false),
                Err(error) => self.message(error.to_string(), true),
            }
        }
        if !kind.is_empty()
            && !kind.eq_ignore_ascii_case("BYLAYER")
            && !kind.eq_ignore_ascii_case("BYBLOCK")
            && !self
                .editor
                .linetypes()
                .iter()
                .any(|item| item.name().eq_ignore_ascii_case(&kind))
        {
            ui.label(format!("Undefined linetype: {kind} (displayed solid)"));
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Entity scale");
            ui.add(
                egui::TextEdit::singleline(&mut self.inspector.style.scale)
                    .hint_text("Mixed")
                    .desired_width(65.0),
            )
            .labelled_by(label.id);
            if ui.button("Apply entity scale").clicked() {
                self.finish();
                let result = self
                    .inspector
                    .style
                    .scale
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| "Enter a finite, nonnegative entity linetype scale.".to_owned())
                    .and_then(|scale| {
                        self.editor
                            .assign_selection_linetype_scale(scale)
                            .map_err(|error| error.to_string())
                    });
                match result {
                    Ok(_) => {
                        self.inspector.style.state.clear();
                        self.message("Entity scale applied.", false);
                    }
                    Err(error) => self.message(error, true),
                }
            }
        });
        ui.label(
            RichText::new("Zero uses the default scale of 1.")
                .small()
                .color(MUTED),
        );
    }

    fn current_layer_control(&mut self, ui: &mut egui::Ui) {
        let mut current = self.editor.current_layer().to_owned();
        let response = egui::ComboBox::from_id_salt("current_layer")
            .selected_text(&current)
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                for layer in self.editor.layers() {
                    if ui
                        .selectable_value(&mut current, layer.name().to_owned(), layer.name())
                        .clicked()
                    {
                        ui.close();
                    }
                }
            })
            .response;
        ui.ctx()
            .accesskit_node_builder(response.id, |node| node.set_label("Current layer"));
        if current != self.editor.current_layer() {
            self.finish();
            if let Err(error) = self.editor.set_current_layer(&current) {
                self.message(error.to_string(), true);
            }
        }
    }

    fn layer_panel(&mut self, ui: &mut egui::Ui) {
        let current = self.editor.current_layer().to_owned();
        let layer = self
            .editor
            .layers()
            .iter()
            .find(|layer| layer.name() == self.editor.current_layer())
            .expect("current layer exists");
        let color_state = (layer.name().to_owned(), layer.color_index());
        if self.layer_color.layer.as_ref() != Some(&color_state) {
            self.layer_color.value = layer.color_index().abs().to_string();
            self.layer_color.layer = Some(color_state.clone());
        }
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Color index");
            ui.add(egui::TextEdit::singleline(&mut self.layer_color.value).desired_width(52.0))
                .labelled_by(label.id);
            if ui.button("Apply color").clicked() {
                self.finish();
                let result = self
                    .layer_color
                    .value
                    .trim()
                    .parse::<i16>()
                    .map_err(|_| "Enter a color index from 1 to 255.".to_owned())
                    .and_then(|index| {
                        self.editor
                            .set_layer_color(&color_state.0, index)
                            .map_err(|error| error.to_string())
                    });
                match result {
                    Ok(_) => self.message("Layer color applied.", false),
                    Err(error) => self.message(error, true),
                }
            }
        });
        let mut kind = self
            .editor
            .layers()
            .iter()
            .find(|layer| layer.name() == current)
            .expect("current layer")
            .linetype()
            .to_owned();
        let label = ui.label("Layer linetype");
        egui::ComboBox::from_id_salt("layer_linetype")
            .selected_text(&kind)
            .width(ui.available_width())
            .truncate()
            .show_ui(ui, |ui| {
                for item in self.editor.linetypes() {
                    if !item.name().eq_ignore_ascii_case("BYLAYER")
                        && !item.name().eq_ignore_ascii_case("BYBLOCK")
                        && ui
                            .selectable_value(&mut kind, item.name().to_owned(), item.name())
                            .clicked()
                    {
                        ui.close();
                    }
                }
            })
            .response
            .labelled_by(label.id);
        if self
            .editor
            .layers()
            .iter()
            .find(|layer| layer.name() == current)
            .expect("current layer")
            .linetype()
            != kind
        {
            self.finish();
            match self.editor.set_layer_linetype(&current, &kind) {
                Ok(_) => self.message("Layer linetype applied.", false),
                Err(error) => self.message(error.to_string(), true),
            }
        }
        if !self
            .editor
            .linetypes()
            .iter()
            .any(|item| item.name().eq_ignore_ascii_case(&kind))
        {
            ui.label(format!("Undefined linetype: {kind} (displayed solid)"));
        }
        ui.horizontal(|ui| {
            let label = ui.label("New layer");
            ui.add(egui::TextEdit::singleline(&mut self.new_layer).desired_width(110.0))
                .labelled_by(label.id);
        });
        if ui.button("Add layer").clicked() {
            self.finish();
            match self.editor.add_layer(&self.new_layer) {
                Ok(_) => {
                    self.editor
                        .set_current_layer(&self.new_layer)
                        .expect("new layer exists");
                    self.new_layer.clear();
                    self.message("Layer added and made current.", false);
                }
                Err(error) => self.message(error.to_string(), true),
            }
        }
        egui::ScrollArea::vertical()
            .id_salt("layer_list")
            .max_height(96.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for layer in self.editor.layers().to_vec() {
                    ui.horizontal_wrapped(|ui| {
                        let mut visible = layer.is_visible();
                        if ui
                            .checkbox(&mut visible, format!("Show {}", layer.name()))
                            .changed()
                        {
                            self.finish();
                            if let Err(error) = self.editor.set_layer_visible(layer.name(), visible)
                            {
                                self.message(error.to_string(), true);
                            }
                        }
                        let mut locked = layer.is_locked();
                        if ui
                            .checkbox(&mut locked, format!("Lock {}", layer.name()))
                            .changed()
                        {
                            self.finish();
                            if let Err(error) = self.editor.set_layer_locked(layer.name(), locked) {
                                self.message(error.to_string(), true);
                            }
                        }
                    });
                }
            });
        if ui
            .add_enabled(
                !self.editor.selection().is_empty(),
                Button::new("Assign to current layer"),
            )
            .clicked()
        {
            self.finish();
            match self
                .editor
                .assign_selection_layer(self.editor.current_layer().to_owned().as_str())
            {
                Ok(true) => self.message("Selection assigned to current layer.", false),
                Ok(false) => self.message("Selection already uses this layer.", false),
                Err(error) => self.message(error.to_string(), true),
            }
        }
    }

    fn linetype_panel(&mut self, ui: &mut egui::Ui) {
        let label = ui.label("New linetype");
        ui.add(
            egui::TextEdit::singleline(&mut self.linetype_input.name)
                .desired_width(ui.available_width()),
        )
        .labelled_by(label.id);
        let label = ui.label("Pattern lengths");
        ui.add(
            egui::TextEdit::singleline(&mut self.linetype_input.pattern)
                .desired_width(ui.available_width())
                .hint_text("1,-0.25,0,-0.25"),
        )
        .labelled_by(label.id);
        ui.label("Positive: dash · Negative: gap · Zero: dot");
        if ui.button("Add linetype").clicked() {
            self.finish();
            let result = commands::parse_pattern(&self.linetype_input.pattern)
                .and_then(|pattern| {
                    Linetype::new(&self.linetype_input.name, pattern)
                        .map_err(|error| error.to_string())
                })
                .and_then(|kind| {
                    self.editor
                        .add_linetype(kind)
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(_) => {
                    self.linetype_input.name.clear();
                    self.linetype_input.pattern.clear();
                    self.message("Linetype created. Choose it under Layers.", false);
                }
                Err(error) => self.message(error, true),
            }
        }
        let scale = self.editor.linetype_scale();
        if self.linetype_input.scale_state != Some(scale) {
            self.linetype_input.scale = scale.to_string();
            self.linetype_input.scale_state = Some(scale);
        }
        let label = ui.label("Linetype scale");
        ui.add(egui::TextEdit::singleline(&mut self.linetype_input.scale).desired_width(90.0))
            .labelled_by(label.id);
        if ui.button("Apply scale").clicked() {
            self.finish();
            let result = self
                .linetype_input
                .scale
                .trim()
                .parse::<f64>()
                .map_err(|_| "Enter a positive linetype scale.".to_owned())
                .and_then(|scale| {
                    self.editor
                        .set_linetype_scale(scale)
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(_) => self.message("Linetype scale applied.", false),
                Err(error) => self.message(error, true),
            }
        }
    }

    fn inspector_entity(&self, original: &Entity) -> Result<Entity, String> {
        let values = self
            .inspector
            .fields
            .iter()
            .map(|s| {
                s.parse::<f64>()
                    .map_err(|_| "Enter a number for every coordinate.".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Entity::Polyline(polyline) = original {
            let vertices = values[1..]
                .chunks_exact(3)
                .map(|vertex| Point3::new(vertex[0], vertex[1], values[0]))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            let bulges = values[1..]
                .chunks_exact(3)
                .map(|vertex| vertex[2])
                .collect();
            let replacement = Polyline2::with_bulges(vertices, bulges, polyline.is_closed())
                .map_err(|error| error.to_string())?;
            replacement.length().map_err(|error| error.to_string())?;
            replacement.extrema().map_err(|error| error.to_string())?;
            return Ok(Entity::Polyline(replacement));
        }
        let start = Point3::new(values[0], values[1], values[2]).map_err(|e| e.to_string())?;
        match original {
            Entity::Line(_) => Ok(Entity::Line(Line3::new(
                start,
                Point3::new(values[3], values[4], values[5]).map_err(|e| e.to_string())?,
            ))),
            Entity::Circle(_) => Ok(Entity::Circle(
                Circle3::new(start, values[3]).map_err(|e| e.to_string())?,
            )),
            Entity::Arc(arc) => {
                // Keep untouched angle values exact across the display unit conversion.
                let angle = |value: f64, original: f64| {
                    if value == original.to_degrees() {
                        original
                    } else {
                        value.to_radians()
                    }
                };
                Ok(Entity::Arc(
                    Arc3::new(
                        start,
                        values[3],
                        angle(values[4], arc.start_angle()),
                        angle(values[5], arc.end_angle()),
                    )
                    .map_err(|e| e.to_string())?,
                ))
            }
            Entity::Polyline(_) => unreachable!("polyline fields handled above"),
        }
    }

    fn modify_polyline(
        &mut self,
        index: usize,
        original: &Entity,
        edit: impl FnOnce(&mut Polyline2) -> Result<(), crate::GeometryError>,
    ) {
        let result = self.inspector_entity(original).and_then(|mut entity| {
            let Entity::Polyline(polyline) = &mut entity else {
                unreachable!("polyline controls")
            };
            edit(polyline).map_err(|error| error.to_string())?;
            polyline.length().map_err(|error| error.to_string())?;
            polyline.extrema().map_err(|error| error.to_string())?;
            self.finish();
            self.editor
                .update(index, entity)
                .map_err(|error| error.to_string())
        });
        match result {
            Ok(_) => self.message("Polyline properties applied.", false),
            Err(error) => self.message(error, true),
        }
    }

    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            let mut choice = 0;
            egui::Modal::new(egui::Id::new("unsaved_drawing")).show(ctx, |ui| {
                ui.set_width(370.0);
                ui.heading("Save your drawing?");
                ui.label(format!("{} has unsaved changes.", self.name));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save changes").clicked() {
                        choice = 1;
                    }
                    if ui.button("Discard changes").clicked() {
                        choice = 2;
                    }
                    if ui.button("Cancel").clicked() {
                        choice = 3;
                    }
                });
            });
            if choice == 3 {
                self.pending = None;
            }
            if (choice == 2 || (choice == 1 && self.save(false)))
                && let Some(action) = self.pending.take()
            {
                self.perform(action);
            }
        }
        if self.show_help {
            let mut open = true;
            let mut study = false;
            egui::Window::new("OpenCad · Drawing reference").open(&mut open).resizable(false).show(ctx, |ui| {
                ui.set_width(540.0);ui.label("Draw with the mouse or enter exact coordinates in the command field.");ui.separator();
                egui::Grid::new("help_commands").striped(true).spacing([20.0,10.0]).show(ui, |ui| {
                    for (command,detail) in [
                        ("LINE 0,0 100,0 100,60","Connected line segments; optional XYZ"),
                        ("PLINE 0,0,3 100,0,3 100,60,3 CLOSE","One straight polyline with a common elevation"),
                        ("Polyline → points → Enter / C","Finish open / closed; U removes a draft vertex"),
                        ("Properties → Vertex N bulge","0 straight; +1 CCW / −1 CW semicircle to next vertex"),
                        ("CIRCLE 50,30,2 12","XY circle at Z = 2, radius 12"),
                        ("ARC 50,30,2 12 270 90","XY arc; angles in degrees, counterclockwise"),
                        ("Arc tool → center → start → end direction","Mouse arc at the center's Z elevation"),
                        ("RECTANG 0,0 100,60","Four lines at the first corner's Z"),
                        ("LINE → 0,0 → @20,0 → C","Interactive points; U undoes a segment"),
                        ("SELECT 1 2 / SELECT ALL","Select by entity number"),
                        ("MOVE 10 20 0 / COPY 10 20","Translate the selection"),
                        ("DELETE / UNDO / REDO","Edit history, up to 100 actions"),
                        ("LAYER NEW|SET|SHOW|HIDE name","Create, choose or show/hide a layer"),
                        ("LAYER ASSIGN|LOCK|UNLOCK name","Assign selection; protect layer geometry"),
                        ("LAYER COLOR name 1–255","Change the layer's ACI color"),
                        ("LINETYPE name 1,-0.5,0,-0.5","Create a simple dash/gap/dot pattern"),
                        ("LAYER LINETYPE name pattern","Assign a layer's linetype"),
                        ("LTSCALE positive-number","Scale all drawing linetype patterns"),
                        ("ENTITY COLOR BYLAYER|BYBLOCK|index","Set selected entities' colors"),
                        ("ENTITY LINETYPE name","Set selected entities' linetype"),
                        ("ENTITY LTSCALE nonnegative-number","Set selected entities' pattern scale"),
                        ("ZOOM EXTENTS / FIT","Fit all geometry"),
                        ("OPEN / SAVE / SAVEAS","Native DXF file dialogs; paths accepted"),
                        ("EXPORTSVG / ELEVATION 5","Export XY; set Z for new points"),
                        ("GRID / SNAP / OSNAP / ORTHO / POLAR","Toggle; optional ON or OFF"),
                        ("POLARANG 45","Set the polar angle increment in degrees"),
                    ] {ui.monospace(command);ui.label(detail);ui.end_row();}
                });
                ui.separator();ui.label("⌘N / O / S · New / Open / Save     ⌘Z / ⌘⇧Z · Undo / Redo\nV / L / C / A / R / M / P · Tools     F · Fit     Esc · Cancel\nF3 · Object snap     F7 · Grid     F8 · Ortho     F9 · Grid snap     F10 · Polar\nMiddle or right drag · Pan     Scroll / pinch · Zoom at pointer");
                ui.separator();ui.label(RichText::new("R12 ASCII DXF · LINE, XY CIRCLE / ARC, and line/arc XY POLYLINE at a common elevation on named layers. Hidden layers do not display, select, snap or export to SVG. New layer names: 1–31 ASCII letters, digits, _, $, or -. ACI colors 1–255 display on the canvas and in SVG; color 7 is white on the dark canvas and black on SVG's white background. Imported layer colors 0 and 256 display as color 7 until explicitly changed. Simple dash, gap and dot linetypes display on the canvas and in SVG. Current layer is workspace state. Imported header, other tables, blocks and handles are preserved. Coordinates are unitless. Polyline widths, fitted curves, 3D polylines, arbitrary planes, DWG and other entity types are not supported yet.").small().color(MUTED));
                if ui.button("Load courtyard study").clicked() {study = true;}
            });
            self.show_help = open && !study;
            if study {
                self.request(PendingAction::Study);
            }
        }
    }
}
