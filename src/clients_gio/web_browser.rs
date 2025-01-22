use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_type::ClientType;
use crate::message::{ContentType, Message, TextRequest, TextResponse};
use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, Packet};
use crate::clients_gio::client_chat::ChatClient;
use crate::clients_gio::client_command::ClientEvent::WrongDestinationType;
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_struct::ClientStruct;
use crate::DEBUG_MODE;
use crate::routing::{Nodes, RouteList};

pub struct WebBrowser{
    comm: ClientStruct, //common client duh

    //web browser specks
    arrived_content: HashMap<NodeId, Vec<Vec<u8>>>,
    catalogue: HashMap<NodeId, Vec<u64>>,
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
        self.comm.node_id
    }
}

impl NetworkEdgeErrors for WebBrowser {
    fn check_type(&mut self, id: NodeId) {
        todo!()
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        todo!()
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        todo!()
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        todo!()
    }
}

impl ClientTrait for WebBrowser {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        WebBrowser {
            comm: ClientStruct::new(node_id, command_recv, event_send, packet_recv, packet_send),
            arrived_content: Default::default(),
            catalogue: Default::default(),
        }
    }

    fn run(&mut self) {
        self.comm.run();
    }

    fn handle_command(&mut self, command: ClientCommand) {
        self.comm.handle_command(command);
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::WebBrowser
    }

    fn send_event(&self, ce: ClientEvent) {
        self.comm.send_event(ce);
    }
}
