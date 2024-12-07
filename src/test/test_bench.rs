use std::collections::{HashMap};
use std::{thread, vec};
use std::thread::JoinHandle;
use crossbeam_channel::{select, select_biased, unbounded, Receiver, Sender};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::controller::DroneCommand::{SetPacketDropRate};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Fragment, Nack, NackType, Packet, PacketType};
use crate::skylink_drone::drone::SkyLinkDrone;
use crate::test::test_initializer::test_initialize;

fn packet_printer(packet: Packet) {
    match packet.pack_type.clone() {
        PacketType::MsgFragment(msg_fragment) => {
            println!("Fragment received:
            source_routing_header: {:?}
            session id: {:?}
            msg_fragment: {:?}\n", packet.routing_header, packet.session_id, msg_fragment.fragment_index);
        },
        PacketType::Ack(ack) => {
            println!("Ack received:
            source_routing_header: {:?}
            session id: {:?}
            ack: {:?}\n", packet.routing_header, packet.session_id, ack);
        },
        PacketType::Nack(nack) => {
            println!("Nack received:
            source_routing_header: {:?}
            session id: {:?}
            nack: {:?}\n", packet.routing_header, packet.session_id, nack);
        },
        PacketType::FloodRequest(flood_request) => {
            println!("Flood request received:
            session id: {:?}
            flood_id: {:?}
            initiator.id: {:?}
            path_trace: {:?}\n", packet.session_id, flood_request.flood_id, flood_request.initiator_id, flood_request.path_trace);
        },
        PacketType::FloodResponse(flood_response) => {
            println!("Flood response received:
            source_routing_header: {:?}
            session id: {:?}
            flood_id: {:?}
            path_trace: {:?}\n", packet.routing_header, packet.session_id, flood_response.flood_id, flood_response.path_trace);

        }
    }
}

fn event_printer(event: DroneEvent) {
    match event {
        DroneEvent::PacketSent(packet) => {
            match packet.pack_type.clone() {
                PacketType::FloodRequest(mut flood_request) => {
                    let prev = flood_request.path_trace.pop().unwrap().0;
                    println!("FloodRequest sent from {}", prev);
                    packet_printer(packet);
                },
                _ => {
                    let index = packet.routing_header.hop_index;
                    let prev = packet.routing_header.hops[index-1];
                    let next = packet.routing_header.hops[index];
                    println!("Packet sent from {} to {}:", prev, next);
                    packet_printer(packet);
                }
            }
        },
        DroneEvent::PacketDropped(packet) => {
            let id = packet.routing_header.hops[0];
            println!("Packet dropped by {}:", id); //Not sure the index is right.
            packet_printer(packet);
        },
        DroneEvent::ControllerShortcut(packet) => {
            println!("Controller Shortcut used by this packet:");
            packet_printer(packet);
        }
    }
}

fn create_packet(hops: Vec<NodeId>) -> Packet {
    Packet {
        pack_type: PacketType::MsgFragment(Fragment {
            fragment_index: 0,
            total_n_fragments: 1,
            length: 128,
            data: [1; 128],
        }),
        routing_header: SourceRoutingHeader {
            hop_index: 1,
            hops,
        },
        session_id: 1,
    }
}

fn send_packet(packet: Packet, sender: &Sender<Packet>) {
    match sender.send(packet){
        Ok(_) => {println!("Packet sent successfully!")},
        Err(error) => {println!("{}", error)}
    };
}

fn listen_handle(sim_recv: Receiver<DroneEvent>, client_receiver: Receiver<Packet>) -> JoinHandle<()>{
    thread::spawn(move || {
        loop {
            select_biased! {
                recv(sim_recv) -> event => {
                    if let Ok(e) = event {
                        event_printer(e);
                    }
                }
                recv(client_receiver) -> packet => {
                    if let Ok(p) = packet {
                        packet_printer(p);
                    }
                }
            }
        }
    })
}

fn client_only_listen_handle(client_receiver: Receiver<Packet>) -> JoinHandle<()>{
    thread::spawn(move || {
        loop {
            select! {
                recv(client_receiver) -> packet => {
                    if let Ok(p) = packet {
                        packet_printer(p);
                    }
                }
            }
        }
    })
}

/// This function is used to test the packet forward functionality of a drone.
pub fn test_generic_fragment_forward() {
    let (sim_contr, clients, mut handles) = test_initialize("inputs/input_generic_fragment_forward.toml");

    let client_receiver = clients.get(1).unwrap().client_recv.clone();
    handles.push(listen_handle(sim_contr.event_recv, client_receiver));

    let msg = create_packet(vec![0,1,2,3]);

    send_packet(msg, clients.get(0).unwrap().client_send.get(&1).unwrap());

    for i in handles {
        i.join().unwrap();
    }
}
//passed

pub fn test_generic_drop(){
    let (_sim_contr, clients, mut handles) = test_initialize("inputs/input_generic_nack.toml");

    let msg = create_packet(vec![1,11,12,21]);

    // "Client 1" sends packet to the drone
    send_packet(msg, clients.get(0).unwrap().client_send.get(&11).unwrap());

    let client_receiver = clients.get(0).unwrap().client_recv.clone();
    // Client receive an NACK originated from 'd2'
    assert_eq!(
        client_receiver.clone().recv().unwrap(),
        Packet {
            pack_type: PacketType::Nack(Nack {
                fragment_index: 0,
                nack_type: NackType::Dropped,
            }),
            routing_header: SourceRoutingHeader {
                hop_index: 2,
                hops: vec![12, 11, 1],
            },
            session_id: 1,
        }
    );

    // handles.push(listen_handle(_sim_contr.event_recv, client_receiver));

    for i in handles {
        i.join().unwrap();
    }
}
//passed

pub fn test_generic_nack(){
    let (sim_contr, clients, mut handles) = test_initialize("inputs/input_generic_nack.toml");

    let msg = create_packet(vec![1,11,21]);

    send_packet(msg, clients.get(0).unwrap().client_send.get(&11).unwrap());

    let client_receiver = clients.get(0).unwrap().client_recv.clone();
    handles.push(listen_handle(sim_contr.event_recv, client_receiver));

    for i in handles {
        i.join().unwrap();
    }
}
//passed

pub fn test_flood(){
    let (_sim_contr, clients, mut handles) = test_initialize("inputs/input_flood.toml");

    let flood_request = wg_2024::packet::FloodRequest{
        flood_id: 1,
        initiator_id: 0,
        path_trace: vec![],
    };
    let flood = PacketType::FloodRequest(flood_request);
    let packet = Packet{
        pack_type: flood,
        routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
        session_id: 0,
    };

    let client_receiver = clients.get(0).unwrap().client_recv.clone();
    // handles.push(listen_handle(_sim_contr.event_recv, client_receiver));
    handles.push(client_only_listen_handle(client_receiver));

    send_packet(packet, clients.get(0).unwrap().client_send.get(&1).unwrap());

    for i in handles {
        i.join().unwrap();
    }
}
//passed

pub fn test_double_chain_flood(){
    let (_sim_contr, clients, mut handles) = test_initialize("inputs/input_double_chain_flood.toml");

    let flood_request = wg_2024::packet::FloodRequest{
        flood_id: 1,
        initiator_id: 0,
        path_trace: vec![],
    };
    let flood = PacketType::FloodRequest(flood_request);
    let packet = Packet{
        pack_type: flood,
        routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
        session_id: 0,
    };

    let client1_receiver = clients.get(0).unwrap().client_recv.clone();
    let client2_receiver = clients.get(1).unwrap().client_recv.clone();
    handles.push(client_only_listen_handle(client1_receiver));
    handles.push(client_only_listen_handle(client2_receiver));
    /*handles.push(thread::spawn(move || {
        loop {
            select! {
                recv(_sim_contr.event_recv) -> event => {
                    if let Ok(e) = event {
                        event_printer(e);
                    }
                }
            }
        }
    }));*/


    send_packet(packet, clients.get(0).unwrap().client_send.get(&1).unwrap());

    for i in handles {
        i.join().unwrap();
    }
    // let discovered_paths = dest_path.lock().unwrap();
    // println!("Are all paths discovered? {:?}", are_path_discovered(&*discovered_paths));
}
//passed

pub fn test_star_flood(){
    let (_sim_contr, clients, mut handles) = test_initialize("inputs/input_star.toml");

    let flood_request = wg_2024::packet::FloodRequest{
        flood_id: 1,
        initiator_id: 0,
        path_trace: vec![],
    };
    let flood = PacketType::FloodRequest(flood_request);
    let packet = Packet{
        pack_type: flood,
        routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
        session_id: 0,
    };

    send_packet(packet, clients.get(0).unwrap().client_send.get(&1).unwrap());


    let client_receiver = clients.get(0).unwrap().client_recv.clone();
    // handles.push(listen_handle(_sim_contr.event_recv, client_receiver));
    handles.push(client_only_listen_handle(client_receiver));


    for i in handles {
        i.join().unwrap();
    }
}
//passed

pub fn test_butterfly_flood(){
    let (_sim_contr, clients, mut handles) = test_initialize("inputs/input_butterfly.toml");

    let flood_request = wg_2024::packet::FloodRequest{
        flood_id: 1,
        initiator_id: 0,
        path_trace: vec![],
    };
    let flood = PacketType::FloodRequest(flood_request);
    let packet = Packet{
        pack_type: flood,
        routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
        session_id: 0,
    };

    send_packet(packet, clients.get(0).unwrap().client_send.get(&1).unwrap());

    let client_receiver = clients.get(0).unwrap().client_recv.clone();
    // handles.push(listen_handle(_sim_contr.event_recv, client_receiver));
    handles.push(client_only_listen_handle(client_receiver));


    for i in handles {
        i.join().unwrap();
    }
}
//passed
pub fn test_tree_flood(){
    let (_sim_contr, clients, mut handles) = test_initialize("inputs/input_tree.toml");

    let flood_request = wg_2024::packet::FloodRequest{
        flood_id: 1,
        initiator_id: 0,
        path_trace: vec![],
    };
    let flood = PacketType::FloodRequest(flood_request);
    let packet = Packet{
        pack_type: flood,
        routing_header: SourceRoutingHeader { hop_index: 0, hops: vec![] },
        session_id: 0,
    };

    send_packet(packet, clients.get(0).unwrap().client_send.get(&1).unwrap());

    let client_receiver = clients.get(0).unwrap().client_recv.clone();
    // handles.push(listen_handle(_sim_contr.event_recv, client_receiver));
    handles.push(client_only_listen_handle(client_receiver));


    for i in handles {
        i.join().unwrap();
    }
}


pub fn test_drone_commands(){
    let mut handles = Vec::new();

    let (d1_packet_sender, d1_packet_receiver) = unbounded::<Packet>();
    let (d2_packet_sender, d2_packet_receiver) = unbounded::<Packet>();


    let (sc_sender, sc_receiver) = unbounded();

    let (d1_command_sender, d1_command_receiver) = unbounded::<DroneCommand>();
    let (d2_command_sender, d2_command_receiver) = unbounded::<DroneCommand>();


    let neighbour_d1 = HashMap::from([(2, d2_packet_sender.clone())]);
    let neighbour_d2 = HashMap::from([(1, d1_packet_sender.clone())]);

    let mut drone1 = SkyLinkDrone::new(
        1,
        sc_sender.clone(),
        d1_command_receiver,
        d1_packet_receiver,
        neighbour_d1,
        0.0);

    let d1_handle = thread::spawn(move || {
        drone1.run();
    });
    handles.push(d1_handle);

    let mut drone2 = SkyLinkDrone::new(
        2,
        sc_sender.clone(),
        d2_command_receiver,
        d2_packet_receiver,
        neighbour_d2,
        0.0);

    let d2_handle = thread::spawn(move || {
        drone2.run();
    });
    handles.push(d2_handle);

    let handle_sc = thread::spawn(move || {
        loop {
            select! {
                recv(sc_receiver) -> event => {
                    if let Ok(e) = event {
                        event_printer(e);
                    }
                }
            }
        }
    });
    handles.push(handle_sc);

    // d1_command_sender.send(Crash).unwrap();
    // d2_command_sender.send(Crash).unwrap();
    d1_command_sender.send(SetPacketDropRate(0.5)).unwrap();
    d2_command_sender.send(SetPacketDropRate(0.8)).unwrap();

    let msg = create_packet(vec![0,1,2]);

    d1_packet_sender.send(msg).unwrap();


    // d1_command_sender.send(RemoveSender(2)).unwrap();
    // d2_command_sender.send(RemoveSender(1)).unwrap();
    // //
    // drop(d1_packet_sender);
    // drop(d2_packet_sender);



    for i in handles {
        i.join().unwrap();
    }
}

//Use star configuration and test busy network with a full route around the configuration, sending u64::max messages.
pub fn test_busy_network(){
    let (_sim_contr, clients, mut handles) = test_initialize("inputs/input_star.toml");

    let packet = create_packet(vec![0,1,4,7,10,3,6,9,2,5,8,1,0]);

    for _i in 0..u8::MAX {
        for (_, s) in &clients.get(0).unwrap().client_send {
            if let Ok(_) = s.send(packet.clone()) {
                // println!("Packet {:?} sent successfully!", packet);
            } else {
                println!("Doesn't work");
            }
        }
    }

    let client_receiver = clients.get(0).unwrap().client_recv.clone();
    // handles.push(listen_handle(_sim_contr.event_recv, client_receiver));
    handles.push(client_only_listen_handle(client_receiver));


    for i in handles {
        i.join().unwrap();
    }
}
