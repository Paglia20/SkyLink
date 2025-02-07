// sam_client_base.rs

use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::message::{Message, ContentType, TypeExchange};
use super::sam_events::{ClientCommand, ClientEvent, ConnectionState};
use super::sam_client_trait::Client;
use super::sam_client_type::ClientType;
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, Packet, PacketType, FloodRequest};

const MAX_RETRIES: u32 = 3;

pub struct SamClientBase {
    pub node_id: NodeId,
    pub command_recv: Receiver<ClientCommand>,
    pub event_send: Sender<ClientEvent>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,
    pub fragments: HashMap<(u64, NodeId), (NodeId, Option<ContentType>, Vec<Fragment>)>,
    pub flood_ids: HashSet<(u64, NodeId)>,
    pub node_states: HashMap<NodeId, ConnectionState>,
    retry_counts: HashMap<(u64, NodeId), u32>,
    next_session_id: u64,
    next_flood_id: u64,
}

impl SamClientBase {
    pub fn new(
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
            fragments: HashMap::new(),
            flood_ids: HashSet::new(),
            node_states: HashMap::new(),
            retry_counts: HashMap::new(),
            next_session_id: 0,
            next_flood_id: 0,
        }
    }

    fn handle_retry(&mut self, session_id: u64, destination: NodeId) -> bool {
        let entry = self.retry_counts.entry((session_id, destination)).or_insert(0);
        *entry += 1;
        if *entry >= MAX_RETRIES {
            self.retry_counts.remove(&(session_id, destination));
            self.send_event(ClientEvent::LostMessage(session_id, destination));
            false
        } else {
            true
        }
    }

    fn process_flood_request(&mut self, mut request: FloodRequest, packet: Packet) {
        request.path_trace.push((self.node_id, wg_2024::packet::NodeType::Client));

        if self.flood_ids.insert((request.flood_id, request.initiator_id)) {
            if self.packet_send.len() == 1 {
                let response = wg_2024::packet::FloodResponse {
                    flood_id: request.flood_id,
                    path_trace: request.path_trace,
                };
                let resp_packet = Packet::new_flood_response(
                    wg_2024::network::SourceRoutingHeader::default(),
                    request.flood_id,
                    response
                );
                if let Some(sender) = self.packet_send.values().next() {
                    if sender.send(resp_packet.clone()).is_ok() {
                        self.send_event(ClientEvent::PacketSent(resp_packet));
                    }
                }
            } else {
                let mut prev = request.initiator_id;
                if request.path_trace.len() > 1 {
                    prev = request.path_trace[request.path_trace.len() - 2].0;
                }

                for (key, sender) in self.packet_send.iter() {
                    if *key != prev {
                        let mut new_packet = packet.clone();
                        new_packet.pack_type = PacketType::FloodRequest(request.clone());
                        if sender.send(new_packet.clone()).is_ok() {
                            self.send_event(ClientEvent::PacketSent(new_packet));
                        }
                    }
                }
            }
        }
    }

    fn process_message_fragment(&mut self, fragment: Fragment, packet: Packet) {
        let session_id = packet.session_id;
        let source_id = packet.routing_header.hops[0];
        let destination = self.node_id;

        match self.fragments.entry((session_id, source_id)) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().2.push(fragment.clone());

                if entry.get().2.len() as u64 == fragment.total_n_fragments {
                    if let Ok(message) = Self::reassemble_message(session_id, source_id, &entry.get().2) {
                        self.handle_message(message);
                        entry.remove();
                    } else {
                        self.send_event(ClientEvent::ErrorReassembling(self.node_id));
                    }
                }
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((destination, None, vec![fragment]));
            }
        }

        self.send_ack(packet.clone(), fragment.fragment_index);
        self.send_event(ClientEvent::PacketReceived(packet));
    }
}

impl NetworkEdge for SamClientBase {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        let session_id = self.get_session_id();
        let fragments = Self::fragment_message(&message);
        self.fragments.insert((session_id, self.node_id), (destination, Some(message.content), fragments.clone()));

        for fragment in fragments {
            self.send_fragment(fragment, destination, session_id);
        }
    }

    fn handle_packet(&mut self, packet: Packet) {
        match packet.pack_type {
            PacketType::MsgFragment(fragment) => {
                self.process_message_fragment(fragment, packet);
            }
            PacketType::Ack(_) => {
                if let Some(source) = packet.routing_header.source() {
                    self.retry_counts.remove(&(packet.session_id, source));
                }
                self.send_event(ClientEvent::AckReceived(packet));
            }
            PacketType::Nack(nack) => {
                if let Some(source) = packet.routing_header.source() {
                    if self.handle_retry(packet.session_id, source) {
                        self.send_fragment_after_nack(packet.clone(), nack);
                    }
                }
                self.send_event(ClientEvent::NackReceived(packet));
            }
            PacketType::FloodRequest(request) => {
                self.process_flood_request(request, packet);
            }
            PacketType::FloodResponse(response) => {
                let path_trace = response.path_trace;
                for (node_id, node_type) in path_trace {
                    if node_type != wg_2024::packet::NodeType::Drone {
                        self.node_states.insert(node_id, ConnectionState::WaitingForType);
                        self.check_type(node_id);
                    }
                }
            }
        }
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        if destination == self.node_id {
            return;
        }

        let packet = Packet::new_fragment(
            wg_2024::network::SourceRoutingHeader::new(vec![self.node_id, destination], 0),
            session_id,
            fragment
        );

        if let Some(sender) = self.packet_send.get(&destination) {
            if sender.send(packet.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet));
            } else {
                self.send_event(ClientEvent::PacketSendingError(packet));
                self.add_unsent_fragment(fragment, session_id, destination);
            }
        } else {
            self.send_event(ClientEvent::MissingDestination(self.node_id, destination));
        }
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        if let Some((_, _, fragments)) = self.fragments.get_mut(&(session_id, self.node_id)) {
            fragments.push(fragment);
        } else {
            self.fragments.insert((session_id, self.node_id), (destination, None, vec![fragment]));
        }
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, _nack: Nack) {
        if let Some((destination, _, fragments)) = self.fragments.get(&(packet.session_id, self.node_id)) {
            for fragment in fragments {
                self.send_fragment(fragment.clone(), *destination, packet.session_id);
            }
        }
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        let source = packet.routing_header.hops[0];
        let ack_packet = Packet::new_ack(
            wg_2024::network::SourceRoutingHeader::new(vec![self.node_id, source], 0),
            packet.session_id,
            fragment_index
        );

        if let Some(sender) = self.packet_send.get(&source) {
            if sender.send(ack_packet.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(ack_packet));
            }
        }
    }

    fn flood(&mut self) {
        self.send_event(ClientEvent::Flooding(self.node_id));

        let flood_id = self.get_flood_id();
        let flood_request = FloodRequest {
            flood_id,
            initiator_id: self.node_id,
            path_trace: vec![(self.node_id, wg_2024::packet::NodeType::Client)],
        };

        let packet = Packet::new_flood_request(
            wg_2024::network::SourceRoutingHeader::default(),
            flood_id,
            flood_request
        );

        for sender in self.packet_send.values() {
            if sender.send(packet.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet.clone()));
            }
        }
    }

    fn get_flood_id(&mut self) -> u64 {
        let id = self.next_flood_id;
        self.next_flood_id += 1;
        id
    }

    fn get_session_id(&mut self) -> u64 {
        let id = self.next_session_id;
        self.next_session_id += 1;
        id
    }

    fn handle_message(&mut self, _message: Message) {
        // Implemented by derived clients
    }
}

impl NetworkEdgeErrors for SamClientBase {
    fn check_type(&mut self, id: NodeId) {
        let type_request = Message::new(
            self.node_id,
            self.get_session_id(),
            ContentType::TypeExchange(TypeExchange::TypeRequest {
                from: self.node_id,
            })
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
        let packet = Packet::new_nack(
            wg_2024::network::SourceRoutingHeader::new(vec![self.node_id, dst], 0),
            self.get_session_id(),
            0,
            nack
        );

        if let Some(sender) = self.packet_send.get(&dst) {
            if sender.send(packet.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet));
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
        SamClientBase::new(id, command_recv, event_send, packet_recv, packet_send)
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.command_recv) -> cmd => {
                    if let Ok(command) = cmd {
                        self.handle_command(command);
                    }
                }
                recv(self.packet_recv) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
            }
        }
    }

    fn handle_command(&mut self, command: ClientCommand) {
        match command {
            ClientCommand::RemoveSender(node_id) => {
                self.packet_send.remove(&node_id);
                self.node_states.remove(&node_id);
            }
            ClientCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
                self.node_states.insert(node_id, ConnectionState::WaitingForType);
            }
            ClientCommand::Flood => {
                self.flood();
            }
            _ => {}
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::ChatClient
    }

    fn send_event(&self, ce: ClientEvent) {
        let _ = self.event_send.send(ce);
    }
}