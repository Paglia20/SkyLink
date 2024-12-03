use crossbeam_channel::{select, unbounded, Receiver, Sender};
use std::thread::JoinHandle;
use std::collections::HashMap;
use std::thread;
use wg_2024::controller::{DroneCommand, NodeEvent};
use wg_2024::controller::DroneCommand::{AddSender, RemoveSender};
use wg_2024::drone::*;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;
use crate::my_drone::SkyLinkDrone;

pub struct SimulationControl{
    node_send: HashMap<NodeId, Sender<DroneCommand>>,
    node_recv: Receiver<NodeEvent>,
    channel_for_drone: Sender<NodeEvent>, // questo serve così ogni volta che creo un nuovo drone, quando gli devo dare il channel per comunicare con il drone, mi limito a clonare questo
    all_sender_packets: HashMap<NodeId, Sender<Packet>>, //hashmap con tutti i sender packet così puoi clonarli nel spawn
    network_graph: HashMap<NodeId, Vec<NodeId>>,
    log: Vec<String>,
}

impl SimulationControl{
    fn new(channel_for_drone :Sender<NodeEvent> , all_sender_packets: HashMap<NodeId, Sender<Packet>>, node_send: HashMap<NodeId, Sender<DroneCommand>>, node_recv: Receiver<NodeEvent>, network_graph: HashMap<NodeId, Vec<NodeId>>)->Self{
        SimulationControl{
            node_send,
            node_recv,
            channel_for_drone,
            all_sender_packets,
            network_graph,
            log: Vec::new(),
        }
    }

    fn run(&mut self){
        loop{
            select! {
            recv(self.node_recv) -> e =>{
                    if let Ok(event) = e {
                        self.add_to_log(event);
                    }
                }
            }
        }
    }

    fn add_to_log(&mut self, e: NodeEvent){
        match e {
            NodeEvent::PacketSent(packet) => {
                let id_drone = packet.routing_header.hops.get(packet.routing_header.hops.len() -1).unwrap();
                self.log.push( format!("Drone {} sent fragment {:?} of type: {:?}",id_drone ,packet.session_id, packet.pack_type))}
            NodeEvent::PacketDropped(packet) => {
                let id_drone = packet.routing_header.hops.get(packet.routing_header.hops.len() -1).unwrap();
                self.log.push( format!("Drone {} dropped fragment {:?} of type: {:?}",id_drone ,packet.session_id, packet.pack_type))}
            NodeEvent::ControllerShortcut(packet) => {
                let id_drone = packet.routing_header.hops.get(packet.routing_header.hops.len() -1).unwrap();
                self.log.push( format!("Received {:?} from drone {:?}", packet.pack_type, id_drone));
            }
        }
    }

    fn spawn_drone (&mut self, pdr: f32, node_in: Vec<NodeId>, node_out: Vec<NodeId>) -> JoinHandle<()>{
        let new_id = self.generate_id();

        let (control_sender, control_receiver) = unbounded();  //canale per il Sim che manda drone command al drone
        self.node_send.insert(new_id.clone(), control_sender.clone());                                      // do al sim il sender per questo drone


        let (packet_send, packet_recv) = unbounded();                       //canale per il drone, il recv gli va dentro, il send va dato in copia a tutti i droni che vogliono comunicare con lui
        for (id, sender) in self.node_send.iter() {                        // per dare a tutti i droni in node_in il sender al new drone
            for i in node_in.clone() {
                if i == *id {
                    sender.send(AddSender(new_id, packet_send.clone())).unwrap();
                }
            }
        }

        let mut packet_send = HashMap::new();
        //riempi la hashmap
        for (id, sender) in &self.all_sender_packets {
            for i in node_out.clone() {
                if i == *id{
                    packet_send.insert(*id, sender.clone());
                }
            }
        }

        let channel_clone = self.channel_for_drone.clone();

        //crea thread
        let handle = thread::spawn(move || {
            let mut new_drone = SkyLinkDrone::new(new_id, channel_clone, control_receiver, packet_recv, packet_send, pdr);
            new_drone.run();
        });
        handle
    }

    fn generate_id (&mut self) -> NodeId {                  //just a function to generate an id that is empty in our hashmap, if is 1-3-4, it should give 2, if it's 1-2-3, should give 4.
        for k in 0..=u8::MAX {
            // Se `k` non è una chiave nella mappa, restituiscilo
            if !self.node_send.contains_key(&k) {
                return k;
            }
        }

        unreachable!("No free key found");
    }

    fn crash_drone(&mut self, id: NodeId){
        if let Some(sender) = self.node_send.get(&id) {
            if let Err(e) = sender.send(DroneCommand::Crash) {
                println!("error in crashing drone {}: {:?}", id, e);
            } else {
                println!("crash command sent do the drone {}", id);


                // remove the drone from the neighbour's sends
                if let Some(vec) = self.network_graph.get(&id) {
                    for (neighbor_id, neighbor_sender) in &self.node_send {
                        if vec.contains(neighbor_id) {
                            neighbor_sender.send(RemoveSender(id)).unwrap()
                        }
                    }
                }
                self.node_send.remove(&id);
                self.log.push(format!("drone {} crashed.", id));
            }
        } else {
            println!("drone {} not found in the network.", id);
        }
    }
}

