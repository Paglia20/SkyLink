use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use crossbeam_channel::{select, unbounded};
use wg_2024::controller::DroneCommand::{Crash, RemoveSender};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::SourceRoutingHeader;
use wg_2024::packet::{Fragment, Packet, PacketType};
use crate::my_drone::SkyLinkDrone;
use crate::sim_control::SimulationControl;

pub fn create_sample_packet() -> Packet {
    Packet {
        pack_type: PacketType::MsgFragment(Fragment {
            fragment_index: 1,
            total_n_fragments: 1,
            length: 128,
            data: [1; 128] ,
        }),
        routing_header: SourceRoutingHeader {
            hop_index: 1,
            hops: vec![0, 1, 2],
        },
        session_id: 1,
    }
}


/// This function is used to test the packet forward functionality of a drone.
#[test]
pub fn generic_fragment_forward() {
    let mut handles = Vec::new();

    let (d0_packet_sender, d0_packet_receiver) = unbounded::<Packet>();
    let (d1_packet_sender, d1_packet_receiver) = unbounded::<Packet>();
    let (d2_packet_sender, d2_packet_receiver) = unbounded::<Packet>();
    let (d3_packet_sender, d3_packet_receiver) = unbounded::<Packet>();


    let (sc_sender, sc_receiver) = unbounded();

    let (d1_command_sender, d1_command_receiver) = unbounded::<DroneCommand>();
    let (d2_command_sender, d2_command_receiver) = unbounded::<DroneCommand>();


    let neighboor_d1 = HashMap::from([(2, d2_packet_sender.clone()), (0, d0_packet_sender.clone())]);
    let neighboor_d2 = HashMap::from([(1, d1_packet_sender.clone()), (3, d3_packet_sender.clone())]);

    let mut drone1 = SkyLinkDrone::new(
        1,
        sc_sender.clone(),
        d1_command_receiver,
        d1_packet_receiver,
        neighboor_d1,
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
                neighboor_d2,
                0.0);

    let d2_handle = thread::spawn(move || {
                drone2.run();
    });

    handles.push(d2_handle);

    // let handle_sc = thread::spawn(move || {
    //     for i in 0..2{
    //         select! {
    //             recv(sc_receiver) -> event => {
    //                 if let Ok(e) = event {
    //                     println!(" event received: {:?}", e);
    //                 }
    //                 else{
    //                     println!("porccaccioddio");
    //                 }
    //             }
    //         }
    //         thread::sleep(Duration::from_millis(100));
    //     }
    // });

    //handles.push(handle_sc);

    // let handle_dst = thread::spawn(move || {
    //     for i in 0..5 {
    //         select! {
    //             recv(d3_packet_receiver) -> packet => {
    //                 if let Ok(p) = packet {
    //                     println!(" packet received: {:?}", p);
    //                 }
    //                 else{
    //                     println!("diolamadonnaindiana");
    //                 }
    //             }
    //         }
    //         thread::sleep(Duration::from_millis(100));
    //     }
    // });
    // handles.push(handle_dst);



    let msg = create_sample_packet();

    match d1_packet_sender.send(msg){
        Ok(_) => {println!("D1 packet sent successfully!")},
        Err(error) => {println!("{}", error)}
    };

    // match d1_command_sender.send(Crash){
    //     Ok(_) => {println!("crash successfully!")},
    //     Err(error) => {println!("{}", error)}
    // };

    // thread::sleep(Duration::from_millis(10000));

    d1_command_sender.send(RemoveSender(2)).unwrap();
    d2_command_sender.send(RemoveSender(1)).unwrap();
    std::mem::drop(d1_packet_sender);
    std::mem::drop(d2_packet_sender);


    d1_command_sender.send(Crash).unwrap();
    d2_command_sender.send(Crash).unwrap();

    for i in handles {
        i.join().unwrap();
    }

    loop {
        break;
        select! {
            recv(d3_packet_receiver) -> packet => {
                if let Ok(p) = packet {
                    println!(" packet received: {:?}", p);
                break;
                }
                else{
                    println!("diolamadonnaindiana");
                break;
                }
            }
        }

    }
}
