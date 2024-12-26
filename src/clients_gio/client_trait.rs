use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_type::ClientType;
use crate::message::MessageType;
use crate::network_edge::NetworkEdge;
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use wg_2024::network::*;
use wg_2024::packet::Packet;

pub trait Client<M: MessageType>: NetworkEdge<M> {
    fn new(
        id: NodeId,
        command_recv: Receiver<ClientCommand<M>>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self;

    fn run(&mut self);

    fn handle_command(&mut self, command: ClientCommand<M>);

    fn get_client_type(&self) -> ClientType;
}
