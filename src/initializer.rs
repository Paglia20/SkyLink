use crate::sim_control::SimulationControl;
use crate::skylink_drone::drone::SkyLinkDrone;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::thread::JoinHandle;
use std::{fs, thread};
use wg_2024::config;
use wg_2024::drone::Drone;
use wg_2024::network::NodeId;
use wg_2024::packet::{NodeType, Packet};
use crate::{ALL_CHAT, ALL_CONTENT};
use crate::clients_gio::client_chat::ChatClient;
use crate::clients_gio::web_browser::WebBrowser;
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::ClientTrait;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::simulation_control::sim_daniel::NodeNature;
use crate::simulation_control::sim_daniel::NodeNature::*;


pub fn initialize(file: &str) -> (SimulationControl, Vec<JoinHandle<()>>) {
    let config = parse_config(file);
    let mut handles = Vec::new();
    //I'll return the handles of the threads, and join them to the main thread.

    let mut drone_command_send = HashMap::new();
    let mut client_command_send = HashMap::new();
    let mut server_command_send = HashMap::new();

    //This will be given to the Sim Contr to command the drones.
    let (drone_event_send, drone_event_recv) = unbounded();
    //I create the channel, the 'send' will be given to every drone,
    //while the 'recv' will go to the Sim contr.

    let (client_event_send, client_event_recv) = unbounded();
    let (server_event_send, server_event_recv) = unbounded();



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

    //I crate a hashmap that will be used as graph by the Simulation Controller.
    let mut network_graph = HashMap::new();
    for drone in config.drone.iter() {
        network_graph.insert(drone.id, (NodeNature::Drone, HashSet::from_iter(drone.connected_node_ids.clone())));
    }

    for drone in config.drone.into_iter() {
        //Adding the sender to this drone to the senders of the Sim Contr.
        let (contr_send, contr_recv) = unbounded();
        drone_command_send.insert(drone.id, contr_send);

        //Give the drone a copy of the sender of events to the Sim Contr.
        let node_event_send = drone_event_send.clone();

        //Take the channels necessary to this drone.
        let drone_recv = packet_receivers.remove(&drone.id).unwrap();
        let drone_send = drone
            .connected_node_ids
            .into_iter()
            .map(|id| (id, packet_senders[&id].clone()))
            .collect();

        //create the thread of the drone, and add it to a Vec to be pushed afterward
        handles.push(thread::spawn(move || {
            let mut drone = SkyLinkDrone::new(
                drone.id,
                node_event_send,
                contr_recv,
                drone_recv,
                drone_send,
                drone.pdr,
            );

            drone.run();
        }));
        //This will probably need to be changed based on the
        //implementation of other groups drones in our network.
    }

    // for client in config.client.into_iter() {
    //     //Adding the sender to this client to the senders of the Sim Contr.
    //     let (contr_send, contr_recv) = unbounded();
    //     client_command_send.insert(client.id, contr_send);
    //
    //     //Give the client a copy of the sender of events to the Sim Contr.
    //     let node_event_send = client_event_send.clone();
    //     network_graph.insert(client.id, (NodeNature::ChatClient, HashSet::from_iter(client.connected_drone_ids.clone())));
    //
    //     //Take the channels necessary to this client.
    //     let client_recv = packet_receivers.remove(&client.id).unwrap();
    //     let client_send: HashMap<NodeId, Sender<Packet>> = client
    //         .connected_drone_ids
    //         .into_iter()
    //         .map(|id| (id, packet_senders[&id].clone()))
    //         .collect();
    //
    //     //create the thread of the Client, and add it to a Vec to be pushed afterward
    //     handles.push(thread::spawn(move || {
    //         let mut client = ChatClient::new(
    //             client.id,
    //             contr_recv,
    //             node_event_send,
    //             client_recv,
    //             client_send,
    //         );
    //         client.run();
    //     }));
    //     //This will probably need to be changed based on the
    //     //implementation of other groups drones in our network.
    // }

    // I create the servers in an external function, that'll add them to the 'handles' vector.
    let (chat_servers, media_servers) = create_servers(config.server.clone(),
                                                       &mut handles,
                                                       &mut server_command_send,
                                                       &server_event_send,
                                                       &packet_senders,
                                                       &mut packet_receivers,
                                                       &mut network_graph
    );
    create_clients(config.client.clone(),
                   &mut handles,
                   &mut client_command_send,
                   &client_event_send,
                   &packet_senders,
                   &mut packet_receivers,
                   chat_servers,
                   media_servers,
                   &mut network_graph,
    );

    let mut sim_contr = SimulationControl::new(
        drone_command_send,
        client_command_send,
        server_command_send,
        drone_event_recv,
        client_event_recv,
        server_event_recv,
        drone_event_send,
        packet_senders,
        network_graph,
    );

    (sim_contr, handles)
}

fn parse_config(file: &str) -> config::Config {
    let file_str = fs::read_to_string(file).unwrap();
    toml::from_str(&file_str).unwrap()
}

fn create_servers(servers: Vec<config::Server>,
                  handles: &mut Vec<JoinHandle<()>>,
                  server_command_send: &mut HashMap<NodeId, Sender<ServerCommand>>,
                  server_event_send: &Sender<ServerEvent>,
                  packet_senders: &HashMap<NodeId, Sender<Packet>>,
                  packet_receivers: &mut HashMap<NodeId, Receiver<Packet>>,
                  network_graph: &HashMap<NodeId, (NodeNature, HashSet<NodeId>)>) -> (bool, bool) {

    let length = servers.len();
    let mut chooser = 0;
    let (mut chat_servers, mut media_servers) = (false, false);

    for server in servers.into_iter() {
        // Adding the sender to this server to the senders of the Sim Contr.
        let (contr_send, contr_recv) = unbounded();
        server_command_send.insert(server.id, contr_send);

        // Give the server a copy of the sender of events to the Sim Contr.
        let node_event_send = server_event_send.clone();


        // Take the channels necessary to this client.
        let server_recv = packet_receivers.remove(&server.id).unwrap();
        let server_send: HashMap<NodeId, Sender<Packet>> = server
            .connected_drone_ids
            .into_iter()
            .map(|id| (id, packet_senders[&id].clone()))
            .collect();

        // Create the thread of the server,
        // and add it to a Vec to be pushed afterward.

        // I also need to choose which server to pick, to do that we:
        // - Check if we have more than 2 server available, since text and media server can't exist alone.
        // - Check a chooser variable, which at each iteration of the for creates a different server type.
        if length >= 2 && chooser == 0 {
            handles.push(thread::spawn(move || {
                //create text server

            }));
            chooser += 1;
        } else if length >= 2 && chooser == 1 {
            handles.push(thread::spawn(move || {
                //create media server

            }));
            media_servers = true;
            chooser += 1;
        } else {
            handles.push(thread::spawn(move || {
                //create chat server

            }));
            chat_servers = true;
            chooser = 0;
        }
    }

    (chat_servers, media_servers)
}

fn create_clients(clients: Vec<config::Client>,
                  handles: &mut Vec<JoinHandle<()>>,
                  client_command_send: &mut HashMap<NodeId, Sender<ClientCommand>>,
                  client_event_send: &Sender<ClientEvent>,
                  packet_senders: &HashMap<NodeId, Sender<Packet>>,
                  packet_receivers: &mut HashMap<NodeId, Receiver<Packet>>,
                  chat_server: bool,
                  media_server: bool,
                  network_graph: &mut HashMap<NodeId, (NodeNature, HashSet<NodeId>)>) {

    let length = clients.len();
    let mut chooser = true;

    // I create clients if at least one of the type of servers exists.
    if chat_server || media_server {

        for client in clients.into_iter() {
            //Adding the sender to this client to the senders of the Sim Contr.
            let (contr_send, contr_recv) = unbounded();
            client_command_send.insert(client.id, contr_send);

            //Give the client a copy of the sender of events to the Sim Contr.
            let node_event_send = client_event_send.clone();
            network_graph.insert(client.id, (NodeNature::WebBrowser, HashSet::from_iter(client.connected_drone_ids.clone())));

            //Take the channels necessary to this client.
            let client_recv = packet_receivers.remove(&client.id).unwrap();
            let client_send: HashMap<NodeId, Sender<Packet>> = client
                .connected_drone_ids
                .into_iter()
                .map(|id| (id, packet_senders[&id].clone()))
                .collect();

            //create the thread of the Client, and add it to a Vec to be pushed afterward
            if ALL_CONTENT {
                handles.push(thread::spawn(move || {
                    let mut client = WebBrowser::new(
                        client.id,
                        contr_recv,
                        node_event_send,
                        client_recv,
                        client_send,
                    );
                    client.run();
                }));
            }

            else if (length >= 2 && chat_server && chooser) || ALL_CHAT {
                network_graph.entry(client.id).and_modify(|x|x.0 = NodeNature::ChatClient);
                handles.push(thread::spawn(move || {
                    let mut client = ChatClient::new(
                        client.id,
                        contr_recv,
                        node_event_send,
                        client_recv,
                        client_send,
                    );
                    client.run();
                }));

                chooser = !chooser
            } else {
                //create media client
                handles.push(thread::spawn(move || {
                    let mut client = WebBrowser::new(
                        client.id,
                        contr_recv,
                        node_event_send,
                        client_recv,
                        client_send,
                    );
                    client.run();
                }));

                chooser = !chooser
            }

            if media_server && !chooser {
                chooser = true;
                // This avoids skipping of cycles if we can't create media clients.
            }
        }
    } else {
        panic!("Clients can't work without servers");
    }
}