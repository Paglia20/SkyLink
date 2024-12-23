use std::collections::{HashMap, HashSet};
use crossbeam_channel::{Receiver, Sender};
use wg_2024::network::NodeId;
use wg_2024::packet::{Packet};
use crate::clients_gio::client_trait::{Client, ClientType};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::message::{ChatRequest, ChatResponse};
use crate::network_edge::NetworkEdge;

pub struct ChatClient {
    client_type: ClientType,
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, //just like drones


    paths: HashMap<NodeId, Vec<NodeId>>, //lui ha bisogno di parlare solo con chat server (il node id è di un chatserver)
    contact_list: HashMap<NodeId, NodeId>,  //first NodeId is the client he want to comunicate to, the second one is the server he has to write to, this two hash might be merged in future
}

impl NetworkEdge for ChatClient {
    type RequestType = ChatRequest;
    type ResponseType = ChatResponse;
}
impl Client for ChatClient  {

    fn new(id: NodeId, event_send: Sender<ClientEvent>, command_recv: Receiver<ClientCommand>, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>) -> Self {
        ChatClient{
            client_type: ClientType::WebBrowser,
            node_id: id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            flood_ids: HashSet::new(),
            paths: HashMap::new(),
            contact_list: HashMap::new(),
        }
    }


    fn send_request(&mut self, _request: Self::RequestType) {
        todo!()
    }

    fn handle_response(&mut self, _response: Self::ResponseType) {
        todo!()
    }

    fn get_client_type(&self) -> ClientType {
        todo!()
    }
}