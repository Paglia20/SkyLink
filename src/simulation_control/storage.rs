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
    pub registrations: HashMap<NodeId, Vec<NodeId>>,
    //contents found...
    pub text_lists: HashMap<NodeId, Vec<(u64, String)>>,
    //per il momento, NON voglio che text lists anticipi già che media ha dentro per una risoluzione mirata
    //se vorrò cambiarlo dovrò cambiare anche il clientevent
    pub catalogues: HashMap<NodeId, Vec<u64>>,
    pub medias: HashMap<NodeId, Vec<(u64, String, Vec<u8>)>>,

}
impl SimulationStorage{
    pub fn new() -> SimulationStorage{
        SimulationStorage{
            dropped_packets: Default::default(),
            contacts: Default::default(),
            destinations: Default::default(),
            chats: Default::default(),
            registrations: Default::default(),
            text_lists: Default::default(),
            catalogues: Default::default(),
            medias: Default::default(),
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

    pub(crate) fn add_text_list(&mut self, src: NodeId, text_id: u64, text_name: String) {
        let text_lists = self.text_lists.entry(src).or_insert(vec![]);
        if !text_lists.contains(&(text_id, text_name.clone())){
            text_lists.push((text_id, text_name));
        }
    }

    pub(crate) fn add_to_catalogue(&mut self, src: NodeId, media_id: u64, media_name : String) {
        let medias_to_buy = self.text_lists.entry(src).or_insert(vec![]);
        if !medias_to_buy.contains(&(media_id, media_name.clone())){
            medias_to_buy.push((media_id, media_name));
        }
    }
    pub(crate) fn add_to_medias(&mut self, src: NodeId, media_id: u64, media_name : String, media: Vec<u8>) {
        let medias_of_node = self.medias.entry(src).or_insert(vec![]);
        if !medias_of_node.contains(&(media_id, media_name.clone(),media.clone())){
            medias_of_node.push((media_id,media_name.clone(),media));
        }
    }

    pub(crate) fn add_to_registration(&mut self, src: NodeId, dst: NodeId) {
        let reg = self.registrations.entry(src).or_insert(vec![]);
        if !reg.contains(&dst){
            reg.push(dst);
        }
    }

    pub(crate) fn missing_media(&mut self, p0: NodeId, p1: u64) {
        if let Some(medias) = self.catalogues.get_mut(&p0){
            medias.retain(|n| *n != p1);
        }
    }
    pub(crate) fn missing_txt_list(&mut self, p0: NodeId, p1: u64) {
        if let Some(medias) = self.text_lists.get_mut(&p0){
            medias.retain(|n| n.0 != p1);
        }
    }

}
