use crate::routing::Network;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, FloodResponse, Fragment, Nack, NackType, NodeType, Packet};
use crate::DEBUG_MODE;

type UnsentFragments = HashMap<(u64, NodeId, NodeId), Vec<Fragment>>;
// The second NodeId is the destination, the u8 is a counter to avoid sending too much stuff.

pub struct ServerStruct {
    pub node_id: NodeId,
    pub command_recv: Receiver<ServerCommand>,
    pub event_send: Sender<ServerEvent>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,
    
    pub flood_ids: HashSet<(u64, NodeId)>, // Used to recognize flooding from other nodes.
    pub network: Network, 
    
    pub fragments: HashMap<(u64, NodeId), (NodeId, Vec<Fragment>)>, // (session_id, source), (destination, Vec<Fragment>)
    pub unsent_fragments: UnsentFragments,

    next_flood_id: u64,
    next_session_id: u64,
    is_flooding: bool,
    flood_counter: u8,

    type_checking: HashSet<NodeId>, // I save the nodes for which I already asked the type, to avoid sending it too many times.
    
    pub is_running: bool,
}

impl ServerStruct {
    pub fn new(
        node_id: NodeId,
        command_recv: Receiver<ServerCommand>,
        event_send: Sender<ServerEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ServerStruct {
            node_id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            flood_ids: HashSet::new(),
            network: Network::new(),
            fragments: HashMap::new(),
            unsent_fragments: HashMap::new(),
            next_flood_id: 0,
            next_session_id: 0,
            is_flooding: false,
            flood_counter: 0,
            type_checking: HashSet::new(),
            is_running: true,
        }
    }

    pub fn handle_flood_request(&mut self, flood_request: FloodRequest, session_id: u64) -> bool{

        // I try to insert the new flood in the already known ones.
        if self.flood_ids.insert((flood_request.flood_id,flood_request.initiator_id)) {

            if self.packet_send.len() == 1 {
                // I have to send the flood_response back.
                return true;
            } else {
                let mut prev = flood_request.initiator_id;
                if flood_request.path_trace.clone().len() > 1 {
                    prev = flood_request
                        .path_trace
                        .get(flood_request.path_trace.len() - 2)
                        .unwrap()
                        .0;
                }
                
                let packet = Packet::new_flood_request(SourceRoutingHeader::empty_route(), session_id, flood_request);
                
                //I update the path_trace in the packet.
                for (key, _) in self.packet_send.iter() {
                    if *key != prev {
                        //I send the flooding to everyone except the node I received it from.
                        if let Some(sender) = self.packet_send.get(key) {
                            if sender.send(packet.clone()).is_ok() {
                                self.send_event(ServerEvent::PacketSent(packet.clone()));
                                //If the message was sent, I also notify the sim controller.
                            }
                            //There's no else, since I don't care about nodes which can't be reached.
                        }
                    }
                }
            }
            false
        } else {
            // I have to send the flood_response back.
            true
        }
    }

    pub fn send_event(&self, se: ServerEvent) {
        match self.event_send.try_send(se){
            Ok(_) => {}
            Err(_err) => {
                if DEBUG_MODE {
                    println!("simulation control unreachable")
                }
            }
        }
    }
    
    pub fn handle_fragment(&mut self, fragment: Fragment, packet: Packet){
        let session_id = packet.session_id;
        let initiator_id = packet.routing_header.hops[0];
        let destination = self.node_id; // We know it is the destination.

        // Add new fragment.
        match self.fragments.get_mut(&(session_id, initiator_id)) {
            Some((_,fragment_vec)) => {
                // If it already exists, we push the fragment in it.
                fragment_vec.push(fragment);
            },
            None => {
                // Otherwise we try to create the vector.
                self.fragments.insert((session_id, initiator_id), (destination, vec![fragment]));
            }
        }
        
        // Notify SC that I got a packet
        self.send_event(ServerEvent::PacketReceived(packet.clone()));
    }
    
    pub fn handle_nack(&mut self, nack: Nack, packet: Packet) -> bool{
        self.send_event(ServerEvent::NackReceived(packet.clone()));
        match nack.nack_type {
            NackType::UnexpectedRecipient(wrong_node) => {
                // UnexpectedRecipient means that the hops vector in the message was faulty.
                // I remove all the routes with that destination, since they're probably result of a faulty flooding.
                self.network.remove_node(wrong_node);
                // I send the fragments again.
                true
            },
            NackType::ErrorInRouting(wrong_node) => {
                // I again remove the routes containing the (probably) crushed drone.
                self.network.remove_node(wrong_node);
                // I send the fragments again.
                true
            },
            NackType::DestinationIsDrone => {
                let wrong_node = *packet.routing_header.hops.last().unwrap();
                
                // Since the destination was a drone, the message was faulty,
                // so I remove the destination and consider the message as lost.
                self.network.remove_node(wrong_node);
                self.send_event(ServerEvent::DroneInsideDestination(self.node_id, wrong_node));
                
                // I remove the message since it's faulty.
                self.fragments.remove(&(packet.session_id, self.node_id));
                self.unsent_fragments.remove(&(packet.session_id, self.node_id, packet.routing_header.destination().unwrap()));
                self.send_event(ServerEvent::LostMessage(packet.session_id, self.node_id, "Destination of the message was a drone".to_string()));
                
                // I don't need to resend the fragments, since I'll never have a destination.
                false
            },
            NackType::Dropped => {
                // Who dropped will be source of the NACK, so I apply a negative feedback on it to try to compute a sort of PDR.
                let dropper = packet.routing_header.source().unwrap();
                self.network.negative_feedback(dropper);

                // I just send it again.
                true
            }
        }
    }
    
    pub fn save_flood_response(&mut self, flood_response: FloodResponse) -> bool{
        let mut res = false;
        // I'll ask for the TypeExchange only if it's a client or a server, and I don't know the state yet.
        match flood_response.path_trace.last().unwrap().1 {
            NodeType::Drone => {},
            _ => {
                if self.type_checking.insert(flood_response.path_trace.last().unwrap().0) {
                    res = true;
                }
            }
        }

        self.network.add_route(self.node_id, flood_response.path_trace);

        // I try to check if I have all routes to the nodes I already know, so that if I'm not done;
        // I also have a counter, in case I loose all connection to a node, so that I won't stop to flood forever in that case.
        if self.network.has_all_routes(self.node_id) || self.flood_counter >= 200 {
            // After this is set to false, I can flood again
            self.is_flooding = false;
            self.flood_counter = 0;
        } else {
            self.flood_counter += 1;
        }

        res
    }
    
    pub fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        // If the sending of a fragment gave an error, we put it in a hashmap to try sending it again.
        match self.unsent_fragments.get_mut(&(session_id, self.node_id, destination)) {
            None => {
                let vec = vec![fragment];
                self.unsent_fragments.insert((session_id, self.node_id, destination), vec);
            }
            Some(fragments) => {
                fragments.push(fragment);
            },
        }
    }
    
    pub fn send_to_all(&mut self, packet: Packet) {
        self.packet_send.values().for_each(|sender| {
            // I don't care if the message isn't sent, since if it won't result in the flooding_responses that connection won't be used anyway.
            let _ = sender.send(packet.clone());
        });
    }

    pub fn get_flood_id(&mut self) -> u64 {
        self.next_flood_id += 1;
        self.next_flood_id-1
    }

    pub fn get_session_id(&mut self) -> u64 {
        self.next_session_id += 1;
        self.next_session_id-1
    }
    
    pub fn get_fragments_hm(&mut self) -> &mut HashMap<(u64, NodeId), (NodeId, Vec<Fragment>)> {
        &mut self.fragments
    }

    pub fn get_fragment_to_process(&self) -> Vec<(Fragment, (u64, NodeId, NodeId))> {
        let mut fragment_to_process = Vec::new();
        // I create a vector from the fragments I still need to process due to errors.
        let _ =self.unsent_fragments
            .iter()
            .map(|(identifier, content)| content.iter()
                .map(|fragment| fragment_to_process.push((fragment.clone(), *identifier)))
                .collect::<Vec<_>>()
            )
            .collect::<Vec<_>>();
        fragment_to_process
    }
    
    pub fn reset_unsent_fragments(&mut self) {
        self.unsent_fragments = HashMap::new();
    }
    pub fn check_to_resend_fragments(&mut self) -> bool {
        !self.unsent_fragments.is_empty()
    }
    pub fn can_flood(&mut self) -> bool {
        !self.is_flooding
    }
    
    pub fn starting_to_flood(&mut self) {
        self.is_flooding = true;
    }

    pub fn add_destination_without_path(&mut self, destination: NodeId) {
        self.network.add_destination_without_path(destination);
    }

    pub fn can_type_check(&mut self, dst: NodeId) -> bool {
        self.type_checking.insert(dst)
    }

    pub fn type_checked(&mut self, src: NodeId) {
        self.type_checking.remove(&src);
    }
}
