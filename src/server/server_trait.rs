use crate::network_edge::NetworkEdge;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_type::ServerType;
use crossbeam_channel::{Receiver, Sender};
use dr_ones::Packet;
use std::collections::HashMap;
use wg_2024::network::*;
use crate::message::MessageType;

pub trait Server<M: MessageType>: NetworkEdge<M> {
    fn new(
        id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self;

    fn run(&mut self);

    fn handle_command(&mut self, command: ServerCommand);

    fn get_server_type(&self) -> ServerType;
}
