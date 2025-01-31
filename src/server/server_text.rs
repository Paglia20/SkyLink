use crate::message::{ContentType, EdgeNackType, MediaResponse, Message, TextRequest, TextResponse, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_trait::Server;
use crate::server::server_type::{ContentServerType, ServerType};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::HashMap;
use std::fs;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, Fragment, Nack, NackType, NodeType, Packet, PacketType};
use crate::clients_gio::client_type::ClientType;
use crate::server::server_struct::ServerStruct;
use crate::DEBUG_MODE;

type TextFile = (String, HashMap<String, Vec<(u64, NodeId)>>);

pub struct ContentServer {
    server_struct: ServerStruct,
    text_files: HashMap<u64, TextFile>,
    next_file_id: u64,
}

impl NetworkEdge for ContentServer {
    fn send_message(&mut self, message: Message, destination: NodeId) {

        match message.clone().content{
            ContentType::TypeExchange(_exc) =>{
                self.server_send_fragments(message, destination);
            },
            ContentType::EdgeNack(_nack) => {
                self.server_send_fragments(message, destination);
            }
            _=>{
                if self.is_state_ok(destination) {
                    self.server_send_fragments(message, destination);
                }
                else {
                    let new_nack = ServerEvent::WrongDestinationType(self.get_src_id(), destination);
                    self.server_struct.send_event(new_nack);
                }
            }
        }
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let PacketType::FloodRequest(flood_request) = packet.pack_type.clone(){
            // The inner struct compute the functions and try to send it to all it's neighbours.
            if !self.server_struct.handle_flood_request(flood_request.clone(), packet) {
                // Otherwise if I need to create a flood_response, I call this function common to all network edges.
                self.edge_send_flood_response(flood_request);
            }

        } else if packet.routing_header.destination().unwrap() != self.get_src_id() {
            // If we're not the destination of a packet, we act like a drone wit 0 PDR.
            packet.routing_header.hop_index += 1;
            let next_id = packet.routing_header.hops.get(packet.routing_header.hop_index).unwrap();
            // I obtain the id for the next hop.

            match self.server_struct.packet_send.get(next_id) {
                None => {
                    // In case I don't have the neighbour, I send a Nack back.
                    self.server_struct.send_event(ServerEvent::MissingRoute(self.get_src_id(), *next_id));
                    self.send_drone_nack(packet.routing_header.source().unwrap(), NackType::ErrorInRouting(*next_id));
                }
                Some(sender) => {
                    match sender.try_send(packet.clone()) {
                        Err(_) => {
                            // We send back the same errors a drone would.
                            self.server_struct.send_event(ServerEvent::PacketSendingError(packet.clone()));
                            self.send_drone_nack(packet.routing_header.source().unwrap(), NackType::ErrorInRouting(*next_id));
                        }
                        Ok(_) => {
                            self.server_struct.send_event(ServerEvent::PacketSent(packet.clone()));
                            // If the message was sent, I also notify the sim controller.
                        }
                    }
                }
            }

        } else {
            // We can take for granted it is the destination
            match packet.pack_type.clone() {
                PacketType::MsgFragment(fragment) => {
                    let frag_index = fragment.fragment_index;
                    let tot_num_frag = fragment.total_n_fragments as usize;
                    let session_id = packet.session_id;
                    let initiator_id = packet.routing_header.hops[0];
                    
                    self.server_struct.handle_fragment(fragment, packet.clone());
                    
                    // For each arrived frag, send back an ack
                    self.send_ack(packet.clone(), frag_index);

                    // If all the frag have arrived recreate message
                    let frags_clone: &Vec<Fragment> = self.server_struct.fragments.get(&(packet.session_id, initiator_id)).unwrap().1.as_ref();
                    if frags_clone.len() == tot_num_frag {
                        match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                            Ok(message) => {
                                // If the message is created correctly, we handle it.
                                self.handle_message(message);
                            }
                            Err(e) => {
                                // If the message can't be created, we can't recover it, so we notify the SC.
                                self.server_struct.send_event(ServerEvent::LostMessage(session_id, initiator_id,e));
                                // This should never happen since all the appropriated checks are done previously.
                            }
                        };
                        
                        // We remove the entry from the HashMap.
                        self.server_struct.fragments.remove(&(packet.session_id, initiator_id));
                    }
                }
                PacketType::Ack(ack) => {
                    self.server_struct.send_event(ServerEvent::AckReceived(packet.clone()));

                    // The ACK will have our ID as source, and we 'recognize' the origin from the session_id
                    match self.server_struct.fragments.get_mut(&(packet.session_id, self.get_src_id())) {
                        None => {
                            // In the case we receive an ACK that's not for one of our fragments, we notify the SC and discard it.
                            self.server_struct.send_event(ServerEvent::WrongDestination(self.get_src_id(), packet));
                        }
                        Some((_source,vec)) => {
                            // I retain all the fragments with fragment index different from the ACK one.
                            vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                            // If it's empty I retained all fragments because I received all the Ack, hence I can remove my entry from hashmap
                            if vec.is_empty() {
                                self.server_struct.fragments.remove_entry(&(packet.session_id, self.get_src_id()));
                            }
                            
                            // I apply the positive feed on all nodes in the path
                            let nodes = packet.routing_header.hops;
                            self.server_struct.nodes.positive_feed(nodes);
                        }
                    }
                }
                PacketType::Nack(nack) => {
                    if self.server_struct.handle_nack(nack.clone(), packet.clone()){
                        self.send_fragment_after_nack(packet, nack);
                    }
                }
                PacketType::FloodRequest(_) => {
                    unreachable!() // We already managed them earlier.
                }
                PacketType::FloodResponse(flood_resp) => {
                    self.server_struct.save_flood_response(flood_resp);
                }
            }
        }
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
                        if !self.text_files.contains_key(&file_id) {
                            let resp = TextResponse::NotFound(file_id);
                            // In case we don't have the requested file_id.
                            let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TextResponse(resp));
                            self.send_message(msg, source_id);
                            self.server_struct.send_event(ServerEvent::FileNotFound(self.get_src_id(), file_id));
                        } else {
                            // If I have the text file, I start the check on it
                            let file = self.text_files.get(&file_id).unwrap().1.clone();
                            if file.iter().any(|(_,x)| x.is_empty()) {
                                let resp = TextResponse::Incomplete(file_id);
                                // In case we haven't found all the medias in the file yet.
                                let msg = Message::new(self.get_src_id(), self.get_session_id(), ContentType::TextResponse(resp));
                                self.send_message(msg, source_id);
                                self.server_struct.send_event(ServerEvent::IncompleteFile(self.get_src_id(), file_id));
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
                        self.server_struct.send_event(ServerEvent::FilesState(self.get_src_id(),
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
                                self.server_struct.paths.get_mut(&from).unwrap().0 = 1;
                                // I set it as a media server contact.
                            },
                            /*EdgeType::Client(ClientType::WebBrowser) => {
                                self.server_struct.paths.get_mut(&from).unwrap().0 = 1;
                                // I set it as a usable contact
                            },*/
                            _ => {
                                self.server_struct.paths.get_mut(&from).unwrap().0 = 2;
                                // I set it as a not usable contact.
                            }
                        }
                    }
                }
            }
            ContentType::EdgeNack(nack) => {
                match nack {
                    EdgeNackType::UnexpectedMessage => {
                        // Means that it sent a msg to a dst with a wrong state
                        if let Some((state, _route)) = self.server_struct.paths.get_mut(&message.source_id) {
                            *state = 2;
                        }
                        // Since the destination was wrong, the message is discarded.
                        self.server_struct.send_event(ServerEvent::DiscardedMessage(self.get_src_id(), message.session_id));

                        if DEBUG_MODE {
                            println!("Client {} discarded message to {} after receiving his nack, because state was not good", self.get_src_id(), message.source_id)
                        }
                    }
                }
            },
            _ => {
                // All other types of message shouldn't be received by this server.
                let new_nack = self.create_nack(EdgeNackType::UnexpectedMessage);
                self.send_nack_message(message.source_id, new_nack);
            }
        }

    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        match self.server_struct.paths.get_mut(&destination) {
            None => {
                // I first check if I have any path to the destination
                self.server_struct.send_event(ServerEvent::MissingDestination(self.get_src_id(), destination));
                self.flood(); // Since I miss the destination, I start a flooding.
                self.add_unsent_fragment(fragment, session_id, destination);
            }
            Some((_state, route_list)) => {
                match route_list.get_fastest_route() {
                    None => {
                        // I then check that we have an available route to the destination.
                        self.server_struct.send_event(ServerEvent::MissingRoute(self.get_src_id(), destination));
                        self.flood(); // Since I have a destination, but all routes to it were deleted, I start a flooding.
                        self.add_unsent_fragment(fragment, session_id, destination);
                    },
                    Some(route) => {
                        let srh = route.to_source_routing_header();
                        let first_dst = srh.hops[1];
                        let packet = Packet::new_fragment(srh, session_id, fragment.clone());

                        // If everything worked, I try to send.
                        match self.server_struct.packet_send.get(&first_dst) {
                            Some(sender) => {
                                sender.send(packet.clone()).unwrap();
                                self.server_struct.send_event(ServerEvent::PacketSent(packet.clone()));
                            }
                            None => {
                                // If I want to pass for a node that I don't have as a neighbour, I need to remove
                                // channels who contain it.
                                self.server_struct.send_event(ServerEvent::MissingRoute(self.get_src_id(), destination));
                                self.add_unsent_fragment(fragment, session_id, destination);
                                for (_, (_state,route)) in self.server_struct.paths.iter_mut() {
                                    route.remove_faulty_node(destination);
                                }
                            }
                        }
                    },
                }
            },
        };
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.server_struct.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        match self.server_struct.fragments.get(&(packet.session_id, self.get_src_id())) {
            // I try to find again the fragment, and notify the sim controller if I don't have it anymore.
            None => {
                let err=  String::from("Failed to find message again after NACK");
                self.server_struct.send_event(ServerEvent::LostMessage(packet.session_id, self.get_src_id(), err));
            },
            Some((_,fragments)) => {
                match fragments.get(nack.fragment_index as usize) {
                    None => {
                        self.server_struct.send_event(ServerEvent::LostFragment(packet.session_id, self.get_src_id(), nack.fragment_index));
                    },
                    // If I manage to find the fragment, I send it
                    Some(fragment) => {
                        self.send_fragment(fragment.clone(), *packet.routing_header.hops.first().unwrap(), packet.session_id);
                    }
                }
            }
        }
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        let new_hops = packet.routing_header.hops.clone();
        let next_id = new_hops[1];
        let srh = SourceRoutingHeader::new(new_hops, 1); //is it 1 right?
        let packet_ack = Packet::new_ack(srh, packet.session_id, fragment_index);

        match self.server_struct.packet_send.get(&next_id) {
            Some(sender) => {
                sender.send(packet_ack.clone()).unwrap();
                self.server_struct.send_event(ServerEvent::PacketSent(packet_ack));
            }
            None => {
                self.server_struct.send_event(ServerEvent::MissingDestination(self.get_src_id(), next_id));
            }
        }
    }

    fn flood(&mut self) {
        // I use a counter to avoid flooding the network too often.
        if self.server_struct.flood_counter == 0 {
            let flood_request = FloodRequest{
                flood_id: self.get_flood_id(),
                initiator_id: self.get_src_id(),
                path_trace: vec![(self.get_src_id(), NodeType::Server)],
            };
            let packet = Packet::new_flood_request(SourceRoutingHeader::default(), self.get_session_id(), flood_request);
            self.server_struct.packet_send.values().for_each(|sender| {
                sender.send(packet.clone()).unwrap()
            });
        }
        if self.server_struct.flood_counter == 10 {
            self.server_struct.flood_counter = 0;
        } else {
            self.server_struct.flood_counter += 1;
        }
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

impl NetworkEdgeErrors for ContentServer {
    fn check_type(&mut self, id: NodeId) {
        let req = TypeExchange::TypeRequest { from: self.get_src_id() };
        let exc = ContentType::TypeExchange(req);
        let s_id = self.get_session_id();
        self.send_message(Message::new(self.get_src_id(), s_id, exc), id);

        if DEBUG_MODE {
            println!("sent check from {}", self.get_src_id());
        }
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        let out =  match self.server_struct.paths.get(&node_id){
            Some(path) => {
                path.0 == 1
            }
            None =>{false}
        };
        if !out && DEBUG_MODE{
                println!("dst state was not ok");
            //send nack?
        }
        out
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.send_message(nack, dst);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        let new_nack = Nack{
            fragment_index: 0,
            nack_type: nack,
        };
        let shr = match self.server_struct.paths.get_mut(&dst){
            None => {
                self.server_struct.send_event(ServerEvent::MissingDestination(self.get_src_id(), dst));
                return;
            }
            Some((_state, route)) => {
                if let Some(fastest_route) = route.get_fastest_route(){
                    fastest_route.to_source_routing_header()
                }else {
                    self.server_struct.send_event(ServerEvent::MissingRoute(self.get_src_id(), dst));
                    return;
                }
            }
        };
        let first_hop = shr.next_hop().unwrap_or(self.get_src_id());

        let packet = Packet{
            routing_header: shr,
            session_id: self.get_session_id(),
            pack_type: PacketType::Nack(new_nack),
        };

        match self.server_struct.packet_send.get(&first_hop){
            None => {
                self.server_struct.send_event(ServerEvent::MissingDestination(self.get_src_id(), dst));
            }
            Some(sender) => {
                sender.send(packet).unwrap();
            }
        }
    }
}

impl Server for ContentServer {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        files: Vec<String>
    ) -> Self {
        let mut starting_id:u64 = 0;
        let mut text_files = HashMap::new();
        for e in files.iter() {
            // I read the file as a string
            let file_str = fs::read_to_string(e).unwrap();

            // I divide the string to obtain the name of the medias contained in it.
            let medias = divide_text_file(file_str.clone());

            // I created a unique id that distinguish that media, used by clients to easier computation.
            // The left-most byte is our nodeId, and the rest is dedicated to the file numeration;
            // Since we should have less text files than media ones, only the two right-most bytes are dedicated to text files' ids.
            let file_id = node_id as u64 * u64::from_be_bytes([1,0,0,0,0,0,0,0]) + starting_id;
            starting_id += 1;

            text_files.insert(file_id, (file_str, medias));
        }
        ContentServer {
            server_struct: ServerStruct::new(node_id, command_recv, event_send, packet_recv, packet_send),
            text_files,
            next_file_id: starting_id,
        }
    }

    // I had to comment them because of the M: MessageType I added to network_edge trait, but I don't understand why he complains,
    // in the client one it doesn't complain!
    //only difference is that ChatClient<M: MessageType>..
    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.server_struct.command_recv) -> cmd => {
                    if let Ok(command) = cmd {
                       self.handle_command(command);
                    }
                }
                recv(self.server_struct.packet_recv) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
            }
        }
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
                let file_str = fs::read_to_string(file).unwrap();

                // I divide the string to obtain the name of the medias contained in it.
                let medias = divide_text_file(file_str.clone());

                // I created a unique id that distinguish that media, used by clients to easier computation.
                // The left-most byte is our nodeId, and the rest is dedicated to the file numeration;
                // Since we should have less text files than media ones, only the two right-most bytes are dedicated to text files' ids.
                let file_id = self.get_src_id() as u64 * u64::from_be_bytes([1,0,0,0,0,0,0,0]) + self.next_file_id;
                self.next_file_id += 1;

                self.text_files.insert(file_id, (file_str, medias));
            }
        }
    }
    fn get_server_type(&self) -> ServerType {
        ServerType::Content(ContentServerType::Text)
    }
}

impl ContentServer {
    fn server_send_fragments(&mut self, message: Message, destination: NodeId) {
        let session_id = message.session_id;
        let frags = Self::fragment_message(&message);
        self.server_struct.fragments.insert((session_id, self.get_src_id()), (destination, frags.clone()));
        // I also save the fragments in the memory, in case I have to send them again.

        for fragment in frags {
            self.send_fragment(fragment, destination, session_id);
            // I apply the send operation on each single fragment.
        }
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