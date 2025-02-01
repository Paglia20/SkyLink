use crate::routing::Network;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{FloodRequest, FloodResponse, Fragment, Nack, NackType, NodeType, Packet};
use crate::DEBUG_MODE;

pub struct ServerStruct {
    pub node_id: NodeId,
    pub command_recv: Receiver<ServerCommand>,
    pub event_send: Sender<ServerEvent>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,
    pub flood_ids: HashSet<(u64, NodeId)>, // Used to recognize flooding from other nodes.

    pub network: Network, 
    
    pub fragments: HashMap<(u64, NodeId), (NodeId, Vec<Fragment>)>, // (session_id, source), (destination, Vec<Fragment>)
    pub unsent_fragments: (u8, UnsentFragments),
    // The second NodeId is the destination, the u8 is a counter (for now to the maximum I guess) to avoid sending too much stuff.

    next_flood_id: u64,
    next_session_id: u64,
    pub flood_counter: u8, // Counter used to avoid flooding too often.
}

type UnsentFragments = HashMap<(u64, NodeId, NodeId), Vec<Fragment>>;

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
            unsent_fragments: (0, HashMap::new()),
            next_flood_id: 0,
            next_session_id: 0,
            flood_counter: 0,
        }
    }

    pub fn handle_flood_request(&mut self, mut flood_request: FloodRequest, packet: Packet) -> bool{
        flood_request
            .path_trace
            .push((self.node_id, NodeType::Server));
        // I first add myself to the path_trace.

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
                //I update the path_trace in the packet.
                for (key, _) in self.packet_send.iter() {
                    if *key != prev {
                        //I send the flooding to everyone except the node I received it from.
                        if self.packet_send.get(key).unwrap().send(packet.clone()).is_ok() {
                            self.send_event(ServerEvent::PacketSent(packet.clone()));
                            //If the message was sent, I also notify the sim controller.
                        }
                        //There's no else, since I don't care of nodes which can't be reached.
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
                true
            },
            NackType::ErrorInRouting(wrong_node) => {
                // I again remove the routes containing the (probably) crushed drone.
                self.network.remove_node(wrong_node);
                true
            },
            NackType::DestinationIsDrone => {
                let wrong_node = *packet.routing_header.hops.last().unwrap();
                
                // Since the destination was a drone, the message was faulty,
                // so I remove the destination and consider the message as lost.
                self.network.remove_node(wrong_node);
                false
            },
            NackType::Dropped => {
                // Who dropped will be source of the NACK
                let dropper = packet.routing_header.source().unwrap();
                self.network.negative_feedback(dropper);

                // I just send it again
                true
            }
        }
    }
    
    pub fn save_flood_response(&mut self, flood_response: FloodResponse) {
        self.network.add_route(self.node_id, flood_response.path_trace.clone());
    }
    
    pub fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        // If the sending of a fragment gave an error, we put it in a hashmap to try sending it again.
        match self.unsent_fragments.1.get_mut(&(session_id, self.node_id, destination)) {
            None => {
                let vec = vec![fragment];
                self.unsent_fragments.1.insert((session_id, self.node_id, destination), vec);
            }
            Some(fragments) => {
                fragments.push(fragment);
            },
        }
    }
    
    pub fn can_flood(&mut self) -> bool {
        if self.flood_counter == 0 {
            self.flood_counter += 1;
            return true;
        } else if self.flood_counter == 10 {
            self.flood_counter = 0;
        }
        false
    }
    
    pub fn send_to_all(&mut self, packet: Packet) {
        self.packet_send.values().for_each(|sender| {
            sender.send(packet.clone()).unwrap()
        });
    }

    pub fn get_flood_id(&mut self) -> u64 {
        let res = self.next_flood_id;
        self.next_flood_id += 1;
        res
    }

    pub fn get_session_id(&mut self) -> u64 {
        let res = self.next_session_id;
        self.next_session_id += 1;
        res
    }
    
    pub fn get_fragments_hm(&mut self) -> &mut HashMap<(u64, NodeId), (NodeId, Vec<Fragment>)> {
        &mut self.fragments
    }
}
