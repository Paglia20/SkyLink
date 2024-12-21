
use crossbeam_channel::{select, unbounded, Receiver, Sender};
use std::thread::JoinHandle;
use std::collections::{HashMap, VecDeque};
use std::fmt::{Display, Formatter};
use std::sync::{Arc, RwLock};
use std::thread;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::controller::DroneCommand::{AddSender, RemoveSender};
use wg_2024::drone::*;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;
use crate::skylink_drone::drone::SkyLinkDrone;

pub struct SimBuilder{
    pub(crate) network_graph: HashMap<NodeId, Vec<NodeId>>,
    pub(crate) log: VecDeque<LogEntry>,
}

impl SimBuilder {
    pub fn new(network_graph: HashMap<NodeId, Vec<NodeId>>, log: VecDeque<LogEntry> ) -> SimBuilder {
        SimBuilder{
            network_graph,
            log,
        }
    }
}

pub struct SimulationControl{
    node_send: HashMap<NodeId, Sender<DroneCommand>>,
    node_recv: Receiver<DroneEvent>,
    channel_for_drone: Sender<DroneEvent>, // questo serve così ogni volta che creo un nuovo drone, quando gli devo dare il channel per comunicare con il drone, mi limito a clonare questo
    pub all_sender_packets: HashMap<NodeId, Sender<Packet>>, //hashmap con tutti i sender packet così puoi clonarli nel spawn, made pub for testing

    builder: Arc<RwLock<SimBuilder>>,
}

impl SimulationControl{
    pub fn new(node_send: HashMap<NodeId, Sender<DroneCommand>>, node_recv: Receiver<DroneEvent>, channel_for_drone :Sender<DroneEvent> , all_sender_packets: HashMap<NodeId, Sender<Packet>>, builder: Arc<RwLock<SimBuilder>>)->Self{
        SimulationControl{
            node_send,
            node_recv,
            channel_for_drone,
            all_sender_packets,
            builder,
        }
    }

    pub fn run(&mut self){
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

    pub(crate) fn add_to_log(&mut self, e: DroneEvent){
        match e {
            DroneEvent::PacketSent(packet) => {
                let id_drone = packet.routing_header.hops.get(packet.routing_header.hops.len() -1).unwrap();
                let new_log = LogEntry{
                    node_id: *id_drone,
                    message: format!("Sent fragment {:?} of type: {:?}",packet.session_id, packet.pack_type),
                };
                self.builder.write().unwrap().log.push_back(new_log);
            }
            DroneEvent::PacketDropped(packet) => {
                let id_drone = packet.routing_header.hops.get(packet.routing_header.hops.len() -1).unwrap();
                let new_log = LogEntry{
                    node_id: *id_drone,
                    message: format!("Dropped fragment {:?} of type: {:?}",packet.session_id, packet.pack_type)
                };
                self.builder.write().unwrap().log.push_back(new_log);
            }
            DroneEvent::ControllerShortcut(packet) => {
                let id_drone = packet.routing_header.hops.get(packet.routing_header.hops.len() -1).unwrap();
                let new_log = LogEntry{
                    node_id: *id_drone,
                    message: format!("Sent shortcut {:?}", packet.pack_type)
                };
                self.builder.write().unwrap().log.push_back(new_log);
            }
        }
    }

    pub fn spawn_drone (&mut self, pdr: f32, connections: Vec<NodeId>) -> JoinHandle<()>{
        let new_id = self.generate_id();
        //aggiorna network graph
        self.builder.write().unwrap().network_graph.insert(new_id, connections.clone());

        let (control_sender, control_receiver) = unbounded();  //canale per il Sim che manda drone command al drone
        self.node_send.insert(new_id.clone(), control_sender.clone());                                      // do al sim il sender per questo drone


        let (packet_send, packet_recv) = unbounded();                       //canale per il drone, il recv gli va dentro, il send va dato in copia a tutti i droni che vogliono comunicare con lui
        for (id, sender) in self.node_send.iter() {                        // per dare a tutti i droni in node_in il sender al new drone
            for i in connections.clone() {
                if i == *id {
                    sender.send(AddSender(new_id, packet_send.clone())).unwrap();
                }
            }
        }

        let mut packet_send = HashMap::new();
        //riempi la hashmap
        for (id, sender) in &self.all_sender_packets {
            for i in connections.clone() {
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

    fn generate_id (&mut self) -> NodeId {//just a function to generate an id that is empty in our hashmap, if is 1-3-4, it should give 2, if it's 1-2-3, should give 4.
        for k in 0..=u8::MAX {
            //If k is not a key in the map, I return it.
            if !self.node_send.contains_key(&k) {
                return k;
            }
        }

        unreachable!("No free key found");
    }

    pub fn crash_drone(&mut self, id: NodeId){
        if let Some(sender) = self.node_send.get(&id) {
            if let Err(e) = sender.send(DroneCommand::Crash) {
                println!("error in crashing drone {}: {:?}", id, e);
            } else {
                println!("crash command sent do the drone {}", id);


                // remove the drone from the neighbour's sends
                if let Some(vec) = self.builder.read().unwrap().network_graph.get(&id) {
                    for (neighbor_id, neighbor_sender) in &self.node_send {
                        if vec.contains(neighbor_id) {
                            neighbor_sender.send(RemoveSender(id)).unwrap()
                        }
                    }
                }
                if let Some(to_be_dropped) = self.node_send.remove(&id){
                    drop(to_be_dropped);
                }
                self.builder.write().unwrap().log.push_back(LogEntry::new(id, "Node crashed".to_string()));
            }
        } else {
            println!("drone {} not found in the network.", id);
        }
    }
    fn remove_senders(&mut self, id: NodeId, id_to_remove: NodeId){
        if let Some(sender) = self.node_send.get(&id) {
            if let Err(_e) = sender.send(RemoveSender(id_to_remove)) {
                println!("error in removing drone {} from drone {} senders", id_to_remove, id);
            } else {
                println!("drone {} removed from drone {} senders", id_to_remove, id);
                self.builder.write().unwrap().log.push_back(LogEntry::new(id, format!("drone {} removed from senders", id_to_remove)));
            }
        }
    }

    fn add_sender(&mut self, id: NodeId, id_to_add: NodeId, ){
        if let Some(sender) = self.node_send.get(&id) {
            if let Some(senderpacket) = self.all_sender_packets.get(&id) {
                if let Err(_e) = sender.send(AddSender(id_to_add, senderpacket.clone())) {
                    println!("error adding drone {} to drone {} senders", id_to_add, id);
                } else {
                    println!("drone {} added to drone {} senders", id_to_add, id);
                    self.builder.write().unwrap().log.push_back(LogEntry::new(id, format!("drone {} added to senders", id_to_add)));
                }
            }
        }
    }

    fn set_pdr(&mut self, id: NodeId, pdr: f32 ){
        if let Some(sender) = self.node_send.get(&id) {
            if let Err(_e) = sender.send(DroneCommand::SetPacketDropRate(pdr)) {
                println!("error in setting drone {} pdr to {}", id, pdr);
            } else {
                println!("setting drone {} pdr to {}", id, pdr);
                self.builder.write().unwrap().log.push_back(LogEntry::new(id, format!("drone now has pdr set to {}", pdr)));
            }
        }
    }

}

pub struct LogEntry {
    node_id: NodeId,
    message: String,
}
impl LogEntry {
    pub fn new(node_id: NodeId, message: String) -> LogEntry {
        LogEntry{node_id, message}
    }
    pub fn get_id(&self) -> NodeId {
        self.node_id
    }
}

impl Display for LogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Node {} notified {}", self.node_id, self.message)
    }
}

