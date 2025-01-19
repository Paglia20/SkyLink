use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_type::ClientType;
use crate::message::{ContentType, Message, TextRequest, TextResponse};
use crate::network_edge::NetworkEdge;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, Packet};
use crate::clients_gio::client_command::ClientEvent::WrongDestinationType;
use crate::DEBUG_MODE;
use crate::routing::{Nodes, RouteList};

pub struct WebBrowser{
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, // Just like drones
    used_session_id: HashSet<u64>,     // Do we need this?


    paths: HashMap<NodeId, (u8, RouteList)>, // These NodeId are servers and clients, the u8 indicate if usable (1), if not usable (2), or if yet to be checked (0)
    nodes: Nodes, // Map of all Nodes, to apply checks on the PDRs.
    contact_list: HashMap<NodeId, Vec<NodeId>>, // First NodeId is the client we communicate with, the second one is the vec of servers that make the connection possible
    fragments: HashMap<(u64, NodeId, NodeId), Vec<Fragment>>, //(session_id, source, destination)
    arrived_messages: HashMap<NodeId, Vec<Vec<u8>>>,
    unsent_fragments: (u8, HashMap<(u64, NodeId, NodeId), Vec<(Fragment)>>),
    // The second NodeId is the destination, the u8 is a counter (for now to the maximum I guess) to avoid sending too much stuff.
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

    fn flood(&mut self) {
        todo!()
    }
    fn get_flood_id(&mut self) -> u64 {
        todo!()
    }

    fn get_session_id(&mut self) -> u64 {
        todo!()
    }

    fn get_src_id(&self) -> NodeId {
        self.node_id
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
