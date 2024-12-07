use std::collections::{HashMap, HashSet};
use std::{thread, vec};
use std::time::Duration;
use crossbeam_channel::{select, select_biased, unbounded};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Fragment, Nack, NackType, NodeType, Packet, PacketType, Ack};
use crate::skylink_drone::drone::SkyLinkDrone;
use crate::test::test_initializer::test_initialize;

/// This function is used to test the packet forward functionality of a drone.
pub fn test_generic_fragment_forward() {
    let (sim_contr, clients, mut handles) = test_initialize("input_generic_fragment_forward.toml");

    let client_receiver = clients.get(1).unwrap().client_recv.clone();
    let handle = thread::spawn(move || {
        loop {
            select_biased! {
                recv(sim_contr.event_recv) -> event => {
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
    });
    handles.push(handle);

    let msg = create_packet(vec![0,1,2,3]);

    match clients.get(0).unwrap().client_send.get(&1).unwrap().send(msg){
        Ok(_) => {println!("Packet sent successfully!")},
        Err(error) => {println!("{}", error)}
    };

    for i in handles {
        i.join().unwrap();
    }
}

pub fn test_generic_drop(){
    let (sim_contr, clients, mut handles) = test_initialize("input_generic_nack.toml");

    let msg = create_packet(vec![1,11,12,21]);

    // "Client 1" sends packet to the drone
    match clients.get(0).unwrap().client_send.get(&11).unwrap().send(msg){
        Ok(_) => {println!("Packet sent successfully!")},
        Err(error) => {println!("{}", error)}
    };

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

    /*let handle = thread::spawn(move || {
        loop {
            select_biased! {
                recv(sim_contr.event_recv) -> event => {
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
    });
    handles.push(handle);*/

    for i in handles {
        i.join().unwrap();
    }
}

pub fn test_flood(){
    let mut handles = Vec::new();

    let (cl_flood_sender, cl_flood_receiver) = unbounded::<Packet>();
    let (d1_flood_sender, d1_flood_receiver) = unbounded::<Packet>();
    let (d2_flood_sender, d2_flood_receiver) = unbounded::<Packet>();
    let (d3_flood_sender, d3_flood_receiver) = unbounded::<Packet>();


    let (sc_sender, sc_receiver) = unbounded();
    let (d1_command_sender, d1_command_receiver) = unbounded::<DroneCommand>();
    let (d2_command_sender, d2_command_receiver) = unbounded::<DroneCommand>();
    let (d3_command_sender, d3_command_receiver) = unbounded::<DroneCommand>();

    let neighbour_d1 = HashMap::from([(2, d2_flood_sender.clone()), (0, cl_flood_sender.clone())]);
    let neighbour_d2 = HashMap::from([(1, d1_flood_sender.clone()), (3, d3_flood_sender.clone())]);
    let neighbour_d3 = HashMap::from([(2, d2_flood_sender.clone())]);

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

    let mut drone1 = SkyLinkDrone::new(
        1,
        sc_sender.clone(),
        d1_command_receiver,
        d1_flood_receiver,
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
        d2_flood_receiver,
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

    let mut drone3 = SkyLinkDrone::new(
        3,
        sc_sender.clone(),
        d3_command_receiver,
        d3_flood_receiver,
        neighbour_d3,
        0.0);

    let d3_handle = thread::spawn(move || {
        drone3.run();
    });
    handles.push(d3_handle);

    let handle_dst = thread::spawn(move || {
        loop {
            select! {
                recv(cl_flood_receiver) -> packet => {
                    if let Ok(p) = packet {
                        packet_printer(p);
                    }
                }
            }
        }
    });
    handles.push(handle_dst);



    match d1_flood_sender.send(packet){
        Ok(_) => {println!("D1 flood sent successfully!")},
        Err(error) => {println!("{}", error)}
    };
     thread::sleep(Duration::from_millis(3000));


    for i in handles {
        i.join().unwrap();
    }
}

//passed
pub fn test_double_chain_flood(){
    let mut handles = Vec::new();

    let (cl_flood_sender, cl_flood_receiver) = unbounded::<Packet>();
    let (d1_flood_sender, d1_flood_receiver) = unbounded::<Packet>();
    let (d2_flood_sender, d2_flood_receiver) = unbounded::<Packet>();
    let (d3_flood_sender, d3_flood_receiver) = unbounded::<Packet>();
    let (d4_flood_sender, d4_flood_receiver) = unbounded::<Packet>();
    let (d5_flood_sender, d5_flood_receiver) = unbounded::<Packet>();
    let (d6_flood_sender, d6_flood_receiver) = unbounded::<Packet>();
    let (d7_flood_sender, d7_flood_receiver) = unbounded::<Packet>();
    let (d8_flood_sender, d8_flood_receiver) = unbounded::<Packet>();
    let (d9_flood_sender, d9_flood_receiver) = unbounded::<Packet>();
    let (d10_flood_sender, d10_flood_receiver) = unbounded::<Packet>();
    let (dest_flood_sender, dest_flood_receiver) = unbounded::<Packet>();


    let (sc_sender, sc_receiver) = unbounded();
    let (d1_command_sender, d1_command_receiver) = unbounded::<DroneCommand>();
    let (d2_command_sender, d2_command_receiver) = unbounded::<DroneCommand>();
    let (d3_command_sender, d3_command_receiver) = unbounded::<DroneCommand>();
    let (d4_command_sender, d4_command_receiver) = unbounded::<DroneCommand>();
    let (d5_command_sender, d5_command_receiver) = unbounded::<DroneCommand>();
    let (d6_command_sender, d6_command_receiver) = unbounded::<DroneCommand>();
    let (d7_command_sender, d7_command_receiver) = unbounded::<DroneCommand>();
    let (d8_command_sender, d8_command_receiver) = unbounded::<DroneCommand>();
    let (d9_command_sender, d9_command_receiver) = unbounded::<DroneCommand>();
    let (d10_command_sender, d10_command_receiver) = unbounded::<DroneCommand>();

    let neighbour_d1 = HashMap::from([(2, d2_flood_sender.clone()), (0, cl_flood_sender.clone()), (6, d6_flood_sender.clone())]);
    let neighbour_d2 = HashMap::from([(1, d1_flood_sender.clone()), (3, d3_flood_sender.clone()), (7, d7_flood_sender.clone())]);
    let neighbour_d3 = HashMap::from([(2, d2_flood_sender.clone()), (4, d4_flood_sender.clone()), (8, d8_flood_sender.clone())]);
    let neighbour_d4 = HashMap::from([(3, d3_flood_sender.clone()), (5, d5_flood_sender.clone()), (9, d9_flood_sender.clone())]);
    let neighbour_d5 = HashMap::from([(4, d4_flood_sender.clone()), (10, d10_flood_sender.clone())]);

    let neighbour_d6 = HashMap::from([(1, d1_flood_sender.clone()), (7, d7_flood_sender.clone())]);
    let neighbour_d7 = HashMap::from([(6, d6_flood_sender.clone()), (8, d8_flood_sender.clone()), (2, d2_flood_sender.clone())]);
    let neighbour_d8 = HashMap::from([(7, d7_flood_sender.clone()), (9, d9_flood_sender.clone()), (3, d3_flood_sender.clone())]);
    let neighbour_d9 = HashMap::from([(8, d8_flood_sender.clone()), (10, d10_flood_sender.clone()), (4, d4_flood_sender.clone())]);
    let neighbour_d10 = HashMap::from([(9, d9_flood_sender.clone()), (5, d5_flood_sender.clone()), (11, dest_flood_sender.clone())]);


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

    let mut drone1 = SkyLinkDrone::new(
        1,
        sc_sender.clone(),
        d1_command_receiver,
        d1_flood_receiver,
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
        d2_flood_receiver,
        neighbour_d2,
        0.0);

    let d2_handle = thread::spawn(move || {
        drone2.run();
    });
    handles.push(d2_handle);

    let mut drone3 = SkyLinkDrone::new(
        3,
        sc_sender.clone(),
        d3_command_receiver,
        d3_flood_receiver,
        neighbour_d3,
        0.0);

    let d3_handle = thread::spawn(move || {
        drone3.run();
    });
    handles.push(d3_handle);

    let mut drone4 = SkyLinkDrone::new(
        4,
        sc_sender.clone(),
        d4_command_receiver,
        d4_flood_receiver,
        neighbour_d4,
        0.0);

    let d4_handle = thread::spawn(move || {
        drone4.run();
    });
    handles.push(d4_handle);

    let mut drone5 = SkyLinkDrone::new(
        5,
        sc_sender.clone(),
        d5_command_receiver,
        d5_flood_receiver,
        neighbour_d5,
        0.0);

    let d5_handle = thread::spawn(move || {
        drone5.run();
    });
    handles.push(d5_handle);

    let mut drone6 = SkyLinkDrone::new(
        6,
        sc_sender.clone(),
        d6_command_receiver,
        d6_flood_receiver,
        neighbour_d6,
        0.0);

    let d6_handle = thread::spawn(move || {
        drone6.run();
    });
    handles.push(d6_handle);

    let mut drone7 = SkyLinkDrone::new(
        7,
        sc_sender.clone(),
        d7_command_receiver,
        d7_flood_receiver,
        neighbour_d7,
        0.0);

    let d7_handle = thread::spawn(move || {
        drone7.run();
    });
    handles.push(d7_handle);

    let mut drone8 = SkyLinkDrone::new(
        8,
        sc_sender.clone(),
        d8_command_receiver,
        d8_flood_receiver,
        neighbour_d8,
        0.0);

    let d8_handle = thread::spawn(move || {
        drone8.run();
    });
    handles.push(d8_handle);

    let mut drone9 = SkyLinkDrone::new(
        9,
        sc_sender.clone(),
        d9_command_receiver,
        d9_flood_receiver,
        neighbour_d9,
        0.0);

    let d9_handle = thread::spawn(move || {
        drone9.run();
    });
    handles.push(d9_handle);

    let mut drone10 = SkyLinkDrone::new(
        10,
        sc_sender.clone(),
        d10_command_receiver,
        d10_flood_receiver,
        neighbour_d10,
        0.0);

    let d10_handle = thread::spawn(move || {
        drone10.run();
    });
    handles.push(d10_handle);

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



    // let dest_path = Arc::new(Mutex::new(Vec::new()));
    // let dest_path_clone = Arc::clone(&dest_path);

    let handle_dst = thread::spawn(move || {
        loop {
            select! {
                recv(cl_flood_receiver) -> packet => {
                    if let Ok(p) = packet {
                        packet_printer(p);
                        // println!("\n client flood response received: {:?}", p);
                        // let path = match p.pack_type {
                        //     PacketType::FloodResponse(flood) => flood.path_trace,
                        //     _ => Vec::new(),
                        // };
                        // let mut dp = dest_path_clone.lock().unwrap();
                        // dp.push(path);
                    }
                }
                recv(dest_flood_receiver) -> packet => {
                    if let Ok(p) = packet {
                        packet_printer(p);
                        //should simulate a server, but since is only a channel it doesn't produce a float response.
                    }
                }
            }
        }
    });
    handles.push(handle_dst);



    match d1_flood_sender.send(packet){
        Ok(_) => {println!("D1 flood sent successfully!")},
        Err(error) => {println!("{}", error)}
    };
    // thread::sleep(Duration::from_millis(3000));

    for i in handles {
        i.join().unwrap();
    }
    // let discovered_paths = dest_path.lock().unwrap();
    // println!("Are all paths discovered? {:?}", are_path_discovered(&*discovered_paths));


}

//passed
pub fn test_star_flood(){
    let mut handles = Vec::new();

    let (cl_flood_sender, cl_flood_receiver) = unbounded::<Packet>();
    let (d1_flood_sender, d1_flood_receiver) = unbounded::<Packet>();
    let (d2_flood_sender, d2_flood_receiver) = unbounded::<Packet>();
    let (d3_flood_sender, d3_flood_receiver) = unbounded::<Packet>();
    let (d4_flood_sender, d4_flood_receiver) = unbounded::<Packet>();
    let (d5_flood_sender, d5_flood_receiver) = unbounded::<Packet>();
    let (d6_flood_sender, d6_flood_receiver) = unbounded::<Packet>();
    let (d7_flood_sender, d7_flood_receiver) = unbounded::<Packet>();
    let (d8_flood_sender, d8_flood_receiver) = unbounded::<Packet>();
    let (d9_flood_sender, d9_flood_receiver) = unbounded::<Packet>();
    let (d10_flood_sender, d10_flood_receiver) = unbounded::<Packet>();
    // let (dest_flood_sender, dest_flood_receiver) = unbounded::<Packet>();


    let (sc_sender, sc_receiver) = unbounded();
    let (d1_command_sender, d1_command_receiver) = unbounded::<DroneCommand>();
    let (d2_command_sender, d2_command_receiver) = unbounded::<DroneCommand>();
    let (d3_command_sender, d3_command_receiver) = unbounded::<DroneCommand>();
    let (d4_command_sender, d4_command_receiver) = unbounded::<DroneCommand>();
    let (d5_command_sender, d5_command_receiver) = unbounded::<DroneCommand>();
    let (d6_command_sender, d6_command_receiver) = unbounded::<DroneCommand>();
    let (d7_command_sender, d7_command_receiver) = unbounded::<DroneCommand>();
    let (d8_command_sender, d8_command_receiver) = unbounded::<DroneCommand>();
    let (d9_command_sender, d9_command_receiver) = unbounded::<DroneCommand>();
    let (d10_command_sender, d10_command_receiver) = unbounded::<DroneCommand>();

    let neighbour_d1 = HashMap::from([(4, d4_flood_sender.clone()), (0, cl_flood_sender.clone()), (8, d8_flood_sender.clone())]);
    let neighbour_d2 = HashMap::from([(5, d5_flood_sender.clone()), (9, d9_flood_sender.clone())]);
    let neighbour_d3 = HashMap::from([(6, d6_flood_sender.clone()), (10, d10_flood_sender.clone())]);
    let neighbour_d4 = HashMap::from([(7, d7_flood_sender.clone()), (1, d1_flood_sender.clone())]);
    let neighbour_d5 = HashMap::from([(2, d2_flood_sender.clone()), (8, d8_flood_sender.clone())]);

    let neighbour_d6 = HashMap::from([(3, d3_flood_sender.clone()), (9, d9_flood_sender.clone())]);
    let neighbour_d7 = HashMap::from([(4, d4_flood_sender.clone()), (10, d10_flood_sender.clone()) ]);
    let neighbour_d8 = HashMap::from([(5, d5_flood_sender.clone()), (1, d1_flood_sender.clone()) ]);
    let neighbour_d9 = HashMap::from([(6, d6_flood_sender.clone()), (2, d2_flood_sender.clone())]);
    let neighbour_d10 = HashMap::from([(3, d3_flood_sender.clone()), (7, d7_flood_sender.clone())]);



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

    let mut drone1 = SkyLinkDrone::new(
        1,
        sc_sender.clone(),
        d1_command_receiver,
        d1_flood_receiver,
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
        d2_flood_receiver,
        neighbour_d2,
        0.0);

    let d2_handle = thread::spawn(move || {
        drone2.run();
    });
    handles.push(d2_handle);

    let mut drone3 = SkyLinkDrone::new(
        3,
        sc_sender.clone(),
        d3_command_receiver,
        d3_flood_receiver,
        neighbour_d3,
        0.0);

    let d3_handle = thread::spawn(move || {
        drone3.run();
    });
    handles.push(d3_handle);

    let mut drone4 = SkyLinkDrone::new(
        4,
        sc_sender.clone(),
        d4_command_receiver,
        d4_flood_receiver,
        neighbour_d4,
        0.0);

    let d4_handle = thread::spawn(move || {
        drone4.run();
    });
    handles.push(d4_handle);

    let mut drone5 = SkyLinkDrone::new(
        5,
        sc_sender.clone(),
        d5_command_receiver,
        d5_flood_receiver,
        neighbour_d5,
        0.0);

    let d5_handle = thread::spawn(move || {
        drone5.run();
    });
    handles.push(d5_handle);

    let mut drone6 = SkyLinkDrone::new(
        6,
        sc_sender.clone(),
        d6_command_receiver,
        d6_flood_receiver,
        neighbour_d6,
        0.0);

    let d6_handle = thread::spawn(move || {
        drone6.run();
    });
    handles.push(d6_handle);

    let mut drone7 = SkyLinkDrone::new(
        7,
        sc_sender.clone(),
        d7_command_receiver,
        d7_flood_receiver,
        neighbour_d7,
        0.0);

    let d7_handle = thread::spawn(move || {
        drone7.run();
    });
    handles.push(d7_handle);

    let mut drone8 = SkyLinkDrone::new(
        8,
        sc_sender.clone(),
        d8_command_receiver,
        d8_flood_receiver,
        neighbour_d8,
        0.0);

    let d8_handle = thread::spawn(move || {
        drone8.run();
    });
    handles.push(d8_handle);

    let mut drone9 = SkyLinkDrone::new(
        9,
        sc_sender.clone(),
        d9_command_receiver,
        d9_flood_receiver,
        neighbour_d9,
        0.0);

    let d9_handle = thread::spawn(move || {
        drone9.run();
    });
    handles.push(d9_handle);

    let mut drone10 = SkyLinkDrone::new(
        10,
        sc_sender.clone(),
        d10_command_receiver,
        d10_flood_receiver,
        neighbour_d10,
        0.0);

    let d10_handle = thread::spawn(move || {
        drone10.run();
    });
    handles.push(d10_handle);

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


    let handle_dst = thread::spawn(move || {
        loop {
            select! {
                recv(cl_flood_receiver) -> packet => {
                    if let Ok(p) = packet {
                        packet_printer(p);
                    }
                }
            }
        }
    });
    handles.push(handle_dst);



    match d1_flood_sender.send(packet){
        Ok(_) => {println!("D1 flood sent successfully!")},
        Err(error) => {println!("{}", error)}
    };



    for i in handles {
        i.join().unwrap();
    }
}

//check it, don't work
pub fn test_butterfly_flood(){
    let mut handles = Vec::new();

    let (cl_flood_sender, cl_flood_receiver) = unbounded::<Packet>();
    let (d1_flood_sender, d1_flood_receiver) = unbounded::<Packet>();
    let (d2_flood_sender, d2_flood_receiver) = unbounded::<Packet>();
    let (d3_flood_sender, d3_flood_receiver) = unbounded::<Packet>();
    let (d4_flood_sender, d4_flood_receiver) = unbounded::<Packet>();
    let (d5_flood_sender, d5_flood_receiver) = unbounded::<Packet>();
    let (d6_flood_sender, d6_flood_receiver) = unbounded::<Packet>();
    let (d7_flood_sender, d7_flood_receiver) = unbounded::<Packet>();
    let (d8_flood_sender, d8_flood_receiver) = unbounded::<Packet>();
    let (d9_flood_sender, d9_flood_receiver) = unbounded::<Packet>();
    let (d10_flood_sender, d10_flood_receiver) = unbounded::<Packet>();
    // let (dest_flood_sender, dest_flood_receiver) = unbounded::<Packet>();


    let (sc_sender, sc_receiver) = unbounded();
    let (d1_command_sender, d1_command_receiver) = unbounded::<DroneCommand>();
    let (d2_command_sender, d2_command_receiver) = unbounded::<DroneCommand>();
    let (d3_command_sender, d3_command_receiver) = unbounded::<DroneCommand>();
    let (d4_command_sender, d4_command_receiver) = unbounded::<DroneCommand>();
    let (d5_command_sender, d5_command_receiver) = unbounded::<DroneCommand>();
    let (d6_command_sender, d6_command_receiver) = unbounded::<DroneCommand>();
    let (d7_command_sender, d7_command_receiver) = unbounded::<DroneCommand>();
    let (d8_command_sender, d8_command_receiver) = unbounded::<DroneCommand>();
    let (d9_command_sender, d9_command_receiver) = unbounded::<DroneCommand>();
    let (d10_command_sender, d10_command_receiver) = unbounded::<DroneCommand>();

    let neighbour_d1 = HashMap::from([(0, cl_flood_sender.clone()), (5, d5_flood_sender.clone()),(6, d6_flood_sender.clone())]);
    let neighbour_d2 = HashMap::from([(5, d5_flood_sender.clone()), (6, d6_flood_sender.clone())]);
    let neighbour_d3 = HashMap::from([(7, d7_flood_sender.clone()), (8, d8_flood_sender.clone())]);
    let neighbour_d4 = HashMap::from([(7, d7_flood_sender.clone()), (8, d8_flood_sender.clone())]);
    let neighbour_d5 = HashMap::from([(9, d9_flood_sender.clone()), (1, d1_flood_sender.clone()), (2, d2_flood_sender.clone())]);

    let neighbour_d6 = HashMap::from([(10, d10_flood_sender.clone()), (1, d1_flood_sender.clone()), (2, d2_flood_sender.clone())]);
    let neighbour_d7 = HashMap::from([(4, d4_flood_sender.clone()),(9, d9_flood_sender.clone()), (3, d3_flood_sender.clone()) ]);
    let neighbour_d8 = HashMap::from([(4, d4_flood_sender.clone()), (10, d10_flood_sender.clone()), (3, d3_flood_sender.clone())]);
    let neighbour_d9 = HashMap::from([(10, d10_flood_sender.clone()), (7, d7_flood_sender.clone()), (5, d5_flood_sender.clone())]);
    let neighbour_d10 = HashMap::from([(8, d8_flood_sender.clone()), (6, d6_flood_sender.clone()), (9, d9_flood_sender.clone())]);



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

    let mut drone1 = SkyLinkDrone::new(
        1,
        sc_sender.clone(),
        d1_command_receiver,
        d1_flood_receiver,
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
        d2_flood_receiver,
        neighbour_d2,
        0.0);

    let d2_handle = thread::spawn(move || {
        drone2.run();
    });
    handles.push(d2_handle);

    let mut drone3 = SkyLinkDrone::new(
        3,
        sc_sender.clone(),
        d3_command_receiver,
        d3_flood_receiver,
        neighbour_d3,
        0.0);

    let d3_handle = thread::spawn(move || {
        drone3.run();
    });
    handles.push(d3_handle);

    let mut drone4 = SkyLinkDrone::new(
        4,
        sc_sender.clone(),
        d4_command_receiver,
        d4_flood_receiver,
        neighbour_d4,
        0.0);

    let d4_handle = thread::spawn(move || {
        drone4.run();
    });
    handles.push(d4_handle);

    let mut drone5 = SkyLinkDrone::new(
        5,
        sc_sender.clone(),
        d5_command_receiver,
        d5_flood_receiver,
        neighbour_d5,
        0.0);

    let d5_handle = thread::spawn(move || {
        drone5.run();
    });
    handles.push(d5_handle);

    let mut drone6 = SkyLinkDrone::new(
        6,
        sc_sender.clone(),
        d6_command_receiver,
        d6_flood_receiver,
        neighbour_d6,
        0.0);

    let d6_handle = thread::spawn(move || {
        drone6.run();
    });
    handles.push(d6_handle);

    let mut drone7 = SkyLinkDrone::new(
        7,
        sc_sender.clone(),
        d7_command_receiver,
        d7_flood_receiver,
        neighbour_d7,
        0.0);

    let d7_handle = thread::spawn(move || {
        drone7.run();
    });
    handles.push(d7_handle);

    let mut drone8 = SkyLinkDrone::new(
        8,
        sc_sender.clone(),
        d8_command_receiver,
        d8_flood_receiver,
        neighbour_d8,
        0.0);

    let d8_handle = thread::spawn(move || {
        drone8.run();
    });
    handles.push(d8_handle);

    let mut drone9 = SkyLinkDrone::new(
        9,
        sc_sender.clone(),
        d9_command_receiver,
        d9_flood_receiver,
        neighbour_d9,
        0.0);

    let d9_handle = thread::spawn(move || {
        drone9.run();
    });
    handles.push(d9_handle);

    let mut drone10 = SkyLinkDrone::new(
        10,
        sc_sender.clone(),
        d10_command_receiver,
        d10_flood_receiver,
        neighbour_d10,
        0.0);

    let d10_handle = thread::spawn(move || {
        drone10.run();
    });
    handles.push(d10_handle);

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


    let handle_dst = thread::spawn(move || {
        loop {
            select! {
                recv(cl_flood_receiver) -> packet => {
                    if let Ok(p) = packet {
                        packet_printer(p);
                    }
                }
            }
        }
    });
    handles.push(handle_dst);



    match d1_flood_sender.send(packet){
        Ok(_) => {println!("D1 flood sent successfully!")},
        Err(error) => {println!("{}", error)}
    };



    for i in handles {
        i.join().unwrap();
    }
}

pub fn test_tree_flood(){
    let (sim_contr, client, mut handles) = test_initialize("input_tree.toml");

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

    for (_, s) in &client.get(0).unwrap().client_send {
        if let Ok(_) = s.send(packet.clone()) {
            println!("Packet {:?} sent successfully!", packet);
        } else {
            println!("Doesn't work");
        }
    }

    let handle_dst = thread::spawn(move || {
        loop {
            select! {
                recv(client.get(1).unwrap().client_recv) -> packet => {
                    if let Ok(p) = packet {
                        packet_printer(p);
                    }
                }
            }
        }
    });
    handles.push(handle_dst);
    let handle_sc = thread::spawn(move || {
        loop {
            select! {
                recv(sim_contr.event_recv) -> packet => {
                    if let Ok(e) = packet {
                        event_printer(e);
                    }
                }
            }
        }
    });
    handles.push(handle_sc);


    for i in handles {
        i.join().unwrap();
    }
}


//this function should return true if every node is discovered (in this examples 1->10), but you have to use arc and mutex while threads are still on, so not working YET
pub fn are_path_discovered(dest_path: &Vec<Vec<(NodeId, NodeType)>>) -> bool {

    let mut discovered = HashSet::new();

    for path in dest_path {
        for (node_id, _node_type) in path {
            discovered.insert(node_id);
        }
    }
    (1..=10).all(|num| discovered.contains(&num))
}

fn packet_printer(packet: Packet) {
    match packet.pack_type.clone() {
        PacketType::MsgFragment(msg_fragment) => {
            println!("Fragment received:
            source_routing_header: {:?}
            session id: {:?}
            msg_fragment: {:?}", packet.routing_header, packet.session_id, msg_fragment.fragment_index);
        },
        PacketType::Ack(ack) => {
            println!("Ack received:
            source_routing_header: {:?}
            session id: {:?}
            ack: {:?}", packet.routing_header, packet.session_id, ack);
        },
        PacketType::Nack(nack) => {
            println!("Nack received:
            source_routing_header: {:?}
            session id: {:?}
            nack: {:?}", packet.routing_header, packet.session_id, nack);
        },
        PacketType::FloodRequest(flood_request) => {
            println!("Flood request received:
            session id: {:?}
            flood_id: {:?}
            initiator.id: {:?}
            path_trace: {:?}", packet.session_id, flood_request.flood_id, flood_request.initiator_id, flood_request.path_trace);
        },
        PacketType::FloodResponse(flood_response) => {
            println!("Flood response received:
            source_routing_header: {:?}
            session id: {:?}
            flood_id: {:?}
            path_trace: {:?}", packet.routing_header, packet.session_id, flood_response.flood_id, flood_response.path_trace);
        }
    }
}

fn event_printer(event: DroneEvent) {
    match event {
        DroneEvent::PacketSent(packet) => {
            let index = packet.routing_header.hop_index;
            let prev = packet.routing_header.hops[index-1];
            let next = packet.routing_header.hops[index];
            println!("Packet sent from {} to {}:", prev, next);
            packet_printer(packet);
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

/*
NOTES

- By using #test, it won't print until all the threads finish.

- Q: maybe you should drop channels also in sim control
- A: Yea probably

*/