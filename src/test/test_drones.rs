#![cfg(test)]
use rusty_tester::*;
use std::time::Duration;
use lockheedrustin_drone::LockheedRustin;
use rolling_drone::RollingDrone;
use rusty_drones::RustyDrone;
use wg_2024_rust::drone::RustDrone;
use crate::skylink_drone::drone::SkyLinkDrone;

type Tested = RollingDrone;
const TIMEOUT: Duration = Duration::from_millis(20);
const FLOOD_TIMEOUT: Duration = Duration::from_millis(50);

#[test]
fn drone_destination_is_drone() {
    test_drone_destination_is_drone::<Tested>(TIMEOUT);
}

#[test]
fn drone_error_in_routing() {
    test_drone_error_in_routing::<Tested>(TIMEOUT);
}

#[test]
fn drone_packet_1_hop() {
    test_drone_packet_1_hop::<Tested>(TIMEOUT);
}

#[test]
fn drone_packet_3_hop() {
    test_drone_packet_3_hop::<Tested>(TIMEOUT);
}

#[test]
fn drone_packet_3_hop_crash() {
    test_drone_packet_3_hop_crash::<Tested>(TIMEOUT);
}

#[test]
fn easiest_flood() {
    test_easiest_flood::<Tested>(FLOOD_TIMEOUT);
}

#[test]
fn loop_flood() {
    test_loop_flood::<Tested>(FLOOD_TIMEOUT);
}

#[test]
fn hard_loop_flood() {
    test_hard_loop_flood::<Tested>(FLOOD_TIMEOUT);
}

#[test]
fn matrix_loop_flood() {
    test_matrix_loop_flood::<Tested>(FLOOD_TIMEOUT);
}

#[test]
fn star_loop_flood() {
    test_star_loop_flood::<Tested>(FLOOD_TIMEOUT);
}

#[test]
fn butterfly_loop_flood() {
    test_butterfly_loop_flood::<Tested>(FLOOD_TIMEOUT);
}

#[test]
fn tree_loop_flood() {
    test_tree_loop_flood::<Tested>(FLOOD_TIMEOUT);
}
