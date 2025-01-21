use crate::message::{ChatResponse, ContentType, EdgeNackType, Message, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_trait::Server;
use crate::server::server_type::ServerType;
use crossbeam_channel::{select_biased, Receiver, Sender};
use dr_ones::Packet;
use std::collections::HashMap;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, Fragment, Nack, NackType, NodeType, PacketType};
use crate::clients_gio::client_type::ClientType;
use crate::server::server_struct::ServerStruct;
use crate::routing::{Route, RouteList};
use crate::DEBUG_MODE;

pub struct ChatServer {
    server_struct: ServerStruct,
}

impl NetworkEdge for ChatServer {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        match message.clone().content{
            ContentType::TypeExchange(_exc) =>{
                let session_id = message.session_id;
                let frags = Self::fragment_message(&message);
                self.server_struct.fragments.insert((session_id, self.server_struct.node_id, destination), frags.clone());
                // I also save the fragments in the memory, in case I have to send them again.

                for fragment in frags {
                    self.send_fragment(fragment, destination, session_id);
                    // I apply the send operation on each single fragment.
                }
            },
            ContentType::EdgeNack(_nack) => {
                let session_id = message.session_id;
                let frags = Self::fragment_message(&message);
                self.server_struct.fragments.insert((session_id, self.server_struct.node_id, destination), frags.clone());
                // I also save the fragments in the memory, in case I have to send them again.

                for fragment in frags {
                    self.send_fragment(fragment, destination, session_id);
                    // I apply the send operation on each single fragment.
                }
            }
            _=>{
                if self.is_state_ok(destination) {
                    let session_id = message.session_id;
                    let frags = Self::fragment_message(&message);
                    self.server_struct.fragments.insert((session_id, self.server_struct.node_id, destination), frags.clone());
                    // I also save the fragments in the memory, in case I have to send them again.


                    for fragment in frags {
                        self.send_fragment(fragment, destination, session_id);
                        // I apply the send operation on each single fragment.
                    }
                }
                else {
                    let new_nack = ServerEvent::WrongDestinationType(self.get_src_id(), destination);
                    self.send_event(new_nack);
                }
            }
        }
    }

    fn fragment_message(message: &Message) -> Vec<Fragment> {
        todo!()
    }

    fn reassemble_message(session_id: u64, source_id: NodeId, packets: &Vec<Fragment>) -> Result<Message, String> {
        todo!()
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let PacketType::FloodRequest(mut flood_request) = packet.pack_type.clone(){
            flood_request
                .path_trace
                .push((self.server_struct.node_id, NodeType::Server));

            if self.server_struct.flood_ids.insert((
                flood_request.flood_id.clone(),
                flood_request.initiator_id.clone(),
            )) {
                if self.server_struct.packet_send.len() == 1 {
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
                    for (key, _) in self.server_struct.packet_send.iter() {
                        //println!("Previous: {}", prev);
                        //println!("Key: {}", key);
                        if *key != prev {
                            //I send the flooding to everyone except the node I received it from.
                            if let Ok(_) =
                                self.server_struct.packet_send.get(key).unwrap().send(packet.clone())
                            {
                                // self.send_event(ServerEvent::PacketSent(packet.clone()));
                                //If the message was sent, I also notify the sim controller.
                            } //There's no else, since I don't care of nodes which can't be reached.
                        }
                    }
                }
            } else {
                self.send_flood_response(flood_request);
            }
        } else {
            if packet.routing_header.destination().unwrap() != self.server_struct.node_id {
                // If it's not his packet, but he has to act as a drone (that never misses)
                packet.routing_header.hop_index += 1;
                let next_id = match packet.routing_header.hops.get(packet.routing_header.hop_index) {
                    Some(id) => *id,
                    None => {
                        // Theoretically if it's 'none' it's because the destination it's itself.
                        unreachable!()
                    },
                };

                match self.server_struct.packet_send.get(&next_id) {
                    None => {
                        self.send_event(ServerEvent::MissingRoute(next_id))
                    }
                    Some(sender) => {
                        match sender.try_send(packet.clone()) {
                            Err(_) => {
                                // !!You need to send back the same errors a drone would
                                self.send_drone_nack(packet.routing_header.source().unwrap(), NackType::ErrorInRouting(next_id));
                                self.send_event(ServerEvent::PacketSendingError(packet));
                            }
                            Ok(_) => {
                                self.send_event(ServerEvent::PacketSent(packet.clone()));
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
                        let destination = self.server_struct.node_id; //he is the destination
                        let frag_index = fragment.fragment_index;
                        //add new frag
                        if !self.server_struct.fragments.contains_key(&(packet.session_id, initiator_id, destination)) {
                            self.server_struct.fragments.insert((session_id, initiator_id, destination), vec![fragment]);
                        } else {
                            self.server_struct.fragments.get_mut(&(session_id, initiator_id, destination)).unwrap().push(fragment);
                        }

                        //for each arrived frag, send back an ack
                        self.send_ack(packet.clone(), frag_index);

                        //notify sc i got a packet
                        self.send_event(ServerEvent::PacketReceived(packet.clone()));




                        // If all the frag have arrived recreate message
                        let frags_clone = self.server_struct.fragments.get(&(packet.session_id, initiator_id, destination)).unwrap();
                        if frags_clone.len() == tot_num_frag {
                            let message = match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                                Ok(mess) => { mess }
                                Err(e) => {
                                    println!("{e} with {}", self.server_struct.node_id);

                                    unimplemented!() //
                                }
                            };
                            //handle message
                            self.handle_message(message);

                            // empty the hashmap
                            self.server_struct.fragments.remove(&(packet.session_id, initiator_id, destination));
                        }
                    }
                    PacketType::Ack(ack) => {
                        self.send_event(ServerEvent::AckReceived(packet.clone()));

                        //the ack will have the source that was the destination of the initial packet
                        match self.server_struct.fragments.get_mut(&(packet.session_id, self.server_struct.node_id, packet.routing_header.source().unwrap())) {
                            None => {}
                            Some(vec) => {
                                vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                                //if it's empty I retained all fragments because I received all the Ack, hence I can remove my entry from hashmap
                                if vec.is_empty() {
                                    self.server_struct.fragments.remove_entry(&(packet.session_id, self.server_struct.node_id, packet.routing_header.source().unwrap()));
                                }
                            }
                        }

                        // I apply the positive feed on all nodes in the path
                        let nodes = packet.routing_header.hops;
                        self.server_struct.nodes.positive_feed(nodes);
                    }

                    PacketType::Nack(nack) => {
                        self.send_event(ServerEvent::NackReceived(packet.clone()));
                        match nack.nack_type.clone() {
                            NackType::UnexpectedRecipient(wrong_node) => {
                                // I remove all the routes with that destination, since it's probably faulty
                                for (_, (_state,route)) in self.server_struct.paths.iter_mut() {
                                    route.remove_faulty_node(wrong_node);
                                }
                                self.server_struct.nodes.remove_faulty_node(wrong_node);
                                self.send_fragment_after_nack(packet, nack);
                            },
                            NackType::ErrorInRouting(wrong_node) => {
                                // I again remove the routes containing the (probably) crushed drone
                                for (_, (_state,route)) in self.server_struct.paths.iter_mut() {
                                    route.remove_faulty_node(wrong_node);
                                }
                                self.server_struct.nodes.remove_faulty_node(wrong_node);
                                self.send_fragment_after_nack(packet, nack);
                            },
                            NackType::DestinationIsDrone => {
                                let wrong_node = packet.routing_header.hops.last().unwrap();
                                for (_, (_state,route)) in self.server_struct.paths.iter_mut() {
                                    route.remove_faulty_node(*wrong_node);
                                }
                                self.server_struct.nodes.remove_faulty_node(*wrong_node);
                                // Since the destination was a drone, the message was faulty,
                                // so I remove the destination and consider the message as lost.
                                self.server_struct.paths.remove(wrong_node);
                            },
                            NackType::Dropped => {
                                // I just send it again
                                self.send_fragment_after_nack(packet.clone(), nack);

                                // Who dropped will be source of the nack
                                let dropper = packet.routing_header.source().unwrap();
                                self.server_struct.nodes.negative_feed(dropper);
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

                            if (node_type == NodeType::Server || node_type == NodeType::Client) && node_id != self.server_struct.node_id {
                                if !self.server_struct.paths.contains_key(&node_id) {
                                    //if it's first time this server gets seen
                                    self.server_struct.paths.insert(node_id.clone(), (0,RouteList::new()));
                                    println!("{} inserted {:?}",self.server_struct.node_id, node_id);
                                }
                                // Clone the current path for the server and insert it into the route list
                                match self.server_struct.paths.get_mut(&node_id) {
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
                            self.server_struct.contact_list.entry(i).and_modify(|vec|vec.push(source)).or_insert(vec![source]);
                        }
                    }
                    ChatResponse::MessageFrom { from, message } => {
                        self.server_struct.arrived_messages.entry(from).or_insert(Vec::new()).push(message);
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
                            from: self.server_struct.node_id,
                        };
                        let message = Message::new(self.server_struct.node_id, self.get_session_id(), ContentType::TypeExchange(type_resp));

                        if !self.server_struct.paths.contains_key(&from) {
                            println!("i don't have a path with {} to {from}", self.server_struct.node_id);
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
                                    self.server_struct.paths.get_mut(&from).unwrap().0 = 1;
                                    //self.send_event(SendContacts(self.node_id, from));
                                },
                                _ => {
                                    self.server_struct.paths.get_mut(&from).unwrap().0 = 2;
                                    // self.send_event(ServerEvent::SendContacts(self.node_id, from)).unwrap(); to debug

                                }
                            }
                        } else {
                            //if it's a client
                            self.server_struct.paths.get_mut(&from).unwrap().0 = 2;

                            /*if DEBUG_MODE {
                                self.send_event(ServerEvent::SendContacts(self.server_struct.node_id, from)) }
                            */
                        }
                    }
                }
            }
            ContentType::EdgeNack(nack) => {
                match nack {
                    EdgeNackType::UnexpectedMessage => {
                        // means that it sent a msg to a dst with a wrong state
                        if let Some((state, _route)) = self.server_struct.paths.get_mut(&message.source_id){
                            *state = 2;
                        }

                        // and I think the message should be discarded

                        if DEBUG_MODE{
                            println!("Client {} discarded message to {} after receiving his nack, because state was not good", self.server_struct.node_id, message.source_id)
                        }

                    }
                }

            },
            _ => {
                // Gio: no point in getting other types of req
                let new_nack = self.create_nack(EdgeNackType::UnexpectedMessage);
                self.send_nack_message(message.source_id, new_nack);
            }
        }

    }

    fn send_flood_response(&mut self, flood: FloodRequest) {
        todo!()
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        if destination == self.server_struct.node_id {
            println!("Sending message to yourself with {:?}", destination);
            return;
        }

        match self.server_struct.paths.get_mut(&destination) {
            None => {
                //I first check if I have any path to the destination
                println!("Tried to send fragment without path to {destination} with {}", self.server_struct.node_id);
                self.send_event(ServerEvent::MissingDestination(destination));
                self.add_unsent_fragment(fragment, session_id, destination);
            }
            Some((_state, route_list)) => {
                match route_list.get_fastest_route() {
                    None => {
                        // I then check that we have an available route to the destination.
                        self.send_event(ServerEvent::MissingRoute(destination));

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
                                self.send_event(ServerEvent::PacketSent(packet.clone()));

                            }
                            None => {
                                // If I want to pass for a node that I don't have as a neighbour, I need to remove
                                // channels who contain it.
                                self.send_event(ServerEvent::MissingRoute(destination));
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
        // If the sending of a fragment gave an error, we put it in a hashmap to try sending it again.
        match self.server_struct.unsent_fragments.1.get_mut(&(session_id, self.server_struct.node_id, destination)) {
            Some(fragments) => {
                fragments.push(fragment);
            },
            None => {
                let mut vec = Vec::new();
                vec.push(fragment);
                self.server_struct.unsent_fragments.1.insert((session_id, self.server_struct.node_id, destination), vec);
            }
        }
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        match self.server_struct.fragments.get(&(packet.session_id, self.server_struct.node_id, packet.routing_header.destination().unwrap())) {
            // I try to find again the fragment, and notify the sim controller if I don't have it anymore
            None => {
                self.send_event(ServerEvent::LostMessage(packet.session_id, self.server_struct.node_id));
            },
            Some(fragments) => {
                match fragments.get(nack.fragment_index as usize) {
                    None => {
                        self.send_event(ServerEvent::LostFragment(packet.session_id, self.server_struct.node_id, nack.fragment_index));
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

        match self.server_struct.packet_send.get(&next_id) {
            Some(sender) => {
                sender.send(packet_ack.clone()).unwrap();
                self.send_event(ServerEvent::PacketSent(packet_ack))
            }
            None => {
                self.send_event(ServerEvent::MissingDestination(next_id))
            }
        }
    }

    // First need to create an error like in drones
    // then adjust all the calls


    fn flood(&mut self) {

        // !!I'm not sure if this is a good idea or not, since they can't crush I don't
        // !!see why we would need to clear it
        // self.contact_list.clear();

        let flood_request = FloodRequest{
            flood_id: self.get_flood_id(),
            initiator_id: self.server_struct.node_id,
            path_trace: vec![(self.server_struct.node_id, NodeType::Client)],
        };
        let packet = Packet::new_flood_request(SourceRoutingHeader::default(), fastrand::u64(..500), flood_request);
        self.server_struct.packet_send.values().for_each(|sender| {
            sender.send(packet.clone()).unwrap()
        });
    }

    fn get_flood_id(&mut self) -> u64 {
        let min = match self.server_struct.flood_ids.iter().min(){
            Some(min) => (*min).0,
            None => {
                let value = fastrand::u64(..30);
                self.server_struct.flood_ids.insert((value, self.server_struct.node_id));
                return value
            }
        };
        let value = fastrand::u64(min..min + 40);
        self.server_struct.flood_ids.insert((value, self.server_struct.node_id));
        value
    }

    fn get_session_id(&mut self) -> u64 {
        let min = match self.server_struct.used_session_id.iter().min(){
            Some(min) => *min,
            None => {
                let value = fastrand::u64(..30);
                self.server_struct.used_session_id.insert(value);
                return value
            }
        };
        let value = fastrand::u64(min..min + 40);
        self.server_struct.used_session_id.insert(value);
        value
    }

    fn get_src_id(&self) -> NodeId {
        self.server_struct.node_id
    }
}

impl NetworkEdgeErrors for ChatServer {
    fn check_type(&mut self, id: NodeId) {
        let req = TypeExchange::TypeRequest { from: self.server_struct.node_id };
        let exc = ContentType::TypeExchange(req);
        let s_id = self.get_session_id();
        self.send_message(Message::new(self.server_struct.node_id, s_id, exc), id);

        if DEBUG_MODE {
            println!("sent check from {}", self.server_struct.node_id);
        }
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        let out =  match self.server_struct.paths.get(&node_id){
            Some(path) => {
                path.0 == 1
            }
            None =>{false}
        };
        if !out {
            if DEBUG_MODE{
                println!("dst state was not ok");}

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
                self.send_event(ServerEvent::MissingDestination(dst));
                return;
            }
            Some((_state, route)) => {
                if let Some(fastest_route) = route.get_fastest_route(){
                    fastest_route.to_source_routing_header()
                }else {
                    self.send_event(ServerEvent::MissingRoute(dst));
                    return;
                }
            }
        };
        let first_hop = shr.next_hop().unwrap_or(self.server_struct.node_id);

        let packet = Packet{
            routing_header: shr,
            session_id: self.get_session_id(),
            pack_type: PacketType::Nack(new_nack),
        };

        match self.server_struct.packet_send.get(&first_hop){
            None => {
                self.send_event(ServerEvent::MissingDestination(dst));
                return;
            }
            Some(sender) => {
                sender.send(packet).unwrap();
            }
        }
    }
}

impl Server for ChatServer {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ChatServer {
            server_struct: ServerStruct::new(node_id, command_recv, event_send, packet_recv, packet_send),
        }
    }

    // I had to comment them because of the M: MessageType I added to network_edge trait, but I don't understand why he complains,
    // in the client one it doesn't complain!
    //only difference is that ChatClient<M: MessageType>..
    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.server_struct.command_recv) -> cmd => {
                    if let Ok(_command) = cmd {
                       // self.handle_command(command);
                    }
                }
                recv(self.server_struct.packet_recv) -> pkt => {
                    if let Ok(_packet) = pkt {
                        //self.handle_packet(packet);
                    }
                }
            }
        }
    }

    fn handle_command(&mut self, command: ServerCommand) {
        match command {
            ServerCommand::RemoveSender(_) => {}
            ServerCommand::AddSender(_, _) => {} //ServerCommand::SendPacket(_packet) => {} // Remove the _ before packet when you'll use it.
        }
    }
    fn get_server_type(&self) -> ServerType {
        ServerType::Chat
    }

    fn send_event(&self, se: ServerEvent) {
        match self.server_struct.event_send.try_send(se){
            Ok(_) => {}
            Err(_err) => {
                if DEBUG_MODE {
                    println!("simulation control unreachable")
                }
            }
        }
    }
}
