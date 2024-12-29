use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::Client;
use crate::clients_gio::client_type::ClientType;
use crate::message::{ChatResponse, ContentType, Message};
use crate::network_edge::NetworkEdge;
use crate::routing::{Route, RouteList};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, NodeType, Packet, PacketType};
use crate::message::TextRequest::*;

pub struct ChatClient {
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, // Just like drones
    used_session_id: HashSet<u64>,     // Do we need this?

    paths: HashMap<NodeId, RouteList>, // These NodeId are just server_chat nodes.
    contact_list: HashMap<NodeId, Vec<NodeId>>, // First NodeId is the client we want to communicate with, the second one is the server he has to write to, this two hash might be merged in future
    fragments: HashMap<(u64, NodeId), Vec<Fragment>>,
    arrived_messages: HashMap<NodeId, Vec<Vec<u8>>>,
    unsent_fragments: (u8, HashMap<(u64, NodeId), Vec<(Fragment)>>),
    // The NodeId is the destination, the u8 is a counter (for now to the maximum I guess) to avoid sending too much stuff.
}

impl NetworkEdge for ChatClient {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        let session_id = message.session_id;
        let frags = Self::fragment_message(&message);
        self.fragments.insert((session_id, self.node_id), frags.clone());
        // I also save the fragments in the memory, in case I have to send them again.

        for fragment in frags {
            self.send_fragment(fragment, destination, session_id);
            // I apply the send operation on each single fragment.
        }
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if *packet.routing_header.hops.last().unwrap() != self.node_id {
            // If it's not his packet, but he has to act as a drone (that never misses)
            packet.routing_header.hop_index += 1;
            let next_id = packet.routing_header.hops[packet.routing_header.hop_index];

            match self.packet_send.get(&next_id) {
                None => {
                    // !!You need to send back the same errors a drone would
                    /*no more a destination!*/
                }
                Some(sender) => {
                    match sender.try_send(packet.clone()) {
                        Err(_) => {
                            // !!You need to send back the same errors a drone would
                            /*no more a destination!*/
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
                    //add new frag
                    if !self.fragments.contains_key(&(packet.session_id, packet.routing_header.hops[0])) {
                        self.fragments.insert((session_id, initiator_id), vec![fragment]);
                    } else {
                        self.fragments.get_mut(&(session_id, initiator_id)).unwrap().push(fragment);
                    }

                    // If all the frag have arrived recreate message
                    let frags_clone = self.fragments.get(&(packet.session_id, packet.routing_header.hops[0])).unwrap();
                    if frags_clone.len() == tot_num_frag {
                        let message = match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                            Ok(mess) => { mess }
                            Err(_e) => {
                                unimplemented!() //
                            }
                        };
                        //handle message
                        self.handle_message(message);
                    }
                }
                PacketType::Ack(_ack) => {
                    // !!I moved it here, because I need them for the Nack until we receive the Ack
                    // Empty the HashMap
                    self.fragments.remove(&(packet.session_id, packet.routing_header.hops[0]));
                    // !!But this is still to be implemented

                    // !!I have also implemented positive feedback for routes, that'll need to be
                    // !!applied after the Ack, similar to how the negative one is applied in dropped,
                    // !!but this time with every node of the route.
                }

                PacketType::Nack(nack) => {
                    match nack.nack_type.clone() {
                        NackType::UnexpectedRecipient(wrong_node) => {
                            // I remove all the routes with that destination, since it's probably faulty
                            for (_, route) in self.paths.iter_mut() {
                                route.remove_faulty_node(wrong_node);
                            }
                            self.send_fragment_after_nack(packet, nack);
                        },
                        NackType::ErrorInRouting(wrong_node) => {
                            // I again remove the routes containing the (probably) crushed drone
                            for (_, route) in self.paths.iter_mut() {
                                route.remove_faulty_node(wrong_node);
                            }
                            self.send_fragment_after_nack(packet, nack);
                        },
                        NackType::DestinationIsDrone => {
                            let wrong_node = packet.routing_header.hops.last().unwrap();
                            for (_, route) in self.paths.iter_mut() {
                                route.remove_faulty_node(*wrong_node);
                            }
                            // Since the destination was a drone, the message was faulty,
                            // so I remove the destination and consider the message as lost.
                            self.paths.remove(wrong_node);
                        },
                        NackType::Dropped => {
                            // I just send it again
                            self.send_fragment_after_nack(packet, nack);
                            //self.paths.iter_mut().map(|(x,y)| y.negative_feed()).collect();
                            // Still WIP because for some fucking reason Dropped doesn't tell by which drone.
                        }
                    }
                }
                PacketType::FloodRequest(mut flood_request) => {
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
                }
                PacketType::FloodResponse(flood_resp) => {
                    //as of rn it "saves" all possible servers... we want something else I think...
                    let mut current_path = Vec::new();
                    for (node_id, node_type) in flood_resp.path_trace {
                        current_path.push(node_id);

                        if node_type == NodeType::Server {
                            if !self.paths.contains_key(&node_id) {
                                //if it's first time this server gets seen
                                self.paths.insert(node_id.clone(), RouteList::new());
                            }
                            // Clone the current path for the server and insert it into the route list
                            match self.paths.get_mut(&node_id) {
                                None => {
                                    unreachable!()
                                    //i hope it's unreachable
                                }
                                Some(rl) => {
                                    rl.add_route(Route::new(current_path.clone()));
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
                        self.contact_list.insert(message.source_id, list);
                    }
                    ChatResponse::MessageFrom { from, message } => {
                        self.arrived_messages.entry(from).or_insert(Vec::new()).push(message);
                    }
                    ChatResponse::MessageSent => {
                        //not sure, is just an ack?
                    }
                }
            }
            _ => {
                // Gio: no point in getting other types of req
                // !!Leo: We still need to tell that it was an error tho, probably by
                // !!sending a Nack wrong recipient
            }
        }
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        match self.paths.get_mut(&destination) {
            None => {
                //I first check if I have any path to the destination
                self.event_send
                    .send(ClientEvent::MissingDestination(destination))
                    .unwrap();
                self.add_unsent_fragment(fragment, session_id, destination);
            }
            Some(route_list) => {
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
                        let first_dst = srh.hops[0];
                        let packet = Packet::new_fragment(srh, session_id, fragment.clone());

                        // If everything worked, I try to send.
                        match self.packet_send.get(&first_dst) {
                            Some(sender) => {
                                sender.send(packet.clone()).unwrap();
                                self.event_send
                                    .send(ClientEvent::PacketSent(packet))
                                    .unwrap();
                            }
                            None => {
                                // If I want to pass for a node that I don't have as a neighbour, I need to remove
                                // channels who contain it.
                                self.event_send
                                    .send(ClientEvent::MissingRoute(destination))
                                    .unwrap();
                                self.add_unsent_fragment(fragment, session_id, destination);
                                for (_, route) in self.paths.iter_mut() {
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
        match self.unsent_fragments.1.get_mut(&(session_id, destination)) {
            Some(fragments) => {
                fragments.push(fragment);
            },
            None => {
                let mut vec = Vec::new();
                vec.push(fragment);
                self.unsent_fragments.1.insert((session_id, destination), vec);
            }
        }
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        match self.fragments.get(&(packet.session_id, self.node_id)) {
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
            }
            // I check a counter, so that I don't try to send all the fragments every loop.
            if self.unsent_fragments.0 >= 255 {
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
                    self.send_fragment(fragment.clone(), identifier.1, identifier.0);
                }
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
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::ChatClient
    }
}
