use crate::routing::{Nodes, RouteList};
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_trait::Server;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{FloodRequest, Fragment, NodeType, Packet, PacketType};

pub struct ServerStruct {
    pub node_id: NodeId,
    pub command_recv: Receiver<ServerCommand>,
    pub event_send: Sender<ServerEvent>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,
    pub flood_ids: HashSet<(u64, NodeId)>, // Just like drones
    pub used_session_id: HashSet<u64>,     // Do we need this?

    pub paths: HashMap<NodeId, (u8, RouteList)>, // These NodeId are servers and clients, the u8 indicate if usable (1), if not usable (2), or if yet to be checked (0)
    pub nodes: Nodes, // Map of all Nodes, to apply checks on the PDRs.
    pub contact_list: HashMap<NodeId, Vec<NodeId>>, // First NodeId is the client we communicate with, the second one is the vec of servers that make the connection possible
    pub fragments: HashMap<(u64, NodeId, NodeId), Vec<Fragment>>, //(session_id, source, destination)
    pub arrived_messages: HashMap<NodeId, Vec<String>>,
    pub unsent_fragments: (u8, HashMap<(u64, NodeId, NodeId), Vec<(Fragment)>>),
    // The second NodeId is the destination, the u8 is a counter (for now to the maximum I guess) to avoid sending too much stuff.
}

impl ServerStruct {
    pub fn new(
        node_id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ServerStruct {
            node_id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            flood_ids: HashSet::new(),
            used_session_id: HashSet::new(),
            paths: HashMap::new(),
            nodes: Nodes::new(),
            contact_list: HashMap::new(),
            fragments: HashMap::new(),
            arrived_messages: HashMap::new(),
            unsent_fragments: (0, HashMap::new()),
        }
    }

    pub fn handle_flood_request(&mut self, mut flood_request: FloodRequest, packet: Packet) -> bool{
        flood_request
            .path_trace
            .push((self.node_id, NodeType::Server));
        // I first add myself to the path_trace.

        // I try to insert the new flood in the already known ones.
        if self.flood_ids.insert((flood_request.flood_id.clone(),flood_request.initiator_id.clone())) {

            if self.packet_send.len() == 1 {
                return false
            } else {
                let mut prev = flood_request.initiator_id.clone();
                if flood_request.path_trace.clone().len() > 1 {
                    prev = flood_request
                        .path_trace
                        .get(flood_request.path_trace.len() - 2)
                        .unwrap()
                        .0;
                }
                //I update the path_trace in the packet.
                //packet.pack_type = PacketType::FloodRequest(flood_request);
                for (key, _) in self.packet_send.iter() {
                    //println!("Previous: {}", prev);
                    //println!("Key: {}", key);
                    if *key != prev {
                        //I send the flooding to everyone except the node I received it from.
                        if let Ok(_) =
                            self.packet_send.get(key).unwrap().send(packet.clone())
                        {
                            // self.send_event(ServerEvent::PacketSent(packet.clone()));
                            //If the message was sent, I also notify the sim controller.
                        } //There's no else, since I don't care of nodes which can't be reached.
                    }
                }
            }
            true
        } else {
            false
        }
    }
}
