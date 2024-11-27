use eframe::egui;

pub struct SimulationApp;

impl Default for SimulationApp {
    fn default() -> Self {
        Self
    }
}

impl eframe::App for SimulationApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {

            let rect = egui::Rect::from_min_size(egui::Pos2::new(5.0, 10.0), egui::vec2(300.0, 30.0));
            ui.allocate_ui_at_rect(rect, |ui| {
                ui.heading("Simulation Controller");
            });

            let painter = ui.painter();

            // Nodi
            let nodes = vec![
                (100.0, 100.0, "Drone1"),
                (200.0, 200.0, "Drone2"),
            ];

            // Disegno nodi
            for &(x, y, label) in &nodes {
                painter.circle_filled(egui::Pos2::new(x, y), 10.0, egui::Color32::BLUE);
                painter.text(
                    egui::Pos2::new(x, y - 15.0),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::default(),
                    egui::Color32::WHITE,
                );
            }

            // Connessione nodi
            if nodes.len() > 1 {
                painter.line_segment(
                    [
                        egui::Pos2::new(nodes[0].0, nodes[0].1),
                        egui::Pos2::new(nodes[1].0, nodes[1].1),
                    ],
                    (2.0, egui::Color32::GREEN),
                );
            }


            ui.label("by SkyLink");
        });
    }
}

pub fn run_simulation_gui() {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Simulation Controller",
        options,
        Box::new(|_cc| Box::new(SimulationApp::default())),
    );
}