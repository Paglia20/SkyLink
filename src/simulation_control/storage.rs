use crate::event_wrapper::Event;
use crate::sim_control::{Cause, LogEntry, SimulationControl};
use crate::simulation_control::sim_control::Cause::Error;
use crate::simulation_control::sim_daniel::NodeWindowScene::{AddSender, Crash, ShowDestinations, ShowContents, RemoveSender, SetPDR, Start};
use crate::simulation_control::sim_daniel::Scene::*;
use crate::test::test_bench::create_packet;
use eframe::egui;
use egui::{FontId, RichText, Vec2};
use std::cmp::{Ordering, PartialEq};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use wg_2024::controller::DroneEvent::{ControllerShortcut, PacketDropped};
use wg_2024::network::NodeId;
use wg_2024::packet::NodeType::*;
use wg_2024::packet::PacketType::*;
use wg_2024::packet::{NodeType, Packet};
use crate::clients_gio::client_command::ClientEvent;
use crate::message::{ContentType, Message};
use crate::message::ChatRequest::{ClientList, Register, SendMessage};
use crate::simulation_control::sim_daniel::MessageScene::Id;


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

    pub fn add_destionation (&mut self, src: NodeId, dst: NodeId){
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
