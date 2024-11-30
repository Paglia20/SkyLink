use std::collections::HashMap;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use crossbeam_channel::{select_biased, Receiver, Sender};
use wg_2024::controller::{DroneCommand, NodeEvent};
use wg_2024::drone::{Drone, DroneOptions};
use wg_2024::packet::{Packet, PacketType, Nack, FloodResponse, NodeType, FloodRequest};
use wg_2024::packet::Nack::{DestinationIsDrone, Dropped, ErrorInRouting, UnexpectedRecipient};
use rand::Rng;

pub struct MyDrone {
    id: NodeId,
    controller_send: Sender<NodeEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    pdr: u32,
    flood_ids: Vec<u64>,
}

impl Drone for MyDrone {
    fn new(options: DroneOptions) -> Self {
        MyDrone {
            id: options.id,
            controller_send: options.controller_send,
            controller_recv: options.controller_recv,
            packet_recv: options.packet_recv,
            //packet_send: options.packet_send,
            packet_send: HashMap::new(),
            pdr: (options.pdr*100.0) as u32,
            flood_ids: Vec::new(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.controller_recv) -> cmd => {
                    if let Ok(command) = cmd {
                        if let DroneCommand::Crash = command {
                            println!("Drone {} has crashed", self.id);
                            break;
                        }
                        self.handle_command(command);
                    }
                }
                recv(self.packet_recv) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
            }
        }
    }
}

impl MyDrone {
    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
            },
            DroneCommand::SetPacketDropRate(pdr) => {
                self.pdr = (pdr*100.0) as u32;
            },
            DroneCommand::Crash => unreachable!(),
        }
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let PacketType::FloodRequest(mut flood_request) = packet.pack_type.clone() {
            //First check if we're dealing with a flood request, since we ignore its SRH.
            let prev = flood_request.path_trace.get(flood_request.path_trace.len() - 1).unwrap().0;
            flood_request.path_trace.push((self.id, NodeType::Drone));

            if self.flood_ids.contains(&flood_request.flood_id) {
                self.send_flood_response(flood_request);
            }
            else {
                if self.packet_send.len() == 1 {
                    self.send_flood_response(flood_request);
                }
                else {
                    for (key, _) in self.packet_send.iter() {
                        if *key != prev{
                            if !self.forward_packet(packet.clone(), key) {
                                // self.send_nack(ErrorInRouting(*key),
                                //                packet.routing_header.hops.clone(),
                                //                packet.session_id
                                // );

                                //problem with mutable borrow in send nack, and immutable in the other parts
                            }
                        }
                    }
                }
            }
        } else {
            //If it's not a flood request, I need to check if I'm the right node.
            if packet.routing_header.hops[packet.routing_header.hop_index] == self.id { //Step 1
                packet.routing_header.hop_index += 1; //Step 2

                //I also need to check that I'm not the final destination of the packet, since I'm a drone.
                if packet.routing_header.hop_index < packet.routing_header.hops.len() { //Step 3
                    //Check if the next hop is one we know.
                    let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
                    if self.packet_send.contains_key(&next_hop) { //Step 4
                        match packet.pack_type.clone() {
                            PacketType::MsgFragment(msg_fragment) => {
                                let mut rng = rand::thread_rng();
                                let random_number: u32 = rng.gen_range(0..101);

                                if random_number > self.pdr {
                                    if !self.forward_packet(packet.clone(), &next_hop) {
                                        self.send_nack(ErrorInRouting(next_hop),
                                                       packet.routing_header.hops.clone(),
                                                       packet.session_id
                                        );
                                    }
                                } else {
                                    self.send_nack(Dropped(msg_fragment.fragment_index),
                                                   packet.routing_header.hops.clone(),
                                                   packet.session_id
                                    );
                                }
                            },
                            PacketType::FloodRequest(_flood_request) => { unreachable!(); },
                            _ => {
                                if !self.forward_packet(packet.clone(), &next_hop){
                                    self.send_nack(ErrorInRouting(next_hop),
                                                   packet.routing_header.hops.clone(),
                                                   packet.session_id
                                    );
                                }
                            }
                        }
                    } else {
                        self.send_nack(ErrorInRouting(next_hop),
                            packet.routing_header.hops.clone(),
                            packet.session_id
                        );
                    }
                } else {
                    self.send_nack(DestinationIsDrone,
                        packet.routing_header.hops.clone(),
                        packet.session_id
                    );
                }
            } else {
                self.send_nack(UnexpectedRecipient(self.id),
                    packet.routing_header.hops.clone(),
                    packet.session_id
                );
            }
        }

    }

    fn forward_packet(&self, packet: Packet, next_hop: &NodeId) -> bool{
        if let Ok(_) = self.packet_send.get(next_hop).unwrap().send(packet.clone()) {
            self.controller_send.send(NodeEvent::PacketSent(packet)).unwrap();
            //document panic?
            true
        } else {
            false
        }
    }

    fn send_nack(&mut self, nack_type: Nack, routing_vector: Vec<NodeId>, session_id: u64) {
        let err = Packet {
            pack_type: PacketType::Nack(nack_type),
            routing_header: SourceRoutingHeader{
                hop_index: 1,
                hops: routing_vector
                    .into_iter()
                    .rev()
                    .collect::<Vec<NodeId>>()
            },
            session_id
        };
        self.handle_packet(err.clone());
        self.controller_send.send(NodeEvent::PacketSent(err)).unwrap();
    }

    fn send_flood_response(&mut self, flood: FloodRequest) { //take a flood req, generate the response, send it

        let flood_resp = FloodResponse{
            flood_id: flood.flood_id,
            path_trace: flood.path_trace.clone(), //I put a copy of path trace done by the flood
        };

        let resp = Packet {
            pack_type: PacketType::FloodResponse(flood_resp),
            routing_header: SourceRoutingHeader{
                hop_index: 1,
                hops: flood.path_trace
                    .iter()
                    .rev()
                    .map(|(id, _)| *id)
                    .collect::<Vec<NodeId>>() //I take only the ID's from the path trace and reverse them.
            },
            session_id : flood.flood_id,
        };
        self.handle_packet(resp.clone());
        self.controller_send.send(NodeEvent::PacketSent(resp)).unwrap();
    }
}