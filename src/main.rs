use std::thread;
use std::time::Duration;
use crate::initializer::initialize;
use crate::test_bench::my_generic_fragment_forward;
use wg_2024::tests::*;
use crate::my_drone::SkyLinkDrone;
use crate::tests::test_generic;

mod my_drone;
mod sim_app;
mod sim_control;
mod initializer;
mod test_bench;

fn main() {
    println!("Hello, world!");


    thread::spawn(move || {
        sim_app::run_simulation_gui();
    });



    // let test = true;
    // if test {
    //     //Comment functions we aren't testing
    //     my_generic_fragment_forward();
    // } else {
    //     let handles = initialize("input.toml");
    //     for handle in handles.into_iter() {
    //         handle.join().unwrap();
    //     }
    // }

    // generic_fragment_forward::<SkyLinkDrone>();
    // generic_chain_fragment_ack();
    // generic_fragment_drop();
    // generic_chain_fragment_drop();

    // test_generic();
}

mod tests{
    use std::collections::HashMap;
    use std::thread;
    use crossbeam_channel::unbounded;
    use egui::Key::S;
    use wg_2024::drone::Drone;
    use wg_2024::network::SourceRoutingHeader;
    use wg_2024::packet::{Ack, Fragment, Nack, NackType, Packet, PacketType};

    use super::*;
    use wg_2024::tests::{generic_chain_fragment_drop, generic_fragment_forward, generic_fragment_drop, generic_chain_fragment_ack};
    use crate::my_drone::SkyLinkDrone;

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
}
