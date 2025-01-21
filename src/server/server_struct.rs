use crate::message::{ChatRequest, ChatResponse, Message};
use crate::network_edge::NetworkEdge;
use crate::routing::RouteList;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_trait::Server;
use crate::server::server_type::ServerType;
use crossbeam_channel::{select_biased, Receiver, Sender};
use dr_ones::Packet;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, PacketType};

pub struct ServerStruct {
    node_id: NodeId,
    command_recv: Receiver<ServerCommand>,
    event_send: Sender<ServerEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, // Just like drones

    paths: HashMap<NodeId, RouteList>, // These NodeId are just client_chat nodes.
    fragments: HashMap<u64, Vec<Fragment>>, // The u64 is the session id.
}

impl Server {
    fn new(
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
            paths: HashMap::new(),
            fragments: HashMap::new(),
        }
    }
}
