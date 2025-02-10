use crate::clients_gio::client_command::ClientEvent;
use crate::sim_control::{Cause, LogEntry, SimulationControl};
use crate::simulation_control::sim_control::Cause::Error;
use crate::simulation_control::sim_daniel::ContentIdentifier::{Chat, Media, MediaToResolve, RegisterOrList, TextList};
use crate::simulation_control::sim_daniel::NodeWindowScene::{AddSender, Crash, RemoveSender, SetPDR, ShowAuxiliaryLists, ShowContents, ShowDestinations, Start};
use crate::simulation_control::sim_daniel::Scene::*;
use eframe::egui;
use egui::{Color32, FontId, RichText, TextureHandle, Vec2};
use std::cmp::{Ordering, PartialEq};
use std::collections::{HashMap, HashSet};
use std::vec;
use wg_2024::controller::DroneEvent::{ControllerShortcut, PacketDropped};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Fragment, NodeType, Packet, PacketType};
use wg_2024::packet::NodeType::*;
use egui_plot::{Bar, BarChart, Plot};
use wg_2024::controller::DroneEvent;
use crate::DEBUG_MODE;
use crate::server::server_command::ServerEvent;

#[derive(Clone)]
pub struct MyNodes {
    id: NodeId,
    connections: HashSet<NodeId>,
    selected: bool,
    node_type: NodeNature,
    node_window_scenes: NodeWindowScene,
    content: Option<ContentIdentifier>,
    input_text: String,
    texture: Option<TextureHandle>,
    notify: bool,
}


#[derive(Clone, PartialEq)]
pub enum ContentIdentifier{
    Chat(NodeId), //Node id of destination
    RegisterOrList(NodeId),
    TextList(NodeId),
    MediaToResolve, // id
    Media(TextureHandle), // content
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

#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum NodeNature{ //just more complex NodeType
    Drone,
    ChatClient,
    WebBrowser,
    ChatServer,
    TextServer,
    MediaServer,
}
impl NodeNature{
    pub fn simple_type(&self) -> NodeType{
        match self{
            NodeNature::Drone => {Drone}
            NodeNature::ChatClient => {Client}
            NodeNature::WebBrowser => {Client}
            NodeNature::ChatServer => {Server}
            NodeNature::TextServer => {Server}
            NodeNature::MediaServer => {Server}
        }
    }
}

pub enum Scene {
    InitialScene,
    ManageAdd,
    ManageCrash,
    Statistics,
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

    //client scenes
    ShowAuxiliaryLists, //for a chat client will be the clients to which he is registered, for a web browser the text file he can resolve!
    ShowContents, //for a chat client will be chats, for webclient will be medias
    ShowDestinations,   // for a chat client are chat server available(to register ecc..), for a web clint are text server available
}
pub struct MyApp {
    sim_contr: SimulationControl,
    nodes: Vec<MyNodes>,
    side_panel_scenes: Scene,
    selected_drones: Vec<bool>,
    pdr: f32,
    sender_id: NodeId,
    circle_mode: bool,
    sort: bool,
    dropper: Option<NodeId>,
    logo: Option<TextureHandle>,
}



impl MyApp {
    pub(crate) fn new(sim_contr: SimulationControl) -> Self {
        let network_graph = sim_contr.network_graph.clone();

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
                content: None,
                input_text: "".to_string(),
                texture: None,
                notify: false,
            });
            checked.push(false);
            selected_nodes.push(false);
        }
        let mut app = Self {
            nodes: vec,
            side_panel_scenes: InitialScene,
            selected_drones: checked,
            sim_contr,
            pdr: 0.0,
            sender_id: 0,
            circle_mode: true,
            sort: false,
            dropper: None,
            logo: None,
        };
        app
    }

    pub fn update_topology(&mut self) {
        // Create a single HashMap for storing the node data
        let id_to_data: HashMap<_, _> = self
            .nodes
            .iter()
            .map(|x| {(x.id, (x.selected, x.node_window_scenes.clone(), x.content.clone(), x.input_text.clone(), x.texture.clone(), x.notify))})
            .collect();

        // Clear and rebuild nodes
        self.nodes.clear();
        let network_graph = self.sim_contr.network_graph.clone();
        for (node_id, (node_type, connections)) in network_graph {
            let data = id_to_data.get(&node_id).cloned().unwrap_or((false,Start, None, "".to_string(), None, false));

            self.nodes.push(MyNodes {
                id: node_id,
                connections,
                selected: data.0,
                node_type,
                node_window_scenes: data.1,
                content: data.2,
                input_text: data.3,
                texture: data.4,
                notify: data.5,
            });
        }
    }

    fn reset_check(&mut self) {
        self.selected_drones.clear();
        for _ in 0..self.nodes.len() + 1 {
            self.selected_drones.push(false);
        }
    }

    fn get_checked (&self) -> Vec<NodeId>{
        self
            .selected_drones
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
        self.sim_contr.network_graph.get(id).map(|(node_type, _)| node_type.simple_type())
    }

    pub fn turn_on_notification(&mut self, id: NodeId){
        for x in self.nodes.iter_mut() {
            if x.id == id{
                x.notify = true;
            }
        }
    }

    pub fn manage_drone_event(&mut self, drone_event:DroneEvent){
        self.sim_contr.add_drone_event_to_log(drone_event.clone());
        match drone_event {
            PacketDropped(packet) => {
                let dropper = packet.routing_header.current_hop().unwrap();
                let e = self.sim_contr.storage.dropped_packets.entry(dropper).or_insert(vec![]);
                e.push(packet);
            }
            ControllerShortcut(packet) => {
                match packet.clone().pack_type {
                    PacketType::MsgFragment(_) => {
                        self.sim_contr.log.push_back(LogEntry::new(
                            Error,
                            packet.routing_header.hops[packet.routing_header.hop_index],
                            "Shortcut used for unusual packet type: fragment".to_string()))
                    }
                    PacketType::FloodRequest(_) => {
                        self.sim_contr.log.push_back(LogEntry::new(
                            Error,
                            packet.routing_header.hops[packet.routing_header.hop_index],
                            "Shortcut used for unusual packet type: flood request".to_string()))
                    }
                    _ => {
                        let next_id = packet.routing_header.hops[packet.routing_header.hops.len() - 1];

                        let sender = match self.sim_contr.all_sender_packets.get(&next_id) {
                            None => {
                                self.sim_contr.log.push_back(LogEntry::new(
                                    Error,
                                    next_id,
                                    format!("error in sending packet to {} through shortcut (packet not present)", next_id),
                                ));
                                return;
                            },
                            Some(sender) => {
                                sender
                            }
                        };

                        let (n_type , _) = self.sim_contr.network_graph.get(&next_id).unwrap();
                        if n_type.simple_type() == Drone {
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

    pub fn manage_client_event(&mut self, client_event:ClientEvent){
        self.sim_contr.add_client_event_to_log(client_event.clone());


        match client_event {
            ClientEvent::Flooding(src) => {
                self.sim_contr.storage.node_flooding(src);
            }
            ClientEvent::SendContactsToSC(src, dst) => {
                self.sim_contr.storage.add_contacts(src, dst);
            }
            ClientEvent::SendDestinations(src, dst) => {
                self.sim_contr.storage.add_destination(src, dst);
            }
            ClientEvent::MissingDestination(src, dst) => {
                self.sim_contr.storage.remove_destination(src, dst);
            }
            ClientEvent::ReceivedChatText(src, dst, str) => {
                self.turn_on_notification(dst);
                self.sim_contr.storage.add_chat_text(src, dst, str);
            }
            ClientEvent::SendTextList(src, text_id, name) => {
                self.sim_contr.storage.add_text_list(src, text_id, name);
            }
            ClientEvent::SendCatalogue(src, media_id, media_name) => {
                self.sim_contr.storage.add_to_catalogue(src, media_id, media_name);
            }
            ClientEvent::SendMedia(src, media_id, str, media) => {
                // self.turn_on_notification(src);
                self.sim_contr.storage.add_to_medias(src, media_id, str, media);
            }
            ClientEvent::RegisterSuccessfully(src, dst) => {
                self.sim_contr.storage.add_to_registration(src, dst);
            }
            ClientEvent::MissingTextList(src, list) => {
                self.sim_contr.storage.missing_txt_list(src, list);
            }
            ClientEvent::MissingDestForMedia(src, media) => {
                self.sim_contr.storage.missing_media(src, media);
            }

            _ => {
                // Nothing for the others.
            }

        }
    }

    pub fn manage_server_event(&mut self, server_event:ServerEvent){
        self.sim_contr.add_server_event_to_log(server_event.clone());

        match server_event{
            ServerEvent::Flooding(src) => {
                self.sim_contr.storage.node_flooding(src);
            }
            ServerEvent::ClientRegistered(src, client) => {
                self.sim_contr.storage.add_to_registration(src, client);
            }
            ServerEvent::FilesState(src, completed, uncompleted) =>{
                self.sim_contr.storage.add_server_files(src, completed, uncompleted);
            }
            ServerEvent::MediaState(src, medias) =>{
                self.sim_contr.storage.add_server_medias(src, medias);
            }
            _ =>{

            }
        }
    }



    pub fn render_bottom_panel(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom_panel")
            .height_range(225.0..=400.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Simulation Control Log:").font(FontId::proportional(14.0)).color(Color32::WHITE),
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
                // Title Actions
                ui.colored_label(Color32::WHITE, RichText::new("Actions").strong().size(24.0));
                ui.add_space(15.0);

                match self.side_panel_scenes {
                    InitialScene => {

                        if ui.button("Grid").clicked() {
                            self.circle_mode = false;
                            self.sort = false;

                        }
                        if ui.button("Circle").clicked() {
                            self.circle_mode = true;
                            self.sort = false;

                        }
                        if ui.button("Sort").clicked() {
                            self.sort = true;
                        }


                        if ui.button("Add Drone!").clicked() {
                            self.side_panel_scenes = ManageAdd;
                        }
                        if ui.button("remove Drone!").clicked() {
                            self.side_panel_scenes = ManageCrash;
                        }
                        if ui.button("Statistics").clicked() {
                            self.side_panel_scenes = Statistics;
                        }

                        // for testing
                        if DEBUG_MODE {

                            if ui.button("test notification for 0!").clicked() {
                                self.turn_on_notification(0);
                            }


                            if ui.button("test (graphically) chat with 0 and 11!").clicked() {

                                self.sim_contr.storage.add_chat_text(0, 11, "Hi! How are you?".to_string());
                                self.sim_contr.storage.add_chat_text(11, 0, "Fine thanks".to_string());
                                self.sim_contr.storage.add_chat_text(11, 0, "Me too! Nice weather today".to_string());
                                self.sim_contr.storage.add_chat_text(0, 11, "Charizard use flamethrower!".to_string());
                            }

                            if ui.button("test (graphically) media with 12").clicked() {
                                let v = include_bytes!("../test/contents_inputs/media_files/charmander.png").to_vec();
                                self.sim_contr.storage.add_to_medias(12, fastrand::u64(20000..29999), "char.png".to_string(), v);
                            }


                            if ui.button("Test Drop with 5").clicked() {
                                self.sim_contr.set_pdr(5, 100.0);

                                let msg = create_packet(vec![0, 1, 8, 5, 2]);
                                self.sim_contr.all_sender_packets.get(&1).unwrap().send(msg).expect("Node Not connected to SC");
                            }

                            if ui.button("Test sending packet").clicked() {
                                let msg = create_packet(vec![0, 1, 8, 5, 2]);
                                self.sim_contr.all_sender_packets.get(&1).unwrap().send(msg).expect("Node Not connected to SC");
                            }
                        }

                        if ui.button("Clear Log").clicked() {
                            self.sim_contr.log.clear();
                        }

                        ui.separator();
                        if ui.button("Close Simulation").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            self.sim_contr.crash_all();
                        }

                        //load logo
                        if self.logo.is_none(){
                            self.logo = Some(load_texture(ctx,"src/simulation_control/texture_pngs/SkyLinkLogo.png"));
                        } else {
                            if let Some(texture) = self.logo.clone() {

                                // obtain available space
                                let available_size = ui.available_size();

                                let size = texture.size_vec2();
                                let aspect_ratio = size.x / size.y;
                                let new_size = if available_size.x / available_size.y > aspect_ratio {
                                    egui::vec2(available_size.y * aspect_ratio, available_size.y)
                                } else {
                                    egui::vec2(available_size.x, available_size.x / aspect_ratio)
                                };

                                // I want a Central Logo
                                let remaining_space = available_size.y - new_size.y;
                                let vertical_padding = remaining_space / 2.0;
                                ui.add_space(vertical_padding);


                                ui.image((texture.id(), new_size));
                            }
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
                            ui.checkbox(&mut self.selected_drones[i], item.id.to_string());
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
                            if item.node_type.simple_type() == Drone {
                                ui.checkbox(&mut self.selected_drones[i], item.id.to_string());
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
                    Statistics => {
                        //drop stats
                        let max_value_drop = self.sim_contr.storage.dropped_packets.values().map(|vec| vec.len() as f64).fold(0.0, f64::max);

                        if max_value_drop > 0.0 {
                            ui.heading("Packet Droppers Histogram");
                            let bars: Vec<Bar> = self.sim_contr.storage.dropped_packets
                                .iter()
                                .map(|(&id, vec)| {
                                    let length = vec.len() as f64;
                                    Bar::new(id as f64, length / max_value_drop * 10.0)
                                        .width(0.8) // Normalization
                                })
                                .collect();

                            let chart = BarChart::new(bars).name("Normalized length");

                            Plot::new("Histogram Drop")
                                .view_aspect(2.0)
                                .show(ui, |plot_ui| {
                                    plot_ui.bar_chart(chart);
                                });



                            ui.label("Choose a Drone to Inspect:");
                            for (i, dropped) in &self.sim_contr.storage.dropped_packets {
                                if ui.button(format!("{i} dropped {} packets", dropped.len())).clicked() {
                                    self.dropper = Some(*i);
                                }
                            }


                            if let Some(id) = self.dropper{
                                if let Some(dropped_packet) = self
                                    .sim_contr
                                    .storage
                                    .dropped_packets
                                    .get(&id)
                                {
                                    let dropped = match dropped_packet.last(){
                                        None => {"impossible to recover".to_string()}
                                        Some(x) => {x.to_string()}
                                    };
                                    // Display the dropped packet
                                    ui.label(format!("{id} dropped packet: {dropped}", ));

                                    // Options for handling the packet
                                    if ui.button("Close The Inspection").clicked() {
                                        self.dropper = None;
                                    }
                                }
                            }

                        } else {
                            ui.label("No packet dropped so far");
                        }


                        //flooding stats

                        let max_value_flood = self.sim_contr.storage.how_many_flood.values().map(|flood| *flood as f64).fold(0.0, f64::max);

                        if max_value_flood > 0.0 {
                            ui.heading("Packet Flooding Histogram Flood");
                            let bars: Vec<Bar> = self.sim_contr.storage.how_many_flood
                                .iter()
                                .map(|(&id, vec)| {
                                    let length = *vec as f64;
                                    Bar::new(id as f64, length / max_value_flood * 10.0)
                                        .width(0.8) // Normalization
                                        .fill(Color32::BLUE)

                                })
                                .collect();

                            let chart = BarChart::new(bars).name("Normalized length");

                            Plot::new("Histogram Flood")
                                .view_aspect(2.0)
                                .show(ui, |plot_ui| {
                                    plot_ui.bar_chart(chart);
                                });

                            for (i, floods) in &self.sim_contr.storage.how_many_flood {
                                ui.label(format!("{} flooded {} times", i, floods));
                            }

                        }else {
                            ui.label("No Floods so far");
                        }


                        if ui.button("Close").clicked() {
                            self.side_panel_scenes = InitialScene; // Close the alert
                            self.dropper = None
                        }
                    }
                }
            });
    }

    pub fn render_central_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width()); // Adapts the panel to the available length.
                ui.set_height(ui.available_height());

                // Title
                ui.colored_label(Color32::WHITE, RichText::new("Network Topology").strong().size(16.0));

                let available_size = ui.available_size();
                let center = egui::pos2(
                    ui.min_rect().left() + available_size.x / 2.0,
                    ui.min_rect().top() + available_size.y / 2.0,
                );


                let mut positions = Vec::new();
                let mut numbers_positions = Vec::new();
                
                
                if self.sort {
                    self.nodes.sort();
                }

                if self.circle_mode {
                    let radius = available_size.x.min(available_size.y) * 0.4;
                    
                    let total_items = self.nodes.len();
                    for (index, _value) in self.nodes.iter().enumerate() {
                        let angle = (index as f32 / total_items as f32) * std::f32::consts::TAU;
                        let x = center.x + radius * angle.cos();
                        let y = center.y + radius * angle.sin();
                        positions.push(egui::pos2(x, y));
                        let x_num = center.x + (radius + 45.0) * angle.cos();
                        let y_num = center.y + (radius + 45.0) * angle.sin();
                        numbers_positions.push(egui::pos2(x_num, y_num));
                    }
                } else {
                    // I calculate the grid.
                    let total_items = self.nodes.len();
                    let grid_size = (total_items as f32).sqrt().ceil() as usize;
                    let grid_spacing = 150.0; // Spacing between nodes.
                    let grid_width = grid_size as f32 * grid_spacing;
                    let grid_height = grid_size as f32 * grid_spacing;
                    let grid_origin = egui::pos2(
                        center.x - grid_width / 2.3,
                        center.y - grid_height / 3.0,
                    ); // Starting point for the grid (top left).

                    for i in 0..total_items {
                        let row = i / grid_size;
                        let col = i % grid_size;

                        let x = grid_origin.x + col as f32 * grid_spacing;
                        let y = grid_origin.y + row as f32 * grid_spacing;
                        positions.push(egui::pos2(x, y));

                        let num_x = grid_origin.x + col as f32 * grid_spacing + 40.0;
                        let num_y = grid_origin.y + row as f32 * grid_spacing + 40.0;
                        numbers_positions.push(egui::pos2(num_x, num_y));
                    }
                }

                let painter = ui.painter();

                for (i, node) in self.nodes.iter().enumerate() {
                    for &connection in &node.connections {
                        if let Some(j) = self.nodes.iter().position(|n| n.id == connection) {
                            let line_color = Color32::WHITE;
                            painter.line_segment([positions[i], positions[j]], (2.0, line_color));
                        }
                    }
                }

                for node in &mut self.nodes {
                    if node.texture.is_none() {
                        node.texture = match node.node_type {
                            NodeNature::Drone => Some(load_texture(ctx, "src/simulation_control/texture_pngs/drone_mod.png")),
                            NodeNature::ChatServer => Some(load_texture(ctx, "src/simulation_control/texture_pngs/ChatServer.png")),
                            NodeNature::ChatClient => Some(load_texture(ctx, "src/simulation_control/texture_pngs/ChatClient.png")),
                            NodeNature::WebBrowser => Some(load_texture(ctx, "src/simulation_control/texture_pngs/WebBrowser.png")),
                            NodeNature::TextServer => Some(load_texture(ctx, "src/simulation_control/texture_pngs/TextServer.png")),
                            NodeNature::MediaServer => Some(load_texture(ctx, "src/simulation_control/texture_pngs/MediaServer.png")),
                        };
                    }
                }

                for (index, value) in self.nodes.iter_mut().enumerate() {
                    let rect =
                        egui::Rect::from_center_size(positions[index], egui::vec2(80.0, 80.0));
                    let response = ui.interact(rect, egui::Id::new(index), egui::Sense::click());

                    let circle_color = if value.selected {
                        Color32::from_rgb(255, 255, 255)
                    } else {
                        Color32::from_rgb(255, 255, 255)
                    };

                    if value.selected{
                        painter.circle_filled(positions[index], 30.0, Color32::DARK_GRAY);
                    }

                    if let Some(texture) = &value.texture {
                        painter.add(egui::Shape::image(
                            texture.id(),
                            rect,
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            ),
                           circle_color,

                        ));

                        if value.notify {
                            let top_right = rect.right_top();
                            painter.circle_filled(top_right, 7.0, Color32::YELLOW);
                        }
                    }
                    painter.text(
                        numbers_positions[index],
                        egui::Align2::CENTER_CENTER,
                        value.id.to_string(),
                        FontId::proportional(16.0),
                        Color32::WHITE,
                    );
                  
                    if response.clicked() {
                        value.selected = true;
                    }
                }
            });
        });
    }

    pub fn render_nodes_windows(&mut self, ctx: &egui::Context) {
        for node in self.nodes.iter_mut() {
            if node.selected {
                match node.node_type {
                    NodeNature::Drone => {egui::Window::new(format!("Drone {}", node.id))
                        .resizable(true)
                        .collapsible(true)
                        .min_width(500.0)
                        .max_height(400.0)
                        .show(ctx, |ui| {
                            match node.node_window_scenes {
                                Start => {
                                    let connections = MyApp::format_connections(node.clone());
                                    ui.label( RichText::new(format!("Connected to: {}", connections))
                                                  .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                                  .color(Color32::WHITE),);

                                    ui.separator();

                                    if ui.button("Remove Channel").clicked(){
                                        node.node_window_scenes = RemoveSender
                                    }

                                    if ui.button("Crash This Drone").clicked(){
                                        node.node_window_scenes = Crash
                                    }

                                    if ui.button("Add Channel").clicked(){
                                        node.node_window_scenes = AddSender;
                                    }

                                    if ui.button("set PDR").clicked(){
                                        node.node_window_scenes = SetPDR
                                    }

                                    if ui.button("Close").clicked() {
                                        node.selected = false; // Closes the popup.
                                    }

                                    ui.separator();
                                    // Here you can add more info or checks.
                                    self.sender_id = 0;
                                    ui.label( RichText::new("Log:".to_string())
                                                  .font(FontId::new(14.0, egui::FontFamily::Monospace))
                                                  .color(Color32::LIGHT_RED),);
                                    ui.separator();
                                    ui.vertical(|ui| {
                                        egui::ScrollArea::vertical()
                                            .auto_shrink([false; 2]) // Ensures it doesn't shrink horizontally or vertically
                                            .show (ui, |ui|
                                                for s in &self.sim_contr.log {
                                                    if s.get_id() == node.id {
                                                        ui.label(RichText::new(format!("{}", s))
                                                                     .font(FontId::new(13.0, egui::FontFamily::Monospace)) // Monospaced font
                                                                     .color(Color32::LIGHT_GRAY),
                                                        );
                                                        ui.separator();
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
                                        self.sender_id = 0;
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
                                        self.sender_id = 0;
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
                    NodeNature::ChatClient => {
                        egui::Window::new(format!("Chat Client {}", node.id))
                            .resizable(true) // Allow resizing
                            .collapsible(true)
                            .min_width(500.0)
                            .default_size((500.0, 400.0)) // Set default size
                            .default_pos((100.0, 100.0)) // Set default position
                            .show(ctx, |ui| {
                                ui.push_id(format!("client_window_{}", node.id), |ui| {
                                    egui::Frame::default()
                                        .fill(Color32::BLACK) // Set the frame's background color
                                        .stroke(egui::Stroke::new(1.0, Color32::BLACK)) // Add a border
                                        .inner_margin(egui::Margin::symmetric(10.0, 10.0)) // Optional padding
                                        .show(ui, |ui| {
                                            // Split panels with proper layout
                                            egui::SidePanel::left(format!("side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    ui.label(RichText::new(format!("Log of {}:", node.id))
                                                                 .font(FontId::new(15.0, egui::FontFamily::Monospace)) // Monospaced font
                                                                 .color(Color32::WHITE),);
                                                    ui.separator();

                                                    egui::ScrollArea::vertical()
                                                        .auto_shrink([false; 2]) // Prevent shrinking
                                                        .show(ui, |ui| {
                                                            for s in &self.sim_contr.log {
                                                                if s.get_id() == node.id {
                                                                    ui.label(
                                                                        RichText::new(format!("{}", s))
                                                                            .font(FontId::new(13.0, egui::FontFamily::Monospace)) // Monospaced font
                                                                            .color(Color32::LIGHT_RED),
                                                                    );
                                                                    ui.separator();
                                                                }
                                                            }
                                                        });
                                                });

                                            egui::SidePanel::right(format!("right_side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    let connections = MyApp::format_connections(node.clone());

                                                    ui.label(RichText::new(format!("Connected to: {}", connections))
                                                                 .font(FontId::new(15.0, egui::FontFamily::Monospace))
                                                                 .color(Color32::WHITE),);
                                                    ui.separator();

                                                    if ui.button("Add Channel").clicked(){
                                                        node.node_window_scenes = AddSender;
                                                    }

                                                    if ui.button("Remove Channel").clicked(){
                                                        node.node_window_scenes = RemoveSender
                                                    }

                                                    if ui.button("Flood").clicked(){
                                                        self.sim_contr.flood_with(node.id);
                                                        node.content = None;
                                                        node.node_window_scenes = ShowDestinations;
                                                    }

                                                    if ui.button("Show Chats").clicked(){
                                                        node.node_window_scenes = ShowContents;
                                                        node.notify = false;
                                                        node.content = None;
                                                        node.input_text = "".to_string(); //reset input text

                                                    }
                                                    if ui.button("Show Server to witch you are Registered ").clicked(){
                                                        node.node_window_scenes = ShowAuxiliaryLists;
                                                    }

                                                    if ui.button("Show Servers Detected").clicked(){
                                                        node.content = None;
                                                        node.node_window_scenes = ShowDestinations;
                                                    }


                                                    if ui.button("Close").clicked() {
                                                        node.content = None;
                                                        node.selected = false; // Close the window.
                                                    }

                                                    if let Some(texture) = node.texture.clone() {
                                                        // obtain available space
                                                        let available_size = ui.available_size();

                                                        let size = texture.size_vec2();
                                                        let aspect_ratio = size.x / size.y;
                                                        let new_size = if available_size.x / available_size.y > aspect_ratio {
                                                            egui::vec2(available_size.y * aspect_ratio, available_size.y)
                                                        } else {
                                                            egui::vec2(available_size.x, available_size.x / aspect_ratio)
                                                        };

                                                        ui.image((texture.id(), new_size));

                                                    }
                                                });

                                            egui::CentralPanel::default()
                                                .show_inside(ui, |ui| {
                                                    match node.node_window_scenes{
                                                        Start => {
                                                            ui.label("This is the central panel content.");
                                                            ui.label("flood results and chat shits will be here.");
                                                        }
                                                        AddSender => {
                                                            ui.horizontal(|ui| {
                                                                ui.label("Add Channel to:");
                                                                ui.add(egui::DragValue::new(&mut self.sender_id));

                                                            }
                                                            );

                                                            if ui.button("Confirm").clicked() {
                                                                self.sim_contr.add_sender(node.id, self.sender_id);
                                                                self.sender_id = 0;
                                                                node.node_window_scenes = Start;
                                                            }
                                                            if ui.button("back").clicked(){
                                                                node.node_window_scenes = Start;
                                                            }
                                                        }
                                                        RemoveSender => {
                                                            ui.horizontal(|ui| {
                                                                ui.label("Remove Channel to:");
                                                                ui.add(egui::DragValue::new(&mut self.sender_id));

                                                            }
                                                            );

                                                            if ui.button("Confirm").clicked() {
                                                                self.sim_contr.remove_senders(node.id, self.sender_id);
                                                                self.sender_id = 0;
                                                                node.node_window_scenes = Start;
                                                            }
                                                            if ui.button("back").clicked(){
                                                                node.node_window_scenes = Start;
                                                            }
                                                        },
                                                        ShowContents => {
                                                            node.notify = false;
                                                            let node_contacts = match self.sim_contr.storage.contacts.get(&node.id){
                                                                Some(contacts) => contacts.clone(),
                                                                None => HashSet::new()
                                                            };
                                                            if node.content.is_none() {
                                                                ui.label("MyContacts are: ".to_string());
                                                                ui.separator();
                                                                for id in node_contacts {
                                                                    if ui.button(id.to_string()).clicked() {
                                                                        node.content = Some(Chat(id))
                                                                    }
                                                                }
                                                            }
                                                            else {
                                                                if let Some(Chat(dst)) = node.content.clone(){
                                                                    //print chat
                                                                    ui.label(format!("Chat with {dst}"));
                                                                    ui.separator();
                                                                    if let Some(chat) = self.sim_contr.storage.retrieve_chat(node.id, dst){
                                                                        for (id, str) in chat {
                                                                            // Layout for all the messages.
                                                                            ui.horizontal(|ui| {
                                                                                if id == node.id {
                                                                                    // Message sent from right Node.
                                                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                                        ui.group(|ui| {
                                                                                            ui.label(str);
                                                                                        });
                                                                                    });
                                                                                } else {
                                                                                    // Message received from left Node.
                                                                                    ui.group(|ui| {
                                                                                        ui.label(format!("{id}: {str}"));
                                                                                    });
                                                                                }
                                                                            });
                                                                        }
                                                                    }
                                                                    //send message
                                                                    ui.separator();
                                                                    ui.label("Enter a message:");
                                                                    let response = ui.add(egui::TextEdit::singleline(&mut node.input_text));
                                                                    if response.lost_focus() {
                                                                        // Handle Enter key press
                                                                        self.sim_contr.msg_another_client(node.id, dst, node.input_text.clone());
                                                                        node.input_text = "".to_string(); //reset input text
                                                                    }
                                                                    if ui.button("Close Chat").clicked() {
                                                                        node.content = None;
                                                                    }
                                                                }
                                                            }

                                                            if ui.button("Close").clicked() {
                                                                node.content = None;
                                                                node.node_window_scenes = Start; // Close the window.
                                                            }
                                                        }

                                                        ShowDestinations => {
                                                            let node_dst = match self.sim_contr.storage.destinations.get(&node.id){
                                                                Some(destinations) => destinations.clone(),
                                                                None => HashSet::new()
                                                            };
                                                            if node.content.is_none() {
                                                                ui.label( RichText::new("My Servers are: ".to_string())
                                                                              .font(FontId::new(13.0, egui::FontFamily::Monospace))
                                                                              .color(Color32::GRAY),);
                                                                ui.separator();
                                                                for (id) in node_dst {
                                                                   if ui.button(id.to_string()).clicked() {
                                                                       node.content = Some(RegisterOrList(id))
                                                                   }
                                                                }
                                                            } else {
                                                                if let Some(RegisterOrList(dst)) = node.content {
                                                                    ui.label(format!("Contact Server {dst}: "));
                                                                    ui.separator();

                                                                    if ui.button("Register").clicked() {
                                                                        self.sim_contr.register_client_to_server(node.id, dst);
                                                                        node.content = None;
                                                                        node.node_window_scenes = ShowAuxiliaryLists; // Close the window
                                                                    }
                                                                    if ui.button("Get List").clicked() {
                                                                        self.sim_contr.retrive_list_from_server(node.id, dst);
                                                                        node.content = None;
                                                                        node.node_window_scenes = Start; // Close the window

                                                                    }
                                                                }
                                                            }

                                                            if ui.button("Close").clicked() {
                                                                node.content = None;
                                                                node.node_window_scenes = Start; // Close the window.
                                                            }
                                                        }

                                                        ShowAuxiliaryLists => {
                                                            let node_reg = match self.sim_contr.storage.registrations.get(&node.id){
                                                                Some(destinations) => destinations.clone(),
                                                                None => Vec::new()
                                                            };
                                                            ui.label("Registered to Servers ".to_string());
                                                            ui.separator();
                                                            ui.vertical(|ui| {
                                                                egui::ScrollArea::vertical()
                                                                    .auto_shrink([false; 2]) // Ensures it doesn't shrink horizontally or vertically
                                                                    .show(ui, |ui| {
                                                                        for (id) in node_reg {
                                                                            ui.label(format!("Server {id}"));
                                                                        }
                                                                    });
                                                            });
                                                        }

                                                        _ => {
                                                            //drone scenes should be
                                                            unreachable!()
                                                        }
                                                    }
                                                });
                                        });
                                });
                            });
                    }
                    NodeNature::WebBrowser => {
                        egui::Window::new(format!("Web Browser {}", node.id))
                            .resizable(true) // Allow resizing
                            .collapsible(true)
                            .min_width(500.0)
                            .default_size((500.0, 400.0)) // Set default size
                            .default_pos((100.0, 100.0)) // Set default position
                            .show(ctx, |ui| {
                                ui.push_id(format!("client_window_{}", node.id), |ui| {
                                    egui::Frame::default()
                                        .fill(Color32::BLACK) // Set the frame's background color
                                        .stroke(egui::Stroke::new(1.0, Color32::BLACK)) // Add a border
                                        .inner_margin(egui::Margin::symmetric(10.0, 10.0)) // Optional padding
                                        .show(ui, |ui| {
                                            // Split panels with proper layout
                                            egui::SidePanel::left(format!("side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    ui.label( RichText::new(format!("Log of {}:", node.id))
                                                                  .font(FontId::new(15.0, egui::FontFamily::Monospace))
                                                                  .color(Color32::WHITE),);

                                                    ui.separator();

                                                    egui::ScrollArea::vertical()
                                                        .auto_shrink([false; 2]) // Prevent shrinking
                                                        .show(ui, |ui| {
                                                            for s in &self.sim_contr.log {
                                                                if s.get_id() == node.id {
                                                                    ui.label(RichText::new(format!("{}", s))
                                                                                 .font(FontId::new(13.0, egui::FontFamily::Monospace)) // Monospaced font.
                                                                                 .color(Color32::LIGHT_BLUE),
                                                                    );
                                                                    ui.separator();
                                                                }
                                                            }
                                                        });
                                                });

                                            egui::SidePanel::right(format!("right_side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    let connections = MyApp::format_connections(node.clone());

                                                    ui.label( RichText::new(format!("Connected to: {}", connections))
                                                                  .font(FontId::new(15.0, egui::FontFamily::Monospace))
                                                                  .color(Color32::WHITE),);
                                                    ui.separator();

                                                    if ui.button("Add Channel").clicked(){
                                                        node.node_window_scenes = AddSender;
                                                    }

                                                    if ui.button("Remove Channel").clicked(){
                                                        node.node_window_scenes = RemoveSender
                                                    }

                                                    if ui.button("Flood").clicked(){
                                                        self.sim_contr.flood_with(node.id);
                                                        node.content = None;
                                                        node.node_window_scenes = ShowDestinations;
                                                    }
                                                    if ui.button("Resolve a TextList").clicked(){
                                                        node.content = None;
                                                        node.node_window_scenes = ShowAuxiliaryLists;
                                                    }

                                                    if ui.button("Show Catalogue").clicked(){
                                                        node.node_window_scenes = ShowAuxiliaryLists;
                                                        node.content = Some(MediaToResolve)
                                                    }



                                                    if ui.button("Show Possessed Medias").clicked(){
                                                        node.node_window_scenes = ShowContents;
                                                        node.content = None;
                                                    }

                                                    if ui.button("Show Text Servers Detected").clicked(){
                                                        node.content = None;
                                                        node.node_window_scenes = ShowDestinations;
                                                    }


                                                    if ui.button("Close").clicked() {
                                                        node.content = None;
                                                        node.selected = false; // Close the window.
                                                    }

                                                    if let Some(texture) = node.texture.clone() {
                                                        // obtain available space
                                                        let available_size = ui.available_size();

                                                        let size = texture.size_vec2();
                                                        let aspect_ratio = size.x / size.y;
                                                        let new_size = if available_size.x / available_size.y > aspect_ratio {
                                                            egui::vec2(available_size.y * aspect_ratio, available_size.y)
                                                        } else {
                                                            egui::vec2(available_size.x, available_size.x / aspect_ratio)
                                                        };

                                                        ui.image((texture.id(), new_size));

                                                    }
                                                });

                                            egui::CentralPanel::default()
                                                .show_inside(ui, |ui| {
                                                    match node.node_window_scenes{
                                                        Start => {
                                                            ui.label("This is the central panel content.");
                                                            ui.label("flood results and medias will be here.");
                                                        }
                                                        AddSender => {
                                                            ui.horizontal(|ui| {
                                                                ui.label("Add Channel to:");
                                                                ui.add(egui::DragValue::new(&mut self.sender_id));
                                                            }
                                                            );

                                                            if ui.button("Confirm").clicked() {
                                                                self.sim_contr.add_sender(node.id, self.sender_id);
                                                                self.sender_id = 0;
                                                                node.node_window_scenes = Start;
                                                            }
                                                            if ui.button("back").clicked(){
                                                                node.node_window_scenes = Start;
                                                            }
                                                        }
                                                        RemoveSender => {
                                                            ui.horizontal(|ui| {
                                                                ui.label("Remove Channel to:");
                                                                ui.add(egui::DragValue::new(&mut self.sender_id));

                                                            }
                                                            );

                                                            if ui.button("Confirm").clicked() {
                                                                self.sim_contr.remove_senders(node.id, self.sender_id);
                                                                self.sender_id = 0;
                                                                node.node_window_scenes = Start;
                                                            }
                                                            if ui.button("back").clicked(){
                                                                node.node_window_scenes = Start;
                                                            }
                                                        },
                                                        ShowContents => {
                                                            let node_medias = match self.sim_contr.storage.medias.get(&node.id) {
                                                                Some(contacts) => contacts.clone(),
                                                                None => vec![],
                                                            };

                                                            if node.content.is_none() {
                                                                ui.label("MyMedias are: ".to_string());
                                                                ui.label("Click the one you want to show".to_string());
                                                                ui.separator();
                                                                if node_medias.is_empty(){
                                                                    ui.label(RichText::new("It appears you don't posses any media (yet)")
                                                                        .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                                                        .color(Color32::LIGHT_RED));
                                                                }else {
                                                                    for media in node_medias {
                                                                        if ui.button(format!("{} - {}", media.0.clone(), media.1.clone())).clicked() {
                                                                            // load image
                                                                            if let Some(texture) = load_image(ctx, media.2.clone()) {
                                                                                node.content = Some(Media(texture));
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            } else {
                                                                if let Some(Media(texture)) = node.content.clone() {

                                                                    if ui.button("Close image").clicked() {
                                                                        node.content = None;
                                                                    }

                                                                    // obtain available space
                                                                    let available_size = ui.available_size();

                                                                    let size = texture.size_vec2();
                                                                    let aspect_ratio = size.x / size.y;
                                                                    let new_size = if available_size.x / available_size.y > aspect_ratio {
                                                                        egui::vec2(available_size.y * aspect_ratio, available_size.y)
                                                                    } else {
                                                                        egui::vec2(available_size.x, available_size.x / aspect_ratio)
                                                                    };
                                                                  
                                                                    ui.image((texture.id(), new_size));
                                                                }
                                                            }

                                                            if ui.button("Close").clicked() {
                                                                node.content = None;
                                                                node.node_window_scenes = Start; // Close the window.
                                                            }
                                                        },

                                                        ShowDestinations => {
                                                            let node_dst = match self.sim_contr.storage.destinations.get(&node.id){
                                                                Some(destinations) => destinations.clone(),
                                                                None => HashSet::new()
                                                            };
                                                            if node.content.is_none() {
                                                                ui.label("My Text Servers are: ".to_string());
                                                                ui.label("Choose one from witch you want the text lists ".to_string());
                                                                ui.separator();
                                                                for (id) in node_dst {
                                                                    if ui.button(id.to_string()).clicked() {
                                                                        node.content = Some(TextList(id))
                                                                    }
                                                                }
                                                            } else {
                                                                if let Some(TextList(dst)) = node.content {
                                                                    self.sim_contr.retrive_list_from_server(node.id, dst);
                                                                    node.content = None;
                                                                    node.node_window_scenes = Start; // Close the window
                                                                }
                                                            }

                                                            if ui.button("Close").clicked() {
                                                                node.content = None;
                                                                node.node_window_scenes = Start; // Close the window.
                                                            }
                                                        },

                                                        ShowAuxiliaryLists => {
                                                            let node_text_lists = match self.sim_contr.storage.text_lists.get(&node.id){
                                                                Some(texts) => texts.clone(),
                                                                None => vec![],
                                                            };
                                                            if ui.button("Close").clicked() {
                                                                node.content = None;
                                                                node.node_window_scenes = Start; // Close the window.
                                                            }
                                                            ui.add_space(10.0);

                                                            if node.content.is_none() {
                                                                ui.label("My Text TextLists are: ".to_string());
                                                                ui.label("Choose witch you want to resolve to update your catalogue".to_string());
                                                                ui.separator();
                                                                ui.vertical(|ui| {
                                                                    egui::ScrollArea::vertical()
                                                                        .auto_shrink([false; 2]) // Ensures it doesn't shrink horizontally or vertically
                                                                        .show(ui, |ui| {
                                                                            for (id) in node_text_lists {
                                                                                if ui.button(format!("ID: {}\n{}", id.0.clone(), id.1.clone())).clicked() {
                                                                                    self.sim_contr.get_text_file(node.id, id.0);
                                                                                    node.content = Some(MediaToResolve)
                                                                                }
                                                                            }
                                                                        });
                                                                });

                                                            } else {
                                                                if let Some(MediaToResolve) = node.content {
                                                                    if let Some(media_available) = self.sim_contr.storage.catalogues.get(&node.id){
                                                                        ui.label("All Medias you can Get: ".to_string());
                                                                        ui.separator();

                                                                        ui.vertical(|ui| {
                                                                            egui::ScrollArea::vertical()
                                                                                .auto_shrink([true; 2]) // Ensures it doesn't shrink horizontally or vertically
                                                                                .show(ui, |ui| {
                                                                                    for (id, name) in media_available {
                                                                                        if ui.button(format!("{id} - {name}")).clicked() {
                                                                                            self.sim_contr.force_client_get_media(node.id, id.clone());
                                                                                            node.content = None;
                                                                                            node.node_window_scenes = Start; // Close the window
                                                                                        }
                                                                                    }
                                                                                });
                                                                        });

                                                                    } else {
                                                                        ui.label("Catalog is Empty: either is updating, or SC is congested ".to_string());
                                                                    }
                                                                }
                                                            }

                                                            ui.separator();
                                                            ui.label("Or you can enter an id:");
                                                            let response = ui.add(egui::TextEdit::singleline(&mut node.input_text));
                                                            ui.label("Remember, the drone may not know a location for it!");
                                                            if response.lost_focus() {
                                                                // Handle Enter key press
                                                                if let Ok(id) = node.input_text.parse::<u64>() {
                                                                    self.sim_contr.force_client_get_media(node.id, id);
                                                                    node.content = None;
                                                                    node.node_window_scenes = Start; // Close the window
                                                                    node.input_text = "".to_string(); //reset input text
                                                                }else{
                                                                    node.input_text = "".to_string(); //reset input text
                                                                }
                                                            }

                                                            if ui.button("Close Catalogue").clicked() {
                                                                node.content = None;
                                                                node.node_window_scenes = Start;
                                                            }
                                                        },

                                                        _ => {
                                                            //drone scenes should be
                                                            unreachable!()
                                                        }
                                                    }
                                                });
                                        });
                                });
                            });
                    }
                    _ => {
                        egui::Window::new(format!("Server {}", node.id))
                            .resizable(true) // Allow resizing
                            .collapsible(true)
                            .min_width(500.0)
                            .default_size((500.0, 400.0)) // Set default size
                            .default_pos((100.0, 100.0)) // Set default position
                            .show(ctx, |ui| {
                                ui.push_id(format!("server_window_{}", node.id), |ui| {
                                    egui::Frame::default()
                                        .fill(Color32::BLACK) // Set the frame's background color
                                        .stroke(egui::Stroke::new(1.0, Color32::BLACK)) // Add a border
                                        .inner_margin(egui::Margin::symmetric(10.0, 10.0)) // Optional padding
                                        .show(ui, |ui| {
                                            // Split panels with proper layout
                                            egui::SidePanel::left(format!("side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {

                                                    ui.label(RichText::new("Log:".to_string())
                                                                 .font(FontId::new(15.0, egui::FontFamily::Monospace))
                                                                 .color(Color32::WHITE),);

                                                    ui.separator();
                                                    egui::ScrollArea::vertical()
                                                        .auto_shrink([false; 2]) // Prevent shrinking
                                                        .show(ui, |ui| {
                                                            for s in &self.sim_contr.log {
                                                                if s.get_id() == node.id {
                                                                    ui.label(RichText::new(format!("{}", s))
                                                                                 .font(FontId::new(15.0, egui::FontFamily::Monospace))
                                                                                 .color(Color32::YELLOW),);
                                                                    ui.separator();
                                                                }
                                                            }
                                                        });
                                                });

                                            egui::SidePanel::right(format!("right_side_panel_{}", node.id))
                                                .resizable(true)
                                                .default_width(200.0) // Limit side panel width
                                                .show_inside(ui, |ui| {
                                                    let connections = MyApp::format_connections(node.clone());

                                                    ui.label(RichText::new(format!("Connected to: {}", connections))
                                                                 .font(FontId::new(15.0, egui::FontFamily::Monospace))
                                                                 .color(Color32::WHITE),);

                                                    ui.separator();

                                                    if ui.button("Add Channel").clicked(){
                                                        node.node_window_scenes = AddSender;
                                                    }

                                                    if ui.button("Remove Channel").clicked(){
                                                        node.node_window_scenes = RemoveSender;
                                                    }

                                                    let content = match node.node_type{
                                                        NodeNature::ChatServer => {"Registered Clients"}
                                                        NodeNature::TextServer => {"Text Lists"}
                                                        NodeNature::MediaServer => {"Media Possessed"}
                                                        _ =>{
                                                            unreachable!()
                                                        }
                                                    };

                                                    if ui.button(format!("See {}", content)).clicked(){
                                                        node.node_window_scenes = ShowContents;
                                                    }


                                                    if ui.button("Close").clicked() {
                                                        node.content = None;
                                                        node.selected = false; // Close the window
                                                    }

                                                    if let Some(texture) = node.texture.clone() {
                                                        // obtain available space
                                                        let available_size = ui.available_size();

                                                        let size = texture.size_vec2();
                                                        let aspect_ratio = size.x / size.y;
                                                        let new_size = if available_size.x / available_size.y > aspect_ratio {
                                                            egui::vec2(available_size.y * aspect_ratio, available_size.y)
                                                        } else {
                                                            egui::vec2(available_size.x, available_size.x / aspect_ratio)
                                                        };

                                                        ui.image((texture.id(), new_size));

                                                    }
                                                });


                                            egui::CentralPanel::default()
                                                .show_inside(ui, |ui| {
                                                    match node.node_window_scenes{
                                                        Start => {
                                                            ui.label("This is the central panel content.");
                                                            ui.label("Server infos will be here ");
                                                        }
                                                        AddSender => {
                                                            ui.horizontal(|ui| {
                                                                ui.label("Add Channel to:");
                                                                ui.add(egui::DragValue::new(&mut self.sender_id));
                                                            }
                                                            );

                                                            if ui.button("Confirm").clicked() {
                                                                self.sim_contr.add_sender(node.id, self.sender_id);
                                                                self.sender_id = 0;
                                                                node.node_window_scenes = Start;
                                                            }
                                                            if ui.button("back").clicked(){
                                                                node.node_window_scenes = Start;
                                                            }
                                                        },
                                                        RemoveSender => {
                                                            ui.horizontal(|ui| {
                                                                ui.label("Remove Channel to:");
                                                                ui.add(egui::DragValue::new(&mut self.sender_id));
                                                            });

                                                            if ui.button("Confirm").clicked() {
                                                                self.sim_contr.remove_senders(node.id, self.sender_id);
                                                                self.sender_id = 0;
                                                                node.node_window_scenes = Start;
                                                            }
                                                            if ui.button("back").clicked(){
                                                                node.node_window_scenes = Start;
                                                            }
                                                        }
                                                        ShowContents => {
                                                            if NodeNature::ChatServer == node.node_type{
                                                               if let Some(register_clients) = self.sim_contr.storage.registrations.get(&node.id){
                                                                   ui.label(RichText::new("List of Registered Clients:".to_string())
                                                                                 .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                                                                 .color(Color32::WHITE));
                                                                   ui.separator();
                                                                   ui.vertical(|ui| {
                                                                       egui::ScrollArea::vertical()
                                                                           .auto_shrink([false; 2]) // Ensures it doesn't shrink horizontally or vertically
                                                                           .show(ui, |ui| {
                                                                               for i in register_clients {
                                                                                   let s = format!("Client {}", i);
                                                                                   ui.label(s);
                                                                               }
                                                                           });
                                                                   });


                                                               } else {
                                                                   ui.label(RichText::new("No Registered Clients:".to_string())
                                                                       .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                                                       .color(Color32::WHITE));
                                                                   ui.separator();

                                                               }
                                                            } else if NodeNature::TextServer == node.node_type {
                                                                if let Some(lists) = self.sim_contr.storage.text_lists.get(&node.id){
                                                                    ui.label(RichText::new("List of Text List Available:".to_string())
                                                                        .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                                                        .color(Color32::WHITE));
                                                                    ui.label(RichText::new("(some may be uncompleted)".to_string())
                                                                        .font(FontId::new(10.0, egui::FontFamily::Monospace))
                                                                        .color(Color32::WHITE));
                                                                    ui.separator();

                                                                    ui.vertical(|ui| {
                                                                        egui::ScrollArea::vertical()
                                                                            .auto_shrink([false; 2]) // Ensures it doesn't shrink horizontally or vertically
                                                                            .show(ui, |ui| {
                                                                                for i in lists {
                                                                                    let s = format!("Text {}\n{}", i.0, i.1);
                                                                                    ui.label(s);
                                                                                    ui.separator();
                                                                                }
                                                                            });
                                                                    });


                                                                } else {
                                                                    ui.label(RichText::new("No Text Lists Available".to_string())
                                                                        .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                                                        .color(Color32::WHITE));
                                                                    ui.separator();

                                                                }
                                                            }
                                                            else {
                                                                if let Some(list) = self.sim_contr.storage.medias.get(&node.id){
                                                                    ui.label(RichText::new("List of Media Possessed:".to_string())
                                                                        .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                                                        .color(Color32::WHITE));
                                                                    ui.separator();

                                                                    ui.vertical(|ui| {
                                                                        egui::ScrollArea::vertical()
                                                                            .auto_shrink([false; 2]) // Ensures it doesn't shrink horizontally or vertically
                                                                            .show(ui, |ui| {
                                                                                for (id, name, _) in list {
                                                                                    let s = format!("Media {}\n{}", id, name);
                                                                                    ui.label(s);
                                                                                    ui.separator();
                                                                                }
                                                                            });
                                                                    });

                                                                } else {
                                                                    ui.label(RichText::new("No Media Possessed".to_string())
                                                                        .font(FontId::new(12.0, egui::FontFamily::Monospace))
                                                                        .color(Color32::WHITE));
                                                                    ui.separator();

                                                                }
                                                            }
                                                        }
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
        //setting this true assure you keep reading from SC, retest won't work (but you can delete it)
        self.update_topology();
    }

    pub fn update_event_receivers(&mut self) {
        match self.sim_contr.server_event_recv.try_recv() {
            Ok(event) => {
                self.manage_server_event(event);
            }
            _ => {}
        }
        match self.sim_contr.client_event_recv.try_recv() {
            Ok(event) => {
                self.manage_client_event(event);
            }
            _ => {}
        }
        match self.sim_contr.drone_event_recv.try_recv() {
            Ok(event) => {
                self.manage_drone_event(event);
            }
            _ => {}
        }
    }

    pub fn format_connections(node: MyNodes) -> String {
        let mut connections = String::new();
        let mut first = true;
        for connection in node.connections.clone() {
            if !first {
                connections.push_str(", ");
            }
            first = false;
            connections.push_str(&connection.to_string());
        }
        connections
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
        "SkyLink",
        options,
        Box::new(|_cc| Ok(Box::new(MyApp::new(sim_control)))),
    )
}

fn load_texture(ctx: &egui::Context, path: &str) -> TextureHandle {

    let image = image::open(path).expect(format!{"Failed to load image {path}"}.as_str()).to_rgba8();
    let size = [image.width() as usize, image.height() as usize];


    let color_image = eframe::epaint::ColorImage::from_rgba_unmultiplied(size, image.as_flat_samples().as_slice());
    ctx.load_texture(path, color_image, egui::TextureOptions::default())
}

fn load_image(ctx: &egui::Context, image_data: Vec<u8>) -> Option<TextureHandle> {

    let decoded_image = image::load_from_memory(&image_data).ok()?;

    let rgba_image = decoded_image.to_rgba8();
    let (width, height) = rgba_image.dimensions(); // Get the actual dimensions

    let pixels: Vec<Color32> = rgba_image
        .pixels()
        .map(|p| Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]))
        .collect();

    let texture = egui::ColorImage {
        size: [width as usize, height as usize], // Use the actual image size
        pixels,
    };

    Some(ctx.load_texture("immagine", texture, egui::TextureOptions::default()))
}

fn create_packet(hops: Vec<NodeId>) -> Packet {
    Packet {
        pack_type: PacketType::MsgFragment(Fragment {
            fragment_index: 0,
            total_n_fragments: 1,
            length: 128,
            data: [1; 128],
        }),
        routing_header: SourceRoutingHeader { hop_index: 1, hops },
        session_id: 1,
    }

}
