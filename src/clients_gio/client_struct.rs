use crate::clients_gio::client_command::ClientEvent::{MissingDestination, MissingRoute, WrongDestinationType};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::message::{ContentType, Message, TypeExchange};
use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crate::routing::{Nodes, Route, RouteList};
use crate::DEBUG_MODE;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::PacketType::*;
use wg_2024::packet::{Fragment, Nack, NackType, NodeType, Packet};

//here the common struct of both the clients, important: some functions are left unreachable since will be called ad hoc by each client.
//attention, also all function that call handle packet and handle message are unreachable obv


pub struct ClientStruct {
    pub (crate) node_id: NodeId,
    pub (crate) command_recv: Receiver<ClientCommand>,
    pub (crate) event_send: Sender<ClientEvent>,
    pub (crate) packet_recv: Receiver<Packet>,
    pub (crate) packet_send: HashMap<NodeId, Sender<Packet>>,

    pub (crate) flood_ids: HashSet<(u64, NodeId)>, // Just like drones
    pub (crate) used_session_id: HashSet<u64>,     // Do we need this?
    pub (crate) paths: HashMap<NodeId, (u8, RouteList)>, // These NodeId are servers and clients, the u8 indicate if usable (1), if not usable (2), or if yet to be checked (0)
    pub (crate) nodes: Nodes, // Map of all Nodes, to apply checks on the PDRs.
    pub (crate) fragments: HashMap<(u64, NodeId, NodeId), (Option<ContentType>, Vec<Fragment>)>, //(session_id, source, destination) - (copy of content (for registring ecc..) and frags), if the content is None is because it's yet to be fully arrived!
    pub (crate) unsent_fragments: (u8, HashMap<(u64, NodeId, NodeId), Vec<(Fragment)>>), // The second NodeId is the destination, the u8 is a counter (for now to the maximum I guess) to avoid sending too much stuff.
}

impl NetworkEdge for ClientStruct {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        match message.clone().content{
            ContentType::TypeExchange(_exc) =>{
                let session_id = message.session_id;
                let frags = Self::fragment_message(&message);
                self.fragments.insert((session_id, self.node_id, destination), (Some(message.content), frags.clone()));
                // I also save the fragments in the memory, in case I have to send them again.

                for fragment in frags {
                    self.send_fragment(fragment, destination, session_id);
                    // I apply the send operation on each single fragment.
                }
            },
            ContentType::EdgeNack(_nack) => {
                let session_id = message.session_id;
                let frags = Self::fragment_message(&message);
                self.fragments.insert((session_id, self.node_id, destination), (Some(message.content), frags.clone()));
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
                    self.fragments.insert((session_id, self.node_id, destination), (Some(message.content), frags.clone()));
                    // I also save the fragments in the memory, in case I have to send them again.


                    for fragment in frags {
                        self.send_fragment(fragment, destination, session_id);
                        // I apply the send operation on each single fragment.
                    }
                }
                else {
                    let new_nack = WrongDestinationType(self.get_src_id(), destination);
                    self.send_event(new_nack);
                }
            }
        }
    }

    fn handle_packet(&mut self, _packet: Packet) {
        unreachable!()
    }

    fn handle_message(&mut self, _message: Message) {
        unreachable!()
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        if destination == self.node_id {
            println!("Sending message to yourself with {:?}", destination);
            return;
        }

        match self.paths.get_mut(&destination) {
            None => {
                //I first check if I have any path to the destination
                if DEBUG_MODE {
                    println!("Tried to send fragment without path to {destination} with {}", self.node_id);
                }
                self.send_event(MissingDestination(self.node_id, destination));
                self.add_unsent_fragment(fragment, session_id, destination);
            }
            Some((_state, route_list)) => {
                match route_list.get_fastest_route() {
                    None => {
                        // I then check that we have an available route to the destination.
                        self.send_event(MissingRoute(self.get_src_id(), destination));

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
                                self.send_event(ClientEvent::PacketSent(packet.clone()));

                            }
                            None => {
                                // If I want to pass for a node that I don't have as a neighbour, I need to remove
                                // channels who contain it.
                                self.send_event(MissingRoute(self.get_src_id(), destination));
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
                self.send_event(ClientEvent::LostMessage(packet.session_id, self.node_id));
            },
            Some((_, fragments)) => {
                match fragments.get(nack.fragment_index as usize) {
                    None => {
                        self.send_event(ClientEvent::LostFragment(packet.session_id, self.node_id, nack.fragment_index));
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
                self.send_event(ClientEvent::PacketSent(packet_ack))
            }
            None => {
                self.send_event(MissingDestination(self.node_id, next_id))
            }
        }
    }

    fn flood(&mut self) {
        self.send_event(ClientEvent::Flooding(self.node_id));

        let flood_request = wg_2024::packet::FloodRequest {
            flood_id: self.get_flood_id(),
            initiator_id: self.node_id,
            path_trace: vec![(self.node_id, NodeType::Client)],
        };
        let packet = Packet::new_flood_request(SourceRoutingHeader::default(), self.get_session_id(), flood_request);
        self.packet_send.iter().for_each(|(id, sender)| {
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

    fn get_src_id(&self) -> NodeId {
        self.node_id
    }

    fn remove_sender(&mut self, id: NodeId) {
        if self.packet_send.contains_key(&id) {
            if let Some(to_be_dropped) = self.packet_send.remove(&id) {
                drop(to_be_dropped);
                //println!("Client {} no more has a connection to {}!", self.node_id, node_id);
            }
        }
    }
}

impl NetworkEdgeErrors for ClientStruct {
    fn check_type(&mut self, id: NodeId) {
        let req = TypeExchange::TypeRequest { from: self.node_id };
        let exc = ContentType::TypeExchange(req);
        let s_id = self.get_session_id();
        self.send_message(Message::new(self.node_id, s_id, exc), id);

        if DEBUG_MODE {
            println!("sent check from {}", self.node_id);
        }
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        let out =  match self.paths.get(&node_id){
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
        let shr = match self.paths.get_mut(&dst){
            None => {
                self.send_event(MissingDestination(self.node_id, dst));
                return;
            }
            Some((_state, route)) => {
                if let Some(fastest_route) = route.get_fastest_route(){
                    fastest_route.to_source_routing_header()
                }else {
                    self.send_event(MissingRoute(self.get_src_id(), dst));
                    return;
                }
            }
        };
        let first_hop = shr.next_hop().unwrap_or(self.node_id);

        let packet = Packet{
            routing_header: shr,
            session_id: self.get_session_id(),
            pack_type: Nack(new_nack),
        };

        match self.packet_send.get(&first_hop){
            None => {
                self.send_event(MissingDestination(self.node_id, dst));
                return;
            }
            Some(sender) => {
                sender.send(packet).unwrap();
            }
        }
    }
}

impl ClientTrait for ClientStruct {
    fn new(node_id: NodeId, command_recv: Receiver<ClientCommand>, event_send: Sender<ClientEvent>, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>) -> Self {
        Self { node_id, command_recv, event_send, packet_recv, packet_send, flood_ids: HashSet::default(), used_session_id: HashSet::default(), paths: HashMap::default(), nodes: Nodes::new(), fragments: HashMap::default(), unsent_fragments: (0, HashMap::new()) }
    }

    fn run(&mut self) {
        unreachable!();
    }

    fn handle_command(&mut self, command: ClientCommand) {
        unreachable!()
    }

    fn get_client_type(&self) -> ClientType {
        unreachable!()
    }

   fn send_event(&self, ce: ClientEvent) {
        match self.event_send.try_send(ce.clone()){
            Ok(_) => {}
            Err(_err) => {
                if DEBUG_MODE {
                    println!("{} - simulation control unreachable for {:?}", self.node_id, ce)
                }
            }
        }
   }
}

impl ClientStruct {
    //sta fn la metterei in networkedge
   pub (crate) fn get_optimal_dest (&mut self, v: &Vec<NodeId>) -> Option<NodeId> {
        let mut out: Option<(Route, NodeId)> = None;
        for i in v {
            if let Some((state, routelist)) = self.paths.get_mut(i) {
                if *state == 1{
                    if let Some(route) = routelist.get_fastest_route(){
                        if let Some((best_route, _)) = &out {
                            if route > *best_route {
                                out = Some((route, *i));
                            }
                        } else {
                            // Prima route valida trovata
                            out = Some((route, *i));
                        }
                    }
                }
            }
        }
        out.map(|(_, id)| id)
    }
}
