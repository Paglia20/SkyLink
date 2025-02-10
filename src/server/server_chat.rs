use crate::message::{ChatRequest, ChatResponse, ContentType, EdgeNackType, Message, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_trait::Server;
use crate::server::server_type::ServerType;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, FloodResponse, Fragment, Nack, NackType, Packet};
use crate::clients_gio::client_type::ClientType;
use crate::server::server_struct::ServerStruct;


pub struct ChatServer {
    server_struct: ServerStruct,
    registered_clients: HashSet<NodeId>,
}

impl NetworkEdge for ChatServer {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.server_send_message(message, destination, self.get_src_id());
    }

    fn handle_packet(&mut self, packet: Packet) {
        self.server_handle_packet(packet);
    }

    fn handle_message(&mut self, message: Message) {
        
        match message.content {
            ContentType::ChatRequest(chat_request) => {
                match chat_request {
                    ChatRequest::ClientList => {
                        self.send_chat_list(message.source_id);
                    },
                    ChatRequest::Register(node_id) => {
                        if self.registered_clients.insert(node_id) {
                            self.send_event(ServerEvent::ClientRegistered(self.get_src_id(), node_id));
                        } else {
                            self.send_event(ServerEvent::ClientAlreadyRegistered(self.get_src_id(), node_id));
                        }
                        // I update the ChatList in all registered clients.
                        for id in self.registered_clients.clone().iter() {
                            self.send_chat_list(*id);
                        }
                    },
                    ChatRequest::SendMessage {from, to, message: msg} => {
                        if self.registered_clients.contains(&to) {
                            // If I have the client, I send the message.

                            // I also send the list, to be sure that the client has the chat with who is contacting him.
                            // Send the actual message.
                            let resp = ChatResponse::MessageFrom {from, message: msg};
                            let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::ChatResponse(resp));
                            self.send_message(msg, to);
                        } else {
                            // Otherwise I notify back.
                            let resp = ChatResponse::ClientNotFound(to);
                            let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::ChatResponse(resp));
                            self.send_message(msg, from);
                        }
                    }
                }
            },
            ContentType::TypeExchange(exchange) => {
                match exchange {
                    TypeExchange::TypeRequest { from } => {
                        let type_resp = TypeExchange::TypeResponse {
                            edge_type: EdgeType::Server(self.get_server_type()),
                            from: self.get_src_id(),
                        };
                        let message = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TypeExchange(type_resp));

                        // I don't have to worry about having the path to 'from', since if it's missing floods will be initialized afterward.
                        self.send_message(message, from);
                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        match edge_type {
                            EdgeType::Client(ClientType::ChatClient) => {
                                self.update_node_state(from, 1);
                                // I set it as a usable contact
                            },
                            _ => {
                                self.update_node_state(from, 2);
                                // I set it as a not usable contact.
                            }
                        }
                        // self.type_checked(from);
                    }
                }
            }
            ContentType::EdgeNack(nack) => {
                self.handle_edge_nack(nack, message.source_id, message.session_id);
            },
            _ => {
                // All other types of message shouldn't be received by this server.
                let new_nack = self.create_nack(EdgeNackType::UnexpectedMessage);
                self.send_nack_message(message.source_id, new_nack);
            }
        }

    }

    fn send_fragment(&mut self, _: Fragment, _: NodeId, _: u64) {
        unimplemented!()
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.server_struct.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, _packet_session_id: u64, _nack: Nack)  {
        // self.server_send_fragment_after_nack(packet_session_id, nack, self.get_src_id());
        unimplemented!()
    }

    fn send_ack(&mut self, _: Packet, _: u64) {
        unimplemented!()
    }

    fn flood(&mut self) {
        self.start_flood();
    }

    fn get_flood_id(&mut self) -> u64 {
        self.server_struct.get_flood_id()
    }

    fn get_session_id(&mut self) -> u64 {
        self.server_struct.get_session_id()
    }

    fn get_src_id(&self) -> NodeId {
        self.server_struct.node_id
    }

    fn remove_sender(&mut self, id: NodeId) {
        self.server_struct.packet_send.remove(&id);
    }
}

impl NetworkEdgeErrors for ChatServer {
    fn check_type(&mut self, _id: NodeId) {
        // self.server_check_type(id);
        unimplemented!()
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        self.server_is_state_ok(node_id)
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.send_message(nack, dst);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType, session_id: u64) {
        self.server_send_drone_nack(dst, nack, session_id);
    }
}

impl Server for ChatServer {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        _files: Vec<String>
    ) -> Self {
        let server_struct = ServerStruct::new(node_id, command_recv, event_send, packet_recv, packet_send);
        ChatServer {
            server_struct,
            registered_clients: HashSet::new(),
        }
    }
    fn remove_faulty_connection(&mut self, node: NodeId) {
        self.server_struct.network.remove_faulty_connection(self.get_src_id(), node);
    }
    fn handle_command(&mut self, command: ServerCommand) {
        match command {
            ServerCommand::RemoveSender(node_id) => {
                self.remove_sender(node_id)
            }
            ServerCommand::AddSender(node_id, sender) => {
                self.server_struct.packet_send.insert(node_id, sender);
            }
            ServerCommand::Flood =>{
                self.flood();
            },
            ServerCommand::InstantCrash => {
                self.server_struct.is_running = false;
            }
            ServerCommand::AddFile(file) => {
                // I notify the sim controller that I shouldn't have received this command.
                self.send_event(ServerEvent::WrongCommandGiven(self.get_src_id(), ServerCommand::AddFile(file)));
            }
        }
    }
    fn send_event(&self, new_nack: ServerEvent) {
        self.server_struct.send_event(new_nack);
    }
    fn handle_fragment(&mut self, fragment: Fragment, packet: Packet) {
        self.server_struct.handle_fragment(fragment, packet);
    }
    fn handle_flood_request(&mut self, flood_request: FloodRequest, session_id: u64) -> bool {
        self.server_struct.handle_flood_request(flood_request.clone(), session_id)
    }
    fn handle_nack(&mut self, nack: Nack, packet: Packet) -> bool {
        self.server_struct.handle_nack(nack.clone(), packet)
    }
    fn positive_feed(&mut self, nodes: Vec<NodeId>) {
        self.server_struct.network.positive_feedback(nodes);
    }
    fn save_flood_response(&mut self, flood_resp: FloodResponse) -> bool {
        self.server_struct.save_flood_response(flood_resp)
    }
    fn send_to_all(&mut self, packet: Packet) {
        self.server_struct.send_to_all(packet);
    }
    fn update_node_state(&mut self, source_id: NodeId, value: u8) {
        // println!("update_node_state: source_id={:?}, value={:?}", source_id, value);
        self.server_struct.network.update_state(source_id, value);
    }
    fn check_to_resend_fragments(&mut self) -> bool {
        self.server_struct.check_to_resend_fragments()
    }
    fn reset_unsent_fragments(&mut self) {
        self.server_struct.reset_unsent_fragments();
    }
    fn can_flood(&mut self) -> bool {
        self.server_struct.can_flood()
    }
    fn starting_to_flood(&mut self) {
        self.server_struct.starting_to_flood();
    }
    fn can_type_check(&mut self, dst: NodeId) -> bool {
        self.server_struct.can_type_check(dst)
    }
    fn type_checked(&mut self, src: NodeId) {
        self.server_struct.type_checked(src);
    }
    fn add_destination_without_path(&mut self, dst: NodeId) {
        self.server_struct.add_destination_without_path(dst);
    }
    fn is_running(&self) -> bool {
        self.server_struct.is_running
    }
    fn get_command_recv(&self) -> Receiver<ServerCommand> {
        self.server_struct.command_recv.clone()
    }
    fn get_packet_recv(&self) -> Receiver<Packet> {
        self.server_struct.packet_recv.clone()
    }
    fn get_fragments_hm(&mut self) -> &mut HashMap<(u64, NodeId), (NodeId, Vec<Fragment>)> {
        self.server_struct.get_fragments_hm()
    }
    fn get_packet_sender(&self, next_id: &NodeId) -> Option<&Sender<Packet>> {
        self.server_struct.packet_send.get(next_id)
    }
    fn get_srh(&self, destination: NodeId) -> Option<SourceRoutingHeader> {
        self.server_struct.network.get_srh(&self.get_src_id(), &destination)
    }
    fn get_node_state(&self, destination: NodeId) -> Option<u8> {
        self.server_struct.network.get_state(&destination)
    }
    fn get_unresolved(&self) -> Vec<NodeId> {
        self.server_struct.network.get_unresolved()
    }
    fn get_fragment_to_process(&self) -> Vec<(Fragment, (u64, NodeId, NodeId))> {
        self.server_struct.get_fragment_to_process()
    }
    fn get_server_type(&self) -> ServerType {
        ServerType::Chat
    }
}

impl ChatServer {
    fn send_chat_list(&mut self, destination: NodeId) {
        // I send the list of every client except the one that asked for it.
        let resp = ChatResponse::ClientList(self
            .registered_clients
            .iter()
            .copied()
            .filter(|id| *id != destination)
            .collect::<Vec<NodeId>>());
        let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::ChatResponse(resp));
        self.send_message(msg, destination);
    }
}