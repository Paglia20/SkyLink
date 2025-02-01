use crate::message::{ContentType, EdgeNackType, MediaRequest, MediaResponse, Message, TextRequest, TextResponse, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_trait::Server;
use crate::server::server_type::{ContentServerType, ServerType};
use crossbeam_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::fs;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, FloodResponse, Fragment, Nack, NackType, Packet};
use crate::clients_gio::client_type::ClientType;
use crate::server::server_struct::ServerStruct;

type TextFile = (String, HashMap<String, Vec<(u64, NodeId)>>);

pub struct TextServer {
    server_struct: ServerStruct,
    text_files: HashMap<u64, TextFile>,
    next_file_id: u64,
}

impl NetworkEdge for TextServer {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.server_send_message(message, destination);
    }

    fn handle_packet(&mut self, packet: Packet) {
        self.server_handle_packet(packet);
    }

    fn handle_message(&mut self, message: Message) {
        match message.content {
            ContentType::TextRequest(text_request) => {
                let source_id = message.source_id;
                match text_request {
                    TextRequest::TextList => {
                        let resp = TextResponse::TextList(self
                            .text_files
                            .iter()
                            .map(|(x,y)| (*x,y.0.clone()))
                            .collect()
                        );
                        let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TextResponse(resp));
                        self.send_message(msg, source_id);
                    },
                    TextRequest::TextFile(file_id) => {
                        match self.text_files.get(&file_id) {
                            Some((_,file)) => {
                                // If I have the text file, I start the check on it
                                if file.iter().any(|(_,x)| x.is_empty()) {
                                    let resp = TextResponse::Incomplete(file_id);
                                    // In case we haven't found all the medias in the file yet.
                                    let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TextResponse(resp));
                                    self.send_message(msg, source_id);
                                    self.send_event(ServerEvent::IncompleteFile(self.get_src_id(), file_id));
                                } else {
                                    // If the requested text file is ready, I created the response from it
                                    let resp = TextResponse::MediaReferences(file
                                        .iter()
                                        .map(|(x,y)|
                                            (y.first().unwrap().0,
                                             (x.clone(),
                                              y.iter().map(|(_,y)|*y).collect()
                                             )
                                            )
                                        )
                                        .collect()
                                    );
                                    let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TextResponse(resp));
                                    self.send_message(msg, source_id);
                                }
                            },
                            None => {
                                let resp = TextResponse::NotFound(file_id);
                                // In case we don't have the requested file_id.
                                let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TextResponse(resp));
                                self.send_message(msg, source_id);
                                self.send_event(ServerEvent::FileNotFound(self.get_src_id(), file_id));
                            }
                        }
                    }
                }
            }
            ContentType::MediaResponse(media_response) => {
                match media_response {
                    MediaResponse::MediaList(media_list) => {
                        let source = message.source_id;
                        for (media_id, media_name) in media_list {
                            for (_,(_,x)) in self.text_files.iter_mut() {
                                match x.get_mut(&media_name) {
                                    None => {
                                        // I don't have this media, so I don't care about it.
                                    }
                                    Some(media_vec) => {
                                        media_vec.push((media_id, source));
                                        // If instead I have the media, I add this as a possible location.
                                    }
                                }
                            }
                        }
                        // I notify to the SC the state of the files, if they're completed or not.
                        self.send_event(ServerEvent::FilesState(self.get_src_id(),
                                                                              self.text_files
                                                                                  .iter()
                                                                                  .filter(|(_,(_,x))| !x.iter().any(|(_,y)|y.is_empty()) )
                                                                                  .map(|(a,(b,_))| (*a,b.clone()))
                                                                                  .collect(), // Keeps only files with all medias.
                                                                              self.text_files
                                                                                  .iter()
                                                                                  .filter(|(_,(_,x))| x.iter().any(|(_,y)|y.is_empty()) )
                                                                                  .map(|(a,(b,_))| (*a,b.clone()))
                                                                                  .collect(), // Keeps only files with at least one missing media,
                        ));
                    }
                    _ => {
                        // Other types of media responses shouldn't be received by this server.
                        let new_nack = self.create_nack(EdgeNackType::UnexpectedMessage);
                        self.send_nack_message(message.source_id, new_nack);
                    }
                }
            }
            ContentType::TypeExchange(exchange) => {
                match exchange {
                    TypeExchange::TypeRequest { from } => {
                        let type_resp = TypeExchange::TypeResponse {
                            edge_type: EdgeType::Client(ClientType::ChatClient),
                            from: self.get_src_id(),
                        };
                        let message = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TypeExchange(type_resp));
                        
                        // I don't have to worry about having the path to 'from', since if it's missing floods will be initialized afterward.
                        self.send_message(message, from);
                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        match edge_type {
                            EdgeType::Server(ServerType::Content(ContentServerType::Media)) => {
                                // I set it as a media server contact.
                                self.update_node_state(from, 1);
                                // Since I found a media server, I ask for his medias.
                                let message = Message::new(self.get_src_id(), self.get_session_id(), ContentType::MediaRequest(MediaRequest::MediaList));
                                self.send_message(message, from);
                            },
                            EdgeType::Client(ClientType::WebBrowser) => {
                                self.update_node_state(from, 1);
                                // I set it as a contactable node, since we have a check for it later.
                            }
                            _ => {
                                self.update_node_state(from, 2);
                                // I set it as a not usable contact.
                            }
                        }
                    }
                }
            }
            ContentType::EdgeNack(nack) => {
                self.handle_edge_nack(nack, message.source_id, message.session_id)
            },
            _ => {
                // All other types of message shouldn't be received by this server.
                let new_nack = self.create_nack(EdgeNackType::UnexpectedMessage);
                self.send_nack_message(message.source_id, new_nack);
            }
        }

    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        self.server_send_fragment(fragment, destination, session_id);
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.server_struct.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        self.server_send_fragment_after_nack(packet, nack, self.get_src_id());
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        self.server_send_ack(packet, fragment_index);
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
        // Currently unused I think;
    }
}

impl NetworkEdgeErrors for TextServer {
    fn check_type(&mut self, id: NodeId) {
        self.server_check_type(id);
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        self.server_is_state_ok(node_id)
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.send_message(nack, dst);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        self.server_send_drone_nack(dst, nack);
    }
}

impl Server for TextServer {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        files: Vec<String>
    ) -> Self {
        let server_struct = ServerStruct::new(node_id, command_recv, event_send, packet_recv, packet_send);
        let mut starting_id:u64 = 0;
        let mut text_files = HashMap::new();
        for e in files.into_iter() {
            // I read the file as a string
            match fs::read_to_string(e.clone()) {
                Ok(file_str) => {
                    // I divide the string to obtain the name of the medias contained in it.
                    let medias = divide_text_file(file_str.clone());

                    // I created a unique id that distinguish that media, used by clients to easier computation.
                    // The left-most byte is our nodeId, and the rest is dedicated to the file numeration;
                    // Since we should have less text files than media ones, only the two right-most bytes are dedicated to text files' ids.
                    let file_id = node_id as u64 * u64::from_be_bytes([1,0,0,0,0,0,0,0]) + starting_id;
                    starting_id += 1;

                    text_files.insert(file_id, (file_str, medias));
                },
                Err(err) => {
                    // I notify the SC and discard the file.
                    server_struct.send_event(ServerEvent::FileNotReadable(node_id, e, err.to_string()));
                }
            }
        }
        TextServer {
            server_struct,
            text_files,
            next_file_id: starting_id,
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
            }
            ServerCommand::AddFile(file) => {
                // I read the file as a string
                match fs::read_to_string(file.clone()) {
                    Ok(file_str) => {
                        // I divide the string to obtain the name of the medias contained in it.
                        let medias = divide_text_file(file_str.clone());

                        // I created a unique id that distinguish that media, used by clients to easier computation.
                        // The left-most byte is our nodeId, and the rest is dedicated to the file numeration;
                        // Since we should have less text files than media ones, only the two right-most bytes are dedicated to text files' ids.
                        let file_id = self.get_src_id() as u64 * u64::from_be_bytes([1,0,0,0,0,0,0,0]) + self.next_file_id;
                        self.next_file_id += 1;

                        self.text_files.insert(file_id, (file_str, medias));
                    },
                    Err(err) => {
                        // I notify the SC and discard the file.
                        self.send_event(ServerEvent::FileNotReadable(self.get_src_id(), file, err.to_string()));
                    }
                }
            }
        }
    }

    fn send_event(&self, new_nack: ServerEvent) {
        self.server_struct.send_event(new_nack);
    }
    fn handle_fragment(&mut self, fragment: Fragment, packet: Packet) {
        self.server_struct.handle_fragment(fragment, packet);
    }
    fn handle_flood_request(&mut self, flood_request: FloodRequest, packet: Packet) -> bool {
        self.server_struct.handle_flood_request(flood_request.clone(), packet)
    }
    fn handle_nack(&mut self, nack: Nack, packet: Packet) -> bool {
        self.server_struct.handle_nack(nack.clone(), packet)
    }
    fn positive_feed(&mut self, nodes: Vec<NodeId>) {
        self.server_struct.network.positive_feedback(nodes);
    }
    fn save_flood_response(&mut self, flood_resp: FloodResponse) {
        self.server_struct.save_flood_response(flood_resp);
    }
    fn can_flood(&mut self) -> bool {
        self.server_struct.can_flood()
    }
    fn send_to_all(&mut self, packet: Packet) {
        self.server_struct.send_to_all(packet);
    }
    fn update_node_state(&mut self, source_id: NodeId, value: u8) {
        self.server_struct.network.update_state(source_id, value);
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
    fn get_path_to(&self, destination: NodeId) -> Option<(Vec<NodeId>, f64)> {
        self.server_struct.network.best_path(&self.get_src_id(), &destination)
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
    fn get_server_type(&self) -> ServerType {
        ServerType::Content(ContentServerType::Text)
    }
}

fn divide_text_file(file_str: String) -> HashMap<String, Vec<(u64, NodeId)>> {
    let mut res = HashMap::new();
    let mut tmp_string = String::new();
    for c in file_str.chars() {
        if c != '\n' {
            tmp_string.push(c);
        } else {
            // I save the name of the media, but still can't know which media server might have it.
            res.insert(tmp_string, Vec::new());
            tmp_string = String::new();
        }
    }
    res
}
