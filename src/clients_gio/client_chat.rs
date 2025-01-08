use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::Client;
use crate::clients_gio::client_type::ClientType;
use crate::message::{ChatResponse, ContentType, Message, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge};
use crate::routing::{Nodes, Route, RouteList};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::thread::sleep;
use std::time::Duration;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, Fragment, Nack, NackType, NodeType, Packet, PacketType};
use crate::clients_gio::client_command::ClientEvent::SendContacts;
use crate::clients_gio::client_type::ClientType::*;
use crate::DEBUG_MODE;
use crate::message::TextRequest::*;
use crate::server::server_type::ServerType;

pub struct ChatClient {
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, // Just like drones
    used_session_id: HashSet<u64>,     // Do we need this?

    paths: HashMap<NodeId, (u8, RouteList)>, // These NodeId are servers and clients, the u8 indicate if usable (1), if not usable (2), or if yet to be checked (0)
    nodes: Nodes, // Map of all Nodes, to apply checks on the PDRs.  
    contact_list: HashMap<NodeId, Vec<NodeId>>, // First NodeId is the client we communicate with, the second one is the vec of servers that make the connection possible
    fragments: HashMap<(u64, NodeId, NodeId), Vec<Fragment>>, //(session_id, source, destination)
    arrived_messages: HashMap<NodeId, Vec<Vec<u8>>>,
    unsent_fragments: (u8, HashMap<(u64, NodeId, NodeId), Vec<(Fragment)>>),
    // The second NodeId is the destination, the u8 is a counter (for now to the maximum I guess) to avoid sending too much stuff.
}

impl NetworkEdge for ChatClient {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        if let ContentType::TypeExchange(_exc) = message.clone().content {
            let session_id = message.session_id;
            let frags = Self::fragment_message(&message);
            self.fragments.insert((session_id, self.node_id, destination), frags.clone());
            // I also save the fragments in the memory, in case I have to send them again.

            for fragment in frags {
                self.send_fragment(fragment, destination, session_id);
                // I apply the send operation on each single fragment.
            }
        } else {
            if self.is_state_ok(destination) {
                let session_id = message.session_id;
                let frags = Self::fragment_message(&message);
                self.fragments.insert((session_id, self.node_id, destination), frags.clone());
                // I also save the fragments in the memory, in case I have to send them again.


                for fragment in frags {
                    self.send_fragment(fragment, destination, session_id);
                    // I apply the send operation on each single fragment.
                }
            }
            else {
                //new ClientEvent: state not good
            }
        }

    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let PacketType::FloodRequest(mut flood_request) = packet.pack_type.clone(){
            flood_request
                .path_trace
                .push((self.node_id, NodeType::Client));

            if self.flood_ids.insert((
                flood_request.flood_id.clone(),
                flood_request.initiator_id.clone(),
            )) {
                if self.packet_send.len() == 1 {
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
                    packet.pack_type = PacketType::FloodRequest(flood_request);
                    for (key, _) in self.packet_send.iter() {
                        //println!("Previous: {}", prev);
                        //println!("Key: {}", key);
                        if *key != prev {
                            //I send the flooding to everyone except the node I received it from.
                            if let Ok(_) =
                                self.packet_send.get(key).unwrap().send(packet.clone())
                            {
                                self.event_send
                                    .send(ClientEvent::PacketSent(packet.clone()))
                                    .unwrap();
                                //If the message was sent, I also notify the sim controller.
                            } //There's no else, since I don't care of nodes which can't be reached.
                        }
                    }
                }
            } else {
                self.send_flood_response(flood_request);
            }
        } else {
            if packet.routing_header.destination().unwrap() != self.node_id {
                // If it's not his packet, but he has to act as a drone (that never misses)
                packet.routing_header.hop_index += 1;
                let next_id = match packet.routing_header.hops.get(packet.routing_header.hop_index) {
                    Some(id) => *id,
                    None => {
                        //send nack NackType::ErrorInRouting(*next_hop))???

                        return;
                    },
                };

                match self.packet_send.get(&next_id) {
                    None => {
                        self.event_send.send(ClientEvent::MissingRoute(next_id)).unwrap()
                    }
                    Some(sender) => {
                        match sender.try_send(packet.clone()) {
                            Err(_) => {
                                // !!You need to send back the same errors a drone would
                                //send nack NackType::ErrorInRouting(*next_hop)) ??? ,
                                self.event_send.send(ClientEvent::MissingRoute(next_id)).unwrap()
                            }
                            Ok(_) => {
                                self.event_send
                                    .send(ClientEvent::PacketSent(packet.clone()))
                                    .unwrap();
                                // If the message was sent, I also notify the sim controller.
                            }
                        }
                    }
                }
            } else {
                // We can take for granted he is the destination
                match packet.pack_type.clone() {
                    PacketType::MsgFragment(fragment) => {
                        let tot_num_frag = fragment.total_n_fragments as usize;
                        let session_id = packet.session_id;
                        let initiator_id = packet.routing_header.hops[0];
                        let destination = self.node_id; //he is the destination
                        let frag_index = fragment.fragment_index;
                        //add new frag
                        if !self.fragments.contains_key(&(packet.session_id, initiator_id, destination)) {
                            self.fragments.insert((session_id, initiator_id, destination), vec![fragment]);
                        } else {
                            self.fragments.get_mut(&(session_id, initiator_id, destination)).unwrap().push(fragment);
                        }

                        //for each arrived frag, send back an ack
                        self.send_ack(packet.clone(), frag_index);

                        //notify sc i got a packet
                        self.event_send.send(ClientEvent::PacketReceived(packet.clone())).unwrap();




                        // If all the frag have arrived recreate message
                        let frags_clone = self.fragments.get(&(packet.session_id, initiator_id, destination)).unwrap();
                        if frags_clone.len() == tot_num_frag {
                            let message = match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                                Ok(mess) => { mess }
                                Err(e) => {
                                    println!("{e} with {}", self.node_id);

                                    unimplemented!() //
                                }
                            };
                            //handle message
                            self.handle_message(message);

                            // empty the hashmap
                            self.fragments.remove(&(packet.session_id, initiator_id, destination));
                        }
                    }
                    PacketType::Ack(ack) => {
                        self.event_send.send(ClientEvent::AckReceived(packet.clone())).unwrap();

                        //the ack will have the source that was the destination of the initial packet
                        match self.fragments.get_mut(&(packet.session_id, self.node_id, packet.routing_header.source().unwrap())) {
                            None => {}
                            Some(vec) => {
                                vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                                //if it's empty I retained all fragments because I received all the Ack, hence I can remove my entry from hashmap
                                if vec.is_empty() {
                                    self.fragments.remove_entry(&(packet.session_id, self.node_id, packet.routing_header.source().unwrap()));
                                }
                            }
                        }

                        // I apply the positive feed on all nodes in the path
                        let nodes = packet.routing_header.hops;
                        self.nodes.positive_feed(nodes);
                    }

                    PacketType::Nack(nack) => {
                        self.event_send.send(ClientEvent::NackReceived(packet.clone())).unwrap();
                        match nack.nack_type.clone() {
                            NackType::UnexpectedRecipient(wrong_node) => {
                                // I remove all the routes with that destination, since it's probably faulty
                                for (_, (_state,route)) in self.paths.iter_mut() {
                                    route.remove_faulty_node(wrong_node);
                                }
                                self.nodes.remove_faulty_node(wrong_node);
                                self.send_fragment_after_nack(packet, nack);
                            },
                            NackType::ErrorInRouting(wrong_node) => {
                                // I again remove the routes containing the (probably) crushed drone
                                for (_, (_state,route)) in self.paths.iter_mut() {
                                    route.remove_faulty_node(wrong_node);
                                }
                                self.nodes.remove_faulty_node(wrong_node);
                                self.send_fragment_after_nack(packet, nack);
                            },
                            NackType::DestinationIsDrone => {
                                let wrong_node = packet.routing_header.hops.last().unwrap();
                                for (_, (_state,route)) in self.paths.iter_mut() {
                                    route.remove_faulty_node(*wrong_node);
                                }
                                self.nodes.remove_faulty_node(*wrong_node);
                                // Since the destination was a drone, the message was faulty,
                                // so I remove the destination and consider the message as lost.
                                self.paths.remove(wrong_node);
                            },
                            NackType::Dropped => {
                                // I just send it again
                                self.send_fragment_after_nack(packet.clone(), nack);

                                // Who dropped will be source of the nack
                                let dropper = packet.routing_header.source().unwrap();
                                self.nodes.negative_feed(dropper);
                            }
                        }
                    }
                    PacketType::FloodRequest(_) => {
                        unreachable!()
                    }
                    PacketType::FloodResponse(flood_resp) => {
                        // As of rn it "saves" all possible servers and client... we want something else I think...
                        let mut current_path = Vec::new();
                        for (node_id, node_type) in flood_resp.path_trace {
                          
                             current_path.push((node_id, node_type));

                            if (node_type == NodeType::Server || node_type == NodeType::Client) && node_id != self.node_id {
                                if !self.paths.contains_key(&node_id) {
                                    //if it's first time this server gets seen
                                    self.paths.insert(node_id.clone(), (0,RouteList::new()));
                                    println!("{} inserted {:?}",self.node_id, node_id);
                                }
                                // Clone the current path for the server and insert it into the route list
                                match self.paths.get_mut(&node_id) {
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
                            self.contact_list.entry(i).and_modify(|vec|vec.push(source)).or_insert(vec![source]);
                        }
                    }
                    ChatResponse::MessageFrom { from, message } => {
                        self.arrived_messages.entry(from).or_insert(Vec::new()).push(message);
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
                            from: self.node_id,
                        };
                        let message = Message::new(self.node_id, self.get_session_id(), ContentType::TypeExchange(type_resp));

                        if !self.paths.contains_key(&from) {
                            println!("i don't have a path with {} to {from}", self.node_id);
                            self.flood();
                            // sleep(Duration::from_millis(100));
                        }

                        self.send_message(message, from);

                        // println!("Sent message with {} to {from}", self.node_id);

                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        if let EdgeType::Server(server_type) = edge_type{
                            match server_type{
                                ServerType::Chat => {
                                    self.paths.get_mut(&from).unwrap().0 = 1;
                                    self.event_send.send(SendContacts(self.node_id, from)).unwrap();
                                    },
                                _ => {
                                    self.paths.get_mut(&from).unwrap().0 = 2;
                                    // self.event_send.send(ClientEvent::SendContacts(self.node_id, from)).unwrap(); to debug

                                }
                            }
                        } else {
                            //if it's a client
                            self.paths.get_mut(&from).unwrap().0 = 2;

                            if DEBUG_MODE {
                            self.event_send.send(ClientEvent::SendContacts(self.node_id, from)).unwrap(); }

                        }
                    }
                }
            }
            _ => {
                // Gio: no point in getting other types of req
                // !!Leo: We still need to tell that it was an error tho, probably by
                // !!sending a Nack UnexpectedRecipient(self.NodeId),

                //todo send_nack()

            }
        }

    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        if destination == self.node_id {
            println!("Sending message to yourself with {:?}", destination);
            return;
        }

        match self.paths.get_mut(&destination) {
            None => {
                //I first check if I have any path to the destination
                println!("Tried to send fragment without path to {destination} with {}", self.node_id);
                self.event_send
                    .send(ClientEvent::MissingDestination(destination))
                    .unwrap();
                self.add_unsent_fragment(fragment, session_id, destination);
            }
            Some((_state, route_list)) => {
                match route_list.get_fastest_route() {
                    None => {
                        // I then check that we have an available route to the destination.
                        self.event_send
                            .send(ClientEvent::MissingRoute(destination))
                            .unwrap();

                        self.add_unsent_fragment(fragment, session_id, destination);
                    },
                    Some(route) => {
                        let srh = route.to_source_routing_header();
                        let first_dst = srh.hops[1];
                        let packet = Packet::new_fragment(srh, session_id, fragment.clone());

                        // If everything worked, I try to send.
                        match self.packet_send.get(&first_dst) {
                            Some(sender) => {
                                sender.send(packet.clone()).unwrap();
                                self.event_send
                                    .send(ClientEvent::PacketSent(packet.clone()))
                                    .expect(format!("panicked with {}", self.node_id).as_str());

                            }
                            None => {
                                // If I want to pass for a node that I don't have as a neighbour, I need to remove
                                // channels who contain it.
                                self.event_send
                                    .send(ClientEvent::MissingRoute(destination))
                                    .unwrap();
                                self.add_unsent_fragment(fragment, session_id, destination);
                                for (_, (_state,route)) in self.paths.iter_mut() {
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
        // If the sending of a fragment gave an error, we put it in a hashmap to try sending it again.
        match self.unsent_fragments.1.get_mut(&(session_id, self.node_id, destination)) {
            Some(fragments) => {
                fragments.push(fragment);
            },
            None => {
                let mut vec = Vec::new();
                vec.push(fragment);
                self.unsent_fragments.1.insert((session_id, self.node_id, destination), vec);
            }
        }
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        match self.fragments.get(&(packet.session_id, self.node_id, packet.routing_header.destination().unwrap())) {
            // I try to find again the fragment, and notify the sim controller if I don't have it anymore
            None => {
                self.event_send
                    .send(ClientEvent::LostMessage(packet.session_id, self.node_id))
                    .unwrap();
            },
            Some(fragments) => {
                match fragments.get(nack.fragment_index as usize) {
                    None => {
                        self.event_send
                            .send(ClientEvent::LostFragment(packet.session_id, self.node_id, nack.fragment_index))
                            .unwrap();
                    },
                    // If I manage to find the fragment, I send it
                    Some(fragment) => {
                        self.send_fragment(fragment.clone(), *packet.routing_header.hops.get(0).unwrap(), packet.session_id);
                    }
                }
            }
        }
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        let new_hops: Vec<NodeId> = packet.routing_header.hops.iter().rev().map(|(id)| *id)
            .collect::<Vec<NodeId>>();
        let next_id = new_hops[1];
        let srh = SourceRoutingHeader::new(new_hops, 1); //is it 1 right?
        let packet_ack = Packet::new_ack(srh, packet.session_id, fragment_index);

        match self.packet_send.get(&next_id) {
            Some(sender) => {
                sender.send(packet_ack.clone()).unwrap();
                self.event_send.send(ClientEvent::PacketSent(packet_ack)).unwrap();
            }
            None => {
                self.event_send.send(ClientEvent::MissingDestination(next_id)).unwrap();
            }
        }
    }

    fn flood(&mut self) {

        // !!I'm not sure if this is a good idea or not, since they can't crush I don't
        // !!see why we would need to clear it
        // self.contact_list.clear();

        let flood_request = FloodRequest{
            flood_id: self.get_flood_id(),
            initiator_id: self.node_id,
            path_trace: vec![(self.node_id, NodeType::Client)],
        };
        let packet = Packet::new_flood_request(SourceRoutingHeader::default(), fastrand::u64(..500), flood_request);
        self.packet_send.values().for_each(|sender| {
            sender.send(packet.clone()).unwrap()
        });
    }

    fn get_flood_id(&mut self) -> u64 {
        let min = match self.flood_ids.iter().min(){
            Some(min) => (*min).0,
            None => {
                let value = fastrand::u64(..30);
                self.flood_ids.insert((value, self.node_id));
                return value
            }
        };
        let value = fastrand::u64(min..min + 40);
        self.flood_ids.insert((value, self.node_id));
        value
    }

    fn get_session_id(&mut self) -> u64 {
        let min = match self.used_session_id.iter().min(){
            Some(min) => *min,
            None => {
                let value = fastrand::u64(..30);
                self.used_session_id.insert(value);
                return value
            }
        };
        let value = fastrand::u64(min..min + 40);
        self.used_session_id.insert(value);
        value
    }


    fn check_type(&mut self, id: NodeId) {
        let req = TypeExchange::TypeRequest { from: self.node_id };
        let exc = ContentType::TypeExchange(req);
        let s_id = self.get_session_id();
        self.send_message(Message::new(self.node_id, s_id, exc), id);
        println!("sent check from {}", self.node_id);
    }

    fn is_state_ok(&mut self, node_id: NodeId) -> bool {
        let out =  match self.paths.get(&node_id){
            Some(path) => {
                path.0 == 1
            }
            None =>{false}
        };
        if !out {
            println!("dst state was not ok");
            //send nack?
        }
        out
    }
}


impl Client for ChatClient {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ChatClient {
            node_id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            flood_ids: HashSet::new(),
            used_session_id: HashSet::new(),
            paths: HashMap::new(),
            nodes: Nodes::new(),
            contact_list: HashMap::new(),
            fragments: HashMap::new(),
            arrived_messages: HashMap::new(),
            unsent_fragments: (0, HashMap::new()),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.command_recv) -> cmd => {
                    if let Ok(command) = cmd {
                        self.handle_command(command);
                    }
                }
                recv(self.packet_recv) -> pkt => {
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
            if self.unsent_fragments.0 >= 150 {
                //if I have some unchecked nodes I try to check them

                self.paths.clone().iter().for_each(|(dst, (state, path))| {
                    if *state == 0{
                        self.check_type(dst.clone());
                    }
                });

                // I create a temporary copy of the fragments that needs to be processed.
                let mut to_process = Vec::new();
                for (identifier, content) in self.unsent_fragments.1.iter() {
                    for fragment in content.iter() {
                        to_process.push((fragment.clone(), identifier.clone()));
                    }
                }
                // I then empty the HashMap to not have any duplicate.as
                self.unsent_fragments.1 = HashMap::new();
                self.unsent_fragments.0 = 0;

                for (fragment, identifier) in to_process {
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
                self.unsent_fragments.0 += 1;
            }
        }
    }

    fn handle_command(&mut self, command: ClientCommand) {
        match command {
            ClientCommand::RemoveSender(node_id) => {
                if self.packet_send.contains_key(&node_id) {
                    if let Some(to_be_dropped) = self.packet_send.remove(&node_id) {
                        drop(to_be_dropped);
                        //println!("Client {} no more has a connection to {}!", self.node_id, node_id);
                    }
                }
            }
            ClientCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
            }
            ClientCommand::SendMessage(node_id, message) => {
                self.send_message(message, node_id);
            }
            ClientCommand::Flood =>{
                self.flood();
            }
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::ChatClient
    }
}
