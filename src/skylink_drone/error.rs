use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Nack, NackType, Packet, PacketType};

pub fn create_error(starting_id: NodeId, packet: Packet, nack_type: NackType) -> Packet {
    let mut fragment_index = 0;
    if let PacketType::MsgFragment(msg_fragment) = packet.pack_type {
        fragment_index = msg_fragment.fragment_index;
    }

    let hops = packet.routing_header.hops
        .into_iter()
        .rev()
        .collect::<Vec<NodeId>>();
    let position = hops
        .iter()
        .position(|x| *x == starting_id)
        .unwrap();
    Packet {
        pack_type: PacketType::Nack(Nack {
            fragment_index,
            nack_type,
        }),
        routing_header: SourceRoutingHeader {
            hop_index: position,
            hops
        },
        session_id: packet.session_id,
    }
}
