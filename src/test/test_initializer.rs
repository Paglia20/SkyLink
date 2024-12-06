use std::{fs, thread};
use std::thread::JoinHandle;
use std::collections::HashMap;
use crossbeam_channel::{unbounded, Receiver, Sender};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Packet, PacketType};
use crate::skylink_drone::drone::SkyLinkDrone;

pub fn test_initialize(file: &str) -> (MySimContr, MyClient, Vec<JoinHandle<()>>) {
    let config = parse_config(file);
    let mut handles = Vec::new();
    //I'll return the handles of the threads, and join them to the main thread.

    let mut command_send = HashMap::new();
    //This will be given to the Sim Contr to command the drones.
    let (event_send, event_recv) = unbounded();
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


    for drone in config.drone.into_iter() {
        //Adding the sender to this drone to the senders of the Sim Contr.
        let (contr_send, contr_recv) = unbounded();
        command_send.insert(drone.id, contr_send);

        //Give the drone a copy of the sender of events to the Sim Contr.
        let node_event_send = event_send.clone();

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
    }

    let client = config.client.get(0).unwrap();
    let client_recv = packet_receivers.remove(&client.id).unwrap();
    let client_send = client.clone()
        .connected_drone_ids
        .into_iter()
        .map(|id| (id, packet_senders[&id].clone()))
        .collect();
    println!("Client {} - channels:\n{:?}",client.id, client_send);

    let my_client = MyClient {
        id: client.id,
        client_send,
        client_recv
    };

    let sim_contr = MySimContr {
        command_send,
        event_recv,
        // event_send,
        // packet_senders,
        // network_graph
    };


    (sim_contr, my_client, handles)
}

fn parse_config(file: &str) -> Config {
    let file_str = fs::read_to_string(file).unwrap();
    toml::from_str(&file_str).unwrap()
}

#[derive(Debug)]
pub struct MyClient {
    pub id: NodeId,
    pub client_send: HashMap<NodeId, Sender<Packet>>,
    pub client_recv: Receiver<Packet>
}
pub struct MySimContr {
    pub command_send: HashMap<NodeId,Sender<DroneCommand>>,
    pub event_recv: Receiver<DroneEvent>,
    // pub event_send: Sender<DroneEvent>,
    // pub packet_senders,
    // pub network_graph
}
