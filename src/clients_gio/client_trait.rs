use std::collections::HashMap;
use crossbeam_channel::{Receiver, Sender};
use wg_2024::network::*;
use wg_2024::packet::Packet;
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::network_edge::NetworkEdge;

pub enum ClientType{
    WebBrowser,
    ChatClient,
}

pub trait Client: NetworkEdge {
    fn new(
        id: NodeId,
        event_send: Sender<ClientEvent>,
        command_recv: Receiver<ClientCommand>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    )-> Self;

    fn send_request(&mut self, _request: Self::RequestType);

    fn handle_response(&mut self, _response: Self::ResponseType);

    fn get_client_type(&self) -> ClientType;

}