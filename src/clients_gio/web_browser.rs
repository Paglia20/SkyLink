use std::collections::{HashMap, HashSet};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;
use crate::clients_gio::client_trait::{Client};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_type::ClientType;
use crate::message::{Message, MessageType, TextRequest, TextResponse};
use crate::network_edge::NetworkEdge;

pub struct WebBrowser {
    client_type: ClientType,
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, //just like drones

    paths: HashMap<NodeId, Vec<NodeId>>, //lui ha bisogno di parlare solo con content server, di cui non occorre specificare il type visto che sarà un textserver
}

impl NetworkEdge for WebBrowser {
    type RequestType = TextRequest;
    type ResponseType = TextResponse;

    fn send_message<M: MessageType>(&mut self, message: Message<M>, destination: NodeId) -> Result<(), String> {
        todo!()
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