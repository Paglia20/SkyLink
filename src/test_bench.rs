use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use crossbeam_channel::{select, unbounded};
use wg_2024::controller::DroneCommand::{Crash, RemoveSender};
use wg_2024::controller::{DroneCommand};
use wg_2024::drone::Drone;
use wg_2024::network::SourceRoutingHeader;
use wg_2024::packet::{Fragment, Packet, PacketType};
use crate::my_drone::SkyLinkDrone;

fn my_create_sample_packet() -> Packet {
    Packet {
        pack_type: PacketType::MsgFragment(Fragment {
            fragment_index: 1,
            total_n_fragments: 1,
            length: 128,
            data: [1; 128] ,
        }),
        routing_header: SourceRoutingHeader {
            hop_index: 1,
            hops: vec![0,1,2,3],
        },
        session_id: 1,
    }
}


/// This function is used to test the packet forward functionality of a drone.
pub fn my_generic_fragment_forward() {
    let mut handles = Vec::new();

    let (d0_packet_sender, _d0_packet_receiver) = unbounded::<Packet>();
    let (d1_packet_sender, d1_packet_receiver) = unbounded::<Packet>();
    let (d2_packet_sender, d2_packet_receiver) = unbounded::<Packet>();
    let (d3_packet_sender, d3_packet_receiver) = unbounded::<Packet>();


    let (sc_sender, sc_receiver) = unbounded();

    let (d1_command_sender, d1_command_receiver) = unbounded::<DroneCommand>();
    let (d2_command_sender, d2_command_receiver) = unbounded::<DroneCommand>();


    let neighbour_d1 = HashMap::from([(2, d2_packet_sender.clone()), (0, d0_packet_sender.clone())]);
    let neighbour_d2 = HashMap::from([(1, d1_packet_sender.clone()), (3, d3_packet_sender.clone())]);

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
                        println!("\nEvent received: {:?}", e);
                    }
                }
            }
        }
    });
    handles.push(handle_sc);

    let handle_dst = thread::spawn(move || {
         loop {
            select! {
                recv(d3_packet_receiver) -> packet => {
                    if let Ok(p) = packet {
                        println!("\nPacket received: {:?}", p);
                    }
                }
            }
        }
    });
    handles.push(handle_dst);


    let msg = my_create_sample_packet();

    match d1_packet_sender.send(msg){
        Ok(_) => {println!("D1 packet sent successfully!")},
        Err(error) => {println!("{}", error)}
    };
    thread::sleep(Duration::from_millis(3000));

    d1_command_sender.send(RemoveSender(2)).unwrap();
    d2_command_sender.send(RemoveSender(1)).unwrap();
    drop(d1_packet_sender);
    drop(d2_packet_sender);

    d1_command_sender.send(Crash).unwrap();
    d2_command_sender.send(Crash).unwrap();

    /*
    loop {
        select! {
            recv(d0_packet_receiver) -> packet => {
                if let Ok(p) = packet {
                    println!("d0 packet received: {:?}", p);
                break;
                }
                else{
                    println!("diolamadonnaindiana");
                break;
                }
            }

            recv(d3_packet_receiver) -> packet => {
                if let Ok(p) = packet {
                    println!("d3 packet received: {:?}", p);
                break;
                }
                else{
                    println!("diolamadonnaindiana");
                break;
                }
            }
             default(Duration::from_secs(5)) => {
            println!("Timeout: No packets received within 5 seconds.");
            break;
        }
        }

    }
    d1_command_sender.send(RemoveSender(2)).unwrap();
    d2_command_sender.send(RemoveSender(1)).unwrap();
    std::mem::drop(d1_packet_sender);
    std::mem::drop(d2_packet_sender);


    d1_command_sender.send(Crash).unwrap();
    d2_command_sender.send(Crash).unwrap();*/

    for i in handles {
        i.join().unwrap();
    }


}



/*
NOTES

- By using #test, it won't print until all the threads finish.

- Q: maybe you should drop channels also in sim control
- A: Yea probably

*/