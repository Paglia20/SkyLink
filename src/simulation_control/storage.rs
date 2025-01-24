use crate::simulation_control::sim_daniel::Scene::*;
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::NodeType::*;
use wg_2024::packet::Packet;
use wg_2024::packet::PacketType::*;
use crate::server::server_type::ContentServerType::{Media, Text};
use crate::server::server_type::ServerType;

pub struct SimulationStorage {
    pub dropped_packets: Vec<(NodeId, Packet)>, // to display dropped packets
    pub contacts: HashMap<NodeId, HashSet<NodeId>>,  //if you want them sort change this in a BtreeSet
    pub destinations: HashMap<NodeId, HashSet<(NodeId)>>,
    pub chats: HashMap<NodeId, HashMap<NodeId, Vec<(NodeId, String)>>>, // 1st is node, second is the contact, third is chat (each string has the associated sender)

    //chats
    //contents found...
}

impl SimulationStorage{
    pub fn new() -> SimulationStorage{
        SimulationStorage{
            dropped_packets: Default::default(),
            contacts: Default::default(),
            destinations: Default::default(),
            chats: Default::default(),
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
    pub fn add_chat_text(&mut self, src: NodeId, dst: NodeId, str: String){
        let new_text = (src, str);
        //first add to src chats the message he is sending
        let contact_map = self.chats.entry(src).or_insert( HashMap::new());
        let chat = contact_map.entry(dst).or_insert(Vec::new());
        // Push the new chat message (source, text) to the vector
        chat.push(new_text.clone());

        //then add to dst chats the message he has received
        let contact_map = self.chats.entry(dst).or_insert(HashMap::new());
        let chat = contact_map.entry(src).or_insert(Vec::new());
        // Push the new chat message (source, text) to the vector
        chat.push(new_text);
    }

    pub fn retrieve_chat (&self, src: NodeId, dst: NodeId) -> Option<Vec<(NodeId, String)>>{
        if let Some(src_chats) = self.chats.get(&src){
            if let Some(chat) = src_chats.get(&dst){
               return Some(chat.clone())
            }
        }
        None
    }

}
