use std::collections::{HashMap, HashSet};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use crossbeam_channel::{select, select_biased, Receiver, Sender};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::controller::DroneEvent::ControllerShortcut;
use wg_2024::drone::Drone;
use wg_2024::packet::{Packet, PacketType, FloodResponse, NodeType, FloodRequest, NackType};

pub struct SkyLinkDrone {
    id: NodeId,
    controller_send: Sender<DroneEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    pdr: u32,
    flood_ids: HashSet<u64>,
    crashing: bool,
}

impl Drone for SkyLinkDrone {
    fn new(id: NodeId,
           controller_send: Sender<DroneEvent>,
           controller_recv: Receiver<DroneCommand>,
           packet_recv: Receiver<Packet>,
           packet_send: HashMap<NodeId, Sender<Packet>>,
           pdr: f32) -> Self {
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
                break;
                select! {
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
            },
            DroneCommand::SetPacketDropRate(pdr) => {
                self.pdr = (pdr*100.0) as u32;
            },
            DroneCommand::Crash => {
                self.crashing = true;
                println!("Crashed!");
            },
            DroneCommand::RemoveSender(node_id) => {
                if self.packet_send.contains_key(&node_id){
                    self.packet_send.remove(&node_id);
                }
            }
        }
    }

    fn handle_packet(&mut self, packet: Packet) {
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
                            if let Ok(_) = self.packet_send.get(key).unwrap().send(packet.clone()) {
                                self.controller_send.send(DroneEvent::PacketSent(packet.clone())).unwrap();
                                //If the message was sent, I also notify the sim controller.
                            }//There's no else, since I don't care of nodes which can't be reached.
                        }
                    }
                }
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
                    let err = error::create_error(self.id, packet, NackType::ErrorInRouting(next_hop));
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
                                PacketType::FloodRequest(_) => {unreachable!()},
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
            PacketType::MsgFragment(_fragment) =>{
                //If the message is a fragment, I send back a Nack
                let err = error::create_error(self.id, packet, NackType::ErrorInRouting(self.id));
                self.send_nack(&err.routing_header.hops[1].clone(), err);
            }
            PacketType::FloodRequest(_flood_request) => {}, //I discard them.
            _ => {
                self.handle_packet(packet);
                //If the message is an Ack, Nack or FloodResponse, I route it normally.
            }
        }
    }

    fn send_nack (&self, index: &NodeId, err: Packet) {
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
        check_packet::id_hop_match_check(&self, packet.clone())?;
        //Increase the index.
        packet.routing_header.hop_index += 1;
        //Check if we're a final destination.
        check_packet::final_destination_check(&self, packet.clone())?;
        //Check if the packet is dropped (only when msg_fragment).
        check_packet::pdr_check(&self, packet.clone())?;
        //Check if the next_hop exists.
        check_packet::is_next_hop_check(&self, packet.clone())?;

        //If no check gave an error, we return the starting packet.
        Ok(packet)
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
        self.controller_send.send(DroneEvent::PacketSent(resp)).unwrap();
    }
}

mod error {
    use wg_2024::network::{NodeId, SourceRoutingHeader};
    use wg_2024::packet::{Nack, NackType, Packet, PacketType};

    pub fn create_error(starting_id: NodeId, packet: Packet, nack_type: NackType) -> Packet {
        let mut fragment_index = 0;
        if let PacketType::MsgFragment(msg_fragment) = packet.pack_type {
            fragment_index = msg_fragment.fragment_index;
        }
        let position = packet.routing_header.hops
            .iter()
            .position(|x| *x == starting_id).unwrap();
        Packet {
            pack_type: PacketType::Nack(Nack{
                fragment_index,
                nack_type,
            }),
            routing_header: SourceRoutingHeader{
                hop_index: 0,
                hops: packet.routing_header.hops[0..position + 1].to_vec()
                    .into_iter()
                    .rev()
                    .collect::<Vec<NodeId>>()
            },
            session_id: packet.session_id,
        }
    }
}

mod check_packet {
    use wg_2024::packet::{NackType, Packet, PacketType};
    use crate::my_drone::{error, SkyLinkDrone};

    pub fn id_hop_match_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
        if packet.routing_header.hops[packet.routing_header.hop_index] == drone.id {
            Ok(())
        } else {
            match packet.pack_type.clone() {
                PacketType::MsgFragment(_fragment) => {
                    Err(error::create_error(packet.routing_header.hops[packet.routing_header.hop_index-1], packet, NackType::UnexpectedRecipient(drone.id)))
                },
                _ => {
                    Err(packet)
                }
            }}
    }
    pub fn final_destination_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
        if packet.routing_header.hop_index < packet.routing_header.hops.len() {
            Ok(())
        } else {
            match packet.pack_type.clone() {
                PacketType::MsgFragment(_fragment) => {
                    Err(error::create_error(drone.id, packet, NackType::DestinationIsDrone))
                },
                _ => {
                    Err(packet)
                }
            }
        }
    }
    pub fn is_next_hop_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
        let next_hop = &packet.routing_header.hops[packet.routing_header.hop_index];
        if drone.packet_send.contains_key(next_hop) {
            Ok(())
        } else {
            match packet.pack_type.clone() {
                PacketType::MsgFragment(_fragment) => {
                    Err(error::create_error(drone.id, packet, NackType::ErrorInRouting(drone.id)))
                },
                _ => {
                    Err(packet)
                }
            }
        }
    }
    pub fn pdr_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
        if let PacketType::MsgFragment(_) = packet.pack_type.clone() {
            let random_number: u32 = fastrand::u32(0..101);
            if random_number < drone.pdr {
                return Err(error::create_error(drone.id, packet, NackType::Dropped))
            }
        }
        Ok(())
    }
}