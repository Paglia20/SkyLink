// sam_web_browser_system.rs
use super::sam_client_base::SamClientBase;
use super::sam_events::{ClientCommand, ClientEvent, ConnectionState};
use super::sam_client_trait::Client;
use super::sam_client_type::ClientType;
use crate::message::{Message, ContentType, TextRequest, TextResponse, MediaRequest, MediaResponse, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, Packet};

pub struct WebBrowser {
    base: SamClientBase,
    text_servers: HashMap<NodeId, Vec<(u64, String)>>,         // server -> [(text_id, name)]
    media_servers: HashMap<NodeId, Vec<(u64, String)>>,        // server -> [(media_id, name)]
    media_cache: HashMap<u64, (String, Vec<u8>)>,             // media_id -> (name, content)
    available_texts: HashMap<u64, HashSet<NodeId>>,            // text_id -> servers
    available_media: HashMap<u64, HashSet<NodeId>>,           // media_id -> servers
}

impl WebBrowser {
    pub fn new(
        id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        WebBrowser {
            base: SamClientBase::new(id, command_recv, event_send, packet_recv, packet_send),
            text_servers: HashMap::new(),
            media_servers: HashMap::new(),
            media_cache: HashMap::new(),
            available_texts: HashMap::new(),
            available_media: HashMap::new(),
        }
    }

    fn handle_text_response(&mut self, source: NodeId, response: TextResponse) {
        match response {
            TextResponse::TextList(texts) => {
                self.text_servers.insert(source, texts.clone());
                for (id, name) in texts {
                    self.base.send_event(ClientEvent::SendTextList(
                        self.base.node_id,
                        id,
                        name
                    ));
                    self.available_texts
                        .entry(id)
                        .or_insert_with(HashSet::new)
                        .insert(source);
                }
            }
            TextResponse::MediaReferences(media_refs) => {
                for (media_id, (name, servers)) in media_refs {
                    self.base.send_event(ClientEvent::SendCatalogue(
                        self.base.node_id,
                        media_id,
                        name.clone()
                    ));
                    let entry = self.available_media
                        .entry(media_id)
                        .or_insert_with(HashSet::new);
                    entry.extend(servers);
                }
            }
            TextResponse::NotFound(text_id) => {
                self.base.send_event(ClientEvent::MissingTextList(
                    self.base.node_id,
                    text_id
                ));
            }
            TextResponse::Incomplete(text_id) => {
                // Request media for incomplete text
                if let Some(servers) = self.available_media.get(&text_id) {
                    if let Some(&server) = servers.iter().next() {
                        self.request_media(server, text_id);
                    }
                }
            }
        }
    }

    fn handle_media_response(&mut self, source: NodeId, response: MediaResponse) {
        match response {
            MediaResponse::Media(media_id, name, data) => {
                self.media_cache.insert(media_id, (name.clone(), data.clone()));
                self.base.send_event(ClientEvent::SendMedia(
                    self.base.node_id,
                    media_id,
                    name,
                    data
                ));
            }
            MediaResponse::MediaList(media_list) => {
                self.media_servers.insert(source, media_list);
            }
            MediaResponse::NotFound(media_id) => {
                if let Some(servers) = self.available_media.get_mut(&media_id) {
                    servers.remove(&source);
                }
                self.base.send_event(ClientEvent::MissingDestForMedia(
                    self.base.node_id,
                    media_id
                ));
            }
        }
    }

    fn request_media(&mut self, server: NodeId, media_id: u64) {
        let message = Message::new(
            self.base.node_id,
            self.base.get_session_id(),
            ContentType::MediaRequest(MediaRequest::Media(media_id))
        );
        self.base.send_message(message, server);
    }
}

impl NetworkEdge for WebBrowser {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.base.send_message(message, destination)
    }

    fn handle_packet(&mut self, packet: Packet) {
        self.base.handle_packet(packet)
    }

    fn handle_message(&mut self, message: Message) {
        match message.content {
            ContentType::TextResponse(response) => {
                self.handle_text_response(message.source_id, response);
            }
            ContentType::MediaResponse(response) => {
                self.handle_media_response(message.source_id, response);
            }
            ContentType::TypeExchange(exchange) => {
                match exchange {
                    TypeExchange::TypeRequest { from } => {
                        let response = Message::new(
                            self.base.node_id,
                            self.base.get_session_id(),
                            ContentType::TypeExchange(TypeExchange::TypeResponse {
                                edge_type: EdgeType::Client(ClientType::WebBrowser),
                                from: self.base.node_id,
                            })
                        );
                        self.send_message(response, from);
                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        if let EdgeType::Server(server_type) = edge_type {
                            match server_type {
                                crate::server::server_type::ServerType::Content(content_type) => {
                                    self.base.node_states.insert(from, ConnectionState::Ready);
                                    self.base.send_event(ClientEvent::SendDestinations(
                                        self.base.node_id,
                                        from
                                    ));
                                }
                                _ => {
                                    self.base.node_states.insert(from, ConnectionState::Failed);
                                }
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

impl Client for WebBrowser {
    fn new(
        id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        WebBrowser::new(id, command_recv, event_send, packet_recv, packet_send)
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
            ClientCommand::GetTextFile(text_id) => {
                if let Some(servers) = self.available_texts.get(&text_id) {
                    if let Some(&server) = servers.iter().next() {
                        let message = Message::new(
                            self.base.node_id,
                            self.base.get_session_id(),
                            ContentType::TextRequest(TextRequest::TextFile(text_id))
                        );
                        self.base.send_message(message, server);
                    }
                }
            }
            ClientCommand::GetContent(media_id) => {
                if let Some(servers) = self.available_media.get(&media_id) {
                    if let Some(&server) = servers.iter().next() {
                        self.request_media(server, media_id);
                    }
                }
            }
            _ => self.base.handle_command(command)
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::WebBrowser
    }

    fn send_event(&self, ce: ClientEvent) {
        self.base.send_event(ce);
    }
}