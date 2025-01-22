use crate::clients_gio::client_command::ClientEvent::{SendChatText, SendContactsToSC, SendDestinations};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::clients_gio::client_type::ClientType::*;
use crate::clients_gio::client_struct::ClientStruct;
use crate::message::EdgeNackType::*;
use crate::message::TextRequest::*;
use crate::message::{ChatResponse, ContentType, Message, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::routing::{Route, RouteList};
use crate::server::server_type::ServerType;
use crate::{ALL_FLOOD_MODE, DEBUG_MODE};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;
use wg_2024::network::NodeId;
use wg_2024::packet::NackType::ErrorInRouting;
use wg_2024::packet::PacketType::*;
use wg_2024::packet::{Fragment, Nack, NackType, NodeType, Packet, PacketType};
use crate::message::ChatRequest::{ClientList, Register, SendMessage};
use crate::message::ContentType::*;

pub struct ChatClient {
    comm: ClientStruct, //common client duh
    //chat client specks
    contact_list: HashMap<NodeId, Vec<NodeId>>, // First NodeId is the client we communicate with, the second one is the vec of servers that make the connection possible
    all_messages: HashMap<NodeId, Vec<(NodeId, String)>>,

}

impl NetworkEdge for ChatClient {
    fn send_message(&mut self, message: Message, destination: NodeId) {
       self.comm.send_message(message, destination)
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let FloodRequest(mut flood_request) = packet.pack_type.clone(){
            flood_request
                .path_trace
                .push((self.comm.node_id, NodeType::Client));

            if self.comm.flood_ids.insert((
                flood_request.flood_id.clone(),
                flood_request.initiator_id.clone(),
            )) {
                if self.comm.packet_send.len() == 1 {
                    self.send_flood_response(flood_request);
                } else {
                    let mut prev = flood_request.initiator_id.clone();
                    if flood_request.path_trace.clone().len() > 1 {
                        prev = flood_request
                            .path_trace
                            .get(flood_request.path_trace.len() - 2)
                            .unwrap()
                            .0;
                    }
                    //I update the path_trace in the packet.
                    packet.pack_type = FloodRequest(flood_request);
                    for (key, _) in self.comm.packet_send.iter() {
                        //println!("Previous: {}", prev);
                        //println!("Key: {}", key);
                        if *key != prev {
                            //I send the flooding to everyone except the node I received it from.
                            if let Ok(_) =
                                self.comm.packet_send.get(key).unwrap().send(packet.clone())
                            {
                                self.send_event(ClientEvent::PacketSent(packet.clone()));
                                //If the message was sent, I also notify the sim controller.
                            } //There's no else, since I don't care of nodes which can't be reached.
                        }
                    }
                }
            } else {
                self.send_flood_response(flood_request);
            }
        } else {
            if packet.routing_header.destination().unwrap() != self.comm.node_id {
                // If it's not his packet, but he has to act as a drone (that never misses)
                packet.routing_header.hop_index += 1;
                let next_id = match packet.routing_header.hops.get(packet.routing_header.hop_index) {
                    Some(id) => *id,
                    None => {
                        //teoricamente se è none è perchè è lui stesso la destinazione
                        unreachable!()
                    },
                };

                match self.comm.packet_send.get(&next_id) {
                    None => {
                        self.send_event(ClientEvent::MissingRoute(next_id))
                    }
                    Some(sender) => {
                        match sender.try_send(packet.clone()) {
                            Err(_) => {
                                // !!You need to send back the same errors a drone would
                                self.send_drone_nack(packet.routing_header.source().unwrap(), ErrorInRouting(next_id));
                                self.send_event(ClientEvent::PacketSendingError(packet));
                            }
                            Ok(_) => {
                                self.send_event(ClientEvent::PacketSent(packet.clone()));
                                // If the message was sent, I also notify the sim controller.
                            }
                        }
                    }
                }
            } else {
                // We can take for granted he is the destination
                match packet.pack_type.clone() {
                    MsgFragment(fragment) => {
                        let tot_num_frag = fragment.total_n_fragments as usize;
                        let session_id = packet.session_id;
                        let initiator_id = packet.routing_header.hops[0];
                        let destination = self.comm.node_id; //he is the destination
                        let frag_index = fragment.fragment_index;
                        //add new frag
                        if !self.comm.fragments.contains_key(&(packet.session_id, initiator_id, destination)) {
                            self.comm.fragments.insert((session_id, initiator_id, destination), vec![fragment]);
                        } else {
                            self.comm.fragments.get_mut(&(session_id, initiator_id, destination)).unwrap().push(fragment);
                        }

                        //for each arrived frag, send back an ack
                        self.send_ack(packet.clone(), frag_index);

                        //notify sc i got a packet
                        self.send_event(ClientEvent::PacketReceived(packet.clone()));




                        // If all the frag have arrived recreate message
                        let frags_clone = self.comm.fragments.get(&(packet.session_id, initiator_id, destination)).unwrap();
                        if frags_clone.len() == tot_num_frag {
                            let message = match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                                Ok(mess) => { mess }
                                Err(e) => {
                                    println!("{e} with {}", self.comm.node_id);

                                    unimplemented!() //
                                }
                            };
                            //handle message
                            self.handle_message(message);

                            // empty the hashmap
                            self.comm.fragments.remove(&(packet.session_id, initiator_id, destination));
                        }
                    }
                    Ack(ack) => {
                        self.send_event(ClientEvent::AckReceived(packet.clone()));

                        //the ack will have the source that was the destination of the initial packet
                        match self.comm.fragments.get_mut(&(packet.session_id, self.comm.node_id, packet.routing_header.source().unwrap())) {
                            None => {}
                            Some(vec) => {
                                vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                                //if it's empty I retained all fragments because I received all the Ack, hence I can remove my entry from hashmap
                                if vec.is_empty() {
                                    self.comm.fragments.remove_entry(&(packet.session_id, self.comm.node_id, packet.routing_header.source().unwrap()));
                                }
                            }
                        }

                        // I apply the positive feed on all nodes in the path
                        let nodes = packet.routing_header.hops;
                        self.comm.nodes.positive_feed(nodes);
                    }

                    PacketType::Nack(nack) => {
                        self.send_event(ClientEvent::NackReceived(packet.clone()));
                        match nack.nack_type.clone() {
                            NackType::UnexpectedRecipient(wrong_node) => {
                                // I remove all the routes with that destination, since it's probably faulty
                                for (_, (_state,route)) in self.comm.paths.iter_mut() {
                                    route.remove_faulty_node(wrong_node);
                                }
                                self.comm.nodes.remove_faulty_node(wrong_node);
                                self.send_fragment_after_nack(packet, nack);
                            },
                            ErrorInRouting(wrong_node) => {
                                // I again remove the routes containing the (probably) crushed drone
                                for (_, (_state,route)) in self.comm.paths.iter_mut() {
                                    route.remove_faulty_node(wrong_node);
                                }
                                self.comm.nodes.remove_faulty_node(wrong_node);
                                self.send_fragment_after_nack(packet, nack);
                            },
                            NackType::DestinationIsDrone => {
                                let wrong_node = packet.routing_header.hops.last().unwrap();
                                for (_, (_state,route)) in self.comm.paths.iter_mut() {
                                    route.remove_faulty_node(*wrong_node);
                                }
                                self.comm.nodes.remove_faulty_node(*wrong_node);
                                // Since the destination was a drone, the message was faulty,
                                // so I remove the destination and consider the message as lost.
                                self.comm.paths.remove(wrong_node);
                            },
                            NackType::Dropped => {
                                // I just send it again
                                self.send_fragment_after_nack(packet.clone(), nack);

                                // Who dropped will be source of the nack
                                let dropper = packet.routing_header.source().unwrap();
                                self.comm.nodes.negative_feed(dropper);
                            }
                        }
                    }
                    FloodRequest(_) => {
                        unreachable!()
                    }
                    FloodResponse(flood_resp) => {
                        // As of rn it "saves" all possible servers and client... we want something else I think...
                        let mut current_path = Vec::new();
                        for (node_id, node_type) in flood_resp.path_trace {
                          
                             current_path.push((node_id, node_type));

                            if (node_type == NodeType::Server || node_type == NodeType::Client) && node_id != self.comm.node_id {
                                if !self.comm.paths.contains_key(&node_id) {
                                    //if it's first time this server gets seen
                                    self.comm.paths.insert(node_id.clone(), (0,RouteList::new()));
                                    println!("{} inserted {:?}",self.comm.node_id, node_id);
                                }
                                // Clone the current path for the server and insert it into the route list
                                match self.comm.paths.get_mut(&node_id) {
                                    None => {
                                        unreachable!()
                                        //i hope it's unreachable
                                    }
                                    Some((_state,route_list)) => {
                                        // There's a check inside add_route that doesn't add a route if it's already inside the list.
                                        route_list.add_route(Route::new(current_path.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_message(&mut self, message: Message) {
        match message.content {
            ContentType::ChatResponse(resp) => {
                match resp{
                    ChatResponse::ClientList(list) => {
                        let source= message.source_id;
                        for i in list {
                            let is_new = self.contact_list.entry(i).and_modify(|vec| vec.push(source)).or_insert(vec![source]).len() == 1;

                            if is_new {
                                //notify sc that you now have that contact
                                self.send_event(SendContactsToSC(self.comm.node_id, i));
                            }

                        }
                    }
                    ChatResponse::MessageFrom { from, message } => {
                        self.send_event(SendChatText(self.get_src_id(), from, message.clone()));
                        self.all_messages.entry(from).or_insert(vec![(from, message.clone())]).push((from, message));
                    }
                    ChatResponse::MessageSent => {
                        // not sure, is just an ack? I don't think we need this (also because if they
                        // don't have any information I can't know which message are they referring too)
                    }
                }
            }

            ContentType::TypeExchange(exchange) => {
                match exchange {
                    TypeExchange::TypeRequest { from } => {
                        let type_resp = TypeExchange::TypeResponse {
                            edge_type: EdgeType::Client(ClientType::ChatClient),
                            from: self.comm.node_id,
                        };
                        let message = Message::new(self.comm.node_id, self.get_session_id(), ContentType::TypeExchange(type_resp));

                        if !self.comm.paths.contains_key(&from) {
                            println!("i don't have a path with {} to {from}", self.comm.node_id);
                            self.flood();
                        }

                        self.send_message(message, from);

                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        if let EdgeType::Server(server_type) = edge_type{
                            match server_type{
                                ServerType::Chat => {
                                    self.comm.paths.get_mut(&from).unwrap().0 = 1;
                                    self.send_event(SendDestinations(self.comm.node_id, from));
                                    },
                                _ => {
                                    self.comm.paths.get_mut(&from).unwrap().0 = 2;
                                }
                            }
                        } else {
                            //if it's a client
                            self.comm.paths.get_mut(&from).unwrap().0 = 2;

                            if ALL_FLOOD_MODE {
                                self.send_event(SendDestinations(self.comm.node_id, from));}
                                self.send_event(SendContactsToSC(self.comm.node_id, from));

                        }
                    }
                }
            }
            ContentType::EdgeNack(nack) => {
                match nack {
                    UnexpectedMessage => {
                        //vuol dire che ha mandato un message al dst con state sbagliato.
                       if let Some((state, _route)) = self.comm.paths.get_mut(&message.source_id){
                           *state = 2;
                       }

                        //e il messaggio viene scartato credo

                        if DEBUG_MODE{
                            println!("Client {} discarded message to {} after receiving his nack, because state was not good", self.comm.node_id, message.source_id)
                        }

                    }
                }

            },
            _ => {
                // Gio: no point in getting other types of req
                let new_nack = self.create_nack(UnexpectedMessage);
                self.send_nack_message(message.source_id, new_nack);
            }
        }

    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
       self.comm.send_fragment(fragment, destination, session_id)
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.comm.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        self.comm.send_fragment_after_nack(packet, nack)
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        self.comm.send_ack(packet, fragment_index);
    }

    fn flood(&mut self) {
        self.comm.flood();
    }

    fn get_flood_id(&mut self) -> u64 {
        self.comm.get_flood_id()
    }

    fn get_session_id(&mut self) -> u64 {
       self.comm.get_session_id()
    }

    fn get_src_id(&self) -> NodeId {
        self.comm.get_src_id()
    }
}

impl NetworkEdgeErrors for ChatClient {
    fn check_type(&mut self, id: NodeId) {
        self.comm.check_type(id);
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        self.comm.is_state_ok(node_id)
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.comm.send_nack_message(dst, nack);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        self.comm.send_drone_nack(dst, nack);
    }
}

impl ClientTrait for ChatClient {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ChatClient {
            comm: ClientStruct::new(node_id, command_recv, event_send, packet_recv, packet_send),
            contact_list: HashMap::new(),
            all_messages: HashMap::new(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.comm.command_recv) -> cmd => {
                    if let Ok(command) = cmd {
                        self.handle_command(command);
                    }
                }
                recv(self.comm.packet_recv) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
                default => {
                     sleep(Duration::from_millis(10));
                    // Wait a second before going on.
                }
            }
            // I check a counter, so that I don't try to send all the fragments every loop.
            if self.comm.unsent_fragments.0 >= 150 {
                //if I have some unchecked nodes I try to check them

                self.comm.paths.clone().iter().for_each(|(dst, (state, path))| {
                    if *state == 0{
                        self.check_type(dst.clone());
                    }
                });

                // I create a temporary copy of the fragments that needs to be processed.
                let mut to_process = Vec::new();
                for (identifier, content) in self.comm.unsent_fragments.1.iter() {
                    for fragment in content.iter() {
                        to_process.push((fragment.clone(), identifier.clone()));
                    }
                }
                // I then empty the HashMap to not have any duplicate.as
                self.comm.unsent_fragments.1 = HashMap::new();
                self.comm.unsent_fragments.0 = 0; for (fragment, identifier) in to_process {
                    self.send_fragment(fragment.clone(), identifier.2, identifier.0);
                }

                //uncomment to check flood periodically

                // let mut path_printable = String::new();
                // self.paths.clone().iter_mut().for_each(|(dst, (state, path))| {
                //     let destination = format!("Node {}, State: {}, path: *not now* \n", dst, state);
                //     path_printable.push_str(destination.as_str());
                // });
                // println!("{} has paths: {:?}",self.node_id, path_printable);



            } else {
                self.comm.unsent_fragments.0 += 1;
            }
        }
    }


    fn handle_command(&mut self, command: ClientCommand) {
        match command {
            ClientCommand::RemoveSender(node_id) => {
                if self.comm.packet_send.contains_key(&node_id) {
                    if let Some(to_be_dropped) = self.comm.packet_send.remove(&node_id) {
                        drop(to_be_dropped);
                        //println!("Client {} no more has a connection to {}!", self.node_id, node_id);
                    }
                }
            }
            ClientCommand::AddSender(node_id, sender) => {
                self.comm.packet_send.insert(node_id, sender);
            }

            ClientCommand::Flood =>{
                self.flood();
            }

            //commands for chatclient
            ClientCommand::RetrieveList(id) => {
                self.get_list(id);
            }
            ClientCommand::Register(id) => {
                self.register(id);
            }
            ClientCommand::SendMSG(id, str) => {
                self.send_chat_text(id, str);
            }

            //ignore other commands cause are webclients commands
            _ =>{

            }
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::ChatClient
    }

    fn send_event(&self, ce: ClientEvent) {
       self.comm.send_event(ce);
    }
}

impl ChatClient {
    fn get_list(&mut self, id: NodeId){
        let src = self.comm.get_src_id();
        let session = self.comm.get_session_id();
        let content = ChatRequest(ClientList);
        let msg = Message::new(src, session, content);
        self.comm.send_message(msg, id);

        if DEBUG_MODE{
            println!("sent client list req from {src} to server {id}")
        }
    }
    fn register(&mut self, id: NodeId){
        let src = self.get_src_id();
        let session = self.get_session_id();
        let content = ChatRequest(Register(src));
        let msg = Message::new(src, session, content);
        self.comm.send_message(msg, id);

        if DEBUG_MODE{
            println!("sent register req from {src} to server {id}")
        }
    }

    //id is the client receiver
    fn send_chat_text(&mut self, id: NodeId, str: String){
        let src = self.get_src_id();
        if let Some(servers) = self.contact_list.get(&id){
            if let Some(server_id) = self.comm.get_optimal_dest (servers){
                //decide witch server to contact, for the moment just the first one is okay

                let session = self.get_session_id();
                let content = ChatRequest(SendMessage {
                    from: src,
                    to: id,
                    message: str.clone(),
                });

                //keep track of the outgoing message in our personal chat
                self.send_event(SendChatText(self.get_src_id(), id, str.clone()));
                self.all_messages.entry(id).or_insert(vec!((src, str.clone()))).push((src, str));

                let msg = Message::new(src, session, content);
                self.comm.send_message(msg, server_id);
            }
        } else {
            self.send_event(ClientEvent::MissingContacts(src,id))
            // i don't think we should resend it when we have it, if we want to resend it just wait another input

        }

    }
}