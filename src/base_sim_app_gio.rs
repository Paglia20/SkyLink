
use std::cmp::Ordering;
use eframe::egui;
use eframe::egui::{CentralPanel, Color32, Frame, SidePanel};
// use egui::Rangef;
use wg_2024::network::NodeId;


#[derive(Debug, Clone)]
pub struct MyNodes {
    id : NodeId,
    connections: Vec<NodeId>
}

impl Eq for MyNodes {

}

impl PartialEq<Self> for MyNodes {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialOrd<Self> for MyNodes {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.id.partial_cmp(&other.id)
    }
}

impl Ord for MyNodes {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}


pub enum Scene{
    Start,
    Manage,
}
pub struct MyApp {
    //field per simulation controll
    nodes: Vec<MyNodes>,
    scene: Scene,
    checked: Vec<bool>,
    selected_nodes: Vec<bool>, // Salva l'indice del drone selezionato
}

impl MyApp {
    pub(crate) fn new() -> Self {
        let mut vec: Vec<MyNodes> = Vec::new();
        let mut checked = Vec::new();
        let mut selected_nodes = Vec::new();
        for _ in 0..fastrand::usize(12..18) {
            let new_drone = MyNodes {
                id: fastrand::u8(0..255),
                connections: Vec::new(),
            };
            vec.push(new_drone);
            checked.push(false);
            selected_nodes.push(false);
        }
        Self {
            nodes: vec,
            scene: Scene::Start,
            checked,
            selected_nodes,
        }
    }

    fn retest(&mut self) {
        self.nodes.clear();
        self.checked.clear();
        for _ in 0..fastrand::usize(12..18) {
            let new_drone = MyNodes {
                id: fastrand::u8(0..255),
                connections: Vec::new(),
            };
            self.nodes.push(new_drone);
            self.checked.push(false);
            self.selected_nodes.push(false);
        }
        println!("len: {:?} vec: {:?}", self.nodes.len(), self.nodes.clone());
    }

    fn reset_check(&mut self) {
        self.checked.clear();
        for _ in 0..self.nodes.len() {
            self.checked.push(false);
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // BottomPanel ridimensionabile
        egui::TopBottomPanel::bottom("bottom_panel")
            .height_range(100.0..=200.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Pannello inferiore (ridimensionabile)");
                });
            });

        // SidePanel sulla sinistra
        SidePanel::left("side_panel")
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Pannello laterale");
                match self.scene {
                    Scene::Start => {
                        if ui.button("Retest!").clicked() {
                            self.retest();
                        }
                        if ui.button("Add Drone!").clicked() {
                            self.scene = Scene::Manage;
                        }
                        if ui.button("remove Drone!").clicked() {
                            self.scene = Scene::Manage;
                        }
                    }
                    Scene::Manage => {
                        for (i, item) in self.nodes.iter().enumerate() {
                            ui.checkbox(&mut self.checked[i], item.id.to_string());
                        }

                        if ui.button("Process Checked Items").clicked() {
                            let checked_indices: Vec<NodeId> = self
                                .checked
                                .iter()
                                .enumerate()
                                .filter_map(|(i, &is_checked)| if is_checked { Some(self.nodes[i].id) } else { None })
                                .collect();
                            add_drone(&checked_indices);
                            self.reset_check();
                            self.scene = Scene::Start;
                        }
                    }
                }
            });

        for (index, is_selected) in self.selected_nodes.iter_mut().enumerate() {
            if *is_selected {
                egui::Window::new(format!("Log for Node {}", self.nodes[index].id))
                    .resizable(true) // Permetti il ridimensionamento
                    .collapsible(true)
                    .min_height(500.0)
                    .min_width(500.0)
                    //  .default_size(egui::vec2(300.0, 1000.0)) // Dimensione iniziale
                    .show(ctx, |ui| {
                        ui.label(format!("Dettagli del nodo {}:", self.nodes[index].id));

                        // Qui puoi aggiungere ulteriori informazioni o controlli
                        ui.label("Log:");

                        //insert log of the drone (idk how)
                        if ui.button("Chiudi").clicked() {
                            *is_selected = false; // Chiudi il popup
                        }
                    });
            }
        }


        // Pannello centrale
        CentralPanel::default().show(ctx, |ui| {
            Frame::dark_canvas(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width()); // Adatta il pannello alla larghezza disponibile
                ui.set_height(ui.available_height());

                ui.heading("Network Topology");

                egui::ScrollArea::horizontal().show(ui, |ui| {
                    // Calcola il centro e il raggio
                    let available_size = ui.available_size();
                    let center = egui::pos2(
                        ui.min_rect().left() + available_size.x / 2.0,
                        ui.min_rect().top() + available_size.y / 2.0,
                    );
                    let radius = available_size.x.min(available_size.y) * 0.4;

                    // Ordina i valori
                    self.nodes.sort();
                    let total_items = self.nodes.len();

                    // Gestione interattiva dei cerchi
                    for (index, value) in self.nodes.iter().enumerate() {
                        // Angolo per ciascun cerchio
                        let angle = (index as f32 / total_items as f32) * std::f32::consts::TAU;

                        // Posizione del cerchio sulla circonferenza
                        let x = center.x + radius * angle.cos();
                        let y = center.y + radius * angle.sin();

                        // Rettangolo cliccabile
                        let rect = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(50.0, 50.0));
                        let response = ui.interact(rect, egui::Id::new(index), egui::Sense::click());

                        // Disegna il cerchio
                        let painter = ui.painter();
                        let circle_color = if self.selected_nodes[index] {
                            Color32::YELLOW
                        } else {
                            Color32::GREEN
                        };

                        painter.circle_filled(rect.center(), 15.0, circle_color);

                        // Disegna il testo
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            value.id.to_string(),
                            egui::FontId::proportional(16.0),
                            Color32::BLUE,
                        );

                        // Gestisci il clic
                        if response.clicked() {
                            self.selected_nodes[index] = true;
                            println!("Drone selezionato: {:?}", self.nodes[index]);


                        }
                    }
                });
            });
        });
    }


}

fn add_drone(checked_indices: &Vec<NodeId>) {
    println!("Checked items indices: {:?}", checked_indices);

    //todo
}


/*
feel free to update this list.
STARTING FROM THIS BASE, WHAT DO I HAVE TO DO:

STRICTLY FOR SIM CONTROL PART:
1) correct spawn, and remove NODES. not only drones anymore
2) change accordingly to sim app parts
3) probably is in great part to be redone, but it's a good start i think


STRICTLY FOR SIM APP PART:
0) add field in MyNodes that tell the Type of the Node (NodeType), and change creation of the Circles depending on the Nodetype.
1) make a field for a rc<refcell<simcontroll>
2) make that the vectors in MyApp get filled from the informations on simcontroll.network_graph
3) make the pop up bigger and such that it display the NodeEvent sent to the sim controll by that drone
4) add simulation controller log in bottom panel.
5) add connections between nodes


--test everything, then continue with other things

6) make functions add_drone and remove drone that not only eliminate graphically the drones and connections, but also in the network saved in sim controll
7) add bottons in the pop ups for clients/servers that send flood req or certain messages
8) at the end, change the circles in drones/clients/server small entities

(.. more to come)

 */