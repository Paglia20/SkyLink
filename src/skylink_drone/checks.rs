use crate::skylink_drone::drone::SkyLinkDrone;
use crate::skylink_drone::error::create_error;
use wg_2024::packet::{NackType, Packet, PacketType};

pub fn id_hop_match_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
    if packet.routing_header.hops[packet.routing_header.hop_index] == drone.get_id() {
        Ok(())
    } else {
        match packet.pack_type.clone() {
            PacketType::MsgFragment(_fragment) => Err(create_error(
                packet.routing_header.hops[packet.routing_header.hop_index - 1],
                packet,
                NackType::UnexpectedRecipient(drone.get_id()),
            )),
            PacketType::FloodRequest(_) => unreachable!(),
            // In case the packet wasn't a message I send it back as it is to pass through the SC shortcut.
            _ => Err(packet),
        }
    }
}
pub fn final_destination_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
    if packet.routing_header.destination().unwrap() != drone.get_id() {
        Ok(())
    } else {
        match packet.pack_type.clone() {
            PacketType::MsgFragment(_fragment) => Err(create_error(
                drone.get_id(),
                packet,
                NackType::DestinationIsDrone,
            )),
            PacketType::FloodRequest(_) => unreachable!(),
            _ => Err(packet),
        }
    }
}
pub fn is_next_hop_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
    let next_hop = &packet.routing_header.hops.clone()[packet.routing_header.hop_index];
    if drone.get_packet_send().contains_key(next_hop) {
        Ok(())
    } else {
        match packet.pack_type.clone() {
            PacketType::MsgFragment(_fragment) => Err(create_error(
                drone.get_id(),
                packet,
                NackType::ErrorInRouting(*next_hop)),
            ),
            PacketType::FloodRequest(_) => unreachable!(),
            _ => Err(packet),
        }
    }
}
pub fn pdr_check(drone: &SkyLinkDrone, packet: Packet) -> Result<(), Packet> {
    if let PacketType::MsgFragment(_) = packet.pack_type.clone() {
        let random_number: u32 = fastrand::u32(0..100);
        if random_number < drone.get_pdr() {
            return Err(create_error(drone.get_id(), packet, NackType::Dropped));
        }
    }
    Ok(())
}
