use crate::clients_gio::client_command::ClientEvent::{MissingDestination, MissingRoute, WrongDestinationType};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::message::{ContentType, EdgeNackType, Message, TypeExchange};
use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crate::routing::{Network};
use crate::DEBUG_MODE;
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::PacketType::*;
use wg_2024::packet::{Fragment, Nack, NackType, NodeType, Packet, Ack};
use wg_2024::packet::NackType::ErrorInRouting;
//here the common struct of both the clients, important: some functions are left unreachable since will be called ad hoc by each client.
//attention, also all function that call handle packet and handle message are unreachable obv


pub struct ClientStruct {
    pub node_id: NodeId,
    pub command_recv: Receiver<ClientCommand>,
    pub event_send: Sender<ClientEvent>,
    pub packet_recv: Receiver<Packet>,
    pub packet_send: HashMap<NodeId, Sender<Packet>>,

    pub flood_ids: HashSet<(u64, NodeId)>, // Just like drones
    pub used_session_id: HashSet<u64>,     // Do we need this?
    pub network: Network,
    pub fragments: HashMap<(u64, NodeId, NodeId), (Option<ContentType>, Vec<Fragment>)>, //(session_id, source, destination) - (copy of content (for registering ecc…) and frags), if the content is None is because it's yet to be fully arrived!
    pub unsent_fragments: (u8, HashMap<(u64, NodeId, NodeId), Vec<(Fragment)>>), // The second NodeId is the destination, the u8 is a counter (for now to the maximum I guess) to avoid sending too much stuff.
}

impl NetworkEdge for ClientStruct {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        match message.clone().content{
            ContentType::TypeExchange(_exc) =>{
                self.client_send_fragment(message, destination);
            },
            ContentType::EdgeNack(_nack) => {
                self.client_send_fragment(message, destination);
            }
            _=>{
                if self.is_state_ok(destination) {
                    self.client_send_fragment(message, destination);
                }
                else {
                    let new_nack = WrongDestinationType(self.get_src_id(), destination);
                    self.send_event(new_nack);
                }
            }
        }
    }

    fn handle_packet(&mut self, _packet: Packet) {
        unreachable!()
    }

    fn handle_message(&mut self, _message: Message) {
        unreachable!()
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        if destination == self.node_id {
            if DEBUG_MODE {
                println!("Sending message to yourself with {:?}", destination);
            }
            return;
        }

        match self.network.get_srh(&self.node_id, &destination){
            None => {
                if DEBUG_MODE {
                    println!("Tried to send fragment without path to {destination} with {}", self.node_id);
                }
                self.send_event(MissingDestination(self.get_src_id(), destination));
                self.add_unsent_fragment(fragment, session_id, destination);
            }
            Some(srh) => {
                let first_dst = srh.hops[1];
                let packet = Packet::new_fragment(srh, session_id, fragment.clone());

                // If everything worked, I try to send.
                match self.packet_send.get(&first_dst) {
                    Some(sender) => {
                        sender.send(packet.clone()).unwrap();
                        self.send_event(ClientEvent::PacketSent(packet.clone()));

                    }
                    None => {
                        // If I want to pass for a node that I don't have as a neighbour, I need to remove
                        // channels who contain it.
                        self.send_event(MissingRoute(self.get_src_id(), destination));
                        self.add_unsent_fragment(fragment, session_id, destination);
                        self.network.remove_faulty_connection(self.node_id, first_dst);
                    }
                }
            }
        }

    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        // If the sending of a fragment gave an error, we put it in a hashmap to try sending it again.
        match self.unsent_fragments.1.get_mut(&(session_id, self.node_id, destination)) {
            Some(fragments) => {
                fragments.push(fragment);
            },
            None => {
                let mut vec = Vec::new();
                vec.push(fragment);
                self.unsent_fragments.1.insert((session_id, self.node_id, destination), vec);
            }
        }
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        match self.fragments.get(&(packet.session_id, self.node_id, packet.routing_header.destination().unwrap())) {
            // I try to find again the fragment, and notify the sim controller if I don't have it anymore
            None => {
                self.send_event(ClientEvent::LostMessage(packet.session_id, self.node_id));
            },
            Some((_, fragments)) => {
                match fragments.get(nack.fragment_index as usize) {
                    None => {
                        self.send_event(ClientEvent::LostFragment(packet.session_id, self.node_id, nack.fragment_index));
                    },
                    // If I manage to find the fragment, I send it
                    Some(fragment) => {
                        self.send_fragment(fragment.clone(), *packet.routing_header.hops.get(0).unwrap(), packet.session_id);
                    }
                }
            }
        }
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        let new_hops: Vec<NodeId> = packet.routing_header.hops.iter().rev().map(|(id)| *id)
            .collect::<Vec<NodeId>>();
        let next_id = new_hops[1];
        let srh = SourceRoutingHeader::new(new_hops, 1); //is it 1 right?
        let packet_ack = Packet::new_ack(srh, packet.session_id, fragment_index);

        match self.packet_send.get(&next_id) {
            Some(sender) => {
                sender.send(packet_ack.clone()).unwrap();
                self.send_event(ClientEvent::PacketSent(packet_ack))
            }
            None => {
                self.send_event(MissingDestination(self.node_id, next_id))
            }
        }
    }

    fn flood(&mut self) {
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
    }

    fn get_flood_id(&mut self) -> u64 {
        let min = match self.flood_ids.iter().min(){
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

    fn get_src_id(&self) -> NodeId {
        self.node_id
    }

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
    fn check_type(&mut self, id: NodeId) {
        let req = TypeExchange::TypeRequest { from: self.node_id };
        let exc = ContentType::TypeExchange(req);
        let s_id = self.get_session_id();
        self.send_message(Message::new(self.node_id, s_id, exc), id);

        if DEBUG_MODE {
            println!("sent check from {} to {id}", self.node_id);
        }
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        let out =  match self.network.get_state(&node_id){
            Some(s) => {
               s == 1
            }
            None =>{false}
        };
        if !out {
            if DEBUG_MODE{
                println!("dst state was not ok");}

            //send nack?
        }
        out
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.send_message(nack, dst);
    }

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
                pack_type: Nack(new_nack),
            };

            match self.packet_send.get(&first_hop){
                None => {
                    self.send_event(MissingDestination(self.node_id, dst));
                    return;
                }
                Some(sender) => {
                    sender.send(packet).unwrap();
                }
            }
        } else {
            self.send_event(MissingDestination(self.node_id, dst));
            return;
        }
    }
}

impl ClientTrait for ClientStruct {
    fn new(node_id: NodeId, command_recv: Receiver<ClientCommand>, event_send: Sender<ClientEvent>, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>) -> Self {
        Self { node_id, command_recv, event_send, packet_recv, packet_send, flood_ids: HashSet::default(), used_session_id: HashSet::default(), network: Network::new(), fragments: HashMap::default(), unsent_fragments: (0, HashMap::new()) }
    }

    fn run(&mut self) {
        unreachable!();
    }

    fn handle_command(&mut self, _command: ClientCommand) {
        unreachable!()
    }

    fn get_client_type(&self) -> ClientType {
        unreachable!()
    }

   fn send_event(&self, ce: ClientEvent) {
        match self.event_send.try_send(ce.clone()){
            Ok(_) => {}
            Err(_err) => {
                if DEBUG_MODE {
                    println!("{} - simulation control unreachable for {:?}", self.node_id, ce)
                }
            }
        }
   }
}

impl ClientStruct {
    //sta fn la metterei in networking
    pub (crate) fn get_optimal_dest (&mut self, v: &Vec<NodeId>) -> Option<NodeId> {
        let mut out: Option<NodeId> = None;
        let mut weight = f64::MIN;
        for i in v {
            if let Some((r, r_weight)) = self.network.best_path(&self.node_id, i){
                if weight < r_weight{
                    if !r.is_empty(){
                        out = Some(r[0]);
                    }
                }
            }
        }
        out
    }


    pub fn send_as_drone(&mut self, mut packet: Packet){
        packet.routing_header.hop_index += 1;
        if let Some(&next_id) = packet.routing_header.hops.get(packet.routing_header.hop_index) {
            match self.packet_send.get(&next_id) {
                None => {
                    self.send_event(MissingRoute(self.get_src_id(), next_id))
                }
                Some(sender) => {
                    match sender.try_send(packet.clone()) {
                        Err(_) => {
                            // !!You need to send back the same errors a drone would
                            self.send_drone_nack(packet.routing_header.source().unwrap(), ErrorInRouting(next_id));
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

    pub fn periodic_check_type(&mut self){
        for i in self.network.get_unresolved(){
            self.check_type(i)
        }
    }

    pub fn client_send_fragment(&mut self, message: Message, destination: NodeId){
        let session_id = message.session_id;
        let frags = Self::fragment_message(&message);
        self.fragments.insert((session_id, self.node_id, destination), (Some(message.content), frags.clone()));
        // I also save the fragments in the memory, in case I have to send them again.


        for fragment in frags {
            self.send_fragment(fragment, destination, session_id);
            // I apply the send operation on each single fragment.
        }
    }

    pub fn process_unsent_periodically(&mut self){
        // I create a temporary copy of the fragments that needs to be processed.
        let mut to_process = Vec::new();
        for (identifier, content) in self.unsent_fragments.1.iter() {
            for fragment in content.iter() {
                to_process.push((fragment.clone(), identifier.clone()));
            }
        }
        // I then empty the HashMap to not have any duplicate.as
        self.unsent_fragments.1 = HashMap::new();
        self.unsent_fragments.0 = 0; for (fragment, identifier) in to_process {
            self.send_fragment(fragment.clone(), identifier.2, identifier.0);
        }
    }

    pub fn handle_nack(&mut self, nack: Nack, packet: Packet){
        match nack.nack_type.clone() {
            NackType::UnexpectedRecipient(wrong_node) => {
                self.network.remove_node(wrong_node);
                self.send_fragment_after_nack(packet, nack);
            },
            ErrorInRouting(wrong_node) => {
                // I again remove the routes containing the (probably) crushed drone
                self.network.remove_node(wrong_node);
                self.send_fragment_after_nack(packet, nack);
            },
            NackType::DestinationIsDrone => {
                let wrong_node = packet.routing_header.hops.last().unwrap();
                self.network.update_state(*wrong_node, 2);
                // Since the destination was a drone, the message was faulty,
                // so I update the destination state and consider the message as lost.
            },
            NackType::Dropped => {
                // Who dropped will be source of the nack
                let dropper = packet.routing_header.source().unwrap();
                self.network.negative_feedback(dropper);

                // I just send it again
                self.send_fragment_after_nack(packet.clone(), nack);
            }
        }
    }

    pub fn handle_edge_nack(&mut self, nack: EdgeNackType, src: NodeId){
        match nack {
            EdgeNackType::UnexpectedMessage => {
                //vuol dire che ha mandato un message al dst con state sbagliato.
                self.network.update_state(src, 2);

                //e il messaggio viene scartato credo

                if DEBUG_MODE{
                    println!("Client {} discarded message to {} after receiving his nack, because state was not good", self.node_id, src)
                }

            }
        }
    }
}
