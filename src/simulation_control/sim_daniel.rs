use crate::event_wrapper::Event;
use crate::sim_control::{Cause, LogEntry, SimulationControl};
use crate::simulation_control::sim_control::Cause::Error;
use crate::simulation_control::sim_daniel::NodeWindowScene::{AddSender, Crash, CreateMessage, ShowContacts, RemoveSender, SetPDR, Start};
use crate::simulation_control::sim_daniel::Scene::*;
use crate::test::test_bench::create_packet;
use eframe::egui;
use egui::{FontId, RichText, Vec2};
use std::cmp::{Ordering, PartialEq};
use std::collections::HashSet;
use std::process::Command;
use wg_2024::controller::DroneEvent::{ControllerShortcut, PacketDropped};
use wg_2024::network::NodeId;
use wg_2024::packet::NodeType::*;
use wg_2024::packet::PacketType::*;
use wg_2024::packet::{NodeType};
use crate::clients_gio::client_command::ClientEvent;
use crate::message::{ContentType, Message};
use crate::message::ChatRequest::{ClientList, Register, SendMessage};
use crate::simulation_control::sim_daniel::MessageScene::Id;

#[derive(Debug, Clone)]
pub struct MyNodes {
    id: NodeId,
    connections: HashSet<NodeId>,
    selected: bool,
    node_type: NodeType,
    node_window_scenes: NodeWindowScene,
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


pub struct MyMsg{
    dst_id: NodeId,
    session: u64,
    content:ContentType,
    msg_scene: MessageScene,
    input_text:String,
}

impl MyMsg{
    pub fn new() -> Self{
        Self{
            dst_id: 0,
            session: fastrand::u64(..300),
            content: Default::default(),
            msg_scene: Id,
            input_text: "Type here".to_string(),
        }
    }
}

pub enum Scene {
    InitialScene,
    ManageAdd,
    ManageCrash,
    ManageDrop,
}

pub enum MessageScene{
    Id,
    Content,
    AddInput,
    Send,
    Error,
}

#[derive(Clone, Debug)]
pub enum NodeWindowScene {
    //common between types
    Start,
    AddSender,
    RemoveSender,

    //drone scenes
    Crash,
    SetPDR,

    //client/server scenes
    ShowContacts,
    CreateMessage
}
pub struct MyApp {
    sim_contr: SimulationControl,
    nodes: Vec<MyNodes>,
    side_panel_scenes: Scene,
    checked: Vec<bool>,
    pdr: f32,
    sender_id: NodeId,

    msg: MyMsg,
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
                node_window_scenes: Start,
            });
            checked.push(false);
            selected_nodes.push(false);
        }

        let mut app = Self {
            nodes: vec,
            side_panel_scenes: InitialScene,
            checked,
            sim_contr,
            pdr: 0.0,
            sender_id: 0,
            msg: MyMsg::new(),
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
            .map(|x| (x.id, x.node_window_scenes.clone()))
            .collect::<Vec<(NodeId, NodeWindowScene)>>();

        //aggiornamento
        self.nodes.clear();
        let network_graph = self.sim_contr.network_graph.clone();
        for (node_id, neighbors) in network_graph {
            self.nodes.push(MyNodes{
                id: node_id,
                connections: neighbors.1,
                selected: false,
                node_type: neighbors.0,
                node_window_scenes: Start,
            })
        }

        //ripristino selected e dws

        for node in self.nodes.iter_mut() {
            for (id, dws) in id_to_window.iter() {
                if *id == node.id{
                    node.node_window_scenes = dws.clone();
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

    fn get_checked (&self) -> Vec<NodeId>{
        self
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
            .collect::<Vec<NodeId>>()
    }

    pub fn find_node_type(&self, id: &NodeId) -> Option<NodeType> {
        self.sim_contr.network_graph.get(id).map(|(node_type, _)| node_type.clone())
    }
    pub fn manage_event(&mut self, event: Event) {
        match event{
            Event::Drone(drone_event) => {
                self.sim_contr.add_drone_event_to_log(drone_event.clone());
                match drone_event {
                    PacketDropped(packet) => {
                        let dropper = packet.routing_header.current_hop().unwrap();
                        //println!("packet dropped by {dropper}"); debug printing
                        self.sim_contr.dropped_packets.push((dropper, packet));
                        self.side_panel_scenes = ManageDrop;
                    }
                    ControllerShortcut(packet) => {
                        match packet.clone().pack_type {
                            MsgFragment(_) => {
                                self.sim_contr.log.push_back(LogEntry::new(
                                    Error,
                                    packet.routing_header.hops[packet.routing_header.hop_index],
                                    "Shortcut used for unusual packet type: msgfragment".to_string()))
                            }
                            FloodRequest(_) => {
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
                                if *n_type == Drone {
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

                match client_event {
                    ClientEvent::SendContacts(src, dst) => {
                        self.sim_contr.add_contacts(src, dst);
                    }
                    _ => {/* degli altri niente */}
                }
            }
        }
    }

    pub fn render_bottom_panel(&self, ctx: &egui::Context) {
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
    }

    pub fn render_side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("side_panel")
            .resizable(true)
            .min_width(300.0)
            .show(ctx, |ui| {
                ui.heading("Actions");
                match self.side_panel_scenes {
                    InitialScene => {
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
                        if ui.button("Test Drop with 5").clicked() {
                            self.sim_contr.set_pdr(5, 100.0);

                            let msg = create_packet(vec![0,1,8,5,2]);
                            self.sim_contr.all_sender_packets.get(&1).unwrap().send(msg);
                        }

                        if ui.button("Test sending packet").clicked() {
                            let msg = create_packet(vec![0,1,8,5,2]);
                            self.sim_contr.all_sender_packets.get(&1).unwrap().send(msg);
                        }

                        if ui.button("Test flooding with 0").clicked() {
                            self.sim_contr.flood_with(0);
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


                        if ui.button("Clear Log").clicked() {
                            self.sim_contr.log.clear();
                        }
                    }
                    ManageAdd => {
                        if ui.button("back").clicked() {
                            self.side_panel_scenes = InitialScene;
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
                            let checked_indices: Vec<NodeId> = self.get_checked();
                            let _id = self.sim_contr.spawn_drone(self.pdr, checked_indices.clone()).1;
                            self.reset_check();
                            self.pdr = 0.0;
                            self.side_panel_scenes = InitialScene;
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
                            for node_id in self.get_checked() {
                                self.sim_contr.crash_drone(node_id);
                                self.nodes.retain(|item| item.id != node_id);

                            }
                            self.reset_check();
                            self.side_panel_scenes = InitialScene;
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
                                self.side_panel_scenes = InitialScene;
                            }
                            if ui.button("Lose it").clicked() {
                                self.side_panel_scenes = InitialScene; // Navigate back to the start
                            }
                        } else {
                            // Inform the user if recovery is not possible
                            ui.label("Impossible to recover the packet.");
                            if ui.button("Close").clicked() {
                                self.side_panel_scenes = InitialScene; // Close the alert
                            }
                        }
                    }
                }
            });
    }

    pub fn render_central_panel(&mut self, ctx: &egui::Context) {
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
                        println!("selected node: {:?}", value.id);
                    }
                }
            });
        });
    }


    pub fn render_nodes_windows(&mut self, ctx: &egui::Context) {
        for node in self.nodes.iter_mut() {
            if node.selected {
                match node.node_type {
                    Drone => {egui::Window::new(format!("Drone {}", node.id))
                        .resizable(true) // Permetti il ridimensionamento
                        .collapsible(true)
                        .min_width(500.0)
                        .max_height(400.0)
                        .show(ctx, |ui| {
                            match node.node_window_scenes {
                                Start => {
                                    let mut connections= String::new();
                                    for connection in node.connections.clone() {
                                        connections.push_str(connection.to_string().as_str());
                                        connections.push_str(", ");
                                    }
                                    ui.label(format!("Connected to :{}", connections));


                                    if ui.button("Add Channel").clicked(){
                                        node.node_window_scenes = AddSender;
                                    }

                                    if ui.button("Remove Channel").clicked(){
                                        node.node_window_scenes = RemoveSender
                                    }

                                    if ui.button("Crash This Drone").clicked(){
                                        node.node_window_scenes = Crash
                                    }

                                    if ui.button("set PDR").clicked(){
                                        node.node_window_scenes = SetPDR
                                    }

                                    if ui.button("Close").clicked() {
                                        node.selected = false; // Chiudi il popup
                                    }


                                    // Qui puoi aggiungere ulteriori informazioni o controlli
                                    self.sender_id = 0;
                                    ui.label("Log:");
                                    ui.vertical(|ui| {
                                        egui::ScrollArea::vertical()
                                            .auto_shrink([false; 2]) // Ensures it doesn't shrink horizontally or vertically
                                            .show (ui, |ui|
                                                for s in &self.sim_contr.log {
                                                    if s.get_id() == node.id {
                                                        ui.label(format!("{}", s));
                                                    }
                                                }
                                            )
                                    });
                                }
                                AddSender => {
                                    ui.horizontal(|ui| {
                                        ui.label("Add Channel With Drone:");
                                        ui.add(egui::DragValue::new(&mut self.sender_id));

                                    }
                                    );

                                    if ui.button("Confirm").clicked() {

                                        self.sim_contr.add_sender(node.id, self.sender_id);
                                        node.node_window_scenes = Start;
                                    }
                                    if ui.button("back").clicked(){
                                        node.node_window_scenes = Start;
                                    }
                                }
                                RemoveSender => {
                                    ui.horizontal(|ui| {
                                        ui.label("Remove Channel With Drone:");
                                        ui.add(egui::DragValue::new(&mut self.sender_id))
                                    });

                                    if ui.button("Confirm").clicked() {

                                        self.sim_contr.remove_senders(node.id, self.sender_id);
                                        node.node_window_scenes = Start;
                                    }
                                    if ui.button("back").clicked(){
                                        node.node_window_scenes = Start;
                                    }
                                }
                                Crash => {
                                    ui.label("Are you sure you want to crash this drone?");
                                    if ui.button("yes, crash").clicked(){
                                        self.sim_contr.crash_drone(node.id);
                                        node.selected = false;
                                    }
                                    if ui.button("no, go back").clicked(){
                                        node.node_window_scenes = Start;
                                    }
                                }
                                SetPDR => {
                                    ui.horizontal(|ui| {
                                        ui.label("insert PDR:");
                                        ui.add(egui::DragValue::new(&mut self.pdr).speed(0.1));
                                    });

                                    if ui.button("Set").clicked() {

                                        self.sim_contr.set_pdr(node.id, self.pdr);
                                        node.node_window_scenes = Start;
                                    }
                                    if ui.button("Back").clicked(){
                                        self.pdr = 0.0;
                                        node.node_window_scenes = Start;
                                    }
                                }

                                _ => {
                                    //since the others are for client and servers only
                                    unreachable!()}
                            }

                        });
                    }
                    Client => {
                        egui::Window::new(format!("Client {}", node.id))
                            .resizable(true) // Allow resizing
                            .collapsible(true)
                            .min_width(500.0)
                            .default_size((500.0, 400.0)) // Set default size
                            .default_pos((100.0, 100.0)) // Set default position
                            .show(ctx, |ui| {
                                ui.push_id(format!("client_window_{}", node.id), |ui| {
                                    egui::Frame::default()
                                        .fill(egui::Color32::BLACK) // Set the frame's background color
                                        .stroke(egui::Stroke::new(1.0, egui::Color32::BLACK)) // Add a border
                                        .inner_margin(egui::Margin::symmetric(10.0, 10.0)) // Optional padding
                                        .show(ui, |ui| {
                                            // Split panels with proper layout
                                            egui::SidePanel::left(format!("side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    ui.label("Log:");

                                                    egui::ScrollArea::vertical()
                                                        .auto_shrink([false; 2]) // Prevent shrinking
                                                        .show(ui, |ui| {
                                                            for s in &self.sim_contr.log {
                                                                if s.get_id() == node.id {
                                                                    ui.label(format!("{}", s));
                                                                }
                                                            }
                                                        });
                                                });

                                            egui::SidePanel::right(format!("right_side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    let mut connections = String::new();
                                                    for connection in node.connections.clone() {
                                                        connections.push_str(connection.to_string().as_str());
                                                        connections.push_str(", ");
                                                    }

                                                    ui.label(format!("Connected to: {}", connections));

                                                    if ui.button("Add Channel").clicked(){
                                                        node.node_window_scenes = AddSender;
                                                    }

                                                    if ui.button("Remove Channel").clicked(){
                                                        node.node_window_scenes = RemoveSender
                                                    }

                                                    if ui.button("Flood").clicked(){
                                                        self.sim_contr.flood_with(node.id);
                                                        node.node_window_scenes = ShowContacts;
                                                    }

                                                    if ui.button("Show Contact").clicked(){
                                                        node.node_window_scenes = ShowContacts;
                                                    }

                                                    if ui.button("Test Message").clicked(){
                                                        node.node_window_scenes = CreateMessage;
                                                        self.msg = MyMsg::new();
                                                    }

                                                    if ui.button("Chiudi").clicked() {
                                                        node.selected = false; // Close the window
                                                    }
                                                });

                                            egui::CentralPanel::default()
                                                .show_inside(ui, |ui| {
                                                    match node.node_window_scenes{
                                                        Start => {
                                                            ui.label("This is the central panel content.");
                                                            ui.label("flood results and chat shits will be here.");
                                                        }
                                                        AddSender => {}
                                                        RemoveSender => {},
                                                        ShowContacts => {
                                                            //questo fammici pensare, se hai idee scrivi pure.
                                                            //l'idea sarebbe se sono in questo stato displayio i nodi che il client/server può raggiungere con un mex

                                                            let node_contacts = match self.sim_contr.contacts.get(&node.id){
                                                                Some(contacts) => contacts.clone(),
                                                                None => HashSet::new()
                                                            };

                                                            let mut contacts = String::new();
                                                            for node in node_contacts {
                                                                contacts.push_str("\n ");
                                                                contacts.push_str(node.to_string().as_str());
                                                            }

                                                            ui.label(format!("MyContacts are: {}", contacts));

                                                            if ui.button("Chiudi").clicked() {
                                                                node.node_window_scenes = Start; // Close the window
                                                            }
                                                        }

                                                        CreateMessage =>{
                                                            match self.msg.msg_scene {
                                                                Id => {
                                                                    let ids = match self.sim_contr.contacts.get(&node.id) {
                                                                        Some(contacts) => { contacts.into_iter().collect()},
                                                                        None => {
                                                                            self.msg.msg_scene = MessageScene::Error;
                                                                            Vec::new()
                                                                        }
                                                                    };
                                                                    ui.label("select drones to contact:");


                                                                    for id in ids{
                                                                        if ui.button(id.to_string()).clicked() {
                                                                            self.msg.dst_id = *id;
                                                                            self.msg.msg_scene = MessageScene::Content;
                                                                            println!("{}", self.msg.dst_id);
                                                                        }
                                                                    }

                                                                },
                                                                MessageScene::Content => {
                                                                    ui.label("select message type:");

                                                                    if ui.button("ClientList").clicked() {
                                                                        self.msg.content = ContentType::ChatRequest(ClientList);
                                                                        self.msg.msg_scene = MessageScene::Send;
                                                                    }
                                                                    if ui.button("Register").clicked() {
                                                                        self.msg.content = ContentType::ChatRequest(Register(node.id));
                                                                        self.msg.msg_scene = MessageScene::Send;

                                                                    }
                                                                    if ui.button("SendMessage").clicked() {
                                                                        self.msg.msg_scene = MessageScene::AddInput;
                                                                    }
                                                                }
                                                                MessageScene::AddInput => {
                                                                    ui.label("Enter a message:");
                                                                    let response = ui.add(egui::TextEdit::singleline(&mut self.msg.input_text));
                                                                    if response.lost_focus() {
                                                                        // Handle Enter key press
                                                                        self.msg.content = ContentType::ChatRequest(SendMessage {
                                                                            from: node.id,
                                                                            to: self.msg.dst_id,
                                                                            message: self.msg.input_text.clone(),
                                                                        });
                                                                        self.msg.msg_scene = MessageScene::Send;
                                                                    }
                                                                }
                                                                MessageScene::Send =>{
                                                                    if ui.button("Send").clicked() {
                                                                        let msg = Message::new(node.id, self.msg.session, self.msg.content.clone());
                                                                        self.sim_contr.force_send_message(node.id, Client, msg);
                                                                        node.node_window_scenes = Start; // Close the window
                                                                    }
                                                                }
                                                                MessageScene::Error => {
                                                                    ui.label("You dont have contacts: did you Flood?");
                                                                }
                                                            }
                                                            if ui.button("Close").clicked() {
                                                                self.msg = MyMsg::new();
                                                                node.node_window_scenes = Start; // Close the window
                                                            }
                                                        }
                                                        _ => {
                                                            unreachable!()
                                                        }
                                                    }
                                                });
                                        });
                                });
                            });
                    }
                    Server => {
                        egui::Window::new(format!("Server {}", node.id))
                            .resizable(true) // Allow resizing
                            .collapsible(true)
                            .min_width(500.0)
                            .default_size((500.0, 400.0)) // Set default size
                            .default_pos((100.0, 100.0)) // Set default position
                            .show(ctx, |ui| {
                                ui.push_id(format!("server_window_{}", node.id), |ui| {
                                    egui::Frame::default()
                                        .fill(egui::Color32::BLACK) // Set the frame's background color
                                        .stroke(egui::Stroke::new(1.0, egui::Color32::BLACK)) // Add a border
                                        .inner_margin(egui::Margin::symmetric(10.0, 10.0)) // Optional padding
                                        .show(ui, |ui| {
                                            // Split panels with proper layout
                                            egui::SidePanel::left(format!("side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    ui.label("Log:");

                                                    egui::ScrollArea::vertical()
                                                        .auto_shrink([false; 2]) // Prevent shrinking
                                                        .show(ui, |ui| {
                                                            for s in &self.sim_contr.log {
                                                                if s.get_id() == node.id {
                                                                    ui.label(format!("{}", s));
                                                                }
                                                            }
                                                        });
                                                });

                                            egui::SidePanel::right(format!("right_side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    let mut connections = String::new();
                                                    for connection in node.connections.clone() {
                                                        connections.push_str(connection.to_string().as_str());
                                                        connections.push_str(", ");
                                                    }

                                                    ui.label(format!("Connected to: {}", connections));

                                                    if ui.button("Add Channel").clicked(){
                                                        node.node_window_scenes = AddSender;
                                                    }

                                                    if ui.button("Remove Channel").clicked(){
                                                        node.node_window_scenes = RemoveSender
                                                    }

                                                    if ui.button("Chiudi").clicked() {
                                                        node.selected = false; // Close the window
                                                    }
                                                });

                                            egui::CentralPanel::default()
                                                .show_inside(ui, |ui| {
                                                    ui.label("This is the central panel content.");
                                                    ui.label("flood results and chat shits will be here.");


                                                    match node.node_window_scenes{
                                                        Start => {}
                                                        AddSender => {}
                                                        RemoveSender => {}
                                                        ShowContacts => {
                                                            //questo fammici pensare, se hai idee scrivi pure.
                                                            //l'idea sarebbe se sono in questo stato displayio i nodi che il client/server può raggiungere con un mex
                                                        }
                                                        CreateMessage => {}
                                                        _ => {}
                                                    }
                                                });
                                        });
                                });
                            });
                    }
                }
            }
        }
    }

    pub fn enable_constant_read(&mut self) {
        //setting this true assure you keep reading from SC, retest wont work (but you can delete it)
        let enable_constant_read = true;
        if enable_constant_read {
            self.update_topology();
        }
    }

    pub fn update_event_receivers(&mut self) {
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
impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.enable_constant_read();
        self.render_bottom_panel(ctx);
        self.render_side_panel(ctx);
        self.render_nodes_windows(ctx);
        self.render_central_panel(ctx);
        self.update_event_receivers();
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

- se vuoi anche fare un "render drone window" "render client"..., insomma spezzare il match alla riga 493 in tre funzioni (che prenderanno sia ctx che anche il nodo stesso)

- ho cambiato le dronewindowscene in nodewindowscene, quello che dovresti fare è aggiungere come hai fatto con i droni le "common" scene (tipo add sender, remove sender..).
il match io lo metterei nel central panel che se vedi ho lasciato da fare. ma poi fai tu

se hai altre idee di scene dimmelo

- cambiare i cerchi in droni

(..)


 */
