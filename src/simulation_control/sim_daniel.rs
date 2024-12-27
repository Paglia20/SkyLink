use crate::sim_control::{Cause, LogEntry, SimulationControl};
use crate::simulation_control::sim_daniel::Scene::*;
use crate::test::test_bench::create_packet;
use eframe::egui;
use egui::{FontId, RichText, Vec2};
use std::cmp::{Ordering, PartialEq};
use std::fmt::format;
use wg_2024::controller::DroneEvent;
use wg_2024::controller::DroneEvent::{ControllerShortcut, PacketDropped};
use wg_2024::network::NodeId;
use wg_2024::packet::NodeType;
use wg_2024::packet::NodeType::Drone;
use crate::simulation_control::sim_daniel::DroneWindowScene::AddSender;

#[derive(Debug, Clone)]
pub struct MyNodes {
    id: NodeId,
    connections: Vec<NodeId>,
    selected: bool,
    node_type: NodeType,
}

impl Eq for MyNodes {}

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

pub enum Scene {
    Start,
    ManageAdd,
    ManageCrash,
    ManageDrop,
    ManageShortcut,
}

pub enum DroneWindowScene {
    Start,
    AddSender,
    RemoveSender,
    Crash,
    SetPDR
}
pub struct MyApp {
    sim_contr: SimulationControl,
    nodes: Vec<MyNodes>,
    side_panel_scenes: Scene,
    drone_window_scenes: DroneWindowScene,
    checked: Vec<bool>,
    pdr: f32,
    sender_id: NodeId
}

impl MyApp {
    pub(crate) fn new(sim_contr: SimulationControl) -> Self {
        let network_graph = sim_contr.network_graph.clone();
        println!("i work ghere2");

        let mut vec: Vec<MyNodes> = Vec::new();
        let mut checked = Vec::new();
        let mut selected_nodes = Vec::new();

        for (node_id, neighbors) in network_graph {
            vec.push(MyNodes {
                id: node_id,
                connections: neighbors.1,
                selected: false,
                node_type: neighbors.0,
            });
            checked.push(false);
            selected_nodes.push(false);
        }

        let mut app = Self {
            nodes: vec,
            side_panel_scenes: Start,
            checked,
            sim_contr,
            pdr: 0.0,
            sender_id: 0,
            drone_window_scenes: DroneWindowScene::Start,
        };
        //app.generate_random_connections();
        app
    }

    pub fn update_topology(&mut self) {
        let id_to_selected = self
            .nodes
            .iter()
            .map(|x| (x.id, x.selected))
            .collect::<Vec<(NodeId, bool)>>();
        self.nodes.clear();
        let network_graph = self.sim_contr.network_graph.clone();
        for (node_id, neighbors) in network_graph {
            if id_to_selected.contains(&(node_id, true)) {
                self.nodes.push(MyNodes {
                    id: node_id,
                    connections: neighbors.1,
                    selected: true,
                    node_type: neighbors.0,
                });
            } else if id_to_selected.contains(&(node_id, false)) {
                self.nodes.push(MyNodes {
                    id: node_id,
                    connections: neighbors.1,
                    selected: false,
                    node_type: neighbors.0,
                });
            } else {
                //if there are new elements, add and make those checkable
                self.nodes.push(MyNodes {
                    id: node_id,
                    connections: neighbors.1,
                    selected: false,
                    node_type: neighbors.0.clone(),
                });
                self.checked.push(false);
            }
        }
    }

    fn reset_check(&mut self) {
        self.checked.clear();
        for _ in 0..self.nodes.len() {
            self.checked.push(false);
        }
    }

    pub fn manage_event(&mut self, event: DroneEvent) {
        self.sim_contr.add_to_log(event.clone());
        match event {
            DroneEvent::PacketDropped(_packet) => {
                self.side_panel_scenes = ManageDrop;
            }
            DroneEvent::ControllerShortcut(_packet) => {
                self.side_panel_scenes = ManageShortcut;
            }
            _ => {}
        }
    }

    pub fn find_node_type(&self, id: &NodeId) -> Option<NodeType> {
        self.sim_contr.network_graph.get(id).map(|(node_type, _)| node_type.clone())
    }

}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        //setting this true assure you keep reading from SC, retest wont work (but you can delete it)
        let enable_constant_read = true;
        if enable_constant_read {
            self.update_topology();
        }

        // BottomPanel ridimensionabile
        egui::TopBottomPanel::bottom("bottom_panel")
            .height_range(100.0..=300.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Simulation Control Log:").font(FontId::proportional(14.0)),
                    );
                });
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2]) // Ensures it doesn't shrink horizontally or vertically
                        .show(ui, |ui| {
                            for s in &self.sim_contr.log {
                                ui.label(format!("{}", s));
                            }
                        });
                });
            });

        // SidePanel sulla sinistra
        egui::SidePanel::left("side_panel")
            .resizable(true)
            .min_width(300.0)
            .show(ctx, |ui| {
                ui.heading("Actions");
                match self.side_panel_scenes {
                    Start => {
                        if ui.button("test log!").clicked() {
                            self.sim_contr.log.push_back(LogEntry::new(
                                Cause::Sent,
                                fastrand::u8(0..10),
                                "ciao".to_string(),
                            ));
                        }
                        if ui.button("Add Drone!").clicked() {
                            self.side_panel_scenes = ManageAdd;
                        }
                        if ui.button("remove Drone!").clicked() {
                            self.side_panel_scenes = ManageCrash;
                        }

                        // for testing
                        if ui.button("Test Drop").clicked() {
                            let msg = create_packet(vec![0, 1, 8]);
                            let drop = PacketDropped(msg);
                            match self.sim_contr.channel_for_drone.try_send(drop) {
                                Ok(_) => {
                                    println!("sent dropping")
                                }
                                Err(_) => {
                                    println!("error dropping packet");
                                }
                            }
                        }
                        if ui.button("Test Shortcut").clicked() {
                            let msg = create_packet(vec![0, 1, 8]);
                            let cs_shortcut = ControllerShortcut(msg);
                            match self.sim_contr.channel_for_drone.try_send(cs_shortcut) {
                                Ok(_) => {
                                    println!("sent through shortcut")
                                }
                                Err(_) => {
                                    println!("error through shortcut");
                                }
                            }
                        }
                    }
                    ManageAdd => {
                        if ui.button("back").clicked() {
                            self.side_panel_scenes = Start;
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
                                .filter_map(|(i, &is_checked)| {
                                    if is_checked {
                                        Some(self.nodes[i].id)
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            add_node(&checked_indices, self.pdr);
                            self.reset_check();
                            self.side_panel_scenes = Start;
                        }
                    }
                    ManageCrash => {
                        ui.separator();
                        ui.label("select drones to crash:");
                        ui.separator();
                        for (i, item) in self.nodes.iter().enumerate() {
                            ui.checkbox(&mut self.checked[i], item.id.to_string());
                        }

                        if ui.button("Confirm").clicked() {
                            let checked_indices: Vec<NodeId> = self
                                .checked
                                .iter()
                                .enumerate()
                                .filter_map(|(i, &is_checked)| {
                                    if is_checked {
                                        Some(self.nodes[i].id)
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            //add_node(&checked_indices);
                            for node_id in checked_indices {
                                self.sim_contr.crash_drone(node_id);
                                self.nodes.retain(|item| item.id != node_id);

                            }
                            self.reset_check();
                            self.side_panel_scenes = Start;
                        }
                    }
                    ManageDrop => {
                        ui.label("Packet has been dropped!");
                        // Attempt to find the last dropped packet in the log
                        if let Some(dropped_packet) = self
                            .sim_contr
                            .log
                            .iter()
                            .rev()
                            .find(|item| matches!(item.cause, Cause::Dropped))
                        {
                            // Display the dropped packet
                            ui.label(format!("Here is the packet: {}", dropped_packet));

                            // Options for handling the packet
                            if ui.button("Resend it").clicked() {
                                // TODO: Implement packet resend logic
                            }
                            if ui.button("Lose it").clicked() {
                                self.side_panel_scenes = Start; // Navigate back to the start
                            }
                        } else {
                            // Inform the user if recovery is not possible
                            ui.label("Impossible to recover the packet.");
                            if ui.button("Close").clicked() {
                                self.side_panel_scenes = Start; // Close the alert
                            }
                        }
                    }
                    ManageShortcut => {
                        //todo not sure wtf a shortcut does

                        if ui.button("Close").clicked() {
                            self.side_panel_scenes = Start; // Close the alert
                        }
                    }
                }
            });

        for node in self.nodes.iter_mut() {
            if node.selected {
                match node.node_type {
                    NodeType::Drone => {egui::Window::new(format!("Drone {}", node.id))
                        .resizable(true) // Permetti il ridimensionamento
                        .collapsible(true)
                        .min_height(500.0)
                        .min_width(500.0)
                        .show(ctx, |ui| {
                            match self.drone_window_scenes {
                                DroneWindowScene::Start => {
                                    // Qui puoi aggiungere ulteriori informazioni o controlli
                                    ui.label("Log:");
                                    ui.vertical(|ui| {
                                        for s in &self.sim_contr.log {
                                            if s.get_id() == node.id {
                                                ui.label(format!("{}", s));
                                            }
                                        }
                                    });
                                    //insert log of the drone (idk how)
                                    if ui.button("Add Sender").clicked(){
                                        self.drone_window_scenes = AddSender
                                    }

                                    if ui.button("Close").clicked() {
                                        node.selected = false; // Chiudi il popup
                                    }
                                }
                                DroneWindowScene::AddSender => {

                                }
                                DroneWindowScene::RemoveSender => {}
                                DroneWindowScene::Crash => {}
                                DroneWindowScene::SetPDR => {}
                            }

                        });
                    }
                    NodeType::Client => { {egui::Window::new(format!(" Client {}", node.id))
                        .resizable(true) // Permetti il ridimensionamento
                        .collapsible(true)
                        .min_height(500.0)
                        .min_width(500.0)
                        .show(ctx, |ui| {

                            // Qui puoi aggiungere ulteriori informazioni o controlli
                            ui.label("Log:");
                            ui.vertical(|ui| {
                                for s in &self.sim_contr.log {
                                    if s.get_id() == node.id {
                                        ui.label(format!("{}", s));
                                    }
                                }
                            });
                            //insert log of the drone (idk how)
                            if ui.button("Chiudi").clicked() {
                                node.selected = false; // Chiudi il popup
                            }
                        });
                    } }
                    NodeType::Server => { {egui::Window::new(format!("Server {}", node.id))
                            .resizable(true) // Permetti il ridimensionamento
                            .collapsible(true)
                            .min_height(500.0)
                            .min_width(500.0)
                            .show(ctx, |ui| {

                                // Qui puoi aggiungere ulteriori informazioni o controlli
                                ui.label("Log:");
                                ui.vertical(|ui| {
                                    for s in &self.sim_contr.log {
                                        if s.get_id() == node.id {
                                            ui.label(format!("{}", s));
                                        }
                                    }
                                });
                                //insert log of the drone (idk how)
                                if ui.button("Chiudi").clicked() {
                                    node.selected = false; // Chiudi il popup
                                }
                            });
                        } }
                }
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

                for (index, value) in self.nodes.iter_mut().enumerate() {
                    let rect =
                        egui::Rect::from_center_size(positions[index], egui::vec2(50.0, 50.0));
                    let response = ui.interact(rect, egui::Id::new(index), egui::Sense::click());

                    let circle_color = if value.selected {
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
                        FontId::proportional(16.0),
                        egui::Color32::WHITE,
                    );

                    // Gestisci il clic
                    if response.clicked() {
                        value.selected = true;
                        println!("Drone selezionato: {:?}", value.id);
                    }
                }
            });
        });

        match self.sim_contr.node_recv.try_recv() {
            Ok(event) => {
                //manage event
                self.manage_event(event);
            }
            Err(_) => {
                // println!("clearly not stucked");
            }
        }
    }
}

fn add_node(checked_indices: &Vec<NodeId>, pdr: f32) {
    // SimulationControl::spawn_node(&mut , pdr, checked_indices.clone());
}

pub fn run_sim_dan(sim_control: SimulationControl) -> Result<(), eframe::Error> {
    let mut options = eframe::NativeOptions::default();
    options.run_and_return = false;
    // options.viewport.fullscreen = Option::from(true);
    options.viewport.min_inner_size = Option::from(Vec2::new(1400.0, 800.0));
    eframe::run_native(
        "SkyLink Interface 1",
        options,
        Box::new(|_cc| Ok(Box::new(MyApp::new(sim_control)))),
    )
}

/*
feel free to update this list.
STARTING FROM THIS BASE, WHAT DO I HAVE TO DO:

STRICTLY FOR SIM APP PART:
- TEST WITH TESTBENCH LAST FUNCTION ALL THE POSSIBLE DRONE EVENTS, THAT COME FROM NACK, ACK, PACKET DROPPED...
0) add field in MyNodes that tell the Type of the Node (NodeType).
2) add in each pop up what type the node is (client/server)

the field node type is important also because the pop up has to have different buttons depending on the type:

//please help me here:
drone: crash? /...
client: send flood req / send message to (open a manage) / ..
server:...

--test everything, then continue with other things

6) make functions add_drone and remove drone that not only eliminate graphically the drones and connections, but also in the network saved in sim controll
7) add bottons in the pop ups for clients/servers that send flood req or certain messages
8) at the end, change the circles in drones/clients/server small entities, so you have to change the creation accordingly to nodetype (matches again)

(.. more to come)

 */
