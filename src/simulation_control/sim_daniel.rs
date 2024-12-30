use std::any::Any;
use crate::sim_control::{Cause, LogEntry, SimulationControl};
use crate::simulation_control::sim_daniel::Scene::*;
use crate::test::test_bench::create_packet;
use eframe::egui;
use egui::{FontId, RichText, Vec2};
use std::cmp::{Ordering, PartialEq};
use std::ops::Deref;
use wg_2024::controller::DroneEvent;
use wg_2024::controller::DroneEvent::{ControllerShortcut, PacketDropped};
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, PacketType};
use wg_2024::packet::NodeType::*;
use crate::clients_gio::client_command::ClientEvent;
use crate::event_wrapper::Event;
use crate::server::server_command::ServerEvent;
use crate::simulation_control::sim_control::Cause::Error;
use crate::simulation_control::sim_daniel::DroneWindowScene::{AddSender, Crash, RemoveSender, SetPDR};

#[derive(Debug, Clone)]
pub struct MyNodes {
    id: NodeId,
    connections: Vec<NodeId>,
    selected: bool,
    node_type: NodeType,
    drone_window_scenes: DroneWindowScene,
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
}

#[derive(Clone, Debug)]
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
    checked: Vec<bool>,
    pdr: f32,
    sender_id: NodeId,
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
                drone_window_scenes: DroneWindowScene::Start,
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

        let id_to_window = self
            .nodes
            .iter()
            .map(|x| (x.id, x.drone_window_scenes.clone()))
            .collect::<Vec<(NodeId, DroneWindowScene)>>();

        //aggiornamento
        self.nodes.clear();
        let network_graph = self.sim_contr.network_graph.clone();
        for (node_id, neighbors) in network_graph {
            self.nodes.push(MyNodes{
                id: node_id,
                connections: neighbors.1,
                selected: false,
                node_type: neighbors.0,
                drone_window_scenes: DroneWindowScene::Start,
            })
        }

        //ripristino selected e dws

        for node in self.nodes.iter_mut() {
            for (id, dws) in id_to_window.iter() {
                if *id == node.id{
                    node.drone_window_scenes = dws.clone();
                }

            }
        }

        for node in self.nodes.iter_mut() {
            for (id, selection) in id_to_selected.iter() {
                if *id == node.id {
                    node.selected = selection.clone();
                }

            }
        }
    }

    fn reset_check(&mut self) {
        self.checked.clear();
        for _ in 0..self.nodes.len() + 1 {
            self.checked.push(false);
        }
    }

    pub fn find_node_type(&self, id: &NodeId) -> Option<NodeType> {
        self.sim_contr.network_graph.get(id).map(|(node_type, _)| node_type.clone())
    }

    pub fn create_drone_id_vector(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = Vec::new();
        for drone in self.nodes.iter() {
            if drone.node_type == Drone{
                ids.push(drone.id);
            }
        }
        ids
    }

    pub fn manage_event(&mut self, event: Event) {
        match event{
            Event::Drone(drone_event) => {
                self.sim_contr.add_drone_event_to_log(drone_event.clone());
                match drone_event {
                    PacketDropped(packet) => {
                        let source_id = packet.routing_header.hops[packet.routing_header.hop_index];
                        self.sim_contr.dropped_packets.push((source_id, packet));
                        self.side_panel_scenes = ManageDrop;
                    }
                    ControllerShortcut(packet) => {
                        match packet.clone().pack_type {
                            PacketType::MsgFragment(_) => {
                                self.sim_contr.log.push_back(LogEntry::new(
                                    Error,
                                    packet.routing_header.hops[packet.routing_header.hop_index],
                                    "Shortcut used for unusual packet type: msgfragment".to_string()))
                            }
                            PacketType::FloodRequest(_) => {
                                self.sim_contr.log.push_back(LogEntry::new(
                                    Error,
                                    packet.routing_header.hops[packet.routing_header.hop_index],
                                    "Shortcut used for unusual packet type: floodrequest".to_string()))
                            }
                            _ => {
                                let next_id = packet.routing_header.hops[packet.routing_header.hops.len() - 1];

                                let sender = match self.sim_contr.all_sender_packets.get(&next_id) {
                                    None => {
                                        self.sim_contr.log.push_back(LogEntry::new(
                                            Error,
                                            next_id,
                                            format!("error in sendig packet to {} through shortcut (packet not present)", next_id),
                                        ));
                                        return;
                                    },
                                    Some(sender) => {
                                        sender
                                    }
                                };

                                let (n_type , _) = self.sim_contr.network_graph.get(&next_id).unwrap();
                                if (*n_type == NodeType::Drone){
                                    self.sim_contr.log.push_back(LogEntry::new(
                                        Error,
                                        next_id,
                                        format!("error in sending packet to {} through shortcut (final destination is drone)", next_id),
                                    ));
                                    return;
                                }

                                match sender.try_send(packet){
                                    Ok(_) => {
                                        self.sim_contr.log.push_back(LogEntry::new(
                                            Cause::Sent,
                                            next_id,
                                            format!("shortcut redirected successfully to {} through shortcut ", next_id),
                                        ));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::Server(server_event) => {
                self.sim_contr.add_server_event_to_log(server_event.clone());
            }
            Event::Client(client_event) => {
                self.sim_contr.add_client_event_to_log(client_event.clone());
            }
        }
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
            .height_range(100.0..=400.0)
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

                        if ui.button("Test sending packet").clicked() {
                            let msg = create_packet(vec![4,1,8,5,2]);

                            self.sim_contr.all_sender_packets.get(&1).unwrap().send(msg);

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
                        self.nodes.sort();
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
                            let _id = self.sim_contr.spawn_drone(self.pdr, checked_indices.clone()).1;
                            self.reset_check();
                            self.pdr = 0.0;
                            self.side_panel_scenes = Start;
                        }
                    }
                    ManageCrash => {
                        ui.separator();
                        ui.label("select drones to crash:");
                        ui.separator();
                        for (i, item) in self.nodes.iter().enumerate() {
                            if item.node_type == Drone {
                                ui.checkbox(&mut self.checked[i], item.id.to_string());
                            }
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
                        if let Some((id,dropped_packet)) = self
                            .sim_contr
                            .dropped_packets
                            .last()
                        {
                            // Display the dropped packet
                            ui.label(format!("{id} dropped packet: {}", dropped_packet));

                            // Options for handling the packet
                            if ui.button("Resend it").clicked() {
                                self.sim_contr.resend_packet(dropped_packet);
                                self.side_panel_scenes = Start;
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
                }
            });

        for node in self.nodes.iter_mut() {
            if node.selected {
                match node.node_type {
                    Drone => {egui::Window::new(format!("Drone {}", node.id))
                        .resizable(true) // Permetti il ridimensionamento
                        .collapsible(true)
                        .min_height(500.0)
                        .min_width(500.0)
                        .show(ctx, |ui| {
                            match node.drone_window_scenes {
                                DroneWindowScene::Start => {
                                    let mut connections= String::new();
                                    for connection in node.connections.clone() {
                                        connections.push_str(connection.to_string().as_str());
                                        connections.push_str(", ");
                                    }
                                    ui.label(format!("Connected to :{}", connections));
                                    // Qui puoi aggiungere ulteriori informazioni o controlli
                                    self.sender_id = 0;
                                    ui.label("Log:");
                                    ui.vertical(|ui| {
                                        for s in &self.sim_contr.log {
                                            if s.get_id() == node.id {
                                                ui.label(format!("{}", s));
                                            }
                                        }
                                    });
                                    //insert log of the drone (idk how)
                                    if ui.button("Add Channel").clicked(){
                                        node.drone_window_scenes = AddSender;
                                    }

                                    if ui.button("Remove Channel").clicked(){
                                        node.drone_window_scenes = RemoveSender
                                    }

                                    if ui.button("Crash This Drone").clicked(){
                                        node.drone_window_scenes = Crash
                                    }

                                    if ui.button("set PDR").clicked(){
                                        node.drone_window_scenes = SetPDR
                                    }

                                    if ui.button("Close").clicked() {
                                        node.selected = false; // Chiudi il popup
                                    }
                                }
                                AddSender => {
                                    ui.horizontal(|ui| {
                                        ui.label("Add Channel With Drone:");
                                        ui.add(egui::DragValue::new(&mut self.sender_id));

                                    }
                                    );

                                    if ui.button("Confirm").clicked() {

                                        self.sim_contr.add_sender(node.id, self.sender_id);
                                        node.drone_window_scenes = DroneWindowScene::Start;
                                    }
                                    if ui.button("back").clicked(){
                                        node.drone_window_scenes = DroneWindowScene::Start;
                                    }
                                }
                                RemoveSender => {
                                    ui.horizontal(|ui| {
                                        ui.label("Remove Channel With Drone:");
                                        ui.add(egui::DragValue::new(&mut self.sender_id))
                                    });

                                    if ui.button("Confirm").clicked() {

                                        self.sim_contr.remove_senders(node.id, self.sender_id);
                                        node.drone_window_scenes = DroneWindowScene::Start;
                                    }
                                    if ui.button("back").clicked(){
                                        node.drone_window_scenes = DroneWindowScene::Start;
                                    }
                                }
                                Crash => {
                                    ui.label("Are you sure you want to crash this drone?");
                                    if ui.button("yes, crash").clicked(){
                                        self.sim_contr.crash_drone(node.id);
                                        node.selected = false;
                                    }
                                    if ui.button("no, go back").clicked(){
                                        node.drone_window_scenes = DroneWindowScene::Start;
                                    }
                                }
                                SetPDR => {
                                    ui.horizontal(|ui| {
                                        ui.label("insert PDR:");
                                        ui.add(egui::DragValue::new(&mut self.pdr).speed(0.1));
                                    });

                                    if ui.button("Set").clicked() {

                                        self.sim_contr.set_pdr(node.id, self.pdr);
                                        node.drone_window_scenes = DroneWindowScene::Start;
                                    }
                                    if ui.button("Back").clicked(){
                                        self.pdr = 0.0;
                                        node.drone_window_scenes = DroneWindowScene::Start;
                                    }
                                }
                            }
                        });
                    }
                    NodeType::Client => { {egui::Window::new(format!(" Client {}", node.id))
                        .resizable(true) // Permetti il ridimensionamento
                        .collapsible(true)
                        .min_height(500.0)
                        .min_width(500.0)
                        .show(ctx, |ui| {

                            let mut connections= String::new();
                            for connection in node.connections.clone() {
                                connections.push_str(connection.to_string().as_str());
                            }
                            ui.label(format!("Connected to :{}", connections));

                            // Qui puoi aggiungere ulteriori informazioni o controlli
                            ui.label("Log:");
                            ui.vertical(|ui| {
                                for s in &self.sim_contr.log {
                                    if s.get_id() == node.id {
                                        ui.label(format!("{}", s));
                                    }
                                }
                            });

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

                                let mut connections= String::new();
                                for connection in node.connections.clone() {
                                    connections.push_str(connection.to_string().as_str());
                                }
                                ui.label(format!("Connected to :{}", connections));

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

        match self.sim_contr.drone_event_recv.try_recv() {
            Ok(event) => {
                //manage event

                self.manage_event(Event::Drone(event));
            }
            _ => {}
        }

        match self.sim_contr.client_event_recv.try_recv() {
            Ok(event) => {
                //manage event
                self.manage_event(Event::Client(event));
            }
            _ => {}
        }

        match self.sim_contr.server_event_recv.try_recv() {
            Ok(event) => {
                //manage event
                self.manage_event(Event::Server(event));
            }
            _ => {}
        }
    }
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
