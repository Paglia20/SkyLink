use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::Client;
use crate::clients_gio::client_type::ClientType;
use crate::message::{ChatRequest, ChatResponse, ContenType, Message};
use crate::network_edge::NetworkEdge;
use crate::routing::{Route, RouteList};
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, NodeType, Packet, PacketType};
use crate::message::TextRequest::*;

pub struct ChatClient {
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, // Just like drones
    used_session_id: HashSet<u64>,     // Do we need this?

    paths: HashMap<NodeId, RouteList>, // These NodeId are just server_chat nodes.
    contact_list: HashMap<NodeId, Vec<NodeId>>, // First NodeId is the client we want to communicate with, the second one is the server he has to write to, this two hash might be merged in future
    fragments: HashMap<(u64, NodeId), Vec<Fragment>>,
    arrived_messages: HashMap<NodeId, Vec<Vec<u8>>>,
}

impl NetworkEdge for ChatClient {
    type RequestType = ChatRequest; // Still questioning if we need this lol -Leo
    type ResponseType = ChatResponse;

    fn send_message(&mut self, message: Message, destination: NodeId) -> Result<(), String> {
        let session_id = message.session_id;
        let frags = Self::fragment_message(&message);
        self.fragments.insert((session_id, self.node_id), frags.clone());

        for fragment in frags {
            //create SRH
            let srh = match self.paths.get(&destination) {
                None => {
                    return Err("Destination not found".to_string());
                }
                Some(route_list) => match route_list.get_fastest_route() {
                    None => return Err("Destination not found".to_string()),
                    Some(route) => route.to_source_routing_header(),
                },
            };

            let first_dst = srh.hops[0];
            let packet = Packet::new_fragment(srh, session_id, fragment);

            match self.packet_send.get(&first_dst) {
                None => {
                    self.event_send
                        .try_send(ClientEvent::PacketSendingError(packet))
                        .map_err(|e| e.to_string())?; // i only need it here i think...
                    return Err("First step not found".to_string());
                }
                Some(sender) => sender.try_send(packet).map_err(|err| err.to_string())?,
            }
        }

        Ok(())
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if *packet.routing_header.hops.last().unwrap() != self.node_id {
            //if it's not his packet, but he has to act as a drone (that never misses)
            let next_id = packet.routing_header.hops[packet.routing_header.hop_index]; //please tell me if it's right

            match self.packet_send.get(&next_id) {
                None => { /*no more a destination!*/ }
                Some(sender) => {
                    match sender.try_send(packet.clone()) {
                        Err(_) => { /*no more a destination!*/ }
                        Ok(_) => {
                            self.event_send
                                .send(ClientEvent::PacketSent(packet.clone()))
                                .unwrap();
                            //If the message was sent, I also notify the sim controller.
                        }
                    }
                }
            }
            return;
        }

        //we can take for granted he is the destination
        match packet.pack_type.clone() {
            PacketType::MsgFragment(fragment) => {
                let tot_num_frag = fragment.total_n_fragments as usize;
                let session_id = packet.session_id;
                let initiator_id = packet.routing_header.hops[0];
                //add new frag
                if !self.fragments.contains_key(&(packet.session_id, packet.routing_header.hops[0])) {
                    self.fragments.insert((session_id, initiator_id), vec![fragment]);
                } else {
                    self.fragments.get_mut(&(session_id, initiator_id)).unwrap().push(fragment);
                }

                //if all the frag have arrived recreate message
                let frags_clone = self.fragments.get(&(packet.session_id, packet.routing_header.hops[0])).unwrap();
                if (frags_clone.len() == tot_num_frag) {
                    let message = match Self::reassemble_message(session_id, initiator_id, frags_clone) {
                        Ok(mess) => { mess }
                        Err(_e) => {
                            unimplemented!() //
                        }
                    };
                    //handle message
                    self.handle_message(message);

                    //svuota hashmap
                    self.fragments.remove(&(packet.session_id, packet.routing_header.hops[0]));
                }
            }
            PacketType::Ack(_ack) => {}
            PacketType::Nack(_nack) => {}
            PacketType::FloodRequest(mut flood_request) => {
                flood_request
                    .path_trace
                    .push((self.node_id, NodeType::Client));

                if self.flood_ids.insert((
                    flood_request.flood_id.clone(),
                    flood_request.initiator_id.clone(),
                )) {
                    if self.packet_send.len() == 1 {
                        self.send_flood_response(flood_request);
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
                        packet.pack_type = PacketType::FloodRequest(flood_request);
                        for (key, _) in self.packet_send.iter() {
                            //println!("Previous: {}", prev);
                            //println!("Key: {}", key);
                            if *key != prev {
                                //I send the flooding to everyone except the node I received it from.
                                if let Ok(_) =
                                    self.packet_send.get(key).unwrap().send(packet.clone())
                                {
                                    self.event_send
                                        .send(ClientEvent::PacketSent(packet.clone()))
                                        .unwrap();
                                    //If the message was sent, I also notify the sim controller.
                                } //There's no else, since I don't care of nodes which can't be reached.
                            }
                        }
                    }
                } else {
                    self.send_flood_response(flood_request);
                }
            }
            PacketType::FloodResponse(flood_resp) => {
                //as of rn it "saves" all possible servers... we want something else i think...
                let mut current_path = Vec::new();
                for (node_id, node_type) in flood_resp.path_trace {
                    current_path.push(node_id);

                    if node_type == NodeType::Server {
                        if !self.paths.contains_key(&node_id) {
                            //if it's first time this server gets seen
                            self.paths.insert(node_id.clone(), RouteList::new());
                        }
                        // Clone the current path for the server and insert it into the route list
                        match self.paths.get_mut(&node_id) {
                            None => {
                                unreachable!()
                                //i hope it's unreachable
                            }
                            Some(rl) => {
                                rl.add_route(Route::new(current_path.clone()));
                            }
                        }
                    }
                }
            }
        }
    }


    fn handle_message(&mut self, message: Message) {
        match message.content {
            ContenType::ChatResponse(resp) => {
                match resp{
                    ChatResponse::ClientList(list) => {
                        self.contact_list.insert(message.source_id, list);
                    }
                    ChatResponse::MessageFrom { from, message } => {
                        self.arrived_messages.entry(from).or_insert(Vec::new()).push(message);
                    }
                    ChatResponse::MessageSent => {
                        //not sure, is just an ack?
                    }
                }
            }
            _ => {} //no point in getting other types of req
        }
    }
}


impl Client for ChatClient {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        ChatClient {
            node_id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            flood_ids: HashSet::new(),
            used_session_id: HashSet::new(),
            paths: HashMap::new(),
            contact_list: HashMap::new(),
            fragments: HashMap::new(),
            arrived_messages: HashMap::new(),
        }
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
                if self.packet_send.contains_key(&node_id) {
                    if let Some(to_be_dropped) = self.packet_send.remove(&node_id) {
                        drop(to_be_dropped);
                        //println!("Client {} no more has a connection to {}!", self.node_id, node_id);
                    }
                }
            }
            ClientCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
            }
            ClientCommand::SendMessage(node_id, message) => {
                match self.send_message(message, node_id) {
                    Err(_err) => {
                        //il sc è già stato notificato credo
                    }
                    _ => {}
                }
            }
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::ChatClient
    }
}
