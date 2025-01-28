use crate::network_edge::NetworkEdge;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_type::ServerType;
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use wg_2024::network::*;
use wg_2024::packet::Packet;

pub trait Server: NetworkEdge {
    fn new(
        id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        files: Vec<String> // Left empty in chat servers.
    ) -> Self;

    fn run(&mut self);

    fn handle_command(&mut self, command: ServerCommand);

    fn get_server_type(&self) -> ServerType;

    fn send_event(&self, se: ServerEvent);
}
