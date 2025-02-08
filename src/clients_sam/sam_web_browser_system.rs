// sam_web_browser_system.rs
use super::sam_client_base::SamClientBase;
use super::sam_events::{ClientCommand, ClientEvent, ConnectionState};
use super::sam_client_trait::Client;
use super::sam_client_type::ClientType;
use crate::message::{
    Message, ContentType, TextRequest, TextResponse, MediaRequest, MediaResponse, TypeExchange,
};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, Packet};

struct TextContent {
    name: String,
    media_refs: HashSet<u64>,
    servers: HashSet<NodeId>,
}

struct MediaContent {
    name: String,
    data: Option<Vec<u8>>,
    servers: HashSet<NodeId>,
}

pub struct WebBrowser {
    base: SamClientBase,
    text_contents: HashMap<u64, TextContent>,
    media_contents: HashMap<u64, MediaContent>,
    text_servers: HashSet<NodeId>,
    media_servers: HashSet<NodeId>,
    pending_requests: HashSet<(NodeId, u64)>,
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
            text_contents: HashMap::new(),
            media_contents: HashMap::new(),
            text_servers: HashSet::new(),
            media_servers: HashSet::new(),
            pending_requests: HashSet::new(),
        }
    }

    fn handle_text_response(&mut self, source: NodeId, response: TextResponse) {
        match response {
            TextResponse::TextList(texts) => {
                // Use into_iter() to take ownership of texts.
                for (id, name) in texts.into_iter() {
                    {
                        // Create a block so that the borrow from text_contents ends.
                        let _ = {
                            let entry = self.text_contents.entry(id).or_insert_with(|| TextContent {
                                name: name.clone(),
                                media_refs: HashSet::new(),
                                servers: HashSet::new(),
                            });
                            entry.servers.insert(source);
                        };
                    }
                    // Now the borrow has ended.
                    self.base.send_event(ClientEvent::SendTextList(self.base.node_id, id, name));
                }
            }
            TextResponse::MediaReferences(refs) => {
                for (media_id, (name, servers)) in refs.into_iter() {
                    {
                        let _ = {
                            let entry = self.media_contents.entry(media_id).or_insert_with(|| MediaContent {
                                name: name.clone(),
                                data: None,
                                servers: HashSet::new(),
                            });
                            // Clone each server so that the borrow is ended
                            entry.servers.extend(servers.iter().cloned());
                        };
                    }
                    self.base.send_event(ClientEvent::SendCatalogue(self.base.node_id, media_id, name));
                }
            }
            TextResponse::NotFound(text_id) => {
                {
                    if let Some(content) = self.text_contents.get_mut(&text_id) {
                        content.servers.remove(&source);
                    }
                }
                self.base.send_event(ClientEvent::MissingTextList(self.base.node_id, text_id));
            }
            TextResponse::Incomplete(text_id) => {
                if let Some(content) = self.text_contents.get(&text_id) {
                    // We clone the media_refs so the borrow ends.
                    let refs: Vec<u64> = content.media_refs.iter().cloned().collect();
                    for media_id in refs {
                        if let Some(media_content) = self.media_contents.get(&media_id) {
                            if media_content.data.is_none() {
                                if let Some(&server) = media_content.servers.iter().next() {
                                    self.request_media(server, media_id);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_media_response(&mut self, source: NodeId, response: MediaResponse) {
        match response {
            MediaResponse::Media(media_id, name, data) => {
                {
                    if let Some(content) = self.media_contents.get_mut(&media_id) {
                        content.data = Some(data.clone());
                    }
                }
                self.base.send_event(ClientEvent::SendMedia(self.base.node_id, media_id, name, data));
                self.pending_requests.remove(&(source, media_id));
            }
            MediaResponse::MediaList(media_list) => {
                for (id, name) in media_list.into_iter() {
                    {
                        let _ = {
                            let entry = self.media_contents.entry(id).or_insert_with(|| MediaContent {
                                name,
                                data: None,
                                servers: HashSet::new(),
                            });
                            entry.servers.insert(source);
                        };
                    }
                }
            }
            MediaResponse::NotFound(media_id) => {
                {
                    if let Some(content) = self.media_contents.get_mut(&media_id) {
                        content.servers.remove(&source);
                    }
                }
                self.base.send_event(ClientEvent::MissingDestForMedia(self.base.node_id, media_id));
                self.pending_requests.remove(&(source, media_id));
            }
        }
    }

    fn request_media(&mut self, server: NodeId, media_id: u64) {
        if !self.pending_requests.contains(&(server, media_id)) {
            let message = Message::new(
                self.base.node_id,
                self.base.get_session_id(),
                ContentType::MediaRequest(MediaRequest::Media(media_id)),
            );
            self.base.send_message(message, server);
            self.pending_requests.insert((server, media_id));
        }
    }

    fn request_text(&mut self, server: NodeId, text_id: u64) {
        let message = Message::new(
            self.base.node_id,
            self.base.get_session_id(),
            ContentType::TextRequest(TextRequest::TextFile(text_id)),
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
                            }),
                        );
                        self.send_message(response, from);
                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        if let EdgeType::Server(server_type) = edge_type {
                            match server_type {
                                crate::server::server_type::ServerType::Content(_) => {
                                    self.text_servers.insert(from);
                                    self.base.node_states.insert(from, ConnectionState::Ready);
                                    self.base.send_event(ClientEvent::SendDestinations(self.base.node_id, from));
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

    fn get_src_id(&self) -> NodeId {
        self.base.get_src_id()
    }

    fn remove_sender(&mut self, id: NodeId) {
        self.base.remove_sender(id)
    }
}

impl NetworkEdgeErrors for WebBrowser {
    fn check_type(&mut self, id: NodeId) {
        self.base.check_type(id);
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        self.base.is_state_ok(node_id)
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.base.send_nack_message(dst, nack);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        self.base.send_drone_nack(dst, nack);
    }
}

impl Client for WebBrowser {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        WebBrowser::new(node_id, command_recv, event_send, packet_recv, packet_send)
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
                if let Some(content) = self.text_contents.get(&text_id) {
                    if let Some(&server) = content.servers.iter().next() {
                        self.request_text(server, text_id);
                    }
                }
            }
            ClientCommand::GetContent(media_id) => {
                if let Some(content) = self.media_contents.get(&media_id) {
                    if let Some(&server) = content.servers.iter().next() {
                        self.request_media(server, media_id);
                    }
                }
            }
            ClientCommand::RetrieveList(server_id) => {
                let message = Message::new(
                    self.base.node_id,
                    self.base.get_session_id(),
                    ContentType::TextRequest(TextRequest::TextList)
                );
                self.base.send_message(message, server_id);
            }
            _ => self.handle_command(command)
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::WebBrowser
    }

    fn send_event(&self, ce: ClientEvent) {
        self.base.send_event(ce);
    }
}