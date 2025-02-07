// sam_client_chat_system.rs
use super::sam_client_base::SamClientBase;
use super::sam_events::{ClientCommand, ClientEvent, ConnectionState};
use super::sam_client_trait::Client;
use super::sam_client_type::ClientType;
use crate::message::{Message, ContentType, ChatRequest, ChatResponse, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, Packet};


struct ChatSession {
    server_id: NodeId,
    last_message_id: u64,
    messages: Vec<String>,
}

pub struct ChatClient {
    base: SamClientBase,
    chat_servers: HashSet<NodeId>,           // Available chat servers
    active_sessions: HashMap<NodeId, ChatSession>,  // Active chat sessions per client
    available_contacts: HashMap<NodeId, HashSet<NodeId>>, // Server -> Set of available clients
    message_cache: HashMap<(NodeId, u64), String>,  // Cache messages by (source, msg_id)
}

impl ChatClient {
    pub fn new(
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

    fn handle_chat_response(&mut self, source: NodeId, response: ChatResponse) {
        match response {
            ChatResponse::ClientList(clients) => {
                let entry = self.available_contacts.entry(source).or_insert_with(HashSet::new);
                for client_id in clients {
                    if !entry.contains(&client_id) {
                        entry.insert(client_id);
                        // Notify about new contact
                        self.base.send_event(ClientEvent::SendContactsToSC(
                            self.base.node_id,
                            client_id
                        ));
                    }
                }
            }
            ChatResponse::MessageFrom { from, message } => {
                // Store in cache with unique message ID
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

                // Send event as expected by project
                self.base.send_event(ClientEvent::ReceivedChatText(
                    from,
                    self.base.node_id,
                    message
                ));
            }
            ChatResponse::ClientNotFound(client_id) => {
                // Clean up our internal state
                if let Some(contacts) = self.available_contacts.get_mut(&source) {
                    contacts.remove(&client_id);
                }
                self.active_sessions.remove(&client_id);

                // Send expected event
                self.base.send_event(ClientEvent::MissingContacts(
                    self.base.node_id,
                    client_id
                ));
            }
        }
    }

    fn send_chat_message(&mut self, destination: NodeId, content: String) {
        // Find a server to route through
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

            // Update our session state
            if let Some(session) = self.active_sessions.get_mut(&destination) {
                session.last_message_id += 1;
                session.messages.push(content);
            } else {
                self.active_sessions.insert(destination, ChatSession {
                    server_id,
                    last_message_id: 0,
                    messages: vec![content],
                });
            }

            self.base.send_message(message, server_id);
        }
    }
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

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        self.base.send_fragment_after_nack(packet, nack)
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
}

impl Client for ChatClient {
    fn new(
        id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ChatClient::new(id, command_recv, event_send, packet_recv, packet_send)
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