// sam_client_base.rs
use super::sam_events::{ConnectionState};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use super::sam_client_trait::Client;
use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crate::message::{Message, ContentType, TypeExchange};
use crate::routing::Network;
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
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
    pub last_flood: Option<Instant>,
    pub flood_count: u64,
    pub reassembled_message: Option<Message>,
    pub types_requested_for: HashSet<NodeId>,
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
        // seems like we need to be capable of relaying packages.
        // edge_send_flood_response emits packets to this function
        // so we need to forward them, just like the gio version does.

        // copied from gio
        if
        !matches!(packet.pack_type, PacketType::FloodRequest(_)) &&
            packet.routing_header.destination().unwrap() != self.node_id
        {
            // If it's not his packet, but he has to act as a drone (that never misses)
            self.send_as_drone(packet);
            return;
        }

        match packet.pack_type.clone() {
            PacketType::FloodRequest(mut flood_request) => {
                flood_request.path_trace.push((self.node_id, NodeType::Client));

                let flood_id = (
                    flood_request.flood_id,
                    flood_request.initiator_id,
                );

                if self.flood_ids.insert(flood_id) {
                    // setting this flag drasctically reduces redundent flooding
                    // but may prevent full network discovery(ie flooding initiated at
                    // A doesn't ensure that B can see C)
                    // self.is_flooding = true;

                    // Forward to all except previous node
                    let prev = if flood_request.path_trace.len() > 1 {
                        flood_request.path_trace[flood_request.path_trace.len() - 2].0
                    } else {
                        flood_request.initiator_id
                    };

                    // Create the path trace string for the event
                    let path: Vec<NodeId> = flood_request.path_trace
                        .iter()
                        .map(|(id, _)| *id)
                        .collect();

                    // Send event with proper format
                    let packet_with_path = Packet {
                        pack_type: PacketType::FloodRequest(flood_request.clone()),
                        routing_header: packet.routing_header.clone(),
                        session_id: packet.session_id,
                    };
                    self.send_event(ClientEvent::PacketSent(packet_with_path.clone()));

                    // Send to all neighbors except previous
                    for (key, sender) in &self.packet_send {
                        if *key != prev {
                            if sender.send(packet_with_path.clone()).is_ok() {
                                // Don't send another event here since we already sent it above
                            }
                        }
                    }

                    // Send flood response
                    self.edge_send_flood_response(flood_request);
                }
            }
            PacketType::MsgFragment(msg_fragment) => {
                let session_id = packet.session_id;
                let source_id = packet.routing_header.hops[0];
                let total_fragments = msg_fragment.total_n_fragments;

                // First send the received event
                self.send_event(ClientEvent::PacketReceived(packet.clone()));

                // Then store fragment
                {
                    let entry = self.fragments
                        .entry((session_id, source_id))
                        .or_insert_with(|| (self.node_id, None::<ContentType>, Vec::new()));
                    entry.2.push(msg_fragment.clone());
                }

                // Send ACK
                self.send_ack(packet.clone(), msg_fragment.fragment_index);

                // Check if message is complete and handle it
                let fragments_complete = if let Some((_, _, frags)) = self.fragments.get(&(session_id, source_id)) {
                    frags.len() as u64 == total_fragments
                } else {
                    false
                };

                if fragments_complete {
                    // Get a clone of the fragments
                    if let Some((_, _, frags)) = self.fragments.get(&(session_id, source_id)) {
                        let frags_clone = frags.clone();

                        // Try to reassemble and handle the message
                        if let Ok(message) = Self::reassemble_message(session_id, source_id, &frags_clone) {
                            // Remove fragments first
                            self.fragments.remove(&(session_id, source_id));
                            // Then handle the message
                            self.handle_message(message);
                        } else {
                            self.send_event(ClientEvent::ErrorReassembling(self.node_id));
                        }
                    }
                }
            }
            PacketType::Ack(ack) => {
                // Send ACK received event first
                self.send_event(ClientEvent::AckReceived(packet.clone()));

                if packet.routing_header.source().is_some() {
                    if let Some((_, _, fragments)) =
                        self.fragments.get_mut(&(packet.session_id, self.node_id))
                    {
                        fragments.retain(|f| f.fragment_index != ack.fragment_index);
                    }
                }
            }
            PacketType::Nack(nack) => {
                self.send_event(ClientEvent::NackReceived(packet.clone()));
                if let Some(source) = packet.routing_header.source() {
                    // Get the values we need first with an immutable borrow
                    let fragments_to_send = if let Some((destination, _, fragments)) =
                        self.fragments.get(&(packet.session_id, source))
                    {
                        // Clone what we need
                        let destination = *destination;
                        let fragments: Vec<Fragment> = fragments.iter().cloned().collect();
                        Some((destination, fragments))
                    } else {
                        None
                    };

                    // Now use the cloned values without holding the borrow
                    if let Some((destination, fragments)) = fragments_to_send {
                        for fragment in fragments {
                            self.send_fragment(fragment, destination, packet.session_id);
                        }
                    }
                }
            }
            PacketType::FloodResponse(response) => {
                // Add route from response
                self.network.add_route(self.node_id, response.path_trace.clone());

                // Update flooding state
                if self.network.has_all_routes(self.node_id) || self.flood_count >= 200 {
                    self.is_flooding = false;
                    self.flood_count = 0;

                    // Try to send any unsent fragments now that we have routes
                    let mut to_process = Vec::new();
                    for (identifier, content) in self.unsent_fragments.1.iter() {
                        for fragment in content.1.iter() {
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

                // Check types of unresolved nodes
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
                self.network.add_destination_without_path(destination);
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

    fn send_fragment_after_nack(&mut self, packet_session_id: u64, _nack: Nack) {
        if let Some((destination, _, fragments)) = self.fragments.get(&(packet_session_id, self.node_id)) {
            let destination = *destination;
            let cloned_fragments: Vec<Fragment> = fragments.iter().cloned().collect();
            for fragment in cloned_fragments {
                self.send_fragment(fragment, destination, packet_session_id);
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
        if let Some(last_flood) = self.last_flood.as_ref() {
            if dbg!(last_flood.elapsed().as_secs()) < 10 { return; }
        }

        self.last_flood = Some(Instant::now());
        self.is_flooding = true;
        // Send flooding notification for this node
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

        // Send to all connected nodes
        self.packet_send.iter().for_each(|(_, sender)| {
            if sender.send(packet.clone()).is_ok() {
                self.send_event(ClientEvent::PacketSent(packet.clone()));
            }
        });
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

    fn handle_message(&mut self, message: Message) {
        assert!(self.reassembled_message.is_none());
        self.reassembled_message = Some(message);
    }
}

impl NetworkEdgeErrors for SamClientBase {
    fn check_type(&mut self, id: NodeId) {
        if !self.types_requested_for.insert(id) { return; }
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

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType, session_id: u64) {
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
            last_flood: None,
            flood_count: 0,
            reassembled_message: None,
            types_requested_for: HashSet::new(),
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

    fn get_client_type(&self) -> crate::clients_gio::client_type::ClientType {
        unreachable!("Base client type should never be queried directly")
    }

    fn send_event(&self, ce: ClientEvent) {
        let _ = self.event_send.send(ce);
    }
}

impl SamClientBase {
    // copied from gio
    ///Send a packet for which we are not the destination, hence acting as a Drone
    pub fn send_as_drone(&mut self, mut packet: Packet) {
        packet.routing_header.hop_index += 1;
        if let Some(&next_id) = packet.routing_header.hops.get(packet.routing_header.hop_index) {
            match self.packet_send.get(&next_id) {
                None => {
                    self.send_event(ClientEvent::MissingRoute(self.get_src_id(), next_id))
                }
                Some(sender) => {
                    match sender.try_send(packet.clone()) {
                        Err(_) => {
                            // !!You need to send back the same errors a drone would
                            self.send_drone_nack(packet.routing_header.source().unwrap(), NackType::ErrorInRouting(next_id));
                            self.send_event(ClientEvent::PacketSendingError(packet));
                        }
                        Ok(_) => {
                            self.send_event(ClientEvent::PacketSent(packet.clone()));
                            // If the message was sent, I also notify the sim controller.
                        }
                    }
                }
            }
        }
    }

    // copied from gio
    ///Send a drone nack
    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        let new_nack = Nack{
            fragment_index: 0,
            nack_type: nack,
        };
        if let Some(shr) = self.network.get_srh(&self.node_id, &dst){
            let first_hop = shr.next_hop().unwrap_or(self.node_id);
            let packet = Packet{
                routing_header: shr,
                session_id: self.get_session_id(),
                pack_type: PacketType::Nack(new_nack),
            };

            match self.packet_send.get(&first_hop){
                None => {
                    self.send_event(ClientEvent::MissingDestination(self.node_id, dst));
                    return;
                }
                Some(sender) => {
                    sender.send(packet).unwrap();
                }
            }
        } else {
            self.send_event(ClientEvent::MissingDestination(self.node_id, dst));
            return;
        }
    }
}
