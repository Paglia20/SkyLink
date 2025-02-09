use super::sam_client_base::SamClientBase;
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use super::sam_events::{ConnectionState};
use super::sam_client_trait::Client;
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::message::{Message, ContentType, ChatRequest, ChatResponse, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, Packet};

struct ChatSession {
    server_id: NodeId,
    last_message_id: u64,
    messages: Vec<String>,
}

pub struct ChatClient {
    base: SamClientBase,
    chat_servers: HashSet<NodeId>,
    active_sessions: HashMap<NodeId, ChatSession>,
    available_contacts: HashMap<NodeId, HashSet<NodeId>>,
    message_cache: HashMap<(NodeId, u64), String>,
}

impl NetworkEdge for ChatClient {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.base.send_message(message, destination)
    }

    fn handle_packet(&mut self, packet: Packet) {
        self.base.handle_packet(packet)
    }

    fn handle_message(&mut self, message: Message) {
        match message.content {
            ContentType::ChatResponse(response) => {
                self.handle_chat_response(message.source_id, response);
            }
            ContentType::TypeExchange(exchange) => {
                match exchange {
                    TypeExchange::TypeRequest { from } => {
                        let response = Message::new(
                            self.base.node_id,
                            self.base.get_session_id(),
                            ContentType::TypeExchange(TypeExchange::TypeResponse {
                                edge_type: EdgeType::Client(ClientType::ChatClient),
                                from: self.base.node_id,
                            })
                        );
                        self.send_message(response, from);
                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        if let EdgeType::Server(server_type) = edge_type {
                            if matches!(server_type, crate::server::server_type::ServerType::Chat) {
                                self.chat_servers.insert(from);
                                self.base.node_states.insert(from, ConnectionState::Ready);
                                self.base.send_event(ClientEvent::SendDestinations(
                                    self.base.node_id,
                                    from
                                ));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        self.base.send_fragment(fragment, destination, session_id)
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.base.add_unsent_fragment(fragment, session_id, destination)
    }

    fn send_fragment_after_nack(&mut self, packet_session_id: u64, nack: Nack) {
        self.base.send_fragment_after_nack(packet_session_id, nack)
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        self.base.send_ack(packet, fragment_index)
    }

    fn flood(&mut self) {
        self.base.flood()
    }

    fn get_flood_id(&mut self) -> u64 {
        self.base.get_flood_id()
    }

    fn get_session_id(&mut self) -> u64 {
        self.base.get_session_id()
    }

    fn get_src_id(&self) -> NodeId {
        self.base.get_src_id()
    }

    fn remove_sender(&mut self, id: NodeId) {
        self.base.remove_sender(id)
    }
}

impl NetworkEdgeErrors for ChatClient {
    fn check_type(&mut self, id: NodeId) {
        self.base.check_type(id)
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        self.base.is_state_ok(node_id)
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.base.send_nack_message(dst, nack)
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        self.base.send_drone_nack(dst, nack)
    }
}

impl Client for ChatClient {
    fn new(
        id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ChatClient {
            base: SamClientBase::new(id, command_recv, event_send, packet_recv, packet_send),
            chat_servers: HashSet::new(),
            active_sessions: HashMap::new(),
            available_contacts: HashMap::new(),
            message_cache: HashMap::new(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.base.command_recv) -> cmd => {
                    if let Ok(command) = cmd {
                        self.handle_command(command);
                    }
                }
                recv(self.base.packet_recv) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
            }
        }
    }

    fn handle_command(&mut self, command: ClientCommand) {
        match command {
            ClientCommand::SendMSG(dst, content) => {
                self.send_chat_message(dst, content);
            }
            ClientCommand::Register(server_id) => {
                let message = Message::new(
                    self.base.node_id,
                    self.base.get_session_id(),
                    ContentType::ChatRequest(ChatRequest::Register(self.base.node_id))
                );
                self.base.send_message(message, server_id);
            }
            ClientCommand::RetrieveList(server_id) => {
                let message = Message::new(
                    self.base.node_id,
                    self.base.get_session_id(),
                    ContentType::ChatRequest(ChatRequest::ClientList)
                );
                self.base.send_message(message, server_id);
            }
            _ => self.base.handle_command(command)
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::ChatClient
    }

    fn send_event(&self, ce: ClientEvent) {
        self.base.send_event(ce);
    }
}

impl ChatClient {
    fn handle_chat_response(&mut self, source: NodeId, response: ChatResponse) {
        match response {
            ChatResponse::ClientList(clients) => {
                let entry = self.available_contacts.entry(source).or_insert_with(HashSet::new);

                // Send destination event for the server
                self.base.send_event(ClientEvent::SendDestinations(
                    self.base.node_id,
                    source
                ));

                for client_id in clients {
                    if !entry.contains(&client_id) {
                        entry.insert(client_id);
                        // Send contact event for each new client
                        self.base.send_event(ClientEvent::SendContactsToSC(
                            self.base.node_id,
                            client_id
                        ));
                    }
                }
            }
            ChatResponse::MessageFrom { from, message } => {
                let msg_id = if let Some(session) = self.active_sessions.get_mut(&from) {
                    session.last_message_id += 1;
                    session.messages.push(message.clone());
                    session.last_message_id
                } else {
                    let mut session = ChatSession {
                        server_id: source,
                        last_message_id: 0,
                        messages: vec![message.clone()],
                    };
                    let msg_id = session.last_message_id;
                    self.active_sessions.insert(from, session);
                    msg_id
                };

                self.message_cache.insert((from, msg_id), message.clone());

                // Send received chat text event
                self.base.send_event(ClientEvent::ReceivedChatText(
                    from,
                    self.base.node_id,
                    message
                ));
            }
            ChatResponse::ClientNotFound(client_id) => {
                if let Some(contacts) = self.available_contacts.get_mut(&source) {
                    contacts.remove(&client_id);
                }
                self.active_sessions.remove(&client_id);

                // Send missing contacts event
                self.base.send_event(ClientEvent::MissingContacts(
                    self.base.node_id,
                    client_id
                ));
            }
        }
    }

    fn send_chat_message(&mut self, destination: NodeId, content: String) {
        if let Some(&server_id) = self.chat_servers.iter().next() {
            let message = Message::new(
                self.base.node_id,
                self.base.get_session_id(),
                ContentType::ChatRequest(ChatRequest::SendMessage {
                    from: self.base.node_id,
                    to: destination,
                    message: content.clone(),
                })
            );

            if let Some(session) = self.active_sessions.get_mut(&destination) {
                session.last_message_id += 1;
                session.messages.push(content.clone());
            } else {
                self.active_sessions.insert(destination, ChatSession {
                    server_id,
                    last_message_id: 0,
                    messages: vec![content.clone()],
                });
            }

            self.base.send_message(message, server_id);
        }
    }
}