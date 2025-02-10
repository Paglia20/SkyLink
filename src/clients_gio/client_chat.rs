use crate::clients_gio::client_command::ClientEvent::{ErrorReassembling, ReceivedChatText, RegisterSuccessfully, SendContactsToSC, SendDestinations};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::clients_gio::client_type::ClientType::*;
use crate::clients_gio::client_struct::ClientStruct;
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

type ChatMsg = (NodeId, String);
const NOTIFY:bool = false;

pub struct ChatClient {
    ///Common Client base
    client_base: ClientStruct,

    ///Chat client speck:
    /// First NodeId is the client we communicate with, the second one is the vec of servers that make the connection possible
    contact_list: HashMap<NodeId, Vec<NodeId>>,

    ///Chat client specks
    ///All Messages a Client received and sent to each contact
    all_messages: HashMap<NodeId, Vec<ChatMsg>>,

    ///Chat client specks
    ///All Servers a Client is registered to
    registered_to: HashSet<NodeId>,

    sound: Vec<u8>,
}

impl NetworkEdge for ChatClient {
    fn send_message(&mut self, message: Message, destination: NodeId) {
       self.client_base.send_message(message, destination)
    }

    ///Handle any incoming packet(for chat client)
    fn handle_packet(&mut self, mut packet: Packet) {
        if let FloodRequest(mut flood_request) = packet.pack_type.clone(){
            flood_request
                .path_trace
                .push((self.client_base.get_src_id(), NodeType::Client));

            //I update the path_trace in the packet.
            packet.pack_type = FloodRequest(flood_request.clone());

            if !self.client_base.handle_flood_request(packet) {
                self.edge_send_flood_response(flood_request);
            }
        } else if packet.routing_header.destination().unwrap() != self.client_base.get_src_id() {
            // If it's not his packet, but he has to act as a drone (that never misses)
            self.client_base.send_as_drone(packet);
        } else {
            // We can take for granted he is the destination
            match packet.pack_type.clone() {
                MsgFragment(fragment) => {
                    let tot_num_frag = fragment.total_n_fragments as usize;
                    let session_id = packet.session_id;
                    let initiator_id = packet.routing_header.hops[0];
                    let destination = self.client_base.get_src_id(); //he is the destination
                    let frag_index = fragment.fragment_index;


                    //add new frag
                    let entry = self.client_base.fragments().entry((session_id, initiator_id)).or_insert((destination, None, vec![]));
                    entry.2.push(fragment);


                    //for each arrived frag, send back an ack
                    self.send_ack(packet.clone(), frag_index);

                    //notify sc i got a packet
                    self.send_event(ClientEvent::PacketReceived(packet.clone()));

                    // If all the frag have arrived recreate message
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
                        //handle message
                        self.handle_message(message);

                        // empty the hashmap
                        self.client_base.fragments().remove(&(packet.session_id, initiator_id));
                    }
                }
                Ack(ack) => {
                    self.send_event(ClientEvent::AckReceived(packet.clone()));
                    //the ack will have the source that was the destination of the initial packet
                    //if it's registered then I also want to notify SC, so I send it
                    let src = self.client_base.get_src_id();
                    match self.client_base.fragments().get_mut(&(packet.session_id,src)) {
                        None => {}
                        Some((_, cont, vec)) => {
                            vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                            //if it's empty I retained all fragments because I received all the Ack, hence I can remove my entry from hashmap
                            if vec.is_empty() {
                                if let Some(ChatRequest(Register(_node))) = cont{
                                    let server = packet.routing_header.source().unwrap();
                                    self.registered_to.insert(server);
                                    self.send_event(RegisterSuccessfully(self.get_src_id(), server));
                                }
                                let src = self.client_base.get_src_id();
                                self.client_base.fragments().remove_entry(&(packet.session_id, src));
                            }
                        }
                    }

                    // I apply the positive feed on all nodes in the path
                    let nodes = packet.routing_header.hops;
                    self.client_base.network().positive_feedback(nodes);
                }

                PacketType::Nack(nack) => {
                    self.send_event(ClientEvent::NackReceived(packet.clone()));
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

    ///Handle any incoming message after reconstruction (for chat client)
    fn handle_message(&mut self, message: Message) {
        match message.content {
            ContentType::ChatResponse(resp) => {
                match resp{
                    ChatResponse::ClientList(list) => {
                        let source= message.source_id;
                        for i in list {
                            let is_new = self.contact_list.entry(i).and_modify(|vec| vec.push(source)).or_insert(vec![source]).len() == 1;

                            if is_new {
                                //notify sc that you now have that contact
                                self.send_event(SendContactsToSC(self.client_base.get_src_id(), i));
                            }

                        }
                    }
                    ChatResponse::MessageFrom { from, message } => {
                        //only when a message arrive in toto, we care to inform SC
                        //here is from the src of the text, so first parameter supplied is from!
                        self.send_event(ReceivedChatText(from, self.client_base.get_src_id(), message.clone()));
                        self.all_messages.entry(from).or_insert(vec![(from, message.clone())]).push((from, message));
                        if NOTIFY {
                            self.play_notification_sound();
                        }
                    }
                    ChatResponse::ClientNotFound(node) => {
                        //update contact list
                        let server_src = message.source_id;
                        if let Some(vec) = self.contact_list.get_mut(&node){
                            vec.retain(|node| *node != server_src);
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

                        ///remove
                        // println!("client {} sent type response to {from}", self.comm.get_src_id());

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
                //no point in getting other types of req
                let new_nack = self.create_nack(UnexpectedMessage);
                self.send_nack_message(message.source_id, new_nack);
            }
        }

    }

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
            client_base: ClientStruct::new(node_id, command_recv, event_send, packet_recv, packet_send),
            contact_list: HashMap::new(),
            all_messages: HashMap::new(),
            registered_to: HashSet::new(),
            sound: buffer,
        }
    }


    ///Run function of chat client, is equal to the web browser but call for different handles
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
                    // Wait a second before going on.
                }
            }
            // I check a counter, so that I don't try to send all the fragments every loop.
            if count >= 128 {
                //if I have some unchecked nodes I try to check them
                self.client_base.periodic_check_type();

                self.client_base.process_unsent_periodically();

                //resent count
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

            //commands for chat client
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
                //ignore other commands cause are webclients commands
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


///Set of functions of our ChatClient
impl ChatClient {

    ///Retrieve a Contact List from a particular server
    fn get_list(&mut self, id: NodeId){
        let src = self.client_base.get_src_id();
        let session = self.client_base.get_session_id();
        let content = ChatRequest(ClientList);
        let msg = Message::new(src, session, content);
        self.client_base.send_message(msg, id);

        if DEBUG_MODE{
            println!("sent client list req from {src} to server {id}")
        }
    }

    ///Register to a Server
    fn register(&mut self, id: NodeId){
        let src = self.get_src_id();
        let session = self.get_session_id();
        let content = ChatRequest(Register(src));
        let msg = Message::new(src, session, content);
        self.client_base.send_message(msg, id);

        if DEBUG_MODE{
            println!("sent register req from {src} to server {id}")
        }
    }

    ///Send a text message to ID, will optimize choosing the best server for us.
    fn send_chat_text(&mut self, id: NodeId, str: String){
        ///Remove
        if NO_SERVER_MODE {
            self.send_event(ReceivedChatText(self.get_src_id(), id, str.clone()));
        }

        let src = self.get_src_id();
        if let Some(servers) = self.contact_list.get(&id){

            //to ensure is also registered to server
            let available_servers: Vec<NodeId> = servers
                .clone()
                .into_iter()
                .filter(|x| self.registered_to.contains(x))
                .collect();

            let src = self.client_base.get_src_id();
            if let Some(server_id) = self.client_base.network().get_optimal_dest(&src, &available_servers){
                //decide witch server to contact, for the moment just the first one is okay

                let session = self.get_session_id();
                let content = ChatRequest(SendMessage {
                    from: src,
                    to: id,
                    message: str.clone(),
                });

                //keep track of the outgoing message in our personal chat
                self.all_messages.entry(id).or_insert(vec!((src, str.clone()))).push((src, str));


                let msg = Message::new(src, session, content);
                self.client_base.send_message(msg, server_id);
            }
        } else {
            self.send_event(ClientEvent::MissingContacts(src,id))
        }

    }

    fn play_notification_sound(&self) {
        let (_stream, stream_handle) = OutputStream::try_default().unwrap();
        let sink = Sink::try_new(&stream_handle).unwrap();


        let cursor = Cursor::new(self.sound.clone());
        let source = Decoder::new(BufReader::new(cursor)).unwrap();

        sink.append(source);
        sink.sleep_until_end();
    }
}