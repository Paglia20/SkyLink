use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;
use eframe::egui;
use wg_2024::network::NodeId;
use crate::sim_control::SimulationControl;

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
    fn cmp(&self, other: &Self) ->Ordering {
        self.id.cmp(&other.id)
    }
}


pub enum Scene{
    Start,
    ManageAdd,
    ManageCrash,
}
pub struct MyApp {
    sim_contr: Rc<RefCell<SimulationControl>>,
    nodes: Vec<MyNodes>,
    scene: Scene,
    checked: Vec<bool>,
    selected_nodes: Vec<bool>, // Salva l'indice del drone selezionato
    pdr: f32
}

impl MyApp {
    pub(crate) fn new(sim_contr: Rc<RefCell<SimulationControl>>) -> Self {
        let network_graph = sim_contr.borrow().network_graph.clone();
        let mut vec: Vec<MyNodes> = Vec::new();
        let mut checked = Vec::new();
        let mut selected_nodes = Vec::new();

        /*for _ in 0..fastrand::usize(12..18) {
            let new_drone = MyNodes {
                id: fastrand::u8(0..255),
                connections: Vec::new(),
            };
            vec.push(new_drone);
            checked.push(false);
            selected_nodes.push(false);
        } */

        for (node_id, neighbors) in network_graph {
            vec.push(MyNodes{id : node_id, connections: neighbors});
            checked.push(false);
            selected_nodes.push(false);
        }

        let mut app = Self {
            nodes: vec,
            scene: Scene::Start,
            checked,
            selected_nodes,
            sim_contr,
            pdr: 0.0
        };
        //app.generate_random_connections();
        app
    }

    pub fn update_topology(&mut self, sim_contr: Rc<RefCell<SimulationControl>>) {
        let network_graph = sim_contr.borrow().network_graph.clone();
        for (node_id, neighbors) in network_graph {
            self.nodes.push(MyNodes{id : node_id, connections: neighbors});
            self.checked.push(false);
            self.selected_nodes.push(false);
        }
    }

    fn generate_random_connections(&mut self) {
        let total_nodes = self.nodes.len();

        for i in 0..total_nodes {
            let num_connections = fastrand::usize(1..=3);
            let mut connections = Vec::new();

            while connections.len() < num_connections {
                let random_index = fastrand::usize(0..total_nodes);

                if random_index != i && !connections.contains(&self.nodes[random_index].id) {
                    connections.push(self.nodes[random_index].id);
                }
            }

            self.nodes[i].connections = connections;
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

        self.generate_random_connections();
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
        egui::SidePanel::left("side_panel")
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Actions");
                match self.scene {
                    Scene::Start => {
                        if ui.button("Retest!").clicked() {
                            self.retest();
                        }
                        if ui.button("Add Drone!").clicked() {
                            self.scene = Scene::ManageAdd;
                        }
                        if ui.button("remove Drone!").clicked() {
                            self.scene = Scene::ManageCrash;
                        }
                    }
                    Scene::ManageAdd => {
                        if ui.button("back").clicked() {
                            self.scene = Scene::Start;
                            self.reset_check();
                            self.pdr = 0.0;
                        }
                        ui.separator();
                        ui.label("select drones to connect the new drone with:");
                        for (i, item) in self.nodes.iter().enumerate() {
                            ui.checkbox(&mut self.checked[i], item.id.to_string());
                        }
                        ui.separator();
                        ui.label("input pdr:");
                        ui.add(egui::DragValue::new(&mut self.pdr).speed(0.1));
                        ui.separator();

                        if ui.button("Confirm").clicked() {
                            let checked_indices: Vec<NodeId> = self
                                .checked
                                .iter()
                                .enumerate()
                                .filter_map(|(i, &is_checked)| if is_checked { Some(self.nodes[i].id) } else { None })
                                .collect();
                            add_node(&checked_indices, self.pdr);
                            self.reset_check();
                            self.scene = Scene::Start;
                        }
                    }
                    Scene::ManageCrash => {
                        ui.separator();
                        ui.label("select drones to crash:");
                        ui.separator();
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
                            //add_node(&checked_indices);
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
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width()); // Adatta il pannello alla larghezza disponibile
                ui.set_height(ui.available_height());

                ui.heading("Network Topology");

                let available_size = ui.available_size();
                let center = egui::pos2(
                    ui.min_rect().left() + available_size.x / 2.0,
                    ui.min_rect().top() + available_size.y / 2.0,
                );
                let radius = available_size.x.min(available_size.y) * 0.4;

                self.nodes.sort();
                let total_items = self.nodes.len();

                let mut positions = Vec::new();
                for (index, _value) in self.nodes.iter().enumerate() {
                    let angle = (index as f32 / total_items as f32) * std::f32::consts::TAU;
                    let x = center.x + radius * angle.cos();
                    let y = center.y + radius * angle.sin();
                    positions.push(egui::pos2(x, y));
                }

                let painter = ui.painter();

                for (i, node) in self.nodes.iter().enumerate() {
                    for &connection in &node.connections {
                        if let Some(j) = self.nodes.iter().position(|n| n.id == connection) {
                            let line_color = egui::Color32::WHITE;
                            painter.line_segment([positions[i], positions[j]], (2.0, line_color));
                        }
                    }
                }

                for (index, value) in self.nodes.iter().enumerate() {
                    let rect = egui::Rect::from_center_size(positions[index], egui::vec2(50.0, 50.0));
                    let response = ui.interact(rect, egui::Id::new(index), egui::Sense::click());

                    let circle_color = if self.selected_nodes[index] {
                        egui::Color32::BLUE
                    } else {
                        egui::Color32::from_rgb(216, 100, 56)
                    };
                    painter.circle_filled(rect.center(), 15.0, circle_color);

                    // Disegna il testo
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        value.id.to_string(),
                        egui::FontId::proportional(16.0),
                        egui::Color32::WHITE,
                    );

                    // Gestisci il clic
                    if response.clicked() {
                        self.selected_nodes[index] = true;
                        println!("Drone selezionato: {:?}", self.nodes[index]);
                    }
                }
            });
        });
    }


}

fn add_node(checked_indices: &Vec<NodeId>, pdr: f32) {
    // SimulationControl::spawn_node(&mut , pdr, checked_indices.clone());

}




pub fn run_sim_dan(sim_control: Rc<RefCell<SimulationControl>>) -> Result<(), eframe::Error>{
    let mut options = eframe::NativeOptions::default();
    options.run_and_return = false;
    eframe::run_native(
        "Interfaccia con layout adattabile",
        options,
        Box::new(|_cc| Ok(Box::new(MyApp::new(sim_control)))),
    )
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
2) add in each pop up what type the node is (client/server)
3) make the pop up bigger and such that it display the NodeEvent sent to the sim controll by that drone
4) add simulation controller log in bottom panel.

the field node type is important because the pop up has to have different buttons depending on the type:

//please help me here:
drone: crash? /...
client: send flood req / send message to (open a manage) / ..
server:...

--test everything, then continue with other things

6) make functions add_drone and remove drone that not only eliminate graphically the drones and connections, but also in the network saved in sim controll
7) add bottons in the pop ups for clients/servers that send flood req or certain messages
8) at the end, change the circles in drones/clients/server small entities

(.. more to come)

 */