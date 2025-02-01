use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::server::server_type::ServerType;
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::HashMap;
use wg_2024::controller::DroneEvent;
use wg_2024::network::*;
use wg_2024::packet::{FloodRequest, FloodResponse, Fragment, Nack, NackType, NodeType, Packet, PacketType};
use crate::DEBUG_MODE;
use crate::message::{ContentType, EdgeNackType, Message, TypeExchange};

pub trait Server: NetworkEdge + NetworkEdgeErrors {
    fn new(
        id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        files: Vec<String> // Left empty in chat servers.
    ) -> Self;

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.get_command_recv()) -> cmd => {
                    if let Ok(command) = cmd {
                       self.handle_command(command);
                    }
                }
                recv(self.get_packet_recv()) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
            }
        }
    }

    fn server_send_message(&mut self, message: Message, destination: NodeId) {
        match message.clone().content{
            ContentType::TypeExchange(_exc) =>{
                self.server_send_fragments(message, destination, self.get_src_id());
            },
            ContentType::EdgeNack(_nack) => {
                self.server_send_fragments(message, destination, self.get_src_id());
            }
            _=>{
                if self.is_state_ok(destination) {
                    self.server_send_fragments(message, destination, self.get_src_id());
                }
                else {
                    let new_nack = ServerEvent::WrongDestinationType(self.get_src_id(), destination);
                    self.send_event(new_nack);
                }
            }
        }
    }
    
    fn server_is_state_ok(&self, destination: NodeId) -> bool {
        let out =  match self.get_node_state(destination) {
            Some(state) => {
                state == 1
            }
            None =>{false}
        };
        if !out && DEBUG_MODE{
            println!("dst state was not ok");
            // send nack?
        }
        out
    }

    fn server_send_fragments(&mut self, message: Message, destination: NodeId, source: NodeId) {
        let session_id = message.session_id;
        let frags = Self::fragment_message(&message);
        self.get_fragments_hm().insert((session_id, source), (destination, frags.clone()));
        // I also save the fragments in the memory, in case I have to send them again.

        for fragment in frags {
            self.send_fragment(fragment, destination, session_id);
            // I apply the send operation on each single fragment.
        }
    }
    
    fn route_packet(&mut self, mut packet: Packet) {
        // If we're not the destination of a packet, we act like a drone wit 0 PDR.
        packet.routing_header.hop_index += 1;
        let next_id = packet.routing_header.hops.get(packet.routing_header.hop_index).unwrap();
        // I obtain the id for the next hop.

        match self.get_packet_sender(next_id) {
            None => {
                // In case I don't have the neighbour, I send a Nack back.
                self.send_event(ServerEvent::MissingRoute(self.get_src_id(), *next_id));
                self.send_drone_nack(packet.routing_header.source().unwrap(), NackType::ErrorInRouting(*next_id));
            }
            Some(sender) => {
                match sender.try_send(packet.clone()) {
                    Err(_) => {
                        // We send back the same errors a drone would.
                        self.send_event(ServerEvent::PacketSendingError(packet.clone()));
                        self.send_drone_nack(packet.routing_header.source().unwrap(), NackType::ErrorInRouting(*next_id));
                    }
                    Ok(_) => {
                        self.send_event(ServerEvent::PacketSent(packet.clone()));
                        // If the message was sent, I also notify the sim controller.
                    }
                }
            }
        }
    }
    

    fn server_handle_packet(&mut self, packet: Packet) {
        if let PacketType::FloodRequest(flood_request) = packet.pack_type.clone(){
            if self.handle_flood_request(flood_request.clone(), packet) {
                self.edge_send_flood_response(flood_request);
            }
        } else if packet.routing_header.destination().unwrap() != self.get_src_id() {
            // If it's not his packet, but he has to act as a drone (that never misses)
            self.route_packet(packet);
        } else {
            self.server_handle_packet_to_self(packet, self.get_src_id());
        }
    }
    
    fn server_handle_packet_to_self(&mut self, packet: Packet, self_id: NodeId) {
        // We can take for granted it is the destination
        match packet.pack_type.clone() {
            PacketType::MsgFragment(fragment) => {
                let frag_index = fragment.fragment_index;
                let tot_num_frag = fragment.total_n_fragments as usize;
                let session_id = packet.session_id;
                let initiator_id = packet.routing_header.hops[0];

                self.handle_fragment(fragment, packet.clone());

                // For each arrived frag, send back an ack
                self.send_ack(packet.clone(), frag_index);

                // If all the frag have arrived recreate message
                match self.get_fragments_hm().get(&(packet.session_id, initiator_id)) {
                    Some((_,frags_clone)) => {
                        if frags_clone.len() == tot_num_frag {
                            match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                                Ok(message) => {
                                    // If the message is created correctly, we handle it.
                                    self.handle_message(message);
                                }
                                Err(e) => {
                                    // If the message can't be created, we can't recover it, so we notify the SC.
                                    self.send_event(ServerEvent::LostMessage(session_id, initiator_id, e));
                                    // This should never happen since all the appropriated checks are done previously.
                                }
                            };

                            // We remove the entry from the HashMap.
                            self.get_fragments_hm().remove(&(packet.session_id, initiator_id));
                        }
                    },
                    None => {
                        self.send_event(ServerEvent::LostMessage(session_id, initiator_id, "Couldn't find message while receiving fragments".to_string()))
                    }
                }
            }
            PacketType::Ack(ack) => {
                self.send_event(ServerEvent::AckReceived(packet.clone()));

                // The ACK will have our ID as source, and we 'recognize' the origin from the session_id
                match self.get_fragments_hm().get_mut(&(packet.session_id, self_id)) {
                    None => {
                        // In the case we receive an ACK that's not for one of our fragments, we notify the SC and discard it.
                        self.send_event(ServerEvent::WrongDestination(self.get_src_id(), packet));
                    }
                    Some((_source, vec)) => {
                        // I retain all the fragments with fragment index different from the ACK one.
                        vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                        // If it's empty I retained all fragments because I received all the Ack, hence I can remove my entry from hashmap
                        if vec.is_empty() {
                            self.get_fragments_hm().remove_entry(&(packet.session_id, self_id));
                        }

                        // I apply the positive feed on all nodes in the path
                        let nodes = packet.routing_header.hops;
                        self.positive_feed(nodes)
                    }
                }
            }
            PacketType::Nack(nack) => {
                if self.handle_nack(nack.clone(), packet.clone()) {
                    self.send_fragment_after_nack(packet, nack);
                }
            }
            PacketType::FloodRequest(_) => {
                unreachable!() // We already managed them earlier.
            }
            PacketType::FloodResponse(flood_resp) => {
                self.save_flood_response(flood_resp);
            }
        }
    }
    
    fn server_send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        match self.get_srh(destination) {
            None => {
                // I first check if I have any path to the destination
                self.send_event(ServerEvent::MissingDestination(self.get_src_id(), destination));
                self.flood(); // Since I miss the destination, I start a flooding.
                self.add_unsent_fragment(fragment, session_id, destination);
            }
            Some(srh) => {
                let first_dst = srh.hops[1];
                let packet = Packet::new_fragment(srh, session_id, fragment.clone());

                // If everything worked, I try to send.
                match self.get_packet_sender(&first_dst) {
                    Some(sender) => {
                        match sender.send(packet.clone()) {
                            Ok(_) => {
                                self.send_event(ServerEvent::PacketSent(packet.clone()));
                            },
                            Err(_e) => {
                                // If the send fails, probably my neighbour isn't my neighbour anymore.
                                self.send_event(ServerEvent::MissingRoute(self.get_src_id(), destination));
                                self.add_unsent_fragment(fragment, session_id, destination);
                                self.remove_faulty_connection(first_dst);
                            }
                        }
                    }
                    None => {
                        // If I want to pass for a node that I don't have as a neighbour, I need to remove
                        // channels who contain it.
                        self.send_event(ServerEvent::MissingRoute(self.get_src_id(), destination));
                        self.add_unsent_fragment(fragment, session_id, destination);
                        self.remove_faulty_connection(first_dst);
                    }
                }
            },
        };
    }
    
    fn server_send_fragment_after_nack(&mut self, packet: Packet, nack: Nack, self_id: NodeId) {
        let mut tmp_frg = None;
        let mut event = None;
        match self.get_fragments_hm().get(&(packet.session_id, self_id)) {
            // I try to find again the fragment, and notify the sim controller if I don't have it anymore.
            None => {
                let err=  String::from("Failed to find message again after NACK");
                event = Some(ServerEvent::LostMessage(packet.session_id, self_id, err));
            },
            Some((_,fragments)) => {
                match fragments.get(nack.fragment_index as usize) {
                    None => {
                        event = Some(ServerEvent::LostFragment(packet.session_id, self_id, nack.fragment_index));
                    },
                    // If I manage to find the fragment, I send it
                    Some(fragment) => {
                        tmp_frg = Some(fragment.clone());
                    }
                }
            }
        }
        if let Some(fragment) = tmp_frg {
            self.send_fragment(fragment.clone(), *packet.routing_header.hops.first().unwrap(), packet.session_id);
        }
        if let Some(event) = event {
            self.send_event(event);
        }
    }
    
    fn server_send_ack(&mut self, packet: Packet, fragment_index: u64) {
        let new_hops = packet.routing_header.hops.clone();
        let next_id = new_hops[1];
        let srh = SourceRoutingHeader::new(new_hops, 1); //is it 1 right?
        let packet_ack = Packet::new_ack(srh, packet.session_id, fragment_index);
    
        match self.get_packet_sender(&next_id) {
            Some(sender) => {
                self.send_ack_and_nack(sender.clone(), packet_ack);
            }
            None => {
                self.send_event(ServerEvent::MissingDestination(self.get_src_id(), next_id));
            }
        }
    }
    
    fn start_flood(&mut self) {
        // I use a counter to avoid flooding the network too often.
        if self.can_flood() {
            let flood_request = FloodRequest{
                flood_id: self.get_flood_id(),
                initiator_id: self.get_src_id(),
                path_trace: vec![(self.get_src_id(), NodeType::Server)],
            };
            let packet = Packet::new_flood_request(SourceRoutingHeader::default(), self.get_session_id(), flood_request);
            self.send_to_all(packet);
        }
    }
    
    fn server_check_type(&mut self, id: NodeId) {
        let req = TypeExchange::TypeRequest { from: self.get_src_id() };
        let exc = ContentType::TypeExchange(req);
        let s_id = self.get_session_id();
        self.send_message(Message::new(self.get_src_id(), s_id, exc), id);

        if DEBUG_MODE {
            println!("sent check from {}", self.get_src_id());
        }
    }
    
    fn server_send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        let new_nack = Nack{
            fragment_index: 0,
            nack_type: nack,
        };
        match self.get_srh(dst) {
            None => {
                self.send_event(ServerEvent::MissingDestination(self.get_src_id(), dst));
            }
            Some(srh) => {
                let first_hop = srh.next_hop().unwrap_or(self.get_src_id());

                let packet = Packet{
                    routing_header: srh,
                    session_id: self.get_session_id(),
                    pack_type: PacketType::Nack(new_nack),
                };

                match self.get_packet_sender(&first_hop) {
                    None => {
                        self.send_event(ServerEvent::MissingRoute(self.get_src_id(), dst));
                    }
                    Some(sender) => {
                        self.send_ack_and_nack(sender.clone(), packet);
                    }
                }
            }
        };
        
    }
    
    fn handle_edge_nack(&mut self, nack: EdgeNackType, source_id: NodeId, session_id: u64) {
        match nack {
            EdgeNackType::UnexpectedMessage => {
                // Means that it sent a msg to a dst with a wrong state
                self.update_node_state(source_id, 2);
                // Since the destination was wrong, the message is discarded.
                self.send_event(ServerEvent::DiscardedMessage(self.get_src_id(), session_id));

                if DEBUG_MODE {
                    println!("Client {} discarded message to {} after receiving his nack, because state was not good", self.get_src_id(), source_id)
                }
            }
        }
    }
    fn send_ack_and_nack(&mut self, sender: Sender<Packet>, packet: Packet) {
        match sender.send(packet.clone()) {
            Ok(_) => {
                self.send_event(ServerEvent::PacketSent(packet));
            },
            Err(_e) => {
                self.send_event(ServerEvent::ControllerShortcut(DroneEvent::ControllerShortcut(packet)));
            }
        }
    }
    fn remove_faulty_connection(&mut self, node: NodeId);
    fn handle_command(&mut self, command: ServerCommand);
    fn send_event(&self, event: ServerEvent);
    fn handle_fragment(&mut self, fragment: Fragment, packet: Packet);
    fn handle_flood_request(&mut self, request: FloodRequest, packet: Packet) -> bool;
    fn handle_nack(&mut self, nack: Nack, packet: Packet) -> bool;
    fn positive_feed(&mut self, nodes: Vec<NodeId>);
    fn save_flood_response(&mut self, flood_resp: FloodResponse);
    fn can_flood(&mut self) -> bool;
    fn send_to_all(&mut self, packet: Packet);
    fn update_node_state(&mut self, source_id: NodeId, value: u8);
    fn get_command_recv(&self) -> Receiver<ServerCommand>;
    fn get_packet_recv(&self) -> Receiver<Packet>;
    fn get_fragments_hm(&mut self) -> &mut HashMap<(u64, NodeId), (NodeId, Vec<Fragment>)>;
    fn get_path_to(&self, destination: NodeId) -> Option<(Vec<NodeId>, f64)>;
    fn get_packet_sender(&self, next_id: &NodeId) -> Option<&Sender<Packet>>;
    fn get_srh(&self, destination: NodeId) -> Option<SourceRoutingHeader>;
    fn get_node_state(&self, destination: NodeId) -> Option<u8>;
    fn get_server_type(&self) -> ServerType;
}
