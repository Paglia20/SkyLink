use eframe::egui::{self, Color32, Context, TextureHandle, Vec2};
use eframe::{App, Frame, NativeOptions};

struct Drone {
    id: String,
    position: Vec2,
    is_crashed: bool,
}

pub struct SimulationApp {
    drones: Vec<Drone>,
    connections: Vec<(usize, usize)>,
    drone_texture: Option<TextureHandle>,
    log: Vec<String>,
    selected_drone: Option<usize>,
    dragging_drone: Option<usize>,
    show_connection_dialog: bool,
    new_drone_index: Option<usize>,
}

impl Default for SimulationApp {
    fn default() -> Self {
        Self {
            drones: vec![
                Drone {
                    id: "Drone1".to_string(),
                    position: Vec2::new(100.0, 100.0),
                    is_crashed: false,
                },
                Drone {
                    id: "Drone2".to_string(),
                    position: Vec2::new(300.0, 100.0),
                    is_crashed: false,
                },
                Drone {
                    id: "Drone3".to_string(),
                    position: Vec2::new(200.0, 300.0),
                    is_crashed: false,
                },
            ],
            connections: vec![(0, 1), (1, 2), (2, 0)],
            drone_texture: None,
            log: Vec::new(),
            selected_drone: None,
            dragging_drone: None,
            show_connection_dialog: false,
            new_drone_index: None,
        }
    }
}

impl SimulationApp {
    fn load_drone_image(&mut self, ctx: &Context) {
        if self.drone_texture.is_none() {
            let image_data = include_bytes!("drone.png");
            let image = image::load_from_memory(image_data)
                .expect("Failed to load image")
                .to_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            let pixels = image.into_raw();

            self.drone_texture = Some(ctx.load_texture(
                "drone_image",
                egui::ColorImage::from_rgba_unmultiplied(size, &pixels),
                egui::TextureOptions::default(),
            ));
        }
    }

    fn render_drones(&mut self, ui: &mut egui::Ui, texture: &TextureHandle) {
        for (i, drone) in self.drones.iter_mut().enumerate() {
            let color_overlay = if drone.is_crashed {
                Color32::RED
            } else if Some(i) == self.selected_drone {
                Color32::YELLOW
            } else {
                Color32::WHITE
            };

            let size = Vec2::new(50.0, 50.0);
            let rect = egui::Rect::from_min_size(
                egui::Pos2::new(drone.position.x, drone.position.y),
                size,
            );

            let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            if response.clicked() {
                self.selected_drone = Some(i);
                self.log.push(format!("{} selected", drone.id));
            }

            if response.dragged() {
                if self.dragging_drone.is_none() {
                    self.dragging_drone = Some(i);
                }

                if let Some(dragging_idx) = self.dragging_drone {
                    if dragging_idx == i {
                        drone.position += response.drag_delta();
                    }
                }
            }

            if response.drag_released() && self.dragging_drone == Some(i) {
                self.dragging_drone = None;
            }

            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_size(
                    egui::Pos2::new(0.0, 0.0),
                    Vec2::new(1.0, 1.0),
                ),
                color_overlay,
            );

            ui.painter().text(
                egui::Pos2::new(drone.position.x + 20.0, drone.position.y - 10.0),
                egui::Align2::CENTER_CENTER,
                &drone.id,
                egui::FontId::default(),
                Color32::WHITE,
            );
        }
    }

    fn render_connections(&self, ui: &mut egui::Ui) {
        for &(i, j) in &self.connections {
            let pos1 = self.drones[i].position + Vec2::new(25.0, 25.0);
            let pos2 = self.drones[j].position + Vec2::new(25.0, 25.0);

            ui.painter().line_segment(
                [egui::Pos2::new(pos1.x, pos1.y), egui::Pos2::new(pos2.x, pos2.y)],
                (2.0, Color32::GREEN),
            );
        }
    }

    fn render_log(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &self.log {
                ui.label(entry);
            }
        });
    }

    fn handle_ui_controls(&mut self, ui: &mut egui::Ui) {
        if ui.button("Add Drone").clicked() {
            let new_id = format!("Drone{}", self.drones.len() + 1);
            let new_drone = Drone {
                id: new_id.clone(),
                position: Vec2::new(200.0, 200.0),
                is_crashed: false,
            };

            self.drones.push(new_drone);
            let new_index = self.drones.len() - 1;
            self.new_drone_index = Some(new_index);

            self.show_connection_dialog = true;
            self.log.push(format!("{} added", new_id));
        }

        if ui.button("Crash Selected Drone").clicked() {
            if let Some(idx) = self.selected_drone {
                if let Some(drone) = self.drones.get_mut(idx) {
                    drone.is_crashed = true;
                    self.log.push(format!("{} crashed", drone.id));
                }
            }
        }

        if ui.button("Reset Selected Drone").clicked() {
            if let Some(idx) = self.selected_drone {
                if let Some(drone) = self.drones.get_mut(idx) {
                    drone.is_crashed = false;
                    self.log.push(format!("{} reset", drone.id));
                }
            }
        }
    }

    fn handle_selection(&mut self, ui: &mut egui::Ui) {
        if let Some(idx) = self.selected_drone {
            let drone = &self.drones[idx];
            ui.label(format!("Selected: {}", drone.id));
        } else {
            ui.label("No Drone Selected");
        }
    }

    fn render_connection_dialog(&mut self, ui: &mut egui::Ui) {
        if self.show_connection_dialog && self.new_drone_index.is_some() {
            egui::Window::new("Connect New Drone")
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label("Select drones to connect the new drone to:");

                    let new_drone_index = self.new_drone_index.unwrap();
                    let mut selected_connections = Vec::new();

                    for (idx, drone) in self.drones.iter().enumerate() {
                        if idx != new_drone_index {
                            let mut is_connected = false;
                            ui.checkbox(&mut is_connected, &drone.id);

                            if is_connected {
                                selected_connections.push(idx);
                            }
                        }
                    }

                    if ui.button("Confirm Connections").clicked() {
                        for connect_idx in selected_connections {
                            self.connections.push((new_drone_index, connect_idx));
                            self.log.push(format!("Connected {} to {}",
                                                  self.drones[new_drone_index].id,
                                                  self.drones[connect_idx].id));
                        }

                        self.show_connection_dialog = false;
                        self.new_drone_index = None;
                    }
                });
        }
    }
}

impl App for SimulationApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.load_drone_image(ctx);

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.heading("SkyLink Simulation");
        });

        egui::SidePanel::left("log").show(ctx, |ui| {
            ui.heading("Log");
            self.render_log(ui);
        });

        egui::SidePanel::right("controls").show(ctx, |ui| {
            ui.heading("Controls");
            self.handle_ui_controls(ui);
            self.handle_selection(ui);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(texture) = self.drone_texture.clone() {
                self.render_connections(ui);
                self.render_drones(ui, &texture);

                self.render_connection_dialog(ui);
            }
        });
    }
}

pub fn run_simulation_gui() {
    let options = NativeOptions::default();
    eframe::run_native(
        "SkyLink Simulation",
        options,
        Box::new(|_cc| Box::new(SimulationApp::default())),
    )
        .expect("Failed to start GUI");
}