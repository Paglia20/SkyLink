use crate::message::{ContentType, EdgeNackType, MediaRequest, MediaResponse, Message, TextRequest, TextResponse, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_trait::{obtain_file_display_name, Server};
use crate::server::server_type::{ContentServerType, ServerType};
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
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
    media_servers: HashSet<NodeId>,
}

impl NetworkEdge for TextServer {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.server_send_message(message, destination, self.get_src_id());
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
                        // println!("A text_file_request arrived of id {file_id}, here the complete: {:?}",self.text_files.get(&file_id));
                        match self.text_files.get(&file_id) {
                            Some((_,file)) => {
                                // If I have the text file, I start the check on it
                                if file.iter().any(|(_,x)| x.is_empty()) {
                                    let resp = TextResponse::Incomplete(file_id);
                                    // In case we haven't found all the medias in the file yet.
                                    let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TextResponse(resp));
                                    self.send_message(msg, source_id);
                                    self.send_event(ServerEvent::IncompleteFile(self.get_src_id(), file_id));

                                    // I ask again for the medias of all others media servers.
                                    let message = Message::new(self.get_src_id(), self.get_session_id(), ContentType::MediaRequest(MediaRequest::MediaList));
                                    for dst in self.media_servers.clone().into_iter() {
                                        self.send_message(message.clone(), dst);
                                    }
                                    // If I'm not already flooding, I might start a new flood in search of media servers.
                                    // self.flood();
                                } else {
                                    // If the requested text file is ready, I created the response from it
                                    // MediaReferences (HashMap<u64, (String, Vec<NodeId>)>)
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
                            edge_type: EdgeType::Server(self.get_server_type()),
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
                                self.media_servers.insert(from);
                                
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
                        // self.type_checked(from);
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

    fn send_fragment(&mut self, _: Fragment, _: NodeId, _: u64) {
        unimplemented!()
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.server_struct.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, _packet_session_id: u64, _nack: Nack) {
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

impl NetworkEdgeErrors for TextServer {
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
            let text_name = obtain_file_display_name(e.clone());
            match fs::read_to_string(e.clone()) {
                Ok(file_str) => {
                    // I divide the string to obtain the name of the medias contained in it.
                    let medias = divide_text_file(file_str.clone());

                    // I created a unique id that distinguish that media, used by clients to easier computation.
                    // The left-most byte is our nodeId, and the rest is dedicated to the file numeration;
                    // Since we should have less text files than media ones, only the two right-most bytes are dedicated to text files' ids.
                    let file_id = node_id as u64 * u64::from_be_bytes([1,0,0,0,0,0,0,0]) + starting_id;
                    starting_id += 1;

                    text_files.insert(file_id, (text_name, medias));
                },
                Err(err) => {
                    // I notify the SC and discard the file.
                    server_struct.send_event(ServerEvent::FileNotReadable(node_id, e, err.to_string()));
                }
            }
        }
        server_struct.send_event(ServerEvent::FilesState(node_id,
                                                text_files
                                                    .iter()
                                                    .filter(|(_,(_,x))| !x.iter().any(|(_,y)|y.is_empty()) )
                                                    .map(|(a,(b,_))| (*a,b.clone()))
                                                    .collect(), // Keeps only files with all medias.
                                                text_files
                                                    .iter()
                                                    .filter(|(_,(_,x))| x.iter().any(|(_,y)|y.is_empty()) )
                                                    .map(|(a,(b,_))| (*a,b.clone()))
                                                    .collect(), // Keeps only files with at least one missing media,
        ));
        TextServer {
            server_struct,
            text_files,
            next_file_id: starting_id,
            media_servers: HashSet::new(),
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
            },
            ServerCommand::InstantCrash => {
                self.server_struct.is_running = false;
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
        // println!("{} update_node_state: source_id={:?}, value={:?}",self.get_src_id(), source_id, value);
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
        ServerType::Content(ContentServerType::Text)
    }
}

fn divide_text_file(file_str: String) -> HashMap<String, Vec<(u64, NodeId)>> {
    let mut res = HashMap::new();
    let mut tmp_string = String::new();
    // I want to divide the file in the references of the media, collected into an HashMap.
    for c in file_str.chars() {
        if c != '\r' && c != '\n' {
            tmp_string.push(c);
            // When I find '\n' the row ends, so I save the string and go to the next one.
        } else if c == '\n' {
            // I save the name of the media, but still can't know which media server might have it.
            res.insert(obtain_file_display_name(tmp_string), Vec::new());
            tmp_string = String::new();
        }
    }
    res
}
