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
    pub is_flooding: bool,
    pub flood_count: u64,
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

    fn handle_packet(&mut self, mut packet: Packet) {
        match packet.pack_type.clone() {
            PacketType::FloodRequest(mut flood_request) => {
                flood_request.path_trace.push((self.node_id, NodeType::Client));
                let flood_key = (flood_request.flood_id, flood_request.initiator_id);

                if self.flood_ids.insert(flood_key) {
                    self.is_flooding = true;
                    self.send_event(ClientEvent::Flooding(self.node_id));

                    if self.packet_send.len() == 1 {
                        self.edge_send_flood_response(flood_request);
                    } else {
                        let prev = if flood_request.path_trace.len() > 1 {
                            flood_request.path_trace[flood_request.path_trace.len() - 2].0
                        } else {
                            flood_request.initiator_id
                        };

                        packet.pack_type = PacketType::FloodRequest(flood_request.clone());

                        // Forward flood to all neighbors except previous
                        for (key, sender) in &self.packet_send {
                            if *key != prev {
                                if sender.send(packet.clone()).is_ok() {
                                    self.send_event(ClientEvent::PacketSent(packet.clone()));
                                }
                            }
                        }
                    }

                    // Generate new flood request with new ID
                    let new_flood_id = self.get_flood_id();
                    let new_flood_request = wg_2024::packet::FloodRequest {
                        flood_id: new_flood_id,
                        initiator_id: self.node_id,
                        path_trace: vec![(self.node_id, NodeType::Client)],
                    };

                    let new_packet = Packet::new_flood_request(
                        wg_2024::network::SourceRoutingHeader::default(),
                        self.get_session_id(),
                        new_flood_request
                    );

                    // Send new flood request to all neighbors
                    for (_, sender) in &self.packet_send {
                        if sender.send(new_packet.clone()).is_ok() {
                            self.send_event(ClientEvent::PacketSent(new_packet.clone()));
                        }
                    }

                } else {
                    self.edge_send_flood_response(flood_request);
                }
            }
            PacketType::MsgFragment(fragment) => {
                if packet.routing_header.destination().unwrap() != self.node_id {
                    // Forward as a drone would
                    packet.routing_header.hop_index += 1;
                    let next_hop = packet.routing_header.hops[packet.routing_header.hop_index];

                    if let Some(sender) = self.packet_send.get(&next_hop) {
                        if sender.send(packet.clone()).is_ok() {
                            self.send_event(ClientEvent::PacketSent(packet));
                        }
                    }
                } else {
                    // Handle message fragment
                    let session_id = packet.session_id;
                    let source_id = packet.routing_header.hops[0];
                    let total_fragments = fragment.total_n_fragments;

                    self.send_event(ClientEvent::PacketReceived(packet.clone()));

                    let destination = self.node_id;
                    let frag_index = fragment.fragment_index;

                    let entry = self.fragments.entry((session_id, source_id)).or_insert((destination, None, vec![]));
                    entry.2.push(fragment);

                    self.send_ack(packet.clone(), frag_index);

                    let fragments = &self.fragments.get(&(session_id, source_id)).unwrap().2;
                    if fragments.len() as u64 == total_fragments {
                        if let Ok(message) = Self::reassemble_message(session_id, source_id, fragments) {
                            self.fragments.remove(&(session_id, source_id));
                            self.handle_message(message);
                        } else {
                            self.send_event(ClientEvent::ErrorReassembling(self.node_id));
                        }
                    }
                }
            }
            PacketType::Ack(ack) => {
                self.send_event(ClientEvent::AckReceived(packet.clone()));

                if let Some(source) = packet.routing_header.source() {
                    if let Some((_, fragments)) = self.unsent_fragments.1.get_mut(&(packet.session_id, source)) {
                        fragments.retain(|f| f.fragment_index != ack.fragment_index);
                    }
                }
            }
            PacketType::Nack(nack) => {
                self.send_event(ClientEvent::NackReceived(packet.clone()));
                let source = packet.routing_header.source().unwrap_or(self.node_id);
                let fragments_to_send = if let Some((destination, _, fragments)) = self.fragments.get(&(packet.session_id, source)) {
                    Some((*destination, fragments.clone()))
                } else {
                    None
                };

                if let Some((destination, fragments)) = fragments_to_send {
                    for fragment in fragments {
                        self.send_fragment(fragment, destination, packet.session_id);
                    }
                }
            }
            PacketType::FloodResponse(response) => {
                self.network.add_route(self.node_id, response.path_trace.clone());

                if self.network.has_all_routes(self.node_id) || self.flood_count >= 200 {
                    self.is_flooding = false;
                    self.flood_count = 0;

                    let mut to_process = Vec::new();
                    for (identifier, content) in self.unsent_fragments.1.iter() {
                        for fragment in &content.1 {
                            to_process.push((fragment.clone(), identifier.clone(), content.0));
                        }
                    }
                    self.unsent_fragments.1.clear();
                    self.unsent_fragments.0 = 0;

                    for (fragment, identifier, dst) in to_process {
                        self.send_fragment(fragment, dst, identifier.0);
                    }
                } else {
                    self.flood_count += 1;
                }

                for id in self.network.get_unresolved() {
                    self.check_type(id);
                }
            }
        }
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        if destination == self.node_id {
            return;
        }

        match self.network.get_srh(&self.node_id, &destination) {
            None => {
                if crate::DEBUG_MODE {
                    println!("Tried to send fragment {session_id} without path to {destination} with {}, so may have flooded again", self.node_id);
                }
                // First send missing destination event
                self.send_event(ClientEvent::MissingDestination(self.get_src_id(), destination));

                // Then store fragment and flood
                self.add_unsent_fragment(fragment, session_id, destination);
                if !self.is_flooding {
                    self.flood();
                }
            }
            Some(srh) => {
                let first_hop = srh.hops[1];
                let packet = Packet::new_fragment(srh, session_id, fragment.clone());

                // Try to send the packet
                match self.packet_send.get(&first_hop) {
                    Some(sender) => {
                        if sender.send(packet.clone()).is_ok() {
                            self.send_event(ClientEvent::PacketSent(packet));
                        } else {
                            self.network.remove_faulty_connection(self.node_id, first_hop);
                            self.add_unsent_fragment(fragment, session_id, destination);
                        }
                    }
                    None => {
                        self.send_event(ClientEvent::MissingRoute(self.get_src_id(), destination));
                        self.add_unsent_fragment(fragment, session_id, destination);
                        self.network.remove_faulty_connection(self.node_id, first_hop);
                    }
                }
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
        // Get the values we need first
        let fragments_to_send = if let Some((destination, _, fragments)) =
            self.fragments.get(&(packet.session_id, self.node_id))
        {
            // Clone what we need
            let destination = *destination;
            let fragments: Vec<Fragment> = fragments.iter().cloned().collect();
            Some((destination, fragments))
        } else {
            None
        };

        // Now use the cloned values
        if let Some((destination, fragments)) = fragments_to_send {
            for fragment in fragments {
                self.send_fragment(fragment, destination, packet.session_id);
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
            if sender.send(ack_packet.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(ack_packet.clone()));
                self.send_event(ClientEvent::AckReceived(packet));
            }
        }
    }


    fn flood(&mut self) {
        self.is_flooding = true;
        // Add flooding event emission
        self.send_event(ClientEvent::Flooding(self.node_id));

        let flood_request = wg_2024::packet::FloodRequest {
            flood_id: self.get_flood_id(),
            initiator_id: self.node_id,
            path_trace: vec![(self.node_id, NodeType::Client)],
        };

        let packet = Packet::new_flood_request(
            wg_2024::network::SourceRoutingHeader::default(),
            self.get_session_id(),
            flood_request
        );

        // Send to all connected nodes and emit packet sent events
        for (_, sender) in &self.packet_send {
            if sender.send(packet.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet.clone()));
            }
        }
    }

    fn get_flood_id(&mut self) -> u64 {
        let min = match self.flood_ids.iter().min() {
            Some(min) => (*min).0,
            None => {
                let value = fastrand::u64(..30);
                self.flood_ids.insert((value, self.node_id));
                return value
            }
        };
        let value = fastrand::u64(min..min + 40);
        self.flood_ids.insert((value, self.node_id));
        value
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
            is_flooding: false,
            flood_count: 0,
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
    fn handle_flood_response(&mut self, flood_resp: wg_2024::packet::FloodResponse) {
        // Add route and paths
        self.network.add_route(self.node_id, flood_resp.path_trace.clone());

        // Check if we have paths to all nodes including servers
        let can_flood = !self.network.has_all_routes(self.node_id) && self.flood_count < 200;

        if !can_flood {
            self.is_flooding = false;
            self.flood_count = 0;

            // Process any unsent fragments
            let fragments_to_send: Vec<_> = self.unsent_fragments.1
                .iter()
                .flat_map(|(id, (dst, frags))| {
                    frags.iter().map(move |f| (f.clone(), *id, *dst))
                })
                .collect();

            self.unsent_fragments.1.clear();
            self.unsent_fragments.0 = 0;

            for (fragment, id, dst) in fragments_to_send {
                self.send_fragment(fragment, dst, id.0);
            }
        } else {
            self.flood_count += 1;
        }

        // Check unresolved nodes
        for id in self.network.get_unresolved() {
            if !self.is_flooding {
                self.check_type(id);
            }
        }
    }

    fn flood(&mut self) {
        if self.is_flooding {
            return;
        }

        self.is_flooding = true;
        self.send_event(ClientEvent::Flooding(self.node_id));

        let flood_request = wg_2024::packet::FloodRequest {
            flood_id: self.get_flood_id(),
            initiator_id: self.node_id,
            path_trace: vec![(self.node_id, NodeType::Client)],
        };

        let packet = Packet::new_flood_request(
            wg_2024::network::SourceRoutingHeader::default(),
            self.get_session_id(),
            flood_request
        );

        for sender in self.packet_send.values() {
            if sender.send(packet.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet.clone()));
            }
        }
    }

    fn process_network_paths(&mut self) {
        if !self.network.has_all_routes(self.node_id) && !self.is_flooding {
            self.flood();
        }
    }

    pub fn edge_send_flood_response(&mut self, flood_request: wg_2024::packet::FloodRequest) {
        let flood_resp = wg_2024::packet::FloodResponse {
            flood_id: flood_request.flood_id,
            path_trace: flood_request.path_trace.clone(),
        };

        let mut hops = flood_request.path_trace
            .iter()
            .map(|(id, _)| *id)
            .rev()
            .collect::<Vec<NodeId>>();

        if flood_request.path_trace[0].0 != flood_request.initiator_id {
            hops.push(flood_request.initiator_id);
        }

        let response_packet = Packet {
            pack_type: PacketType::FloodResponse(flood_resp),
            routing_header: wg_2024::network::SourceRoutingHeader {
                hop_index: 0,
                hops
            },
            session_id: flood_request.flood_id,
        };

        self.handle_packet(response_packet);
    }

    pub fn send_as_drone(&mut self, mut packet: Packet) {
        packet.routing_header.hop_index += 1;
        if let Some(&next_id) = packet.routing_header.hops.get(packet.routing_header.hop_index) {
            if let Some(sender) = self.packet_send.get(&next_id) {
                if sender.send(packet.clone()).is_ok() {
                    self.send_event(ClientEvent::PacketSent(packet));
                } else {
                    self.network.remove_faulty_connection(self.node_id, next_id);
                }
            }
        }
    }

    pub fn handle_edge_nack(&mut self, nack_type: crate::message::EdgeNackType, source_id: NodeId) {
        match nack_type {
            crate::message::EdgeNackType::UnexpectedMessage => {
                self.network.update_state(source_id, 2);
                if crate::DEBUG_MODE {
                    println!("Client {} discarded message after receiving unexpected message nack from {}", self.node_id, source_id);
                }
            }
        }
    }

    pub fn client_send_fragment(&mut self, message: Message, destination: NodeId) {
        let session_id = self.get_session_id();
        let fragments = Self::fragment_message(&message);

        self.fragments.insert(
            (session_id, self.node_id),
            (destination, Some(message.content), fragments.clone())
        );

        for fragment in fragments {
            self.send_fragment(fragment, destination, session_id);
        }
    }

    pub fn periodic_check_type(&mut self) {
        for id in self.network.get_unresolved() {
            self.check_type(id);
        }
    }

    pub fn process_unsent_periodically(&mut self) {
        let to_process: Vec<_> = self.unsent_fragments.1
            .iter()
            .flat_map(|(id, (dst, frags))| {
                frags.iter().map(move |f| (f.clone(), *id, *dst))
            })
            .collect();

        self.unsent_fragments.1.clear();
        self.unsent_fragments.0 = 0;

        for (fragment, id, dst) in to_process {
            self.send_fragment(fragment, dst, id.0);
        }
    }

    pub fn handle_nack(&mut self, nack: Nack, packet: Packet) {
        match nack.nack_type {
            NackType::UnexpectedRecipient(wrong_node) => {
                self.network.remove_node(wrong_node);
                self.send_fragment_after_nack(packet, nack);
            },
            NackType::ErrorInRouting(wrong_node) => {
                self.network.remove_node(wrong_node);
                self.send_fragment_after_nack(packet, nack);
            },
            NackType::DestinationIsDrone => {
                if let Some(wrong_node) = packet.routing_header.hops.last() {
                    self.network.update_state(*wrong_node, 2);
                }
            },
            NackType::Dropped => {
                if let Some(dropper) = packet.routing_header.source() {
                    self.network.negative_feedback(dropper);
                }
                self.send_fragment_after_nack(packet, nack);
            }
        }
    }
}