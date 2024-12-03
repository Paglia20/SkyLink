use std::{fs, thread};
use std::thread::JoinHandle;
use std::collections::HashMap;
use crossbeam_channel::unbounded;
use wg_2024::config::Config;
use wg_2024::drone::{Drone};
use crate::my_drone::SkyLinkDrone;

pub fn initialize(file: &str) -> Vec<JoinHandle<()>>{
    let config = parse_config(file);
    let mut handles = Vec::new();
    //println!("{:?}", config);

    let mut command_send = HashMap::new();
    //This will be given to the Sim Contr to command the drones.
    let (node_event_send, node_event_recv) = unbounded();
    //I create the channel, the 'send' will be given to every drone,
    //while the 'recv' will go to the Sim contr.

    let mut packet_senders = HashMap::new();
    let mut packet_receivers = HashMap::new();
    //I create receivers and senders for every drone.
    for drone in config.drone.iter() {
        let (send, recv) = unbounded();
        packet_senders.insert(drone.id, send);
        packet_receivers.insert(drone.id, recv);
    }
    for client in config.client.iter() {
        let (send, recv) = unbounded();
        packet_senders.insert(client.id, send);
        packet_receivers.insert(client.id, recv);
    }
    for server in config.server.iter() {
        let (send, recv) = unbounded();
        packet_senders.insert(server.id, send);
        packet_receivers.insert(server.id, recv);
    }


    for drone in config.drone.into_iter() {
        //Adding the sender to this drone to the senders of the Sim Contr.
        let (contr_send, contr_recv) = unbounded();
        command_send.insert(drone.id, contr_send);

        //Give the drone a copy of the sender of events to the Sim Contr.
        let node_event_send = node_event_send.clone();

        //Take the channels necessary to this drone.
        let drone_recv = packet_receivers.remove(&drone.id).unwrap();
        let drone_send = drone
            .connected_node_ids
            .into_iter()
            .map(|id| (id, packet_senders[&id].clone()))
            .collect();

        //create the thread of the drone, and add it to a Vec to be pushed afterward
        handles.push(thread::spawn(move || {
            let mut drone = SkyLinkDrone::new(drone.id, node_event_send, contr_recv, drone_recv, drone_send, drone.pdr);

            drone.run();
        }));
        //This will probably need to be changed based on the
        //implementation of other groups drones in our network.
    }

    // let controller =


    handles
}

fn parse_config(file: &str) -> Config {
    let file_str = fs::read_to_string(file).unwrap();
    toml::from_str(&file_str).unwrap()
}