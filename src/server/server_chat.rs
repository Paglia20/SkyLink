use crate::message::{ChatRequest, ChatResponse, Message, MessageType};
use crate::network_edge::NetworkEdge;
use crate::routing::RouteList;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_trait::Server;
use crate::server::server_type::ServerType;
use crossbeam_channel::{select_biased, Receiver, Sender};
use dr_ones::Packet;
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, PacketType};

pub struct ChatServer {
    node_id: NodeId,
    command_recv: Receiver<ServerCommand>,
    event_send: Sender<ServerEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, // Just like drones

    paths: HashMap<NodeId, RouteList>, // These NodeId are just client_chat nodes.
    fragments: HashMap<u64, Vec<Fragment>>, // The u64 is the session id.
}

impl NetworkEdge for ChatServer {
    type RequestType = ChatRequest;
    type ResponseType = ChatResponse;

    fn send_message<M: MessageType>(
        &mut self,
        _message: Message<M>,
        _destination: NodeId,
    ) -> Result<(), String> {
        todo!()
    }
}
impl Server for ChatServer {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ChatServer {
            node_id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            flood_ids: HashSet::new(),
            paths: HashMap::new(),
            fragments: Default::default(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.command_recv) -> cmd => {
                    if let Ok(command) = cmd {
                        self.handle_command(command);
                    }
                }
                recv(self.packet_recv) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
            }
        }
    }

    fn handle_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::MsgFragment(_) => {}
            PacketType::Ack(_) => {}
            PacketType::Nack(_) => {}
            PacketType::FloodRequest(_) => {}
            PacketType::FloodResponse(_) => {}
        }
    }

    fn handle_command(&mut self, command: ServerCommand) {
        match command {
            ServerCommand::RemoveSender(_) => {}
            ServerCommand::AddSender(_, _) => {}
            ServerCommand::SendPacket(_packet) => {} // Remove the _ before packet when you'll use it.
        }
    }
    fn get_server_type(&self) -> ServerType {
        ServerType::Chat
    }
}
