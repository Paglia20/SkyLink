use wg_2024::packet::{NackType, Packet, PacketType};
use crate::error::create_error;
use crate::drone::SkyLinkDrone;

pub fn id_hop_match_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
    if packet.routing_header.hops[packet.routing_header.hop_index] == drone.get_id() {
        Ok(())
    } else {
        match packet.pack_type.clone() {
            PacketType::MsgFragment(_fragment) => {
                Err(create_error(packet.routing_header.hops[packet.routing_header.hop_index-1], packet, NackType::UnexpectedRecipient(drone.get_id())))
            },
            _ => {
                Err(packet)
            }
        }}
}
pub fn final_destination_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
    if packet.routing_header.hop_index < packet.routing_header.hops.len() {
        Ok(())
    } else {
        match packet.pack_type.clone() {
            PacketType::MsgFragment(_fragment) => {
                Err(create_error(drone.get_id(), packet, NackType::DestinationIsDrone))
            },
            _ => {
                Err(packet)
            }
        }
    }
}
pub fn is_next_hop_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
    let next_hop = &packet.routing_header.hops[packet.routing_header.hop_index];
    if drone.get_packet_send().contains_key(next_hop) {
        Ok(())
    } else {
        match packet.pack_type.clone() {
            PacketType::MsgFragment(_fragment) => {
                Err(create_error(drone.get_id(), packet, NackType::ErrorInRouting(drone.get_id())))
            },
            _ => {
                Err(packet)
            }
        }
    }
}
pub fn pdr_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
    if let PacketType::MsgFragment(_) = packet.pack_type.clone() {
        let random_number: u32 = fastrand::u32(0..101);
        if random_number < drone.get_pdr() {
            return Err(create_error(drone.get_id(), packet, NackType::Dropped))
        }
    }
    Ok(())
}
