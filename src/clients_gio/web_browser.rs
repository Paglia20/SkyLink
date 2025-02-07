use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_struct::ClientStruct;
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

type ArrivedMedia = (String, Vec<u8>);

pub struct WebBrowser{
    comm: ClientStruct, //common client duh

    available_text_lists: HashMap<u64, (Vec<NodeId>, String)>,
    catalogue: HashMap<u64, Vec<NodeId>>, //which media server has that id
    arrived_content: HashMap<u64, ArrivedMedia>,

    /*
    arrived_content: media we retrieved

    catalogue: filled with information got from MediaReference(...), is the fkn catalogue of ikea.
    NotFound


    ATTENTION:
    the reason we have duplicated code for handle_packet and run is because both would be implemented in the common structure,
    but both call for the specific handle_message and handle_command, hence cannot be called inside the common struct.

    */


}

impl NetworkEdge for WebBrowser {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.comm.send_message(message, destination)
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let FloodRequest(mut flood_request) = packet.pack_type.clone(){
            flood_request
                .path_trace
                .push((self.comm.node_id, NodeType::Client));

            if self.comm.flood_ids.insert((
                flood_request.flood_id.clone(),
                flood_request.initiator_id.clone(),
            )) {
                if self.comm.packet_send.len() == 1 {
                    self.edge_send_flood_response(flood_request);
                } else {
                    let mut prev = flood_request.initiator_id.clone();
                    if flood_request.path_trace.clone().len() > 1 {
                        prev = flood_request
                            .path_trace
                            .get(flood_request.path_trace.len() - 2)
                            .unwrap()
                            .0;
                    }
                    //I update the path_trace in the packet.
                    packet.pack_type = FloodRequest(flood_request);
                    for (key, _) in self.comm.packet_send.iter() {
                        //println!("Previous: {}", prev);
                        //println!("Key: {}", key);
                        if *key != prev {
                            //I send the flooding to everyone except the node I received it from.
                            if let Ok(_) =
                                self.comm.packet_send.get(key).unwrap().send(packet.clone())
                            {
                                self.send_event(ClientEvent::PacketSent(packet.clone()));
                                //If the message was sent, I also notify the sim controller.
                            } //There's no else, since I don't care of nodes which can't be reached.
                        }
                    }
                }
            } else {
                self.edge_send_flood_response(flood_request);
            }
        } else {
            if packet.routing_header.destination().unwrap() != self.comm.node_id {
                // If it's not his packet, but he has to act as a drone (that never misses)
                self.comm.send_as_drone(packet);

            } else {
                // We can take for granted he is the destination
                match packet.pack_type.clone() {
                    MsgFragment(fragment) => {
                        let tot_num_frag = fragment.total_n_fragments as usize;
                        let session_id = packet.session_id;
                        let initiator_id = packet.routing_header.hops[0];
                        let destination = self.comm.node_id; //he is the destination
                        let frag_index = fragment.fragment_index;
                        //add new frag
                        let entry = self.comm.fragments.entry((session_id, initiator_id)).or_insert((destination, None, vec![]));
                        entry.2.push(fragment);


                        //for each arrived frag, send back an ack
                        self.send_ack(packet.clone(), frag_index);

                        //notify sc i got a packet
                        self.send_event(ClientEvent::PacketReceived(packet.clone()));

                        // If all the frag have arrived recreate message
                        let frags_clone = &self.comm.fragments.get(&(packet.session_id, initiator_id)).unwrap().2;
                        if frags_clone.len() == tot_num_frag {
                            let message = match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                                Ok(mess) => { mess }
                                Err(e) => {
                                    if DEBUG_MODE {
                                        println!("{e} with {}", self.comm.node_id);
                                    }
                                    self.send_event(ErrorReassembling(self.get_src_id()));
                                    return;
                                }
                            };
                            //handle message
                            self.handle_message(message);

                            // empty the hashmap
                            self.comm.fragments.remove(&(packet.session_id, initiator_id));
                        }
                    }
                    Ack(ack) => {
                        self.send_event(ClientEvent::AckReceived(packet.clone()));

                        //the ack will have the source that was the destination of the initial packet
                        match self.comm.fragments.get_mut(&(packet.session_id, self.comm.node_id)) {
                            None => {}
                            Some((_, _, vec)) => {
                                vec.retain(|fragment| fragment.fragment_index != ack.fragment_index);

                                //if it's empty I retained all fragments because I received all the Ack, hence I can remove my entry from hashmap
                                if vec.is_empty() {
                                    self.comm.fragments.remove_entry(&(packet.session_id, self.comm.node_id));
                                }
                            }
                        }

                        // I apply the positive feed on all nodes in the path
                        let nodes = packet.routing_header.hops;
                        self.comm.network.positive_feedback(nodes);
                    }

                    PacketType::Nack(nack) => {
                        self.send_event(ClientEvent::NackReceived(packet.clone()));
                        self.comm.handle_nack(nack, packet);
                    }
                    FloodRequest(_) => {
                        unreachable!()
                    }
                    FloodResponse(_flood_resp) => {
                        self.comm.save_flood_response(packet);
                    }
                }
            }
        }
    }

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
                        self.arrived_content.insert(id, (name.clone(), media.clone()));
                        self.send_event(SendMedia(self.get_src_id(), id, name, media))
                    }
                    MediaResponse::NotFound(id) => {
                        //i update the catalogue
                        if let Some(vec) = self.catalogue.get_mut(&id){
                            vec.retain(|node| *node != src);
                        }

                        //and try to re obtain the same media
                        self.get_media(id);

                    }
                }
            },
            ContentType::TextResponse(text_response) => {
                match text_response{
                    TextResponse::TextList(map) => {
                        for (text_file_id, name) in map {
                            let entry = self.available_text_lists.entry(text_file_id).or_insert((vec![], name.clone()));
                            entry.0.push(src);

                            if entry.0.len() == 1 {
                                self.send_event(SendTextList(self.get_src_id(), text_file_id, name))
                            }
                        }
                    }
                    TextResponse::MediaReferences(media_refs) => {
                        for (media_id, (name, media_server_id)) in media_refs{
                            let entry =  self.catalogue.entry(media_id).or_insert(vec![]);
                            for e in media_server_id {
                                entry.push(e);
                            }

                            if entry.len() == 1 {
                                self.send_event(SendCatalogue(self.get_src_id(), media_id, name))
                            }
                        }
                    }
                    TextResponse::Incomplete(incomplete_text) => {
                        self.retry_get_text_file(incomplete_text);
                    }
                    TextResponse::NotFound(text_id) => {
                        //update catalougue
                        self.available_text_lists.entry(text_id).and_modify(|(v, _)|
                            v.retain(|node_id| *node_id != src));

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
                            from: self.comm.node_id,
                        };
                        let message = Message::new(self.comm.node_id, self.get_session_id(), ContentType::TypeExchange(type_resp));

                        self.send_message(message, from);

                    }
                    TypeExchange::TypeResponse { from, edge_type } => {
                        if let EdgeType::Server(server_type) = edge_type{
                            if let ServerType::Content(ty) = server_type{
                                self.comm.network.update_state(from, 1);

                                if ty == ContentServerType::Text{
                                    //only if it's a text server I will notify the sc that is a dst.
                                    //this because the sc has to chose for a webclient only the text servers to witch i want to ask the lists.
                                    //he will manage the media itself with catalog!!
                                    self.send_event(SendDestinations(self.comm.node_id, from));
                                }
                            }
                            else {
                                self.comm.network.update_state(from, 2);
                            }
                        } else {
                            //if it's a client
                            self.comm.network.update_state(from, 2);

                            if NO_SERVER_MODE {
                                self.send_event(SendDestinations(self.comm.node_id, from));
                            }
                        }
                    }
                }
            }
            ContentType::EdgeNack(nack) => {
                self.comm.handle_edge_nack(nack, message.source_id);

            },
            _ => {
                // Gio: no point in getting other types of req
                let new_nack = self.create_nack(UnexpectedMessage);
                self.send_nack_message(message.source_id, new_nack);
            }
        }

    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        self.comm.send_fragment(fragment, destination, session_id)
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.comm.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        self.comm.send_fragment_after_nack(packet, nack)
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        self.comm.send_ack(packet, fragment_index);
    }

    fn flood(&mut self) {
        self.comm.flood();
    }

    fn get_flood_id(&mut self) -> u64 {
        self.comm.get_flood_id()
    }

    fn get_session_id(&mut self) -> u64 {
        self.comm.get_session_id()
    }

    fn get_src_id(&self) -> NodeId {
        self.comm.get_src_id()
    }

    fn remove_sender(&mut self, id: NodeId) {
        self.comm.remove_sender(id);
    }
}

impl NetworkEdgeErrors for WebBrowser {
    fn check_type(&mut self, id: NodeId) {
        self.comm.check_type(id);
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        self.comm.is_state_ok(node_id)
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.comm.send_nack_message(dst, nack);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        self.comm.send_drone_nack(dst, nack);
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
            comm: ClientStruct::new(node_id, command_recv, event_send, packet_recv, packet_send),
            available_text_lists: Default::default(),
            arrived_content: Default::default(),
            catalogue: Default::default(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.comm.command_recv) -> cmd => {
                    if let Ok(command) = cmd {
                        self.handle_command(command);
                    }
                }
                recv(self.comm.packet_recv) -> pkt => {
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
            if self.comm.unsent_fragments.0 >= 100 {
                //if I have some unchecked nodes I try to check them

                self.comm.periodic_check_type();


                self.comm.process_unsent_periodically();

                //uncomment to check flood periodically

                // let mut path_printable = String::new();
                // self.paths.clone().iter_mut().for_each(|(dst, (state, path))| {
                //     let destination = format!("Node {}, State: {}, path: *not now* \n", dst, state);
                //     path_printable.push_str(destination.as_str());
                // });
                // println!("{} has paths: {:?}",self.node_id, path_printable);



            } else {
                self.comm.unsent_fragments.0 += 1;
            }
        }
    }

    fn handle_command(&mut self, command: ClientCommand) {
        match command {
            ClientCommand::RemoveSender(node_id) => {
                self.remove_sender(node_id);
            }
            ClientCommand::AddSender(node_id, sender) => {
                self.comm.packet_send.insert(node_id, sender);
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

            //commands for WebClient
            ClientCommand::GetContent(id) => {
                self.get_media(id);
            }


            //ignore other commands cause are chat clients commands
            _ =>{

            }
        }
    }


    fn get_client_type(&self) -> ClientType {
        ClientType::WebBrowser
    }

    fn send_event(&self, ce: ClientEvent) {
        self.comm.send_event(ce);
    }
}

impl WebBrowser{
    fn get_list(&mut self, id: NodeId) {
        let src = self.comm.get_src_id();
        let session = self.comm.get_session_id();
        let content = ContentType::TextRequest(TextList);
        let msg = Message::new(src, session, content);
        self.comm.send_message(msg, id);

        if DEBUG_MODE {
            println!("Sent text list request from {src} to server {id}");
        }
    }

    fn get_text_file(&mut self, text_file_id: u64) {
        let src = self.comm.get_src_id();
        let session = self.comm.get_session_id();

        if let Some(map) = self.available_text_lists.get(&text_file_id) {
            let dests = map.0.clone();
            if !dests.is_empty() {
                if let Some(dst) = self.comm.network.get_optimal_dest(&self.get_src_id(), &dests) {
                    let content = ContentType::TextRequest(TextFile(text_file_id));
                    let msg = Message::new(src, session, content);
                    self.comm.send_message(msg, dst);
                    if DEBUG_MODE {
                        println!("Sent text file request from {src} to server {dst}");
                    }
                }
            }
        } else {
            self.send_event(MissingTextList(self.get_src_id(), text_file_id))
        }
    }

    fn retry_get_text_file(&mut self, text_file_id: u64) {
        let wait_time: u32 = (u16::MAX as u32) * 2u32;
        for _ in 0..wait_time {

        }
        self.get_text_file(text_file_id)
    }

    fn get_media(&mut self, cont_id: u64) {
        let src = self.comm.get_src_id();
        let session = self.comm.get_session_id();

        if let Some(dests) = self.catalogue.get(&cont_id) {
            if !dests.is_empty() {
                if let Some(dst) = self.comm.network.get_optimal_dest(&self.get_src_id(), &dests) {
                    let content = ContentType::MediaRequest(Media(cont_id));
                    let msg = Message::new(src, session, content);
                    self.comm.send_message(msg, dst);
                    if DEBUG_MODE {
                        println!("Sent media request from {src} to server {dst}");
                    }
                }
            }
        } else {
            self.send_event(MissingDestForMedia(self.get_src_id(), cont_id))
        }
    }
}


