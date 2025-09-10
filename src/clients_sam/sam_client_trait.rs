use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_type::ClientType;
use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use wg_2024::network::*;
use wg_2024::packet::Packet;

pub trait ClientTrait: NetworkEdge + NetworkEdgeErrors{
    fn new(
        id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self;

    fn run(&mut self);

    fn handle_command(&mut self, command: ClientCommand);

    fn get_client_type(&self) -> ClientType;

    fn send_event(&self, ce: ClientEvent);
}

