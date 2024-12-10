use std::collections::{HashMap, HashSet};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use crossbeam_channel::{select_biased, Receiver, Sender};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::controller::DroneEvent::ControllerShortcut;
use wg_2024::drone::Drone;
use wg_2024::packet::{Packet, PacketType, FloodResponse, NodeType, FloodRequest, NackType};
use crate::error::create_error;
use crate::checks::*;


pub struct SkyLinkDrone {
    id: NodeId,
    controller_send: Sender<DroneEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    pdr: u32,
    flood_ids: HashSet<(u64, NodeId)>, //Tuple with the flood_id and the id of the initiator, to distinguish uniquely every flooding.
    crashing: bool,
}

impl Drone for SkyLinkDrone {
    fn new(id: NodeId,
           controller_send: Sender<DroneEvent>,
           controller_recv: Receiver<DroneCommand>,
           packet_recv: Receiver<Packet>,
           packet_send: HashMap<NodeId, Sender<Packet>>,
           pdr: f32) -> Self {
        let mut pdr = pdr;
        if pdr > 1.00 {
            pdr = 1.00;
        }
        if pdr < 0.00 {
            pdr = 0.00;
        }
        SkyLinkDrone {
            id,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr: (pdr*100.0) as u32,
            flood_ids: HashSet::new(),
            crashing: false,
        }
    }

    fn run(&mut self) {
        loop {
            if !self.crashing {
                select_biased! {
                    recv(self.controller_recv) -> cmd => {
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
            } else {
                select_biased! {
                    recv(self.controller_recv) -> cmd => {
                        // If I'm in crushing behavior, I still listen for RemoveSender command,
                        // to avoid neighbour drones not crushing because of each other existence.
                        if let Ok(command) = cmd {
                            if let DroneCommand::RemoveSender(node_id) = command {
                                if self.packet_send.contains_key(&node_id) {
                                    if let Some(to_be_dropped) = self.packet_send.remove(&node_id) {
                                        drop(to_be_dropped);
                                    }
                                }
                            }
                        }
                    }
                    recv(self.packet_recv) -> pkt => {
                        match pkt {
                            Ok(packet) => {
                                self.crashing_handle_packet(packet);
                            },
                            Err(_error) => {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

impl SkyLinkDrone {
    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
                //println!("Drone {} added a channel to {}!", self.id, node_id);
            },
            DroneCommand::SetPacketDropRate(pdr) => {
                let mut pdr = pdr;
                if pdr > 1.00 {
                    pdr = 1.00;
                }
                if pdr < 0.00 {
                    pdr = 0.00;
                }
                self.pdr = (pdr * 100.0) as u32;
                //println!("Drone {} new pdr: {}%!", self.id, self.pdr);
            },
            DroneCommand::Crash => {
                self.crashing = true;
                //println!("Drone {} crashed!", self.id);
            },
            DroneCommand::RemoveSender(node_id) => {
                if self.packet_send.contains_key(&node_id) {
                    if let Some(to_be_dropped) = self.packet_send.remove(&node_id) {
                        drop(to_be_dropped);
                        //println!("Drone {} no more has a connection to {}!", self.id, node_id);
                    }
                }
            }
        }
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let PacketType::FloodRequest(mut flood_request) = packet.pack_type.clone() {
            //First check if we're dealing with a flood request, since we ignore its SRH.
            flood_request.path_trace.push((self.id, NodeType::Drone));
            //I add myself to the path trace.

            //If I can insert the flooding inside the HashSet, then I never met this flooding.
            if self.flood_ids.insert((flood_request.flood_id.clone(), flood_request.initiator_id.clone())) {
                if self.packet_send.len() == 1 {
                    self.send_flood_response(flood_request);
                } else {
                    let mut prev = flood_request.initiator_id.clone();
                    if flood_request.path_trace.clone().len() > 1 {
                        prev = flood_request.path_trace.get(flood_request.path_trace.len() - 2).unwrap().0;
                    }
                    //I update the path_trace in the packet.
                    packet.pack_type = PacketType::FloodRequest(flood_request);
                    for (key, _) in self.packet_send.iter() {
                        //println!("Previous: {}", prev);
                        //println!("Key: {}", key);
                        if *key != prev {
                            //I send the flooding to everyone except the node I received it from.
                            if let Ok(_) = self.packet_send.get(key).unwrap().send(packet.clone()) {
                                self.controller_send.send(DroneEvent::PacketSent(packet.clone())).unwrap();
                                //If the message was sent, I also notify the sim controller.
                            } //There's no else, since I don't care of nodes which can't be reached.
                        }
                    }
                }
            } else {
                self.send_flood_response(flood_request);
            }
        } else {
            //If the packet is not a flood response.
            match self.apply_checks(packet.clone()) {
                //If every check is passed
                Ok(packet) => {
                    let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];
                    if let Some(sender) = self.packet_send.get(&next_hop) {
                        if let Ok(_) = sender.send(packet.clone()) {
                            self.controller_send.send(DroneEvent::PacketSent(packet)).unwrap();
                            //If the message was sent, I also notify the sim controller.
                            return;
                        }
                    }
                    let err = create_error(self.id, packet, NackType::ErrorInRouting(next_hop));
                    self.send_nack(&err.routing_header.hops[1].clone(), err);
                    //If the message wasn't sent, despite all the checks, I still send an error back.
                },
                //Otherwise the error is already the right one to send.
                Err(err) => {
                    if let PacketType::Nack(nack) = err.pack_type.clone() {
                        if let NackType::UnexpectedRecipient(_) = nack.nack_type {
                            //If my drone isn't the one that should have received the message, I've to
                            //route the message differently, since I'm not the first id in the routing header.
                            self.send_nack(&err.routing_header.hops[0].clone(), err);
                        } else {
                            match packet.pack_type {
                                PacketType::FloodRequest(_) => { unreachable!() },
                                PacketType::MsgFragment(_) => {
                                    self.handle_packet(err);
                                },
                                _ => {
                                    self.controller_send.send(ControllerShortcut(err)).unwrap();
                                    //If I had got an error from the checks of the routing of an
                                    //Ack, Nack or FloodResponse, I just forward it through the Simulation Controller.
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn crashing_handle_packet(&mut self, packet: Packet) {
        match packet.clone().pack_type {
            PacketType::MsgFragment(_fragment) => {
                //If the message is a fragment, I send back a Nack
                let err = create_error(self.id, packet, NackType::ErrorInRouting(self.id));
                self.send_nack(&err.routing_header.hops[1].clone(), err);
            }
            PacketType::FloodRequest(_flood_request) => {}, //I discard them.
            _ => {
                self.handle_packet(packet);
                //If the message is an Ack, Nack or FloodResponse, I route it normally.
            }
        }
    }

    fn send_nack(&self, index: &NodeId, err: Packet) {
        if let Some(sender) = self.packet_send.get(index) {
            sender.send(err.clone()).unwrap();
            self.controller_send.send(DroneEvent::PacketSent(err)).unwrap();
        } else {
            self.controller_send.send(ControllerShortcut(err)).unwrap();
            //If the routing of the nack gives an error, I pass through the Sim Contr.
        }
    }

    fn apply_checks(&self, mut packet: Packet) -> Result<Packet, Packet> {
        //Check if we're on the right hop.
        id_hop_match_check(&self, packet.clone())?;
        //Increase the index.
        packet.routing_header.hop_index += 1;
        //Check if we're a final destination.
        final_destination_check(&self, packet.clone())?;
        //Check if the packet is dropped (only when msg_fragment).
        pdr_check(&self, packet.clone())?;
        //Check if the next_hop exists.
        is_next_hop_check(&self, packet.clone())?;

        //If no check gave an error, we return the starting packet.
        Ok(packet)
    }


    fn send_flood_response(&mut self, flood: FloodRequest) { //take a flood req, generate the response, send it

        let flood_resp = FloodResponse {
            flood_id: flood.flood_id,
            path_trace: flood.path_trace.clone(), //I put a copy of path trace done by the flood
        };

        let mut hops = flood.path_trace
            .iter()
            .rev()
            .map(|(id, _)| *id)
            .collect::<Vec<NodeId>>(); //I take only the ID's from the path trace and reverse them.
        if flood.path_trace[0].0 != flood.initiator_id {
            hops.push(flood.initiator_id);
        }

        let resp = Packet {
            pack_type: PacketType::FloodResponse(flood_resp),
            routing_header: SourceRoutingHeader {
                hop_index: 0,
                hops,
            },
            session_id: flood.flood_id,
        };
        self.handle_packet(resp);
        //self.controller_send.send(DroneEvent::PacketSent(resp)).unwrap(); //Should be set by handle_packet.
    }

    pub fn get_id(&self) -> NodeId {
        self.id
    }
    pub fn get_pdr(&self) -> u32 {
        self.pdr
    }
    pub fn get_packet_send(&self) -> &HashMap<NodeId, Sender<Packet>>{
        &self.packet_send
    }
}
