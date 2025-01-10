use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{Nack, NackType, Packet, PacketType};

pub fn create_error(starting_id: NodeId, packet: Packet, nack_type: NackType) -> Packet {
    let mut fragment_index = 0;
    if let PacketType::MsgFragment(msg_fragment) = packet.pack_type {
        fragment_index = msg_fragment.fragment_index;
    }

    let position = packet.routing_header.hops
        .iter()
        .position(|x| *x == starting_id).unwrap();

    Packet {
        pack_type: PacketType::Nack(Nack {
            fragment_index,
            nack_type,
        }),
        routing_header: SourceRoutingHeader {
            hop_index: 0,
            hops: packet.routing_header.hops[0..position + 1].to_vec()
                .into_iter()
                .rev()
                .collect::<Vec<NodeId>>()
        },
        session_id: packet.session_id,
    }
}

pub fn crashing_create_error(starting_id: NodeId, packet: Packet) -> Packet {
    let mut fragment_index = 0;
    if let PacketType::MsgFragment(msg_fragment) = packet.pack_type {
        fragment_index = msg_fragment.fragment_index;
    }

    let nack_type = NackType::ErrorInRouting(starting_id);

    let position = packet.routing_header.hops
        .iter()
        .position(|x| *x == starting_id).unwrap();

    Packet {
        pack_type: PacketType::Nack(Nack {
            fragment_index,
            nack_type,
        }),
        routing_header: SourceRoutingHeader {
            hop_index: 0,
            // I cut at the cell before, since the error should be like it was sent by previous node.
            hops: packet.routing_header.hops[0..position].to_vec()
                .into_iter()
                .rev()
                .collect::<Vec<NodeId>>()
        },
        session_id: packet.session_id,
    }
}