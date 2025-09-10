// Client-side network edge for SkyLink: channels, routing, flooding, acks/nacks, and helpers. No logic changes—just comment refresh + light reordering.
use crate::clients_gio::client_command::ClientEvent::{MissingDestination, MissingRoute, WrongDestinationType};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::message::{ContentType, EdgeNackType, Message, TypeExchange};
use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crate::routing::Network;
use crate::DEBUG_MODE;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::NackType::ErrorInRouting;
use wg_2024::packet::PacketType::*;
use wg_2024::packet::{Fragment, Nack, NackType, NodeType, Packet};

const BUILD_FLAVOR: &str = "alt-client-v1";


// Core client structure shared by both client types.
// Note: some functions are intentionally left \`unreachable!()\` and are invoked only by the concrete client.
pub struct ClientStruct {
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,

    //Remember all the seen floods
    flood_ids: HashSet<(u64, NodeId)>,
    //Pretty self-explanatory, used to compute a new one whenever asked for it
    used_session_id: HashSet<u64>,
    //Full Network known by this client, has all the unique functions to make it work
    network: Network,

    //Hashmap with all the fragments we sent, in case we have to re-send them because of nack.
    //(session_id, source) - (destination, copy of content (for registering ecc…) and frags), if the content is None is because it's yet to be fully arrived!
    fragments: HashMap<(u64, NodeId), (NodeId, Option<ContentType>, Vec<Fragment>)>,

    //Hashmap with all the unsent fragments, to process a re-send after a new knowledge of network ecc...
    unsent_fragments: HashMap<(u64, NodeId), (NodeId, Vec<(Fragment)>)>,

    //Is the client in flooding mode? prevent to flood too many times
    is_flooding: bool,
    flood_count: u64,

    is_running: bool,
    _phantom: PhantomData<()>,
}

impl NetworkEdge for ClientStruct {
    // Send a single fragment toward a destination using the current best route.
    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        if destination == self.node_id {
            if DEBUG_MODE {
                println!("Sending message to yourself with {:?}", destination);
            }
            return;
        }

        match self.network.get_srh(&self.node_id, &destination){
            None => {
                self.send_event(MissingDestination(self.get_src_id(), destination));
                self.add_unsent_fragment(fragment, session_id, destination);

                if !self.is_flooding {
                    self.flood();
                }
            }
            Some(srh) => {
                let first_dst = srh.hops[1];
                let packet = Packet::new_fragment(srh, session_id, fragment.clone());

                // Route found: attempt to forward the fragment.
                match self.packet_send.get(&first_dst) {
                    Some(sender) => {
                        sender.send(packet.clone()).unwrap();
                        self.send_event(ClientEvent::PacketSent(packet.clone()));

                    }
                    None => {
                        // The next hop is not a direct neighbour anymore; prune the stale channel/edge.
                        self.send_event(MissingRoute(self.get_src_id(), destination));
                        println!("this missing");
                        self.add_unsent_fragment(fragment, session_id, destination);
                        self.network.remove_faulty_connection(self.node_id, first_dst);
                    }
                }
            }
        }
    }

    // Dispatch a message; enforce type/state checks based on payload.
    fn send_message(&mut self, message: Message, destination: NodeId) {
        match message.clone().content{
            ContentType::TypeExchange(_exc) =>{
                self.client_send_in_fragments(message, destination);
            },
            ContentType::EdgeNack(_nack) => {
                self.client_send_in_fragments(message, destination);
            }
            _=>{
                if self.is_state_ok(destination) {
                    self.client_send_in_fragments(message, destination);
                }
                else {
                    let new_nack = WrongDestinationType(self.get_src_id(), destination);
                    self.send_event(new_nack);
                }
            }
        }
    }

    // Packets are not handled at this layer for the client base.
    fn handle_packet(&mut self, _packet: Packet) {
        unreachable!()
    }

    // Messages are not handled here either; see concrete client.
    fn handle_message(&mut self, _message: Message) {
        unreachable!()
    }

    //If the sending of a fragment gave an error, we put it in a hashmap to try sending it again.
    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        match self.unsent_fragments.get_mut(&(session_id, self.node_id)) {
            Some((_, fragments)) => {
                fragments.push(fragment);
            },
            None => {
                let mut vec = Vec::new();
                vec.push(fragment);
                self.unsent_fragments.insert((session_id, self.node_id), (destination, vec));
            }
        }
    }

    //If a fragment resulted in a nack, we try sending it again.
    fn send_fragment_after_nack(&mut self, packet_session_id: u64, nack: Nack) {
        match self.fragments.get(&(packet_session_id, self.node_id)) {
            // I try to find again the fragment, and notify the sim controller if I don't have it anymore
            None => {
                self.send_event(ClientEvent::LostMessage(packet_session_id, self.node_id));

                if !self.is_flooding {
                    self.flood();
                }
                if DEBUG_MODE{
                    println!("lost message, hence flooded again to ensure a correct knowledge of topology!")
                }
            },
            Some((dst,_, fragments)) => {
                match fragments.get(nack.fragment_index as usize) {
                    None => {
                        self.send_event(ClientEvent::LostFragment(packet_session_id, self.node_id, nack.fragment_index));
                    },
                    // If I manage to find the fragment, I send it
                    Some(fragment) => {
                        self.send_fragment(fragment.clone(), *dst, packet_session_id);
                    }
                }
            }
        }
    }

    // Build and forward an ACK back along the reverse path.
    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        let new_hops: Vec<NodeId> = packet.routing_header.hops.iter().rev().map(|(id)| *id)
            .collect::<Vec<NodeId>>();
        let next_id = new_hops[1];
        let srh = SourceRoutingHeader::new(new_hops, 1); // hop index starts at the neighbour on the way back
        let packet_ack = Packet::new_ack(srh, packet.session_id, fragment_index);

        match self.packet_send.get(&next_id) {
            Some(sender) => {
                sender.send(packet_ack.clone()).unwrap();
                self.send_event(ClientEvent::PacketSent(packet_ack))
            }
            None => {
                self.send_event(MissingDestination(self.node_id, next_id))
                //nack?
            }
        }
    }

    // Start a topology discovery flood (rate-limited via `is_flooding`).
    fn flood(&mut self) {
        if !self.packet_send.is_empty() {
            self.is_flooding = true;
            self.send_event(ClientEvent::Flooding(self.node_id));

            let flood_request = wg_2024::packet::FloodRequest {
                flood_id: self.get_flood_id(),
                initiator_id: self.node_id,
                path_trace: vec![(self.node_id, NodeType::Client)],
            };
            let packet = Packet::new_flood_request(SourceRoutingHeader::default(), self.get_session_id(), flood_request);
            self.packet_send.iter().for_each(|(id, sender)| {
                sender.send(packet.clone()).unwrap()
            });
        }else {
            if DEBUG_MODE{
                println!("flood impossible: no channel attached");
            }
        }
    }

    // Generate a pseudo-unique flood ID.
    fn get_flood_id(&mut self) -> u64 {
        let min = match self.flood_ids.iter().min(){
            Some(min) => (*min).0,
            None => {
                let value = fastrand::u64(..20);
                self.flood_ids.insert((value, self.node_id));
                return value
            }
        };
        let value = fastrand::u64(min..min + 40);
        self.flood_ids.insert((value, self.node_id));
        value
    }

    // Generate a pseudo-unique session ID.
    fn get_session_id(&mut self) -> u64 {
        let min = match self.used_session_id.iter().min(){
            Some(min) => *min,
            None => {
                let value = fastrand::u64(..30);
                self.used_session_id.insert(value);
                return value
            }
        };
        let value = fastrand::u64(min..min + 40);
        self.used_session_id.insert(value);
        value
    }

    // Convenience: return this client's NodeId.
    fn get_src_id(&self) -> NodeId {
        self.node_id
    }

    // Drop a neighbour's send channel if present.
    fn remove_sender(&mut self, id: NodeId) {
        if self.packet_send.contains_key(&id) {
            if let Some(to_be_dropped) = self.packet_send.remove(&id) {
                drop(to_be_dropped);
                //println!("Client {} no more has a connection to {}!", self.node_id, node_id);
            }
        }
    }
}

impl NetworkEdgeErrors for ClientStruct {
    // Ask a neighbour to reveal its node type.
    fn check_type(&mut self, id: NodeId) {
        let req = TypeExchange::TypeRequest { from: self.node_id };
        let exc = ContentType::TypeExchange(req);
        let s_id = self.get_session_id();
        self.send_message(Message::new(self.node_id, s_id, exc), id);

        if DEBUG_MODE {
            println!("sent check from {} to {id}", self.node_id);
        }
    }

    // Verify a destination is in a valid (client) state.
    fn is_state_ok(&self, node_id: NodeId) -> bool {
        let out =  match self.network.get_state(&node_id) {
            Some(s) => {
                s == 1
            }
            None =>{false}
        };
        if !out && DEBUG_MODE{
            println!("dst state was not ok");
        }
        out
    }

    // Forward an edge-level NACK message.
    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.send_message(nack, dst);
    }

    // As an intermediate (drone), emit a NACK toward the source.
    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType, session_id: u64) {
        let new_nack = Nack{
            fragment_index: 0,
            nack_type: nack,
        };
        if let Some(shr) = self.network.get_srh(&self.node_id, &dst){
            let first_hop = shr.current_hop().unwrap_or(self.node_id);
            let packet = Packet{
                routing_header: shr,
                session_id,
                pack_type: Nack(new_nack),
            };

            match self.packet_send.get(&first_hop){
                None => {
                    self.send_event(MissingDestination(self.node_id, dst));
                }
                Some(sender) => {
                    sender.send(packet).unwrap();
                }
            }
        } else {
            self.send_event(MissingDestination(self.node_id, dst));
        }
    }
}

impl ClientTrait for ClientStruct {
    fn new(node_id: NodeId,
           command_recv: Receiver<ClientCommand>,
           event_send: Sender<ClientEvent>,
           packet_recv: Receiver<Packet>,
           packet_send: HashMap<NodeId,
               Sender<Packet>>) -> Self {
        Self {
            node_id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            flood_ids: HashSet::default(),
            used_session_id: HashSet::default(),
            network: Network::new(),
            fragments: HashMap::default(),
            unsent_fragments: HashMap::new(),
            is_flooding: false,
            flood_count: 0,
            is_running: true,
            _phantom: PhantomData,
        }
    }

    // Functions intentionally delegated to concrete client implementations
    fn run(&mut self) {
        unreachable!();
    }

    fn handle_command(&mut self, _command: ClientCommand) {
        unreachable!()
    }

    fn get_client_type(&self) -> ClientType {
        unreachable!()
    }

    // Notify the simulation controller (non-blocking).
    fn send_event(&self, ce: ClientEvent) {
        match self.event_send.try_send(ce.clone()){
            Ok(_) => {
            }
            Err(_err) => {
                if DEBUG_MODE {
                    println!("{} - simulation control unreachable for {:?}", self.node_id, ce)
                }
            }
        }
    }
}

impl ClientStruct {
    // Forward a packet we're relaying (drone behaviour).
    pub(crate) fn send_as_drone(&mut self, mut packet: Packet) {
        packet.routing_header.hop_index += 1;
        if let Some(&next_id) = packet.routing_header.hops.get(packet.routing_header.hop_index) {
            match self.packet_send.get(&next_id) {
                None => {
                    self.network.remove_faulty_connection(self.node_id, next_id);
                    self.send_event(MissingRoute(self.get_src_id(), next_id));
                    // Emit a routing error back to the source.
                    self.send_drone_nack(packet.routing_header.source().unwrap(), ErrorInRouting(next_id), packet.session_id);
                }
                Some(sender) => {
                    match sender.try_send(packet.clone()) {
                        Err(_) => {
                            // Mirror drone error semantics back to the source.
                            self.send_drone_nack(packet.routing_header.source().unwrap(), ErrorInRouting(next_id), packet.session_id);
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

    // Fragment a message and push all fragments into the network.
    pub (crate) fn client_send_in_fragments(&mut self, message: Message, destination: NodeId) {
        let session_id = message.session_id;
        let frags = Self::fragment_message(&message);
        self.fragments.insert((session_id, self.node_id), (destination, Some(message.content), frags.clone()));
        // Keep a local copy for possible retransmissions.
        for fragment in frags {
            self.send_fragment(fragment, destination, session_id);
            // Dispatch each fragment individually.
        }
    }

    // Retry backlog: resend fragments that were deferred.
    pub (crate) fn process_unsent_periodically(&mut self) {
        // Snapshot items to avoid borrowing conflicts during iteration.
        let mut to_process = Vec::new();
        for (identifier, content) in self.unsent_fragments.iter() {
            for fragment in content.1.iter() {
                to_process.push((fragment.clone(), identifier.clone(), content.0));
            }
        }
        // Clear the backlog map to prevent duplicates.
        self.unsent_fragments = HashMap::new();
        for (fragment, identifier, dst) in to_process {
            self.send_fragment(fragment.clone(), dst, identifier.0); // Re-dispatch
        }
    }

    //Function called periodically to check type for any node for which we still didn't get a type response.
    pub (crate) fn periodic_check_type(&mut self) {
        for i in self.network.get_unresolved() {
            self.check_type(i)
        }
    }

    // Handle a NACK produced by a drone along the path.
    pub (crate) fn handle_nack(&mut self, nack: Nack, packet: Packet) {
        match nack.nack_type.clone() {
            NackType::UnexpectedRecipient(wrong_node) => {
                self.network.remove_node(wrong_node);
                self.send_fragment_after_nack(packet.session_id, nack);
            },
            ErrorInRouting(wrong_node) => {
                // Remove routes that traverse the suspected failed node.
                self.network.remove_node(wrong_node);
                self.send_fragment_after_nack(packet.session_id, nack);
            },
            NackType::DestinationIsDrone => {
                let wrong_node = packet.routing_header.hops.last().unwrap();
                self.network.update_state(*wrong_node, 2);
                // Destination was a drone: mark state and drop the message.
            },
            NackType::Dropped => {
                // The dropper is the source of the NACK.
                let dropper = packet.routing_header.source().unwrap();
                self.network.negative_feedback(dropper);
                // Try again.
                self.send_fragment_after_nack(packet.session_id, nack);
            }
        }
    }

    // Handle an edge-level NACK.
    pub (crate) fn handle_edge_nack(&mut self, nack: EdgeNackType, src: NodeId) {
        match nack {
            EdgeNackType::UnexpectedMessage => {
                self.network.update_state(src, 2);
            }
        }
    }

    // Integrate flood response into the topology and update flood state.
    pub (crate) fn save_flood_response(&mut self, pack: Packet) {
        if let FloodResponse(flood_resp) = pack.pack_type {
            self.network.add_route(self.node_id, flood_resp.path_trace.clone());

            // Either we've learned all current routes or the flood counter tripped the cap; allow flooding again.
            if self.network.has_all_routes(self.node_id) || self.flood_count >= 200 {
                // Reset flood gating.
                self.is_flooding = false;
                self.flood_count = 0;

                if DEBUG_MODE {
                    println!("now {} can flood again", self.node_id)
                }
            } else {
                self.flood_count += 1;
            }
        }
    }

    pub (crate) fn handle_flood_request(&mut self, mut packet: Packet) -> bool {
        if let FloodRequest(flood_request) = packet.pack_type.clone() {
            if self.flood_ids.insert((
                flood_request.flood_id.clone(),
                flood_request.initiator_id.clone(),
            )) {
                if self.packet_send.len() == 1 {
                    false
                } else {
                    let mut prev = flood_request.initiator_id.clone();
                    if flood_request.path_trace.clone().len() > 1 {
                        prev = flood_request
                            .path_trace
                            .get(flood_request.path_trace.len() - 2)
                            .unwrap()
                            .0;
                    }

                    for (key, sender) in self.packet_send.iter() {
                        //println!("Previous: {}", prev);
                        if *key != prev {
                            // Forward flood to all neighbours except the sender.
                            if sender.send(packet.clone()).is_ok() {
                                self.send_event(ClientEvent::PacketSent(packet.clone()));
                                // Notify the sim controller on successful send.
                            } // Silently ignore unreachable neighbours.
                        }
                    }
                    true
                }
            } else {
                false
            }
        } else {
            unreachable!();
        }
    }

    // Identify this build flavor (purely informational; no behavior change).
    pub fn build_flavor(&self) -> &'static str {
        BUILD_FLAVOR
    }

    // Liveness flag for the client.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    // Simulate a crash by flipping the liveness flag.
    pub fn crash(&mut self){
        self.is_running = false;
    }

    // Borrow the command receiver.
    pub (crate) fn command_recv(&self) -> &Receiver<ClientCommand> {
        &self.command_recv
    }

    // Borrow the event sender.
    pub (crate) fn event_send(&self) -> &Sender<ClientEvent> {
        &self.event_send
    }

    // Borrow the inbound packet receiver.
    pub (crate) fn packet_recv(&self) -> &Receiver<Packet> {
        &self.packet_recv
    }

    // Borrow the outbound packet sender map.
    pub (crate) fn packet_send(&mut self) -> &mut HashMap<NodeId, Sender<Packet>> {
        &mut self.packet_send
    }

    // Read-only view of seen flood IDs.
    pub (crate) fn flood_ids(&self) -> &HashSet<(u64, NodeId)> {
        &self.flood_ids
    }

    // Read-only view of allocated session IDs.
    pub (crate) fn used_session_id(&self) -> &HashSet<u64> {
        &self.used_session_id
    }

    // Mutable access to the local network view.
    pub (crate) fn network(&mut self) -> &mut Network {
        &mut self.network
    }

    // Mutable access to the local sent-fragments store.
    pub (crate) fn fragments(&mut self) -> &mut HashMap<(u64, NodeId), (NodeId, Option<ContentType>, Vec<Fragment>)> {
        &mut self.fragments
    }

    // Read-only access to deferred fragments.
    pub (crate) fn unsent_fragments(&self) -> &HashMap<(u64, NodeId), (NodeId, Vec<(Fragment)>)> {
        &self.unsent_fragments
    }

    // Are we currently rate-limiting floods?
    pub (crate) fn is_flooding(&self) -> bool {
        self.is_flooding
    }

    // How many flood responses processed since last reset.
    pub (crate) fn flood_count(&self) -> u64 {
        self.flood_count
    }
}
