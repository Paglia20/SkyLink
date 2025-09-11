// Web browser client built on ClientStruct: discovery, fragment reassembly, and text/media fetching.
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_sam::sam_client_base::ClientStruct;
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::message::MediaRequest::{Media};
use crate::message::TextRequest::*;
use crate::message::{ContentType, MediaResponse, Message, TypeExchange, TextResponse};
use crate::network_edge::{EdgeType, NetworkEdge, NetworkEdgeErrors};
use crate::{NO_SERVER_MODE, DEBUG_MODE};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, NodeType, Packet, PacketType};
use wg_2024::packet::PacketType::{Ack, FloodRequest, FloodResponse, MsgFragment};
use crate::clients_gio::client_command::ClientEvent::{ErrorReassembling, MissingDestForMedia, MissingTextList, SendCatalogue, SendDestinations, SendMedia, SendTextList};
use crate::message::EdgeNackType::UnexpectedMessage;
use crate::server::server_type::{ContentServerType, ServerType};
use base64::{engine::general_purpose, Engine};
use std::fs::write;
use open;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use crate::clients_sam::sam_client_base::WebTag;

const BUILD_FLAVOR: &str = "alt-web-v1";

type ArrivedMedia = (String, Vec<u8>);
const OPEN_AUTO_MEDIA:bool = true;
// Using WebTag from sam_client_base

pub struct WebBrowser{
    // Shared networking core for clients.
    client_base: ClientStruct<WebTag>,
    // Stored text lists: id -> (servers, name).
    available_text_lists: Arc<Mutex<HashMap<u64, (Vec<NodeId>, String)>>>,
    // Media catalogue: media_id -> candidate servers.
    catalogue: Arc<Mutex<HashMap<u64, Vec<NodeId>>>>,
    // Cached media content: id -> (name, bytes).
    arrived_content: Arc<Mutex<HashMap<u64, ArrivedMedia>>>,
    _marker: PhantomData<WebTag>,
}

impl NetworkEdge for WebBrowser {
    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        self.client_base.send_fragment(fragment, destination, session_id)
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.client_base.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, packet_session_id: u64, nack: Nack)  {
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

    /// Handle inbound packets (web browser role).
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
        }
        else if packet.routing_header.destination().unwrap() != self.client_base.get_src_id() {
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

                    // If all fragments have arrived, reconstruct the message.
                    let frags_clone = &self.client_base.fragments().get(&(packet.session_id, initiator_id)).unwrap().2;
                    if frags_clone.len() == tot_num_frag {
                        let message = match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                            Err(e) => {
                                if DEBUG_MODE {
                                    println!("{e} with {}", self.client_base.get_src_id());
                                }
                                self.send_event(ErrorReassembling(self.get_src_id()));
                                return;
                            }
                            Ok(mess) => { mess }
                        };
                        // Deliver the reassembled message to the handler.
                        self.handle_message(message);

                        // Clear saved fragments for this session.
                        self.client_base.fragments().remove(&(packet.session_id, initiator_id));
                    }
                }
                Ack(ack) => {
                    self.send_event(ClientEvent::AckReceived(packet.clone()));
                    // Record ACK RTT for telemetry
                    self.client_base.on_ack_received(packet.session_id, ack.fragment_index);
                    // ACKs reference the original destination as source; clean up when all fragments are acknowledged.
                    let src = self.client_base.get_src_id();
                    match self.client_base.fragments().get_mut(&(packet.session_id,src)) {
                        None => {}
                        Some((_, _, vec)) => {
                            vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                            if vec.is_empty() {
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
                FloodResponse(_flood_resp) => {
                    self.client_base.save_flood_response(packet);
                }
            }
        }
    }

    // Handle a fully reassembled message (web browser).
    fn handle_message(&mut self,message: Message )  {
        let src = message.source_id;

        match message.content {
            ContentType::MediaResponse(media_response) => {
                match media_response{
                    MediaResponse::MediaList(_list) => {
                        let new_nack = self.create_nack(UnexpectedMessage);
                        self.send_nack_message(message.source_id, new_nack);
                    }
                    MediaResponse::Media(id, name, media) => {
                        if let Ok(mut map) = self.arrived_content.lock() {
                            map.insert(id, (name.clone(), media.clone()));
                        }
                        self.send_event(SendMedia(self.get_src_id(), id, name, media.clone()));

                        if OPEN_AUTO_MEDIA {
                            self.open_media(media)
                        }
                    }
                    MediaResponse::NotFound(id) => {
                        // Update the catalogue.
                        if let Ok(mut cat) = self.catalogue.lock() {
                            if let Some(vec) = cat.get_mut(&id){
                                vec.retain(|node| *node != src);
                            }
                        }
                        // Try to fetch the same media from another server.
                        self.get_media(id);
                    }
                }
            },
            ContentType::TextResponse(text_response) => {
                match text_response{
                    TextResponse::TextList(map) => {
                        if let Ok(mut tl) = self.available_text_lists.lock() {
                            for (text_file_id, name) in map {
                                let entry = tl.entry(text_file_id).or_insert((vec![], name.clone()));
                                entry.0.push(src);
                                if entry.0.len() == 1 {
                                    self.send_event(SendTextList(self.get_src_id(), text_file_id, name))
                                }
                            }
                        }
                    }
                    TextResponse::MediaReferences(media_refs) => {
                        if let Ok(mut cat) = self.catalogue.lock() {
                            for (media_id, (name, media_server_id)) in media_refs{
                                let mut is_new= false;
                                let entry =  cat.entry(media_id).or_insert(vec![]);
                                for e in media_server_id {
                                    entry.push(e);
                                    if entry.len() == 1 {
                                        is_new = true;
                                    }
                                }
                                if is_new {
                                    self.send_event(SendCatalogue(self.get_src_id(), media_id, name))
                                }
                            }
                        }
                    }
                    TextResponse::Incomplete(incomplete_text) => {
                        self.retry_get_text_file(incomplete_text);
                    }
                    TextResponse::NotFound(text_id) => {
                        //update catalogue
                        if let Ok(mut tl) = self.available_text_lists.lock() {
                            tl.entry(text_id).and_modify(|(v, _)| v.retain(|node_id| *node_id != src));
                        }

                        //retry to obtain it
                        self.get_text_file(text_id);
                    }
                }
            }

            ContentType::TypeExchange(exchange) => {
                match exchange {
                    TypeExchange::TypeRequest { from } => {
                        let type_resp = TypeExchange::TypeResponse {
                            edge_type: EdgeType::Client(ClientType::WebBrowser),
                            from: self.client_base.get_src_id(),
                        };
                        let message = Message::new(self.client_base.get_src_id(), self.get_session_id(), ContentType::TypeExchange(type_resp));

                        self.send_message(message, from);

                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        if let EdgeType::Server(server_type) = edge_type{
                            if let ServerType::Content(ty) = server_type{
                                self.client_base.network().update_state(from, 1);
                                if ty == ContentServerType::Text{
                                    // Only text servers are exposed to SC as destinations for list queries; media is handled via catalogue.
                                    self.send_event(SendDestinations(self.client_base.get_src_id(), from));
                                }
                            }
                            else {
                                self.client_base.network().update_state(from, 2);
                            }
                        } else {
                            //if it's a client
                            self.client_base.network().update_state(from, 2);

                            if NO_SERVER_MODE {
                                self.send_event(SendDestinations(self.client_base.get_src_id(), from));
                            }
                        }
                    }
                }
            }
            ContentType::EdgeNack(nack) => {
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

impl NetworkEdgeErrors for WebBrowser {
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

impl ClientTrait for WebBrowser {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        WebBrowser {
            client_base: ClientStruct::<WebTag>::new(node_id, command_recv, event_send, packet_recv, packet_send),
            available_text_lists: Arc::new(Mutex::new(HashMap::new())),
            arrived_content: Arc::new(Mutex::new(HashMap::new())),
            catalogue: Arc::new(Mutex::new(HashMap::new())),
            _marker: PhantomData,
        }
    }

    /// Main event loop for the web browser (delegates I/O to the base).
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
                    // Brief pause to avoid busy-waiting.
                     sleep(Duration::from_millis(11));
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


    //Handle Web Browser Commands
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
            ClientCommand::RetrieveList(id) => {
                self.get_list(id);
            }
            ClientCommand::GetTextFile(id) => {
                self.get_text_file(id);
            }

            // WebClient-specific command
            ClientCommand::GetContent(id) => {
                self.get_media(id);
            }

            ClientCommand::InstantCrash => {
                self.client_base.crash();
            }
            // Ignore non-web commands.
            _ =>{

            }
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::WebBrowser
    }

    fn send_event(&self, ce: ClientEvent) {
        self.client_base.send_event(ce);
    }
}

// WebBrowser helpers.
impl WebBrowser{

    // Build flavor identifier
    pub fn build_flavor(&self) -> &'static str {
        BUILD_FLAVOR
    }

    // Request the list of texts from a server.
    fn get_list(&mut self, id: NodeId) {
        let src = self.client_base.get_src_id();
        let session = self.client_base.get_session_id();
        let content = ContentType::TextRequest(TextList);
        let msg = Message::new(src, session, content);
        self.client_base.send_message(msg, id);
        // Debug trace for list request.
        if DEBUG_MODE {
            println!("Sent text list request from {src} to server {id}");
        }
    }

    // Request a specific text file by ID.
    fn get_text_file(&mut self, text_file_id: u64) {
        self.client_base.get_src_id();
        let session = self.client_base.get_session_id();

        let destinations = {
            self.available_text_lists
                .lock()
                .ok()
                .and_then(|m| m.get(&text_file_id).cloned())
                .map(|pair| pair.0)
                .unwrap_or_default()
        };
            if !destinations.is_empty() {
                let src = self.client_base.get_src_id();
                if let Some(dst) = self.client_base.network().get_optimal_dest(&src, &destinations) {
                    let content = ContentType::TextRequest(TextFile(text_file_id));
                    let msg = Message::new(src, session, content);
                    self.client_base.send_message(msg, dst);
                    if DEBUG_MODE {
                        println!("Sent text file request from {src} to server {dst}");
                    }
                }
            }
        if destinations.is_empty() {
            self.send_event(MissingTextList(self.get_src_id(), text_file_id))
        }
    }

    // Retry fetching a text file that arrived incomplete.
    fn retry_get_text_file(&mut self, text_file_id: u64) {
        let wait_time: u32 = (u16::MAX as u32) * 2u32;
        for _ in 0..wait_time {

        }
        self.get_text_file(text_file_id)
    }

    // Retrieve a media file using the catalogue; open immediately if cached.
    fn get_media(&mut self, cont_id: u64) {
        if let Some(bytes) = { self.arrived_content.lock().ok().and_then(|m| m.get(&cont_id).map(|(_, b)| b.clone())) } {
            self.open_media(bytes)
        } else {
            println!("got here");
            let src = self.client_base.get_src_id();
            let session = self.client_base.get_session_id();

            let destinations = { self.catalogue.lock().ok().and_then(|c| c.get(&cont_id).cloned()).unwrap_or_default() };
            if !destinations.is_empty() {
                if let Some(dst) = self.client_base.network().get_optimal_dest(&src, &destinations) {
                        let content = ContentType::MediaRequest(Media(cont_id));
                        let msg = Message::new(src, session, content);
                        self.client_base.send_message(msg, dst);
                        if DEBUG_MODE {
                            println!("Sent media request from {src} to server {dst}");
                        }
                }
            } else {
                self.send_event(MissingDestForMedia(self.get_src_id(), cont_id))
            }
        }
    }

    // Render media as a data-URL in a temporary HTML file and open it.
    fn open_media(&self, image_bytes: Vec<u8>){
        let base64_image = general_purpose::STANDARD.encode(image_bytes);

        let html_content = format!(
            "<html>
        <head>
            <style>
                img {{
                    max-width: 100%;
                    height: auto;
                    display: block;
                    margin: auto;
                }}
            </style>
        </head>
        <body>
            <img src='data:image/jpeg;base64,{}' />
        </body>
    </html>",
            base64_image
        );

        let file_path = "image.html";
        write(file_path, html_content).expect("error in file");

        if let Err(e) = open::that(file_path) {
            if DEBUG_MODE {
                println!("Error opening file: {}", e);
            }
        }
    }

    // Expose telemetry snapshot from the base
    pub fn telemetry(&self) -> crate::clients_sam::sam_client_base::SamTelemetry {
        self.client_base.telemetry_snapshot()
    }

    // Expose shared views guarded by Arc<Mutex<...>>
    pub fn shared_text_lists_handle(&self) -> Arc<Mutex<HashMap<u64, (Vec<NodeId>, String)>>> {
        self.available_text_lists.clone()
    }

    pub fn shared_catalogue_handle(&self) -> Arc<Mutex<HashMap<u64, Vec<NodeId>>>> {
        self.catalogue.clone()
    }

    pub fn shared_arrived_content_handle(&self) -> Arc<Mutex<HashMap<u64, ArrivedMedia>>> {
        self.arrived_content.clone()
    }
}
