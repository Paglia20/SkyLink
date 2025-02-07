use std::collections::{HashMap, HashSet};
use crossbeam_channel::{Receiver, Sender};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, Packet};
use crate::message::{Message, ContentType, ChatRequest, ChatResponse, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge};
use crate::server::server_type::ServerType;
use super::sam_client_base::BaseClient;
use super::sam_events::{ClientCommand, ClientEvent, ConnectionState};

pub struct TextChat {
    pub(crate) base: BaseClient,
    chat_servers: HashSet<NodeId>,
    active_chats: HashMap<NodeId, HashMap<NodeId, u64>>,
    pending_messages: Vec<(NodeId, String)>,
}

impl TextChat {
    pub fn new(
        id: NodeId,
        command_rx: Receiver<ClientCommand>,
        event_tx: Sender<ClientEvent>,
        packet_rx: Receiver<Packet>,
        packet_tx: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        TextChat {
            base: BaseClient::new(id, command_rx, event_tx, packet_rx, packet_tx),
            chat_servers: HashSet::new(),
            active_chats: HashMap::new(),
            pending_messages: Vec::new(),
        }
    }

    fn handle_chat_response(&mut self, source: NodeId, response: ChatResponse) {
        match response {
            ChatResponse::ClientList(clients) => {
                for client_id in clients {
                    self.active_chats
                        .entry(client_id)
                        .or_insert_with(HashMap::new)
                        .insert(source, 0);
                }

                let pending = std::mem::take(&mut self.pending_messages);
                for (dest, content) in pending {
                    if self.active_chats.contains_key(&dest) {
                        self.send_chat_message(dest, &content);
                    } else {
                        self.pending_messages.push((dest, content));
                    }
                }
            }
            ChatResponse::MessageFrom { from, message } => {
                if let Some(chats) = self.active_chats.get_mut(&from) {
                    for (_server_id, count) in chats.iter_mut() {
                        *count += 1;
                    }
                }
            }
            ChatResponse::MessageSent => {
                // Optionally handle confirmation
            }
        }
    }

    fn send_chat_message(&mut self, to: NodeId, content: &str) {
        if let Some(chats) = self.active_chats.get(&to) {
            if let Some((&server_id, _)) = chats.iter().next() {
                let message = Message::new(
                    self.base.node_id,
                    self.base.get_next_session_id(),
                    ContentType::ChatRequest(ChatRequest::SendMessage {
                        from: self.base.node_id,
                        to,
                        message: content.to_string(),
                    }),
                );
                self.base.send_message(message, server_id);
            }
        } else {
            self.pending_messages.push((to, content.to_string()));
            if let Some(&server_id) = self.chat_servers.iter().next() {
                let request = Message::new(
                    self.base.node_id,
                    self.base.get_next_session_id(),
                    ContentType::ChatRequest(ChatRequest::ClientList),
                );
                self.base.send_message(request, server_id);
            } else {
                self.base.flood();
            }
        }
    }

    pub fn run(&mut self) {
        self.base.run_event_loop();
    }
}

impl NetworkEdge for TextChat {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.base.send_message(message, destination)
    }

    fn handle_packet(&mut self, packet: Packet) {
        self.base.handle_packet(packet)
    }

    fn handle_message(&mut self, message: Message) {
        match message.content {
            ContentType::ChatResponse(resp) => {
                self.handle_chat_response(message.source_id, resp);
            }
            ContentType::TypeExchange(exchange) => {
                match exchange {
                    TypeExchange::TypeResponse {
                        from,
                        edge_type: EdgeType::Server(ServerType::Chat),
                    } => {
                        self.chat_servers.insert(from);
                        self.base.node_states.insert(from, ConnectionState::Ready);
                    }
                    TypeExchange::TypeRequest { from } => {
                        let response = Message::new(
                            self.base.node_id,
                            self.base.get_next_session_id(),
                            ContentType::TypeExchange(TypeExchange::TypeResponse {
                                edge_type: EdgeType::Client(crate::clients_gio::client_type::ClientType::ChatClient),
                                from: self.base.node_id,
                            }),
                        );
                        self.send_message(response, from);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        self.base.send_fragment(fragment, destination, session_id)
    }

    fn flood(&mut self) {
        self.base.flood()
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

    fn get_flood_id(&mut self) -> u64 {
        self.base.get_flood_id()
    }

    fn get_session_id(&mut self) -> u64 {
        self.base.get_session_id()
    }

    fn check_type(&mut self, id: NodeId) {
        self.base.check_type(id)
    }

    fn is_state_ok(&mut self, node_id: NodeId) -> bool {
        self.base.is_state_ok(node_id)
    }

    fn handle_nack(&mut self, packet: &Packet, nack: &Nack) {
        self.base.handle_nack(packet, nack);


        match nack.nack_type {
            wg_2024::packet::NackType::UnexpectedRecipient(wrong_node) => {
                println!(
                    "TextChat: Unexpected recipient node {} for packet {}",
                    wrong_node, packet.session_id
                );
            }
            wg_2024::packet::NackType::ErrorInRouting(wrong_node) => {
                println!(
                    "TextChat: Routing error at node {} for packet {}",
                    wrong_node, packet.session_id
                );
            }
            wg_2024::packet::NackType::DestinationIsDrone => {
                println!(
                    "TextChat: Destination node {} is a drone for packet {}",
                    self.base.node_id, packet.session_id
                );
            }
            wg_2024::packet::NackType::Dropped => {
                println!(
                    "TextChat: Packet {} was dropped, retrying...",
                    packet.session_id
                );
            }
        }
    }
}