use std::collections::HashMap;
use std::thread;
use std::time::Duration;
use crossbeam_channel::{select, unbounded};
use wg_2024::controller::DroneCommand::{Crash, RemoveSender};
use wg_2024::controller::{DroneCommand};
use wg_2024::drone::Drone;
use wg_2024::network::SourceRoutingHeader;
use wg_2024::packet::{Fragment, Packet, PacketType};

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


    for i in handles {
        i.join().unwrap();
    }
}

pub fn test_generic(){
    // Client 1 channels
    let (c_send, c_recv) = unbounded();
    // Server 21 channels
    let (s_send, _s_recv) = unbounded();
    // Drone 11
    let (d_send, d_recv) = unbounded();
    // Drone 12
    let (d12_send, d12_recv) = unbounded();
    // SC - needed to not make the drone crash
    let (_d_command_send, d_command_recv) = unbounded();

    // Drone 11
    let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
    let mut drone = SkyLinkDrone::new(
        11,
        unbounded().0,
        d_command_recv.clone(),
        d_recv.clone(),
        neighbours11,
        0.0,
    );
    // Drone 12
    let neighbours12 = HashMap::from([(11, d_send.clone()), (21, s_send.clone())]);
    let mut drone2 = SkyLinkDrone::new(
        12,
        unbounded().0,
        d_command_recv.clone(),
        d12_recv.clone(),
        neighbours12,
        1.0,
    );

    // Spawn the drone's run method in a separate thread
    thread::spawn(move || {
        drone.run();
    });

    thread::spawn(move || {
        drone2.run();
    });

    let msg = create_sample_packet();

    // "Client" sends packet to the drone
    d_send.send(msg.clone()).unwrap();

    // Client receive an ACK originated from 'd'
    // assert_eq!(
    //     c_recv.recv().unwrap(),
    //     Packet {
    //         pack_type: PacketType::Ack(Ack { fragment_index: 1 }),
    //         routing_header: SourceRoutingHeader {
    //             hop_index: 1,
    //             hops: vec![11, 1],
    //         },
    //         session_id: 1,
    //     }
    // );

    // Client receive an NACK originated from 'd2'
    assert_eq!(
        c_recv.recv().unwrap(),
        Packet {
            pack_type: PacketType::Nack(Nack {
                fragment_index: 1,
                nack_type: NackType::Dropped,
            }),
            routing_header: SourceRoutingHeader {
                hop_index: 2,
                hops: vec![12, 11, 1],
            },
            session_id: 1,
        }
    );
}

fn create_sample_packet() -> Packet {
    Packet {
        pack_type: PacketType::MsgFragment(Fragment {
            fragment_index: 1,
            total_n_fragments: 1,
            length: 128,
            data: [1; 128],
        }),
        routing_header: SourceRoutingHeader {
            hop_index: 1,
            hops: vec![1, 11, 12, 21],
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