use crate::sim_control::SimulationControl;
use skylink::SkyLinkDrone;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::thread::JoinHandle;
use std::{fs, thread};
use std::cmp::max;
use fungi_drone::FungiDrone;
use getdroned::GetDroned;
use lockheedrustin_drone::LockheedRustin;
use rolling_drone::RollingDrone;
use rust_do_it::RustDoIt;
use rustastic_drone::RustasticDrone;
use wg_2024::config;
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;
use crate::{server, ALL_CHAT, ALL_CONTENT, CLIENT_GIO, DEBUG_MODE, NO_SERVER_MODE};
use crate::clients_sam;
use crate::clients_gio;
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_sam::sam_client_trait::Client;
use crate::server::server_trait::*;
use crate::server::server_chat::ChatServer;
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::simulation_control::sim_daniel::NodeNature;
use crate::simulation_control::sim_daniel::NodeNature::*;


pub fn initialize(file: &str) -> Option<(SimulationControl, Vec<JoinHandle<()>>)> {
    let config = parse_config(file);
    if check_config(&config) {
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

        let mut drone_chooser = 0;
        for drone in config.drone.into_iter() {
            //Adding the sender to this drone to the senders of the Sim Contr.
            let (contr_send, contr_recv) = unbounded();
            drone_command_send.insert(drone.id, contr_send);

            //Give the drone a copy of the sender of events to the Sim Contr.
            let node_event_send = drone_event_send.clone();

            //Take the channels necessary to this drone.
            let drone_recv = packet_receivers.remove(&drone.id)?;
            let drone_send = drone
                .connected_node_ids
                .into_iter()
                .map(|id| (id, packet_senders[&id].clone()))
                .collect();

            // println!("Drone {} has {:?}", drone.id, drone_send);

            //create the thread of the drone, and add it to a Vec to be pushed afterward
            /*handles.push(thread::spawn(move || {
                let mut drone = SkyLinkDrone::new(
                    drone.id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    drone.pdr,
                );
            
                drone.run();
            }));*/
            handles.push(create_drone(drone_chooser, drone.id, node_event_send, contr_recv, drone_recv, drone_send, drone.pdr));

            if drone_chooser >= 9 {
                drone_chooser = 0;
            } else {
                drone_chooser += 1;
            }
        }

        // Possible text_files read by servers.
        let text_files = vec![
            "src/test/contents_inputs/text_files/text_list1.txt".to_string(),
            "src/test/contents_inputs/text_files/text_list2.txt".to_string(),
            "src/test/contents_inputs/text_files/text_list3.txt".to_string(),
            "src/test/contents_inputs/text_files/text_list4.txt".to_string(),
            "src/test/contents_inputs/text_files/text_list5.txt".to_string(),
        ];
        // I create the servers in an external function, that'll add them to the 'handles' vector.
        let (chat_servers, media_servers) = create_servers(config.server.clone(),
                                                           &mut handles,
                                                           &mut server_command_send,
                                                           &server_event_send,
                                                           &packet_senders,
                                                           &mut packet_receivers,
                                                           &mut network_graph,
                                                           text_files
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

        let sim_contr = SimulationControl::new(
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

        Some((sim_contr, handles))
    } else {
        None
    }
}

fn parse_config(file: &str) -> Config {
    let file_str = fs::read_to_string(file).unwrap();
    toml::from_str(&file_str).unwrap()
}

fn create_drone(drone_chooser: u8, drone_id: NodeId, node_event_send: Sender<DroneEvent>, contr_recv: Receiver<DroneCommand>, drone_recv: Receiver<Packet>, drone_send: HashMap<NodeId, Sender<Packet>>, pdr: f32) -> JoinHandle<()>{
    match drone_chooser {
        0 => {
            thread::spawn(move || {
                println!("FungiDrone is {}", drone_id);
                let mut drone = FungiDrone::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        1 => {
            thread::spawn(move || {
                println!("RustyDrone is {}", drone_id);
                let mut drone = rusty_drones::RustyDrone::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        2 => {
            thread::spawn(move || {
                println!("RustasticDrone is {}", drone_id);
                let mut drone = RustasticDrone::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        3 => {
            thread::spawn(move || {
                println!("RustDoIt is {}", drone_id);
                let mut drone = RustDoIt::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        4 => {
            thread::spawn(move || {
                println!("RustDrone is {}", drone_id);
                let mut drone = wg_2024_rust::drone::RustDrone::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        5 => {
            thread::spawn(move || {
                println!("LockheedRustin is {}", drone_id);
                let mut drone = LockheedRustin::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        6 => {
            thread::spawn(move || {
                println!("GetDroned is {}", drone_id);
                let mut drone = GetDroned::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        7 => {
            thread::spawn(move || {
                println!("RollingDrone is {}", drone_id);
                let mut drone = RollingDrone::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        8 => {
            thread::spawn(move || {
                println!("d_r_o_n_e is {}", drone_id);
                let mut drone = d_r_o_n_e_drone::MyDrone::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        },
        _ => {
            println!("dr_ones is {}", drone_id);
            thread::spawn(move || {
                let mut drone = dr_ones::Drone::new(
                    drone_id,
                    node_event_send,
                    contr_recv,
                    drone_recv,
                    drone_send,
                    pdr,
                );
                drone.run();
            })
        }
    }
}

fn create_servers(servers: Vec<config::Server>,
                  handles: &mut Vec<JoinHandle<()>>,
                  server_command_send: &mut HashMap<NodeId, Sender<ServerCommand>>,
                  server_event_send: &Sender<ServerEvent>,
                  packet_senders: &HashMap<NodeId, Sender<Packet>>,
                  packet_receivers: &mut HashMap<NodeId, Receiver<Packet>>,
                  network_graph: &mut HashMap<NodeId, (NodeNature, HashSet<NodeId>)>,
                  files: Vec<String>) -> (bool, bool) {

    let length = servers.len();
    let mut chooser = 0;
    let (mut chat_servers, mut media_servers) = (false, false);

    // I simulate how the choosing later would go, to understand how to divide the files.
    let mut text_count = 0;
    let mut media_count = 0;
    let mut test_chooser = 0;
    for _ in 0..length {
        if length >= 2 && test_chooser == 0 {
            text_count += 1;
            test_chooser += 1;
        } else if length >= 2 && test_chooser == 1 {
            media_count += 1;
            test_chooser += 1;
        } else {
            test_chooser = 0;
        }
    }


    // If I don't have media-text servers, I set it to 0.
    let files_per_server = if length >= 2 {
        files.len() / max(text_count, media_count)
    } else {
        0
    };

    if files_per_server < 1 && media_count > 0 && text_count > 0 {
        panic!("Files per server must be > 1");
    }
    // I create an offset for each couple of content servers.
    let mut file_chooser = 0;

    for server in servers.into_iter() {
        // Adding the sender to this server to the senders of the Sim Contr.
        let (contr_send, contr_recv) = unbounded();
        server_command_send.insert(server.id, contr_send);

        // Give the server a copy of the sender of events to the Sim Contr.
        let node_event_send = server_event_send.clone();

        network_graph.insert(server.id, (TextServer, HashSet::from_iter(server.connected_drone_ids.clone())));

        // Take the channels necessary to this client.
        let server_recv = packet_receivers.remove(&server.id).unwrap();
        let server_send: HashMap<NodeId, Sender<Packet>> = server
            .connected_drone_ids
            .into_iter()
            .map(|id| (id, packet_senders[&id].clone()))
            .collect();

        // println!("Server {} has {:?}", server.id, server_send);


        let mut server_files = Vec::new();
        for i in 0..files_per_server {
            server_files.push(files.get(i + file_chooser).unwrap().clone());
        }


        // Create the thread of the server,
        // and add it to a Vec to be pushed afterward.

        // I also need to choose which server to pick, to do that we:
        // - Check if we have more than 2 server available, since text and media server can't exist alone.
        // - Check a chooser variable, which at each iteration of the for creates a different server type.
        if length >= 2 && chooser == 0 {
            handles.push(thread::spawn(move || {
                let mut server = server::server_text::TextServer::new(
                    server.id,
                    contr_recv,
                    node_event_send,
                    server_recv,
                    server_send,
                    server_files
                );
                server.run();
            }));
            chooser += 1;
        } else if length >= 2 && chooser == 1 {
            network_graph.entry(server.id).and_modify(|x|x.0 = MediaServer);

            handles.push(thread::spawn(move || {
                let mut server = server::server_media::MediaServer::new(
                    server.id,
                    contr_recv,
                    node_event_send,
                    server_recv,
                    server_send,
                    server_files
                );
                server.run();
            }));
            media_servers = true;
            file_chooser += files_per_server;
            chooser += 1;
        } else {
            network_graph.entry(server.id).and_modify(|x|x.0 = ChatServer);

            handles.push(thread::spawn(move || {
                let mut chat_server = ChatServer::new(
                    server.id,
                    contr_recv,
                    node_event_send,
                    server_recv,
                    server_send,
                    Vec::new(),
                );
                chat_server.run();
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
                if CLIENT_GIO {
                    handles.push(thread::spawn(move || {
                        let mut client = clients_gio::web_browser::WebBrowser::new(
                            client.id,
                            contr_recv,
                            node_event_send,
                            client_recv,
                            client_send,
                        );
                        client.run();
                    }));
                } else {
                    //SAM MODE
                    handles.push(thread::spawn(move || {
                        let mut client = clients_sam::sam_web_browser_system::WebBrowser::new(
                            client.id,
                            contr_recv,
                            node_event_send,
                            client_recv,
                            client_send,
                        );
                        client.run();
                    }));
                }
            }

            else if (length >= 2 && chat_server && chooser) || ALL_CHAT {
                network_graph.entry(client.id).and_modify(|x|x.0 = NodeNature::ChatClient);
                if CLIENT_GIO {
                    handles.push(thread::spawn(move || {
                        let mut client = clients_gio::client_chat::ChatClient::new(
                            client.id,
                            contr_recv,
                            node_event_send,
                            client_recv,
                            client_send,
                        );
                        client.run();
                    }));
                }else {
                    //SAM MODE
                    handles.push(thread::spawn(move || {
                        let mut client = clients_sam::sam_client_chat_system::ChatClient::new(
                            client.id,
                            contr_recv,
                            node_event_send,
                            client_recv,
                            client_send,
                        );
                        client.run();
                    }));


                }

                chooser = !chooser
            } else {
                //create media client

                if CLIENT_GIO {
                    handles.push(thread::spawn(move || {
                        let mut client = clients_gio::web_browser::WebBrowser::new(
                            client.id,
                            contr_recv,
                            node_event_send,
                            client_recv,
                            client_send,
                        );
                        client.run();
                    }));
                }else {
                    //SAM MODE
                    handles.push(thread::spawn(move || {
                        let mut client = clients_sam::sam_web_browser_system::WebBrowser::new(
                            client.id,
                            contr_recv,
                            node_event_send,
                            client_recv,
                            client_send,
                        );
                        client.run();
                    }));
                }

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

fn check_config(conf: &Config) -> bool {
    check_bidirectional(conf) && check_edges(conf)
}

fn check_edges(conf: &Config) -> bool{
    let all_server_ids: Vec<NodeId> = conf.server.iter().map(|z| z.id).collect();
    let all_client_ids: Vec<NodeId> = conf.client.iter().map(|z| z.id).collect();

    if !NO_SERVER_MODE {
        // Untill we don't have servers I don't check if they are rightfully connected.

        for server in &conf.server {
            if server.connected_drone_ids.len() < 2 || server.connected_drone_ids.contains(&server.id) {
                if DEBUG_MODE {
                    println!("server {} has not enough connections, or has itself as one", server.id)
                }
                return false
            }
            for connection in &server.connected_drone_ids {
                if all_client_ids.contains(connection) || all_server_ids.contains(connection) {
                    if DEBUG_MODE {
                        println!("server {} is wrongly connected to {connection}", server.id)
                    }
                    return false;
                }
            }
        }
    }

    for client in &conf.client {
        if client.connected_drone_ids.len() < 2 || client.connected_drone_ids.contains(&client.id){
            return false
        }
        for connection in &client.connected_drone_ids {
            if all_client_ids.contains(connection) || all_server_ids.contains(connection)  {
                if DEBUG_MODE{
                    println!("client {} is wrongly connected to {connection}", client.id)
                }
                return false;
            }
        }
    }
    true
}


fn check_bidirectional(conf: &Config) -> bool{
    // Check connections for drones.
    for drone in &conf.drone {
        for &conn in &drone.connected_node_ids {
            let mut valid = false;
            // Search the node in drones.
            check_in_drones(conf, drone.id, conn, &mut valid);
            // Search the target node in clients.
            check_in_clients(conf, drone.id, conn, &mut valid);

            // Search the target node in servers.
            check_in_server(conf, drone.id, conn, &mut valid);

            // If we don't find a valid bidirectional connection, we return false.
            if !valid {
                return false;
            }
        }
    }

    // Checks client connections.
    for client in &conf.client {
        for &conn in &client.connected_drone_ids {
            let mut valid = false;
            // Search target node in drones.
            check_in_drones(conf, client.id, conn, &mut valid);

            // We don't need to search it elsewhere becase only drones can be connected to clients.

            // If we find a non-bidirectional connection, we return false.
            if !valid {
                println!("a clients connections is not bidirectional");
                return false;
            }
        }
    }


    if !NO_SERVER_MODE {
        // Check Connections for servers.
        for server in &conf.server {
            for &conn in &server.connected_drone_ids {
                let mut valid = false;
                // Search target node in drones.
                check_in_drones(conf, server.id, conn, &mut valid);

                // If we find a non-valid connection, return false.
                if !valid {
                    return false;
                }
            }
        }
    }

    true
}

fn check_in_drones(conf: &Config, src:NodeId, conn: NodeId, valid: &mut bool){
    for target in &conf.drone {
        if target.id == conn {
            if target.connected_node_ids.contains(&src) {
                *valid = true;
                break;
            } else {
                println!("Input File invalid for not double connection between: {} - {}",target.id, src);
            }
        }
    }
}

fn check_in_clients(conf: &Config, src:NodeId, conn: NodeId, valid: &mut bool){
    for target in &conf.client {
        if target.id == conn {
            if target.connected_drone_ids.contains(&src) {
                *valid = true;
                break;
            } else {
                println!("Input File invalid for not double connection between: {} - {}",target.id, src);
            }
        }
    }
}

fn check_in_server(conf: &Config, src:NodeId, conn: NodeId, valid: &mut bool){
    for target in &conf.server {
        if target.id == conn {
            if target.connected_drone_ids.contains(&src) {
                *valid = true;
                break;
            } else {
                println!("Input File invalid for not double connection between: {} - {}",target.id, src);
            }
        }
    }
}