use crate::simulation_control::sim_daniel::Scene::*;
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::NodeType::*;
use wg_2024::packet::Packet;
use wg_2024::packet::PacketType::*;


pub struct SimulationStorage {
    pub dropped_packets: Vec<(NodeId, Packet)>, // to display dropped packets
    pub contacts: HashMap<NodeId, HashSet<NodeId>>,  //if you want them sort change this in a BtreeSet
    pub destinations: HashMap<NodeId, HashSet<NodeId>>

    //chats
    //contents found...
}

impl SimulationStorage{
    pub fn new() -> SimulationStorage{
        SimulationStorage{
            dropped_packets: Default::default(),
            contacts: Default::default(),
            destinations: Default::default(),
        }
    }

    pub fn add_contacts (&mut self, src: NodeId, contact: NodeId){
        match self.contacts.get_mut(&src){
            Some(contacts) => {
                contacts.insert(contact);
            }
            None => {
                let set = HashSet::from([contact]);
                self.contacts.insert(src, set);

            }
        }
    }

    pub fn add_destination(&mut self, src: NodeId, dst: NodeId){
        match self.destinations.get_mut(&src){
            Some(destinations) => {
                destinations.insert(dst);
            }
            None => {
                let set = HashSet::from([dst]);
                self.destinations.insert(src, set);

            }
        }
    }

}
