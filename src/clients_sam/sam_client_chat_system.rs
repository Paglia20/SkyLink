// Chat client built on ClientStruct: routing, flooding, fragmenting, and chat-specific handling.
use crate::clients_gio::client_command::ClientEvent::{ErrorReassembling, ReceivedChatText, RegisterSuccessfully, SendContactsToSC, SendDestinations};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::clients_gio::client_type::ClientType::*;
use crate::clients_sam::sam_client_base::ClientStruct;
use crate::message::EdgeNackType::*;
use crate::message::TextRequest::*;
use crate::message::{ChatResponse, ContentType, Message, TypeExchange};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::server::server_type::ServerType;
use crate::{NO_SERVER_MODE, DEBUG_MODE};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::thread::sleep;
use std::time::Duration;
use wg_2024::network::NodeId;
use wg_2024::packet::PacketType::*;
use wg_2024::packet::{Fragment, Nack, NackType, NodeType, Packet, PacketType};
use crate::message::ChatRequest::{ClientList, Register, SendMessage};
use crate::message::ContentType::*;
use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use crate::clients_sam::sam_client_base::ChatTag;

const BUILD_FLAVOR: &str = "alt-chat-v1";

type ChatMsg = (NodeId, String);
const NOTIFY:bool = false;

// Type tag imported from the client base

pub struct ChatClient {
    // Shared networking core for clients.
    client_base: ClientStruct<ChatTag>,

    // Contacts map: peer -> candidate servers enabling the path. Shared for cross-thread observers.
    contact_list: Arc<Mutex<HashMap<NodeId, Vec<NodeId>>>>,

    // Chat client state.
    // Per-contact transcript of sent/received messages. Shared for cross-thread observers.
    all_messages: Arc<Mutex<HashMap<NodeId, Vec<ChatMsg>>>>,

    // Chat client state.
    // Servers this client has successfully registered with. Shared for cross-thread observers.
    registered_to: Arc<Mutex<HashSet<NodeId>>>,

    sound: Vec<u8>,
    _marker: PhantomData<ChatTag>,
}

impl NetworkEdge for ChatClient {
    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        self.client_base.send_fragment(fragment, destination, session_id)
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.client_base.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, packet_session_id: u64, nack: Nack) {
        self.client_base.send_fragment_after_nack(packet_session_id, nack)
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        self.client_base.send_ack(packet, fragment_index);
    }

    fn flood(&mut self) {
        self.client_base.flood();
    }

    fn get_flood_id(&mut self) -> u64 {
        self.client_base.get_flood_id()
    }

    fn get_session_id(&mut self) -> u64 {
        self.client_base.get_session_id()
    }

    fn get_src_id(&self) -> NodeId {
        self.client_base.get_src_id()
    }

    fn remove_sender(&mut self, id: NodeId) {
        self.client_base.remove_sender(id);
    }

    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.client_base.send_message(message, destination)
    }

    // Handle inbound packets (chat client role).
    fn handle_packet(&mut self, mut packet: Packet) {
        if let FloodRequest(mut flood_request) = packet.pack_type.clone(){
            flood_request
                .path_trace
                .push((self.client_base.get_src_id(), NodeType::Client));

            // Update the packet's path_trace.
            packet.pack_type = FloodRequest(flood_request.clone());

            if !self.client_base.handle_flood_request(packet) {
                self.edge_send_flood_response(flood_request);
            }
        } else if packet.routing_header.destination().unwrap() != self.client_base.get_src_id() {
            // Not for us: act as a relay (drone).
            self.client_base.send_as_drone(packet);
        } else {
            // We are the destination; handle by packet type.
            match packet.pack_type.clone() {
                MsgFragment(fragment) => {
                    let tot_num_frag = fragment.total_n_fragments as usize;
                    let session_id = packet.session_id;
                    let initiator_id = packet.routing_header.hops[0];
                    let destination = self.client_base.get_src_id(); //he is the destination
                    let frag_index = fragment.fragment_index;

                    // Buffer the arriving fragment.
                    let entry = self.client_base.fragments().entry((session_id, initiator_id)).or_insert((destination, None, vec![]));
                    entry.2.push(fragment);

                    // ACK each received fragment.
                    self.send_ack(packet.clone(), frag_index);

                    // Notify the simulation controller about receipt.
                    self.send_event(ClientEvent::PacketReceived(packet.clone()));
                    // Notify telemetry about packet reception
                    self.client_base.on_packet_received();
                    // Notify telemetry about packet reception
                    self.client_base.on_packet_received();

                    // If all fragments have arrived, reconstruct the message.
                    let frags_clone = &self.client_base.fragments().get(&(packet.session_id, initiator_id)).unwrap().2;
                    if frags_clone.len() == tot_num_frag {
                        let message = match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                            Ok(mess) => { mess }
                            Err(e) => {
                                if DEBUG_MODE {
                                    println!("{e} with {}", self.client_base.get_src_id());
                                }
                                self.send_event(ErrorReassembling(self.get_src_id()));
                                return;
                            }
                        };
                        // Deliver the reassembled message to the handler.
                        self.handle_message(message);

                        // Clear saved fragments for this session.
                        self.client_base.fragments().remove(&(packet.session_id, initiator_id));
                    }
                }
                Ack(ack) => {
                    self.send_event(ClientEvent::AckReceived(packet.clone()));
                    // ACKs reference the original destination as source; optionally notify SC when relevant.
                    // Record ACK RTT for telemetry
                    self.client_base.on_ack_received(packet.session_id, ack.fragment_index);
                    let src = self.client_base.get_src_id();
                    match self.client_base.fragments().get_mut(&(packet.session_id,src)) {
                        None => {}
                        Some((_, cont, vec)) => {
                            vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                            // If no fragments remain, all ACKs have arrived; clean up.
                            if vec.is_empty() {
                                // Registration flow complete: mark server and notify.
                                if let Some(ChatRequest(Register(_node))) = cont{
                                    let server = packet.routing_header.source().unwrap();
                                    if let Ok(mut set) = self.registered_to.lock() {
                                        set.insert(server);
                                    }
                                    self.send_event(RegisterSuccessfully(self.get_src_id(), server));
                                }
                                let src = self.client_base.get_src_id();
                                self.client_base.fragments().remove_entry(&(packet.session_id, src));
                            }
                        }
                    }

                    // Reward successful path with positive feedback.
                    let nodes = packet.routing_header.hops;
                    self.client_base.network().positive_feedback(nodes);
                }

                PacketType::Nack(nack) => {
                    self.send_event(ClientEvent::NackReceived(packet.clone()));
                    // Delegate NACK handling to the base.
                    self.client_base.handle_nack(nack, packet);
                }
                FloodRequest(_) => {
                    unreachable!()
                }
                FloodResponse(_) => {
                    self.client_base.save_flood_response(packet);
                }
            }
        }
    }

    /// Handle a fully reassembled message (chat client).
    fn handle_message(&mut self, message: Message) {
        match message.content {
            ContentType::ChatResponse(resp) => {
                match resp{
                    ChatResponse::ClientList(list) => {
                        let source= message.source_id;
                        if let Ok(mut contacts) = self.contact_list.lock() {
                            for i in list {
                                let is_new = contacts.entry(i).and_modify(|vec| vec.push(source)).or_insert(vec![source]).len() == 1;
                                if is_new {
                                    // Inform SC about the newly learned contact.
                                    self.send_event(SendContactsToSC(self.client_base.get_src_id(), i));
                                }
                            }
                        }
                    }
                    ChatResponse::MessageFrom { from, message } => {
                        // Message arrived intact: notify SC and store locally.
                        self.send_event(ReceivedChatText(from, self.client_base.get_src_id(), message.clone()));
                        if let Ok(mut msgs) = self.all_messages.lock() {
                            msgs.entry(from).or_insert(vec![(from, message.clone())]).push((from, message));
                        }
                        if NOTIFY {
                            self.play_notification_sound();
                        }
                    }
                    ChatResponse::ClientNotFound(node) => {
                        // Remove the server that reported the client as missing.
                        let server_src = message.source_id;
                        if let Ok(mut contacts) = self.contact_list.lock() {
                            if let Some(vec) = contacts.get_mut(&node){
                                vec.retain(|n| *n != server_src);
                            }
                        }
                    }
                }
            }

            ContentType::TypeExchange(exchange) => {
                match exchange {
                    TypeExchange::TypeRequest { from } => {
                        let type_resp = TypeExchange::TypeResponse {
                            edge_type: EdgeType::Client(ClientType::ChatClient),
                            from: self.client_base.get_src_id(),
                        };
                        let message = Message::new(self.client_base.get_src_id(), self.get_session_id(), ContentType::TypeExchange(type_resp));
                        self.send_message(message, from);
                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        match edge_type{
                            EdgeType::Server(ServerType::Chat) => {
                                self.client_base.network().update_state(from, 1);
                                self.send_event(SendDestinations(self.client_base.get_src_id(), from));
                            },

                            _ => {
                                self.client_base.network().update_state(from, 2);
                            }
                        }

                    }
                }
            }
            EdgeNack(nack) => {
                self.client_base.handle_edge_nack(nack, message.source_id);

            },
            _ => {
                // Ignore unrelated request types and reply with an edge-level NACK.
                let new_nack = self.create_nack(UnexpectedMessage);
                self.send_nack_message(message.source_id, new_nack);
            }
        }

    }
}

impl NetworkEdgeErrors for ChatClient {
    fn check_type(&mut self, id: NodeId) {
        self.client_base.check_type(id);
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        self.client_base.is_state_ok(node_id)
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.client_base.send_nack_message(dst, nack);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType, session_id: u64) {
        self.client_base.send_drone_nack(dst, nack, session_id);
    }
}

impl ClientTrait for ChatClient {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        //load notification
        let mut buffer = Vec::new();
        if NOTIFY {
            let mut file = File::open("src/clients_gio/notification.mp3").unwrap();
            file.read_to_end(&mut buffer).unwrap();
        }

        ChatClient {
            client_base: ClientStruct::<ChatTag>::new(node_id, command_recv, event_send, packet_recv, packet_send),
            contact_list: Arc::new(Mutex::new(HashMap::new())),
            all_messages: Arc::new(Mutex::new(HashMap::new())),
            registered_to: Arc::new(Mutex::new(HashSet::new())),
            sound: buffer,
            _marker: PhantomData,
        }
    }


    // Main event loop for the chat client (delegates I/O to the base).
    fn run(&mut self) {
        let mut count = 0;

        while self.client_base.is_running() {
            select_biased! {
                recv(self.client_base.command_recv()) -> cmd => {
                    if let Ok(command) = cmd {
                        self.handle_command(command);
                    }
                }
                recv(self.client_base.packet_recv()) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
                default => {
                     sleep(Duration::from_millis(10));
                    // Brief pause to avoid busy-waiting.
                }
            }
            // Throttle periodic work via a simple counter.
            if count >= 128 {
                // Probe unresolved neighbour types.
                self.client_base.periodic_check_type();

                self.client_base.process_unsent_periodically();

                // Periodic telemetry snapshot (visible when DEBUG_MODE = true)
                if DEBUG_MODE {
                    let t = self.client_base.telemetry_snapshot();
                    println!(
                        "[TELEM:{}] node={} sent={} recv={} inflight={} last_ack_ms={} avg_ack_ms={:?} neigh={}",
                        self.client_base.build_flavor(),
                        t.node_id,
                        t.packets_sent,
                        t.packets_received,
                        t.inflight_awaiting_acks,
                        t.last_ack_ms,
                        t.avg_ack_ms,
                        t.neighbors
                    );
                }

                // Reset counter.
                count = 0;
            } else {
                count += 1;
            }
        }
    }

    ///Handle Chat Client Commands
    fn handle_command(&mut self, command: ClientCommand) {
        match command {
            ClientCommand::RemoveSender(node_id) => {
                self.remove_sender(node_id);
            }
            ClientCommand::AddSender(node_id, sender) => {
                self.client_base.packet_send().insert(node_id, sender);
            }

            ClientCommand::Flood =>{
                self.flood();
            }

            // Chat-specific commands
            ClientCommand::RetrieveList(id) => {
                self.get_list(id);
            }
            ClientCommand::Register(id) => {
                self.register(id);
            }
            ClientCommand::SendMSG(id, str) => {
                self.send_chat_text(id, str);
            }
            ClientCommand::InstantCrash => {
                self.client_base.crash();
            }

            _ =>{
                // Ignore non-chat commands.
            }
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::ChatClient
    }

    fn send_event(&self, ce: ClientEvent) {
        self.client_base.send_event(ce);
    }
}


// ChatClient helpers.
impl ChatClient {

    // Build flavor identifier
    pub fn build_flavor(&self) -> &'static str {
        BUILD_FLAVOR
    }

    // Request the current contact list from a server.
    fn get_list(&mut self, id: NodeId){
        let src = self.client_base.get_src_id();
        let session = self.client_base.get_session_id();
        let content = ChatRequest(ClientList);
        let msg = Message::new(src, session, content);
        self.client_base.send_message(msg, id);

        // Debug trace for list request.
        if DEBUG_MODE{
            println!("sent client list req from {src} to server {id}")
        }
    }

    // Register this client with a server.
    fn register(&mut self, id: NodeId){
        let src = self.get_src_id();
        let session = self.get_session_id();
        let content = ChatRequest(Register(src));
        let msg = Message::new(src, session, content);
        self.client_base.send_message(msg, id);

        // Debug trace for registration request.
        if DEBUG_MODE{
            println!("sent register req from {src} to server {id}")
        }
    }

    // Send a text to peer `id`, choosing an optimal server when possible.
    fn send_chat_text(&mut self, id: NodeId, str: String){
        // Local echo when running without servers.
        if NO_SERVER_MODE {
            self.send_event(ReceivedChatText(self.get_src_id(), id, str.clone()));
        }

        let src = self.get_src_id();
        let servers_opt = { self.contact_list.lock().ok().and_then(|map| map.get(&id).cloned()) };
        if let Some(servers) = servers_opt {

            // Consider only servers we are registered with.
            let available_servers: Vec<NodeId> = {
                let reg = self.registered_to.lock().unwrap();
                servers.into_iter().filter(|x| reg.contains(x)).collect()
            };

            let src = self.client_base.get_src_id();
            if let Some(server_id) = self.client_base.network().get_optimal_dest(&src, &available_servers){
                // Choose the server (current policy: first optimal).

                let session = self.get_session_id();
                let content = ChatRequest(SendMessage {
                    from: src,
                    to: id,
                    message: str.clone(),
                });

                // Append to local transcript.
                if let Ok(mut msgs) = self.all_messages.lock() {
                    msgs.entry(id).or_insert(vec!((src, str.clone()))).push((src, str));
                }


                let msg = Message::new(src, session, content);
                self.client_base.send_message(msg, server_id);
            }
        } else {
            self.send_event(ClientEvent::MissingContacts(src,id))
        }

    }

    // Play the incoming-message notification sound (if enabled).
    fn play_notification_sound(&self) {
        let (_stream, stream_handle) = OutputStream::try_default().unwrap();
        let sink = Sink::try_new(&stream_handle).unwrap();

        let cursor = Cursor::new(self.sound.clone());
        let source = Decoder::new(BufReader::new(cursor)).unwrap();

        sink.append(source);
        sink.sleep_until_end();
    }

    // Expose telemetry snapshot from the base
    pub fn telemetry(&self) -> crate::clients_sam::sam_client_base::SamTelemetry {
        self.client_base.telemetry_snapshot()
    }

    // Expose shared views guarded by Arc<Mutex<...>>
    pub fn shared_contacts_handle(&self) -> Arc<Mutex<HashMap<NodeId, Vec<NodeId>>>> {
        self.contact_list.clone()
    }

    pub fn shared_messages_handle(&self) -> Arc<Mutex<HashMap<NodeId, Vec<ChatMsg>>>> {
        self.all_messages.clone()
    }

    pub fn shared_registered_handle(&self) -> Arc<Mutex<HashSet<NodeId>>> {
        self.registered_to.clone()
    }
}
