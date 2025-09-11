// Client-side network edge for SkyLink: channels, routing, flooding, acks/nacks, and helpers.
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
use std::thread::sleep;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};

// add a tiny randomized backoff before certain retries/floods
const ENABLE_SAM_JITTER: bool = true;

// Role policy: per-client tag controls jitter and flavor at compile time
mod sealed { pub trait Sealed {} }

pub trait SamRole: sealed::Sealed {
    const BUILD_FLAVOR: &'static str;
    const JITTER_MIN_MS: u64;
    const JITTER_MAX_MS: u64;
}

// Public tags usable by Sam clients
#[derive(Debug, Clone, Copy)]
pub struct ChatTag;

#[derive(Debug, Clone, Copy)]
pub struct WebTag;

impl SamRole for ChatTag {
    const BUILD_FLAVOR: &'static str = "sam-chat";
    const JITTER_MIN_MS: u64 = 2;
    const JITTER_MAX_MS: u64 = 6;
}

impl SamRole for WebTag {
    const BUILD_FLAVOR: &'static str = "sam-web";
    const JITTER_MIN_MS: u64 = 3;
    const JITTER_MAX_MS: u64 = 9;
}

impl sealed::Sealed for ChatTag {}
impl sealed::Sealed for WebTag {}

#[inline]
fn sam_jitter<T: SamRole>() {
    if ENABLE_SAM_JITTER && T::JITTER_MAX_MS > 0 {
        let low = T::JITTER_MIN_MS.min(T::JITTER_MAX_MS);
        let high = T::JITTER_MAX_MS.max(T::JITTER_MIN_MS) + 1; // inclusive
        let delay = fastrand::u64(low..high);
        if delay > 0 { sleep(Duration::from_millis(delay)); }
    }
}




pub struct ClientStruct<T> {
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
    _phantom: PhantomData<T>,
    // Runtime metrics
    metrics: SamMetrics,
    // Track send timestamps for fragments awaiting ACKs
    ack_inflight: HashMap<(u64, u64), Instant>,
    // Shared telemetry snapshot guarded by Arc<Mutex<..>> for cross-thread reads
    shared_telemetry: Arc<Mutex<SamTelemetry>>,
}

impl<T: SamRole> NetworkEdge for ClientStruct<T> {
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
                    // Tiny backoff to avoid flood bursts when many clients discover misses simultaneously
                    sam_jitter::<T>();
                    self.flood();
                }
            }
            Some(srh) => {
                let first_dst = srh.hops[1];
                let packet = Packet::new_fragment(srh, session_id, fragment.clone());

                // Route found: attempt to forward the fragment.
                // Track send time for ACK RTT telemetry before attempting send
                let frag_idx = fragment.fragment_index;
                self.ack_inflight.insert((session_id, frag_idx), Instant::now());

                match self.packet_send.get(&first_dst) {
                    Some(sender) => {
                        sender.send(packet.clone()).unwrap();
                        self.send_event(ClientEvent::PacketSent(packet.clone()));
                        self.metrics.packets_sent += 1;

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
                    // Small randomized delay before re-flooding after a loss
                    sam_jitter::<T>();
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
                        // Small randomized delay before retrying an individual fragment
                        sam_jitter::<T>();
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
                self.send_event(ClientEvent::PacketSent(packet_ack));
                self.metrics.acks_sent += 1;
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
            // Small jitter before starting a flood to de-sync multiple clients
            sam_jitter::<T>();
            self.is_flooding = true;
            self.send_event(ClientEvent::Flooding(self.node_id));
            self.metrics.floods_started += 1;

            let flood_request = wg_2024::packet::FloodRequest {
                flood_id: self.get_flood_id(),
                initiator_id: self.node_id,
                path_trace: vec![(self.node_id, NodeType::Client)],
            };
            let packet = Packet::new_flood_request(SourceRoutingHeader::default(), self.get_session_id(), flood_request);
            self.packet_send.iter().for_each(|(id, sender)| {
                sender.send(packet.clone()).unwrap()
            });
            if DEBUG_MODE {
                println!("[SAM:{}] flooding from {}", T::BUILD_FLAVOR, self.node_id);
            }
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

impl<T: SamRole> NetworkEdgeErrors for ClientStruct<T> {
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
        self.metrics.nacks_sent += 1;
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
                    self.metrics.nacks_sent += 1;
                }
            }
        } else {
            self.send_event(MissingDestination(self.node_id, dst));
        }
    }
}

impl<T: SamRole> ClientTrait for ClientStruct<T> {
    fn new(node_id: NodeId,
           command_recv: Receiver<ClientCommand>,
           event_send: Sender<ClientEvent>,
           packet_recv: Receiver<Packet>,
           packet_send: HashMap<NodeId,
               Sender<Packet>>) -> Self {
        let me = Self {
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
            metrics: SamMetrics::default(),
            ack_inflight: HashMap::new(),
            shared_telemetry: Arc::new(Mutex::new(SamTelemetry {
                node_id,
                build_flavor: T::BUILD_FLAVOR,
                packets_sent: 0,
                packets_received: 0,
                floods_started: 0,
                flood_responses: 0,
                acks_sent: 0,
                nacks_sent: 0,
                deferred_unsent_fragments: 0,
                pending_fragments_outbound: 0,
                neighbors: 0,
                inflight_awaiting_acks: 0,
                last_ack_ms: 0,
                avg_ack_ms: None,
            })),
        };
        if DEBUG_MODE {
            println!("[SAM:{}] client {} initialized", T::BUILD_FLAVOR, node_id);
        }
        me
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

impl<T: SamRole> ClientStruct<T> {
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
            self.metrics.flood_responses += 1;

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

    // Build flavor identifier
    pub fn build_flavor(&self) -> &'static str {
        T::BUILD_FLAVOR
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

    // Increment when a packet is received (called from client implementations)
    pub (crate) fn on_packet_received(&mut self) {
        self.metrics.packets_received += 1;
    }

    // Export a snapshot of runtime metrics for observability
    pub fn telemetry_snapshot(&self) -> SamTelemetry {
        let deferred_unsent_fragments = self
            .unsent_fragments
            .values()
            .map(|(_, vec)| vec.len())
            .sum();
        let pending_fragments_outbound = self
            .fragments
            .values()
            .map(|(_, _, vec)| vec.len())
            .sum();
        let avg_ack_ms = if self.metrics.ack_rtt_count > 0 {
            Some(self.metrics.ack_rtt_total_ms as f64 / self.metrics.ack_rtt_count as f64)
        } else { None };
        let snap = SamTelemetry {
            node_id: self.node_id,
            build_flavor: T::BUILD_FLAVOR,
            packets_sent: self.metrics.packets_sent,
            packets_received: self.metrics.packets_received,
            floods_started: self.metrics.floods_started,
            flood_responses: self.metrics.flood_responses,
            acks_sent: self.metrics.acks_sent,
            nacks_sent: self.metrics.nacks_sent,
            deferred_unsent_fragments,
            pending_fragments_outbound,
            neighbors: self.packet_send.len(),
            inflight_awaiting_acks: self.ack_inflight.len(),
            last_ack_ms: self.metrics.last_ack_ms,
            avg_ack_ms,
        };
        if let Ok(mut guard) = self.shared_telemetry.lock() {
            *guard = snap.clone();
        }
        snap
    }

    // Called by Chat/Web on ACK reception to update latency stats
    pub(crate) fn on_ack_received(&mut self, session_id: u64, fragment_index: u64) {
        if let Some(start) = self.ack_inflight.remove(&(session_id, fragment_index)) {
            let elapsed = start.elapsed().as_millis();
            self.metrics.ack_rtt_total_ms += elapsed as u128;
            self.metrics.ack_rtt_count += 1;
            self.metrics.last_ack_ms = elapsed as u64;
        }
    }

    // Get a clone of the shared telemetry handle for cross-thread access
    pub fn shared_telemetry_handle(&self) -> Arc<Mutex<SamTelemetry>> {
        self.shared_telemetry.clone()
    }
}

// Internal counters and exported telemetry
#[derive(Debug, Clone, Default)]
struct SamMetrics {
    packets_sent: u64,
    packets_received: u64,
    floods_started: u64,
    flood_responses: u64,
    acks_sent: u64,
    nacks_sent: u64,
    ack_rtt_total_ms: u128,
    ack_rtt_count: u64,
    last_ack_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SamTelemetry {
    pub node_id: NodeId,
    pub build_flavor: &'static str,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub floods_started: u64,
    pub flood_responses: u64,
    pub acks_sent: u64,
    pub nacks_sent: u64,
    pub deferred_unsent_fragments: usize,
    pub pending_fragments_outbound: usize,
    pub neighbors: usize,
    pub inflight_awaiting_acks: usize,
    pub last_ack_ms: u64,
    pub avg_ack_ms: Option<f64>,
}
