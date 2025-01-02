use crate::skylink_drone::drone::SkyLinkDrone;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display, Formatter};
use std::thread;
use std::thread::JoinHandle;
use egui::ahash::HashSet;
use wg_2024::controller::DroneCommand::{AddSender, RemoveSender};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::*;
use wg_2024::drone::Drone;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet, PacketType};
use wg_2024::packet::NodeType::*;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::simulation_control::sim_control::Cause::{AckReceived, DroneInsideDestination, LostMessage, MissingDestination, NackReceived};

pub struct SimulationControl {
    drone_command_senders: HashMap<NodeId, Sender<DroneCommand>>,
    client_command_senders: HashMap<NodeId, Sender<ClientCommand>>,
    server_command_senders: HashMap<NodeId, Sender<ServerCommand>>,

    pub(crate) drone_event_recv: Receiver<DroneEvent>,
    pub(crate) client_event_recv: Receiver<ClientEvent>,
    pub(crate) server_event_recv: Receiver<ServerEvent>,

    pub channel_for_drone: Sender<DroneEvent>, // questo serve così ogni volta che creo un nuovo drone, quando gli devo dare il channel per comunicare con il drone, mi limito a clonare questo, e per i test
    pub(crate) all_sender_packets: HashMap<NodeId, Sender<Packet>>, //hashmap con tutti i sender packet così puoi clonarli nel spawn, made pub for testing
    pub(crate) network_graph: HashMap<NodeId, (NodeType, HashSet<NodeId>)>,
    pub(crate) log: VecDeque<LogEntry>,

    pub dropped_packets: Vec<(NodeId, Packet)>,
}

impl SimulationControl {
    pub fn new(
        drone_command_senders: HashMap<NodeId, Sender<DroneCommand>>,
        client_command_senders: HashMap<NodeId, Sender<ClientCommand>>,
        server_command_senders: HashMap<NodeId, Sender<ServerCommand>>,
        drone_event_recv: Receiver<DroneEvent>,
        client_event_recv: Receiver<ClientEvent>,
        server_event_recv: Receiver<ServerEvent>,
        channel_for_drone: Sender<DroneEvent>,
        all_sender_packets: HashMap<NodeId, Sender<Packet>>,
        network_graph: HashMap<NodeId, (NodeType, HashSet<NodeId>)>,
    ) -> Self {
        SimulationControl {
            drone_command_senders,
            drone_event_recv,
            client_command_senders,
            server_command_senders,
            client_event_recv,
            server_event_recv,
            channel_for_drone,
            all_sender_packets,
            network_graph,
            log: VecDeque::new(),
            dropped_packets: Vec::new()
        }
    }

    pub(crate) fn add_drone_event_to_log(&mut self, e: DroneEvent) {
        match e {
            //had to correct index due to not having a source routing header in the flood request!!
            DroneEvent::PacketSent(packet) => {
                let mut message = String::new();

                match packet.clone().pack_type{
                    PacketType::MsgFragment(fragment) => {
                        message = format!("sent fragment id: {}, data: {:?}", fragment.fragment_index, fragment.data);
                    }
                    PacketType::Ack(ack) => {
                        message = format!("sent ack id: {} to {}", ack.fragment_index, packet.routing_header.destination().unwrap());
                    }
                    PacketType::Nack(nack) => {
                        message = format!("sent nack id: {} to {}", nack.fragment_index, packet.routing_header.destination().unwrap());
                    }
                    PacketType::FloodRequest(rq) => {
                        message = format!("sent flood request: ({},{}) containing {:?}", rq.flood_id, rq.initiator_id, rq.path_trace);
                    }
                    PacketType::FloodResponse(rr) => {
                        message = format!("sent flood response to {:?}, containing {:?}", packet.routing_header.destination(), rr.path_trace)
                    }
                }

                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => *{
                        packet.routing_header.hops.get(packet.routing_header.hop_index - 1).unwrap()
                    },
                };

                let new_log = LogEntry {
                    cause: Cause::Sent,
                    node_id: id_drone,
                    message: message

                };
                self.log.push_back(new_log);
            }
            DroneEvent::PacketDropped(packet) => {
                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => {
                        packet.routing_header.current_hop().unwrap()
                    },
                };

                let new_log = LogEntry {
                    cause: Cause::Dropped,
                    node_id: id_drone,
                    message: format!(
                        "dropped fragment {:?} of packet: {}",
                        packet.session_id, packet
                    ),
                };
                self.log.push_back(new_log);
            }
            DroneEvent::ControllerShortcut(packet) => {
                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => {
                        packet.routing_header.previous_hop().unwrap_or(255)
                    },
                };
                let new_log = LogEntry {
                    cause: Cause::Shortcut,
                    node_id: id_drone,
                    message: format!("Sent shortcut for packet {}", packet),
                };
                self.log.push_back(new_log);
            }
        }
    }
    pub(crate) fn add_client_event_to_log(&mut self, e: ClientEvent){
        match e {
            ClientEvent::PacketSent(packet) => {
                let mut message = String::new();

                match packet.clone().pack_type{
                    PacketType::MsgFragment(fragment) => {
                        message = format!("sent fragment id: {}, data: {:?}", fragment.fragment_index, fragment.data);
                    }
                    PacketType::Ack(ack) => {
                        message = format!("sent ack id: {} to {}", ack.fragment_index, packet.routing_header.destination().unwrap());
                    }
                    PacketType::Nack(nack) => {
                        message = format!("sent nack id: {} to {}", nack.fragment_index, packet.routing_header.destination().unwrap());
                    }
                    PacketType::FloodRequest(rq) => {
                        message = format!("sent flood request: ({},{}) containing {:?}", rq.flood_id, rq.initiator_id, rq.path_trace);
                    }
                    PacketType::FloodResponse(rr) => {
                        message = format!("sent flood response to {:?}, containing {:?}", packet.routing_header.destination(), rr.path_trace)
                    }
                }

                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => *{
                        packet.routing_header.hops.get(packet.routing_header.hop_index).unwrap() //riguarda per type exchange
                    },
                };

                let new_log = LogEntry {
                    cause: Cause::Sent,
                    node_id: id_drone,
                    message: message

                };
                self.log.push_back(new_log);
            }

            ClientEvent::PacketReceived(packet) => {
                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => *{
                        packet.routing_header.hops.get(packet.routing_header.hop_index - 1).unwrap()
                    },
                };

                let new_log = LogEntry {
                    cause: Cause::Sent,
                    node_id: id_drone,
                    message: format!(
                        "Received fragment {:?} of packet: {}",
                        packet.session_id, packet
                    ),
                };
                self.log.push_back(new_log);
            }
            ClientEvent::PacketSendingError(packet) => {
                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => *{
                        packet.routing_header.hops.get(packet.routing_header.hop_index - 1).unwrap()
                    },
                };
                let new_log = LogEntry {
                    cause: Cause::Error,
                    node_id: id_drone,
                    message: format!(
                        "Error in sending fragment {:?} of packet: {}",
                        packet.session_id, packet
                    ),
                };
                self.log.push_back(new_log);
            }
            ClientEvent::AckReceived(packet) => {
                if let Some(ack_id) = packet.routing_header.destination(){
                    match packet.pack_type {
                        PacketType::Ack(ack) => {
                            let new_log = LogEntry{
                                cause: AckReceived,
                                node_id: ack_id,
                                message: format!(
                                    "Node {:?} received Ack of fragment {}"
                                    , ack_id, ack.fragment_index
                                )
                            };
                            self.log.push_back(new_log);
                        }
                        _ => {}
                    }
                }
            }
            ClientEvent::NackReceived(packet) => {
                if let Some(nack_id) = packet.routing_header.destination(){
                    match packet.pack_type {
                        PacketType::Nack(nack) => {
                            let new_log = LogEntry{
                                cause: NackReceived,
                                node_id: nack_id,
                                message: format!(
                                    "Node {:?} received Nack of fragment {}, nack type:{:?} "
                                    , nack_id, nack.fragment_index, nack.nack_type
                                )
                            };
                            self.log.push_back(new_log);
                        }
                        _ => {

                        }
                    }
                }
            }
            ClientEvent::MissingDestination(node_id) => {
                let new_log = LogEntry{
                    cause: MissingDestination,
                    node_id,
                    message: format!("Couldn't reach {} with a packet (missing destination) ",
                                     node_id),
                };
                self.log.push_back(new_log);
            }
            ClientEvent::MissingRoute(node_id) => {
                let new_log = LogEntry{
                    cause: MissingDestination,
                    node_id,
                    message: format!("Couldn't reach {} with a packet (missing route)",
                                            node_id),
                };
                self.log.push_back(new_log);
            }
            ClientEvent::LostMessage(sess, node_id) => {
                let new_log = LogEntry{
                    cause: LostMessage,
                    node_id,
                    message: format!("node {} lost message from session {:?}", node_id, sess),
                };
                self.log.push_back(new_log);
            }
            ClientEvent::LostFragment(sess, node_id, frag_index) => {
                let new_log = LogEntry{
                    cause: LostMessage,
                    node_id,
                    message: format!(
                        "node {} lost message from session {:?} of fragment index {:?}",
                        node_id, sess, frag_index),
                };
                self.log.push_back(new_log);
            }
            ClientEvent::DroneInsideDestination(node_id) => {
                let new_log = LogEntry{
                    cause: DroneInsideDestination,
                    node_id,
                    message: format!("destination removed because destination of id {} is a drone",
                                     node_id)
                };
                self.log.push_back(new_log);
            }
        }
    }
    pub(crate) fn add_server_event_to_log(&mut self, e: ServerEvent){
        match e {
            ServerEvent::PacketSent(packet) => {
                let mut message = String::new();

                match packet.clone().pack_type{
                    PacketType::MsgFragment(fragment) => {
                        message = format!("sent fragment id: {}, data: {:?}", fragment.fragment_index, fragment.data);
                    }
                    PacketType::Ack(ack) => {
                        message = format!("sent ack id: {} to {}", ack.fragment_index, packet.routing_header.destination().unwrap());
                    }
                    PacketType::Nack(nack) => {
                        message = format!("sent nack id: {} to {}", nack.fragment_index, packet.routing_header.destination().unwrap());
                    }
                    PacketType::FloodRequest(rq) => {
                        message = format!("sent flood request: ({},{}) containing {:?}", rq.flood_id, rq.initiator_id, rq.path_trace);
                    }
                    PacketType::FloodResponse(rr) => {
                        message = format!("sent flood response to {:?}, containing {:?}", packet.routing_header.destination(), rr.path_trace)
                    }
                }

                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => *{
                        packet.routing_header.hops.get(packet.routing_header.hop_index - 1).unwrap()
                    },
                };

                let new_log = LogEntry {
                    cause: Cause::Sent,
                    node_id: id_drone,
                    message: message

                };
                self.log.push_back(new_log);
            }
            ServerEvent::PacketReceived(packet) => {
                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => *{
                        packet.routing_header.hops.get(packet.routing_header.hop_index - 1).unwrap()
                    },
                };

                let new_log = LogEntry {
                    cause: Cause::Sent,
                    node_id: id_drone,
                    message: format!(
                        "Received fragment {:?} of packet: {}",
                        packet.session_id, packet
                    ),
                };
                self.log.push_back(new_log);
            }
            ServerEvent::PacketSendingError(packet) => {
                let id_drone = match packet.clone().pack_type{
                    PacketType::FloodRequest(flood) => {
                        let (id, _) = flood.path_trace.last().unwrap();
                        *id
                    }
                    _ => *{
                        packet.routing_header.hops.get(packet.routing_header.hop_index - 1).unwrap()
                    },
                };
                let new_log = LogEntry {
                    cause: Cause::Error,
                    node_id: id_drone,
                    message: format!(
                        "Error in sending fragment {:?} of packet: {}",
                        packet.session_id, packet
                    ),
                };
                self.log.push_back(new_log);
            }
            ServerEvent::AckReceived(packet) => {
                if let Some(ack_id) = packet.routing_header.destination(){
                    match packet.pack_type {
                        PacketType::Ack(ack) => {
                            let new_log = LogEntry{
                                cause: AckReceived,
                                node_id: ack_id,
                                message: format!(
                                    "Node {:?} received Ack of fragment {}"
                                    , ack_id, ack.fragment_index
                                )
                            };
                            self.log.push_back(new_log);
                        }
                        _ => {}
                    }
                }
            }
            ServerEvent::NackReceived(packet) => {
                if let Some(ack_id) = packet.routing_header.destination(){
                    match packet.pack_type {
                        PacketType::Ack(ack) => {
                            let new_log = LogEntry{
                                cause: AckReceived,
                                node_id: ack_id,
                                message: format!(
                                    "Node {:?} received Nack of fragment {}"
                                    , ack_id, ack.fragment_index
                                )
                            };
                            self.log.push_back(new_log);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn spawn_drone(&mut self, pdr: f32, connections: Vec<NodeId>) -> (JoinHandle<()>, NodeId) {
        println!("-");
        let new_id = self.generate_id();
        //aggiorna network graph
        self.network_graph.insert(new_id, (NodeType::Drone, HashSet::from_iter(connections.clone().into_iter())));

        let (control_sender, control_receiver) = unbounded(); //canale per il Sim che manda drone command al drone
        self.drone_command_senders
            .insert(new_id.clone(), control_sender.clone()); // do al sim il sender per questo drone

        let (packet_send, packet_recv) = unbounded(); //canale per il drone, il recv gli va dentro, il send va dato in copia a tutti i droni che vogliono comunicare con lui
        self.all_sender_packets.insert(new_id.clone(), packet_send.clone());

        let mut packet_send = HashMap::new();
        //riempi la hashmap
        for (id, sender) in &self.all_sender_packets {
            for i in connections.clone() {
                if i == *id {
                    packet_send.insert(*id, sender.clone());
                }
            }
        }

        let channel_clone = self.channel_for_drone.clone();

        //crea thread
        let handle = thread::spawn(move || {
            let mut new_drone = SkyLinkDrone::new(
                new_id,
                channel_clone,
                control_receiver,
                packet_recv,
                packet_send,
                pdr,
            );
            new_drone.run();
        });

        for ids in connections.clone() {
            self.add_sender(new_id, ids);
        }

        (handle, new_id)
    }

    fn generate_id(&mut self) -> NodeId {
        //just a function to generate an id that is empty in our hashmap, if is 1-3-4, it should give 2, if it's 1-2-3, should give 4.
        for k in 0..=u8::MAX {
            //If k is not a key in the map, I return it.
            if !self.network_graph.contains_key(&k) {
                return k;
            }
        }
        unreachable!("No free key found");
    }

    pub fn crash_drone(&mut self, id: NodeId) {
        if let Some(sender) = self.drone_command_senders.get(&id) {
            if let Err(e) = sender.send(DroneCommand::Crash) {
                println!("error in crashing drone {}: {:?}", id, e);
            } else {
                println!("crash command sent do the drone {}", id);

                // remove the drone from the neighbour's sends
                if let mut vec = self.network_graph.keys().cloned().collect::<Vec<_>>() {
                    for (neighbor_id, neighbor_sender) in &self.drone_command_senders {
                        if vec.contains(neighbor_id) {
                            neighbor_sender.send(RemoveSender(id)).unwrap()
                        }
                    }
                }

                if let Some(to_be_dropped) = self.drone_command_senders.remove(&id) {
                    drop(to_be_dropped);
                }
                self.log.push_back(LogEntry::new(
                    Cause::Managing,
                    id,
                    "Node crashed".to_string(),
                ));

                self.network_graph.remove(&id);
            }
        } else {
            println!("drone {} not found in the network.", id);
        }
    }

    pub fn remove_senders(&mut self, id: NodeId, id_to_remove: NodeId) {
        if !self.is_node_connected(id, id_to_remove){
            self.log.push_back(LogEntry::new(
                Cause::Error,
                id,
                format!("drone {} is not connected to {}", id_to_remove, id),
            ));
            return;
            //se non sono connessi non far nulla e returna
        }

        //i created get_type that gets the type of the node from the id,
        // depending on that, you send the correct drone/client/server command
        match self.get_type(id){
            None => {
                //if it returned None it wasnt saved inside the network, you shouldn't reach this anyway but you never know
                self.log.push_back(LogEntry::new(
                    Cause::Error,
                    id,
                    format!("drone {id} is not in network",
                )));
                return;
            }
            Some(n_type) => {
                match n_type {
                    Client => {
                        if let Some(sender) = self.client_command_senders.get(&id) {
                            if let Err(_e) = sender.send(ClientCommand::RemoveSender(id_to_remove)) {
                                println!(
                                    "error in removing node {} from client {} senders",
                                    id_to_remove, id
                                );
                            } else {
                                if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id) {
                                    ids.retain(|id| {id != &id_to_remove});
                                }
                                self.log.push_back(LogEntry::new(
                                    Cause::Managing,
                                    id_to_remove,
                                    format!("node {} removed from senders", id),
                                ));
                            }
                        }
                    }
                    Drone => {
                        if let Some(sender) = self.drone_command_senders.get(&id) {
                            if let Err(_e) = sender.send(RemoveSender(id_to_remove)) {
                                println!(
                                    "error in removing drone {} from drone {} senders",
                                    id_to_remove, id
                                );
                            } else {
                                if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id) {
                                    ids.retain(|id| {id != &id_to_remove});
                                }
                                self.log.push_back(LogEntry::new(
                                    Cause::Managing,
                                    id_to_remove,
                                    format!("drone {} removed from senders", id),
                                ));
                            }
                        }
                    }
                    Server => {
                        if let Some(sender) = self.server_command_senders.get(&id) {
                            if let Err(_e) = sender.send(ServerCommand::RemoveSender(id_to_remove)) {
                                println!(
                                    "error in removing node {} from server {} senders",
                                    id_to_remove, id
                                );
                            } else {
                                if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id) {
                                    ids.retain(|id| {id != &id_to_remove});
                                }
                                self.log.push_back(LogEntry::new(
                                    Cause::Managing,
                                    id_to_remove,
                                    format!("node {} removed from senders", id),
                                ));
                            }
                        }
                    }
                }
            }
        }

        match self.get_type(id_to_remove){
            None => {
                self.log.push_back(LogEntry::new(
                    Cause::Error,
                    id,
                    format!("drone {id_to_remove} is not in network",
                    )));
                return;
            }
            Some(n_type) => {
                match n_type {
                    Client => {
                        if let Some(sender) = self.client_command_senders.get(&id_to_remove) {
                            if let Err(_e) = sender.send(ClientCommand::RemoveSender(id)) {
                                println!(
                                    "error in removing node {} from client {} senders",
                                    id, id_to_remove
                                );
                            } else {
                                if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id_to_remove) {
                                    ids.retain(|x| {x != &id});
                                }
                                self.log.push_back(LogEntry::new(
                                    Cause::Managing,
                                    id,
                                    format!("node {} removed from senders", id_to_remove),
                                ));
                            }
                        }
                    }
                    Drone => {
                        if let Some(sender) = self.drone_command_senders.get(&id_to_remove) {
                            if let Err(_e) = sender.send(RemoveSender(id)) {
                                println!(
                                    "error in removing drone {} from drone {} senders",
                                    id, id_to_remove
                                );
                            } else {
                                if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id_to_remove) {
                                    ids.retain(|x| {x != &id});
                                }
                                self.log.push_back(LogEntry::new(
                                    Cause::Managing,
                                    id,
                                    format!("drone {} removed from senders", id_to_remove),
                                ));
                            }
                        }
                    }
                    Server => {
                        if let Some(sender) = self.server_command_senders.get(&id_to_remove) {
                            if let Err(_e) = sender.send(ServerCommand::RemoveSender(id)) {
                                println!(
                                    "error in removing node {} from server {} senders",
                                    id, id_to_remove
                                );
                            } else {
                                if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id_to_remove) {
                                    ids.retain(|x| {x != &id});
                                }
                                self.log.push_back(LogEntry::new(
                                    Cause::Managing,
                                    id,
                                    format!("node {} removed from senders", id_to_remove),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn flood_with(&mut self, node_id: NodeId){
        if !self.does_drone_exist(node_id) {
            self.log.push_back(LogEntry::new(
                Cause::Error,
                node_id,
                format!("drone {} does not exist in this network.", node_id),
            ));
            return;
        }
        match self.get_type(node_id) {
            None => {self.log.push_back(LogEntry::new(
                Cause::Error,
                node_id,
                format!("drone {node_id} is not in network",
                )));
                return;},

            Some(n_type) => {
                match n_type {
                    Client => {
                        if let Some(sender) = self.client_command_senders.get(&node_id) {
                                if let Err(_e) = sender.send(ClientCommand::Flood) {
                                    println!("error flooding");
                                } else {
                                    println!("flooded successfully");
                                }

                        }
                        else {
                            self.log.push_back(LogEntry::new(
                                Cause::Managing,
                                node_id,
                                format!("error flooding"),
                            ));
                        }
                    }
                    _ => {},
                }
            }
        }
    }

    pub fn add_sender(&mut self, id: NodeId, id_to_add: NodeId) {

        if !self.does_drone_exist(id_to_add) {
            self.log.push_back(LogEntry::new(
                Cause::Error,
                id,
                format!("drone {} does not exist in this network.", id_to_add),
            ));
            return;
        }

        match self.get_type(id) {
            None => {
                self.log.push_back(LogEntry::new(
                    Cause::Error,
                    id,
                    format!("drone {id} is not in network",
                    )));
                return;
            }
            Some(n_type) => {
                match n_type {
                    Client => {
                        if let Some(sender) = self.client_command_senders.get(&id) {
                            if let Some(senderpacket) = self.all_sender_packets.get(&id_to_add) {
                                if let Err(_e) = sender.send(ClientCommand::AddSender(id_to_add, senderpacket.clone())) {
                                    println!("error adding drone {} to client {} senders", id_to_add, id);
                                } else {
                                    if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id) {
                                        ids.insert(id_to_add);
                                    }

                                    println!("drone {} added to client {} senders", id_to_add, id);
                                    self.log.push_back(LogEntry::new(
                                        Cause::Managing,
                                        id,
                                        format!("drone {} added to senders", id_to_add),
                                    ));
                                }
                            }
                        }
                        else {
                            self.log.push_back(LogEntry::new(
                                Cause::Managing,
                                id,
                                format!("error adding node {} to senders (client command channel not found)", id_to_add),
                            ));
                        }
                    }
                    Drone => {
                        if let Some(sender) = self.drone_command_senders.get(&id) {
                            if let Some(senderpacket) = self.all_sender_packets.get(&id_to_add) {
                                if let Err(_e) = sender.send(AddSender(id_to_add, senderpacket.clone())) {
                                    println!("error adding drone {} to drone {} senders", id_to_add, id);
                                } else {
                                    if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id) {
                                        ids.insert(id_to_add);
                                    }

                                    println!("drone {} added to drone {} senders", id_to_add, id);
                                    self.log.push_back(LogEntry::new(
                                        Cause::Managing,
                                        id,
                                        format!(" -- drone {} added to senders", id_to_add),
                                    ));
                                }
                            }
                        } else {
                            self.log.push_back(LogEntry::new(
                                Cause::Managing,
                                id,
                                format!("error adding node {} to senders (drone command channel not found)", id_to_add),
                            ));
                        }
                    }
                    Server => {
                        if let Some(sender) = self.server_command_senders.get(&id) {
                            if let Some(senderpacket) = self.all_sender_packets.get(&id_to_add) {
                                if let Err(_e) = sender.send(ServerCommand::AddSender(id_to_add, senderpacket.clone())) {
                                    println!("error adding drone {} to server {} senders", id_to_add, id);
                                } else {
                                    if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id) {
                                        ids.insert(id_to_add);
                                    }

                                    println!("drone {} added to server {} senders", id_to_add, id);
                                    self.log.push_back(LogEntry::new(
                                        Cause::Managing,
                                        id,
                                        format!("drone {} added to senders", id_to_add),
                                    ));
                                }
                            }
                        } else {
                            self.log.push_back(LogEntry::new(
                                Cause::Managing,
                                id,
                                format!("error adding node {} to senders (server command channel not found)", id_to_add),
                            ));
                        }
                    }
                }
            }
        }

        match self.get_type(id_to_add) {
            None => {
                self.log.push_back(LogEntry::new(
                    Cause::Error,
                    id_to_add,
                    format!("drone {id_to_add} is not in network",
                    )));
                return;
            }
            Some(n_type) => {
                match n_type {
                    Client => {
                        if let Some(sender) = self.client_command_senders.get(&id_to_add) {
                            if let Some(senderpacket) = self.all_sender_packets.get(&id) {
                                if let Err(_e) = sender.send(ClientCommand::AddSender(id, senderpacket.clone())) {
                                    println!("error adding drone {} to client {} senders", id, id_to_add);
                                } else {
                                    if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id_to_add) {
                                        ids.insert(id);
                                    }

                                    println!("drone {} added to client {} senders", id, id_to_add);
                                    self.log.push_back(LogEntry::new(
                                        Cause::Managing,
                                        id_to_add,
                                        format!("drone {} added to senders", id),
                                    ));
                                }
                            }
                        }
                        else {
                            self.log.push_back(LogEntry::new(
                                Cause::Managing,
                                id_to_add,
                                format!("error adding node {} to senders (client command channel not found)", id),
                            ));
                        }
                    }
                    Drone => {
                        if let Some(sender) = self.drone_command_senders.get(&id_to_add) {
                            if let Some(senderpacket) = self.all_sender_packets.get(&id) {
                                if let Err(_e) = sender.send(AddSender(id, senderpacket.clone())) {
                                    println!("error adding drone {} to drone {} senders", id, id_to_add);
                                } else {
                                    if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id_to_add) {
                                        ids.insert(id);
                                    }

                                    println!("drone {} added to drone {} senders", id, id_to_add);
                                    self.log.push_back(LogEntry::new(
                                        Cause::Managing,
                                        id_to_add,
                                        format!("drone {} added to senders", id),
                                    ));
                                }
                            }
                        }
                        else {
                            self.log.push_back(LogEntry::new(
                                Cause::Managing,
                                id_to_add,
                                format!("error adding node {} to senders (drone command channel not found)", id),
                            ));
                        }
                    }
                    Server => {
                        if let Some(sender) = self.server_command_senders.get(&id_to_add) {
                            if let Some(senderpacket) = self.all_sender_packets.get(&id) {
                                if let Err(_e) = sender.send(ServerCommand::AddSender(id, senderpacket.clone())) {
                                    println!("error adding drone {} to server {} senders", id, id_to_add);
                                } else {
                                    if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id_to_add) {
                                        ids.insert(id);
                                    }

                                    println!("drone {} added to server {} senders", id, id_to_add);
                                    self.log.push_back(LogEntry::new(
                                        Cause::Managing,
                                        id_to_add,
                                        format!("drone {} added to senders", id),
                                    ));
                                }
                            }
                        }
                        else {
                            self.log.push_back(LogEntry::new(
                                Cause::Managing,
                                id_to_add,
                                format!("error adding node {} to senders (server command channel not found)", id),
                            ));
                        }
                    }
                }
            }
        }
    }

    pub fn get_type(&self, id: NodeId) -> Option<NodeType> {
        let (node_type, _) = match self.network_graph.get(&id) {
            Some(node) => node,
            None => return None,
        };
        Some(*node_type)
    }

    pub fn set_pdr(&mut self, id: NodeId, pdr: f32) {
        let mut capped_pdr = pdr;
        if (pdr >= 100.0){
            capped_pdr = 100.0;
            self.log.push_back(LogEntry::new(
                Cause::Managing,
                id,
                format!("Capped pdr to 100"),
            ));
        }

        if let Some(sender) = self.drone_command_senders.get(&id) {
            if let Err(_e) = sender.send(DroneCommand::SetPacketDropRate(capped_pdr)) {
                println!("error in setting drone {} pdr to {}", id, capped_pdr);
            } else {
                println!("setting drone {} pdr to {}", id, capped_pdr);
                self.log.push_back(LogEntry::new(
                    Cause::Managing,
                    id,
                    format!("drone now has pdr set to {}", capped_pdr),
                ));
            }
        }
    }

    pub fn does_drone_exist(&mut self, id: NodeId) -> bool {
        let mut exists = false;
        if self.network_graph.contains_key(&id){
            exists = true;
        }
        exists
    }

    pub fn is_node_connected (&self, id: NodeId, rhs: NodeId) -> bool {
        let mut out = true;
        if let Some((_node, vec)) = self.network_graph.get(&id){
            if !vec.contains(&rhs) {
                out = false;
            }
        }
        if let Some((_node, vec)) = self.network_graph.get(&rhs){
            if !vec.contains(&id) {
                out = false;
            }
        }
        out
    }

    pub(crate) fn resend_packet(&self, _p0: &Packet) {
        //tell client/server (depending on source_id) to send it again recomputing the way
    }


}

pub enum Cause {
    Dropped,
    Sent,
    Shortcut,
    Managing, //this cause is for the log entry "caused" by manipulation of the SC
    Error,
    AckReceived,
    NackReceived,
    MissingDestination,
    LostMessage,
    LostFragment,
    DroneInsideDestination
}

pub struct LogEntry {
    pub cause: Cause,
    pub node_id: NodeId,
    pub message: String,
}
impl LogEntry {
    pub fn new(cause: Cause, node_id: NodeId, message: String) -> LogEntry {
        LogEntry {
            cause,
            node_id,
            message,
        }
    }
    pub fn get_id(&self) -> NodeId {
        self.node_id
    }
}

impl Display for LogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Node {} notified {}", self.node_id, self.message)
    }
}

impl Debug for LogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "id: {}", self.node_id)
    }
}
