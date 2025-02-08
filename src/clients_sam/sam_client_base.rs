// sam_client_base.rs
use super::sam_events::{ClientCommand, ClientEvent, ConnectionState};
use super::sam_client_trait::Client;
use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crate::message::{Message, ContentType, TypeExchange};
use crate::routing::Network;
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, Packet, PacketType, NodeType, Ack};

pub struct SamClientBase {
    pub node_id: NodeId,
    pub command_recv: Receiver<ClientCommand>,
    pub event_send: Sender<ClientEvent>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,
    pub network: Network,
    pub fragments: HashMap<(u64, NodeId), (NodeId, Option<ContentType>, Vec<Fragment>)>,
    pub flood_ids: HashSet<(u64, NodeId)>,
    pub node_states: HashMap<NodeId, ConnectionState>,
    pub unsent_fragments: (u8, HashMap<(u64, NodeId), (NodeId, Vec<Fragment>)>),
}

impl NetworkEdge for SamClientBase {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        let session_id = self.get_session_id();
        let fragments = Self::fragment_message(&message);
        self.fragments.insert(
            (session_id, self.node_id),
            (destination, Some(message.content), fragments.clone()),
        );

        for fragment in fragments {
            self.send_fragment(fragment, destination, session_id);
        }
    }

    fn handle_packet(&mut self, packet: Packet) {
        match packet.pack_type.clone() {
            PacketType::FloodRequest(mut flood_request) => {
                flood_request.path_trace.push((self.node_id, NodeType::Client));

                if self.flood_ids.insert((
                    flood_request.flood_id.clone(),
                    flood_request.initiator_id.clone(),
                )) {
                    if self.packet_send.len() == 1 {
                        self.edge_send_flood_response(flood_request.clone());
                    } else {
                        let prev = if flood_request.path_trace.len() > 1 {
                            flood_request.path_trace[flood_request.path_trace.len() - 2].0
                        } else {
                            flood_request.initiator_id
                        };

                        let mut modified_packet = packet.clone();
                        modified_packet.pack_type = PacketType::FloodRequest(flood_request.clone());
                        for (key, sender) in &self.packet_send {
                            if *key != prev {
                                let packet_to_send = modified_packet.clone();
                                if sender.send(packet_to_send.clone()).is_ok() {
                                    self.send_event(ClientEvent::PacketSent(packet_to_send));
                                }
                            }
                        }
                    }
                } else {
                    self.edge_send_flood_response(flood_request);
                }
            }
            PacketType::MsgFragment(msg_fragment) => {
                let session_id = packet.session_id;
                let source_id = packet.routing_header.hops[0];
                let total_fragments = msg_fragment.total_n_fragments;

                // First operation on fragments
                {
                    let entry = self.fragments
                        .entry((session_id, source_id))
                        .or_insert_with(|| (self.node_id, None::<ContentType>, Vec::new()));
                    entry.2.push(msg_fragment.clone());
                }

                self.send_ack(packet.clone(), msg_fragment.fragment_index);
                self.send_event(ClientEvent::PacketReceived(packet.clone()));

                // Second operation on fragments with cloned data
                let fragments = self.fragments.get(&(session_id, source_id))
                    .map(|(_, _, frags)| frags.clone());

                if let Some(frags) = fragments {
                    if frags.len() as u64 == total_fragments {
                        if let Ok(message) = Self::reassemble_message(session_id, source_id, &frags) {
                            self.handle_message(message);
                            self.fragments.remove(&(session_id, source_id));
                        } else {
                            self.send_event(ClientEvent::ErrorReassembling(self.node_id));
                        }
                    }
                }
            }
            PacketType::Ack(ack) => {
                self.send_event(ClientEvent::AckReceived(packet.clone()));
                if let Some(source) = packet.routing_header.source() {
                    if let Some((_, fragments)) =
                        self.unsent_fragments.1.get_mut(&(packet.session_id, source))
                    {
                        fragments.retain(|f| f.fragment_index != ack.fragment_index);
                    }
                }
            }
            PacketType::Nack(nack) => {
                self.send_event(ClientEvent::NackReceived(packet.clone()));
                if let Some(source) = packet.routing_header.source() {
                    if let Some((destination, _, fragments)) =
                        self.fragments.get(&(packet.session_id, source))
                    {
                        let destination = *destination;
                        for fragment in fragments.iter() {
                            self.send_fragment(fragment.clone(), destination, packet.session_id);
                        }
                    }
                }
            }
            PacketType::FloodResponse(response) => {
                self.network.add_route(self.node_id, response.path_trace);
            }
        }
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        if destination == self.node_id {
            return;
        }

        match self.network.get_srh(&self.node_id, &destination) {
            Some(srh) => {
                let first_hop = srh.hops[1];
                let packet = Packet {
                    routing_header: srh,
                    session_id,
                    pack_type: PacketType::MsgFragment(fragment.clone()),
                };

                if let Some(sender) = self.packet_send.get(&first_hop) {
                    let packet_to_send = packet.clone();
                    if sender.send(packet_to_send.clone()).is_ok() {
                        self.send_event(ClientEvent::PacketSent(packet_to_send));
                    } else {
                        self.network.remove_faulty_connection(self.node_id, first_hop);
                        self.add_unsent_fragment(fragment, session_id, destination);
                    }
                } else {
                    self.add_unsent_fragment(fragment, session_id, destination);
                }
            }
            None => {
                self.send_event(ClientEvent::MissingDestination(self.node_id, destination));
                self.add_unsent_fragment(fragment, session_id, destination);
                self.flood();
            }
        }
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.unsent_fragments
            .1
            .entry((session_id, self.node_id))
            .or_insert_with(|| (destination, Vec::new()))
            .1.push(fragment);
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, _nack: Nack) {
        if let Some((destination, _, fragments)) = self.fragments.get(&(packet.session_id, self.node_id)) {
            let destination = *destination;
            for fragment in fragments.iter() {
                self.send_fragment(fragment.clone(), destination, packet.session_id);
            }
        }
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        let mut srh = packet.routing_header.get_reversed();
        srh.hop_index = 1;
        let ack_packet = Packet {
            routing_header: srh,
            session_id: packet.session_id,
            pack_type: PacketType::Ack(Ack { fragment_index }),
        };

        if let Some(sender) = self.packet_send.get(&ack_packet.routing_header.hops[1]) {
            let packet_to_send = ack_packet.clone();
            if sender.send(packet_to_send.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet_to_send));
            }
        }
    }

    fn flood(&mut self) {
        self.send_event(ClientEvent::Flooding(self.node_id));
        let flood_request = wg_2024::packet::FloodRequest {
            flood_id: self.get_flood_id(),
            initiator_id: self.node_id,
            path_trace: vec![(self.node_id, NodeType::Client)],
        };
        let packet = Packet {
            routing_header: wg_2024::network::SourceRoutingHeader::default(),
            session_id: self.get_session_id(),
            pack_type: PacketType::FloodRequest(flood_request),
        };

        for sender in self.packet_send.values() {
            let packet_to_send = packet.clone();
            if sender.send(packet_to_send.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet_to_send));
            }
        }
    }

    fn get_flood_id(&mut self) -> u64 {
        fastrand::u64(..)
    }

    fn get_session_id(&mut self) -> u64 {
        fastrand::u64(..)
    }

    fn get_src_id(&self) -> NodeId {
        self.node_id
    }

    fn remove_sender(&mut self, id: NodeId) {
        if self.packet_send.remove(&id).is_some() {
            self.node_states.remove(&id);
        }
    }

    fn handle_message(&mut self, _message: Message) {
        // Implemented by specific client types.
    }
}

impl NetworkEdgeErrors for SamClientBase {
    fn check_type(&mut self, id: NodeId) {
        let type_request = Message::new(
            self.node_id,
            self.get_session_id(),
            ContentType::TypeExchange(TypeExchange::TypeRequest { from: self.node_id }),
        );
        self.send_message(type_request, id);
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        matches!(self.node_states.get(&node_id), Some(ConnectionState::Ready))
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.send_message(nack, dst);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        let new_nack = Nack {
            fragment_index: 0,
            nack_type: nack,
        };
        let packet = Packet {
            routing_header: wg_2024::network::SourceRoutingHeader::new(vec![self.node_id, dst], 0),
            session_id: self.get_session_id(),
            pack_type: PacketType::Nack(new_nack),
        };

        if let Some(sender) = self.packet_send.get(&dst) {
            let packet_to_send = packet.clone();
            if sender.send(packet_to_send.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet_to_send));
            }
        }
    }
}

impl Client for SamClientBase {
    fn new(
        id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        SamClientBase {
            node_id: id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            network: Network::new(),
            fragments: HashMap::new(),
            flood_ids: HashSet::new(),
            node_states: HashMap::new(),
            unsent_fragments: (0, HashMap::new()),
        }
    }

    fn run(&mut self) {
        unreachable!("Base client should never be run directly");
    }

    fn handle_command(&mut self, command: ClientCommand) {
        match command {
            ClientCommand::RemoveSender(node_id) => {
                self.remove_sender(node_id);
            }
            ClientCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
                self.node_states.insert(node_id, ConnectionState::WaitingForType);
                self.check_type(node_id);
            }
            ClientCommand::Flood => {
                self.flood();
            }
            _ => {}
        }
    }

    fn get_client_type(&self) -> super::sam_client_type::ClientType {
        unreachable!("Base client type should never be queried directly")
    }

    fn send_event(&self, ce: ClientEvent) {
        let _ = self.event_send.send(ce);
    }
}

impl SamClientBase {
    fn edge_send_flood_response(&mut self, flood_request: wg_2024::packet::FloodRequest) {
        let flood_response = wg_2024::packet::FloodResponse {
            flood_id: flood_request.flood_id,
            path_trace: flood_request.path_trace,
        };

        let mut hops: Vec<NodeId> = flood_request
            .path_trace
            .iter()
            .map(|(id, _)| *id)
            .rev()
            .collect();

        if flood_request.path_trace[0].0 != flood_request.initiator_id {
            hops.push(flood_request.initiator_id);
        }

        let packet = Packet {
            pack_type: PacketType::FloodResponse(flood_response),
            routing_header: wg_2024::network::SourceRoutingHeader { hop_index: 0, hops },
            session_id: flood_request.flood_id,
        };

        self.handle_packet(packet);
    }
}