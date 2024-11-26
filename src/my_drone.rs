use std::collections::HashMap;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use crossbeam_channel::{select_biased, Receiver, Sender};
use wg_2024::controller::{DroneCommand, NodeEvent};
use wg_2024::drone::{Drone, DroneOptions};
use wg_2024::packet::{Packet, PacketType, Nack};
use wg_2024::packet::Nack::{DestinationIsDrone, ErrorInRouting};

pub struct MyDrone {
    id: NodeId,
    controller_send: Sender<NodeEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    pdr: f32,
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
            pdr: options.pdr,
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
                self.pdr = pdr;
            },
            DroneCommand::Crash => unreachable!(),
        }
    }

    fn handle_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::MsgFragment(_fragment_id) => {
                //ciao
            },
            PacketType::FloodRequest(_flood_request_id) => {

            },
            _ => {
                let position = packet.routing_header.hop_index;
                if position < packet.routing_header.hops.len() {
                    let next = packet.routing_header.hops[position];
                    if self.packet_send.contains_key(&next) {
                        if let Ok(_) = self.packet_send.get(&next).unwrap().send(packet.clone()) {
                            self.controller_send.send(NodeEvent::PacketSent(packet)).unwrap();
                            //document panic?
                            return;
                        }
                    }
                    let err = build_nack(ErrorInRouting(next),
                                         packet.routing_header.hops.clone(),
                                         packet.session_id
                    );
                    self.handle_packet(err.clone());
                    self.controller_send.send(NodeEvent::PacketSent(err)).unwrap();

                } else {
                    let err = build_nack(DestinationIsDrone,
                                         packet.routing_header.hops.clone(),
                                         packet.session_id
                    );
                    self.handle_packet(err.clone());
                    self.controller_send.send(NodeEvent::PacketSent(err)).unwrap();
                }
            }
        }
    }
}

fn build_nack(nack_type: Nack, routing_vector: Vec<NodeId>, session_id: u64) -> Packet {
    Packet {
        pack_type: PacketType::Nack(nack_type),
        routing_header: SourceRoutingHeader{
            hop_index: 1,
            hops: routing_vector
                .into_iter()
                .rev()
                .collect::<Vec<NodeId>>()
        },
        session_id
    }
}