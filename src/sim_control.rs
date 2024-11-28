use std::cell::RefCell;
use std::sync::Arc;
use crossbeam_channel::{select, Receiver, Sender};
use std::thread::JoinHandle;
use std::collections::HashMap;

use wg_2024::controller::{DroneCommand, NodeEvent};
use wg_2024::network::NodeId;

pub struct SimulationControl{
    node_send: HashMap<NodeId, Sender<DroneCommand>>,
    node_recv: Receiver<NodeEvent>,
    threads: Arc<RefCell<HashMap<NodeId, JoinHandle<()>>>>,
    log: Vec<String>
}

impl SimulationControl{
    fn new(node_send: HashMap<u8, Sender<DroneCommand>>, node_recv: Receiver<NodeEvent>, threads: Arc<RefCell<HashMap<NodeId, JoinHandle<()>>>> )->Self{
        SimulationControl{
            node_send,
            node_recv,
            threads,
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
        }
    }

}

