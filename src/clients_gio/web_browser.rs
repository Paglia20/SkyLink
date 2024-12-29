use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_type::ClientType;
use crate::message::{Message, TextRequest, TextResponse};
use crate::network_edge::NetworkEdge;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, Packet};
use crate::routing::RouteList;

pub struct WebBrowser{
    client_type: ClientType,
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, // Just like drones

    paths: HashMap<NodeId, Vec<NodeId>>, // NodeId will only be content Servers (text servers).
}

impl NetworkEdge for WebBrowser {
    fn send_message(
        &mut self,
        _message: Message,
        _destination: NodeId, // Remove the _ before message and destination when you'll use them.
    ) {
        unimplemented!()
    }

    fn handle_packet(&mut self, _packet: Packet) {
        unimplemented!()
    }

    fn handle_message(&mut self,_message: Message ) {
        unimplemented!()
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {

    }
    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {

    }
    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {

    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        unimplemented!()
    }
}
// impl Client for WebBrowser {
//     fn new(id: NodeId, event_send: Sender<ClientEvent>, command_recv: Receiver<ClientCommand>, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>) -> Self {
//         WebBrowser{
//             client_type: ClientType::WebBrowser,
//             node_id: id,
//             command_recv,
//             event_send,
//             packet_recv,
//             packet_send,
//             flood_ids: HashSet::new(),
//             paths: HashMap::new(),
//         }
//     }
//
//
//     fn send_request(&mut self, _request: Self::RequestType) {
//         todo!()
//     }
//
//     fn handle_response(&mut self, _response: Self::ResponseType) {
//         todo!()
//     }
//
//     fn get_client_type(&self) -> ClientType {
//         todo!()
//     }
// }
