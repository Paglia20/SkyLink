use std::collections::HashMap;
use crossbeam_channel::{select_biased, Receiver, Sender, TrySendError};
use wg_2024::network::*;
use wg_2024::packet::Packet;
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_type::ClientType;
use crate::network_edge::NetworkEdge;


pub trait Client: NetworkEdge {
    fn new(
        id: NodeId,
        event_send: Sender<ClientEvent>,
        command_recv: Receiver<ClientCommand>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    )-> Self;

    fn run(&mut self);

    fn handle_packet(&mut self, packet: Packet);

    fn get_client_type(&self) -> ClientType;

    fn handle_command (&mut self, command: ClientCommand);



}