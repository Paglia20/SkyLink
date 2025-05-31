use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::server::server_command::{ServerCommand, ServerEvent};
use crate::simulation_control::sim_control::Cause::{AckReceived, DroneInsideDestination, Error, Flood, LostMessage, MissingDestination, NackReceived, Sent};
use crate::simulation_control::sim_daniel::NodeNature;
use crate::simulation_control::storage::SimulationStorage;
use skylink::SkyLinkDrone;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{Debug, Display, Formatter};
use std::thread;
use std::thread::JoinHandle;
use wg_2024::controller::DroneCommand::{AddSender, Crash, RemoveSender};
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::drone::*;
use wg_2024::network::NodeId;
use wg_2024::packet::NodeType::*;
use wg_2024::packet::{NodeType, Packet, PacketType};

pub struct SimulationControl {
    drone_command_senders: HashMap<NodeId, Sender<DroneCommand>>,
    client_command_senders: HashMap<NodeId, Sender<ClientCommand>>,
    server_command_senders: HashMap<NodeId, Sender<ServerCommand>>,

    pub drone_event_recv: Receiver<DroneEvent>,
    pub client_event_recv: Receiver<ClientEvent>,
    pub server_event_recv: Receiver<ServerEvent>,


    pub channel_for_drone: Sender<DroneEvent>,
    pub(crate) all_sender_packets: HashMap<NodeId, Sender<Packet>>,
    pub(crate) network_graph: HashMap<NodeId, (NodeNature, HashSet<NodeId>)>,
    pub(crate) log: VecDeque<LogEntry>,


    pub storage: SimulationStorage,

}

impl SimulationControl {
    pub fn new(
        drone_command_senders: HashMap<NodeId, Sender<DroneCommand>>,
        client_command_senders: HashMap<NodeId, Sender<ClientCommand>>,
        server_command_senders: HashMap<NodeId, Sender<ServerCommand>>,
        drone_event_recv: Receiver<DroneEvent>,
        client_event_recv: Receiver<ClientEvent>,
        server_event_recv: Receiver<ServerEvent>,
        channel_for_drone: Sender<DroneEvent>,
        all_sender_packets: HashMap<NodeId, Sender<Packet>>,
        network_graph: HashMap<NodeId, (NodeNature, HashSet<NodeId>)>,
    ) -> Self {
        SimulationControl {
            drone_command_senders,
            drone_event_recv,
            client_command_senders,
            server_command_senders,
            client_event_recv,
            server_event_recv,
            channel_for_drone,
            all_sender_packets,
            network_graph,
            log: VecDeque::new(),
            storage: SimulationStorage::new(),
        }
    }

    pub(crate) fn add_drone_event_to_log(&mut self, e: DroneEvent) {
        match e {
            //had to correct index due to not having a source routing header in the flood request!!
            DroneEvent::PacketSent(packet) => {
                self.d_process_packet_sent(packet);
            }
            DroneEvent::PacketDropped(packet) => {
                self.d_process_packet_dropped(packet);
            }
            DroneEvent::ControllerShortcut(packet) => {
                self.d_process_controller_shortcut(packet);
            }
        }
    }
    pub(crate) fn add_client_event_to_log(&mut self, e: ClientEvent){
        match e {
            ClientEvent::PacketSent(packet) => {
                self.c_process_packet_sent(packet);
            }
            ClientEvent::PacketReceived(packet) => {
                self.c_process_packet_received(packet);
            }
            ClientEvent::PacketSendingError(packet) => {
                self.c_process_packet_sending_error(packet);
            }
            ClientEvent::AckReceived(packet) => {
                self.c_process_ack_received(packet);
            }
            ClientEvent::NackReceived(packet) => {
                self.c_process_nack_received(packet);
            }
            ClientEvent::MissingDestination(src, node_id) => {
                self.c_process_missing_destination(src, node_id);
            }
            ClientEvent::MissingRoute(src, node_id) => {
                self.c_process_missing_route(src, node_id);
            }
            ClientEvent::LostMessage(sess, node_id) => {
                self.c_process_lost_message(sess, node_id);
            }
            ClientEvent::LostFragment(sess, node_id, frag_index) => {
                self.c_process_lost_fragment(sess, node_id, frag_index);
            }
            ClientEvent::DroneInsideDestination(node_id) => {
                self.c_process_drone_inside_destination(node_id);
            }
            ClientEvent::SendContactsToSC(src, _dst) => {
                self.c_process_send_contacts(src)
            }
            ClientEvent::WrongDestinationType(src, node) =>{
                let new_log = LogEntry{
                    cause: Sent,
                    node_id: src,
                    message: format!("{src} think {} is at wrong state",
                                     node)
                };
                self.log.push_back(new_log);
            }
            ClientEvent::MissingContacts(src, dst) => {
                let new_log = LogEntry{
                    cause: Sent,
                    node_id: src,
                    message: format!("{src} do not have {dst} as contact")
                };
                self.log.push_back(new_log);
            }
            ClientEvent::SendDestinations(src, id) => {
                let new_log = LogEntry{
                    cause: Sent,
                    node_id: src,
                    message: format!("{src} now have {id} as server destination")
                };
                self.log.push_back(new_log);
            },
            ClientEvent::ReceivedChatText(src, dst, str) => {
                let new_log = LogEntry{
                    cause: Sent,
                    node_id: src,
                    message: format!("{src} sent txt [{str}] to {dst}")
                };
                self.log.push_back(new_log);
            }
            ClientEvent::SendMedia(src, media_id, name, _media) => {
                let new_log = LogEntry{
                    cause: Sent,
                    node_id: src,
                    message: format!("{src} received media [{media_id} - {name}]")
                };
                self.log.push_back(new_log);
            }
            ClientEvent::SendCatalogue(src, id, ..) => {
                let new_log = LogEntry{
                    cause: Sent,
                    node_id: src,
                    message: format!("{src} updated his catalogue! ({id}) ")
                };
                self.log.push_back(new_log);
            }
            ClientEvent::SendTextList(src, id, ..) => {
                let new_log = LogEntry{
                    cause: Sent,
                    node_id: src,
                    message: format!("{src} updated his TextsLists! ({id}) ")
                };
                self.log.push_back(new_log);
            }
            ClientEvent::Flooding(src) => {
                let new_log = LogEntry{
                    cause: Flood,
                    node_id: src,
                    message: format!("{src} is flooding!")
                };
                self.log.push_back(new_log);
            }
            ClientEvent::RegisterSuccessfully(src, dst) => {
                let new_log = LogEntry{
                    cause: Sent,
                    node_id: src,
                    message: format!("{src} is now registered to {dst}")
                };
                self.log.push_back(new_log);
            }

            ClientEvent::MissingDestForMedia(src,  media) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: src,
                    message: format!("{src} do not have a source for {media}")
                };
                self.log.push_back(new_log);
            }
            ClientEvent::MissingTextList(src,  text) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: src,
                    message: format!("{src} do not have a source for {text}")
                };
                self.log.push_back(new_log);
            }
            ClientEvent::ErrorReassembling(src) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: src,
                    message: format!("{src} couldn't reassemble the message")
                };
                self.log.push_back(new_log);
            }

        }
    }
    pub fn add_server_event_to_log(&mut self, e: ServerEvent){
        match e {
            ServerEvent::PacketSent(packet) => {
                self.s_process_packet_sent(packet);
            }
            ServerEvent::PacketReceived(packet) => {
                self.s_process_packet_received(packet);
            }
            ServerEvent::PacketSendingError(packet) => {
                self.s_process_packet_sending_error(packet);
            }
            ServerEvent::AckReceived(packet) => {
                self.s_process_ack_received(packet);
            }
            ServerEvent::NackReceived(packet) => {
                self.s_process_nack_received(packet);
            }
            ServerEvent::LostFragment(_session_id, node_id, fragment_index) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id,
                    message: format!("fragment {} lost", fragment_index)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::WrongDestinationType(my_node_id, wrong_node_id) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: my_node_id,
                    message: format!("the type of destination drone {} is wrong", wrong_node_id)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::MissingDestination(server_id,missing_destination_id ) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("missing destination {}", missing_destination_id)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::MissingRoute(server_id, missing_route_id) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("missing route for server {}", missing_route_id)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::LostMessage(sess, initiator_id, error) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: initiator_id,
                    message: format!("Lost message in session {}. Initiator {} reports: {} ", sess, initiator_id, error)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::DiscardedMessage(server_id, sess) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("discarded message in sess {}", sess)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::DroneInsideDestination(server_id, drone_id) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("destination {drone_id} removed because it's a drone, not a server")
                };
                self.log.push_back(new_log);
            }
            ServerEvent::WrongDestination(server_id, pack) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("packet {} sent to wrong destination", pack)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::FileNotFound(server_id, file_id ) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("file {} not found: the file is requested but not owned", file_id)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::IncompleteFile(server_id, file_id) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("at least one media of file {file_id} is missing")
                };
                self.log.push_back(new_log);
            }
            ServerEvent::FilesState(server_id, completed_files, incomplete_files) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("\nCOMPLETED FILES: \n {:?} \n INCOMPLETE FILES: \n {:?}\n", completed_files, incomplete_files)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::FileNotReadable(server_id, file_name, error) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("the file named {} is not readable, reported error: {}", file_name, error)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::MediaNotFound(server_id, media_id) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("media {} not found", media_id)
                };
                self.log.push_back(new_log);
            }
            ServerEvent::MediaState(server_id, medias_ids_and_names) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!(
                        "\n{}\n",
                        medias_ids_and_names.iter()
                            .map(|(num, text)| format!("media id: {}, media name: {}", num, text))
                            .collect::<Vec<String>>()
                            .join("\n")
                    )
                };
                self.log.push_back(new_log);
            }
            ServerEvent::ClientRegistered(server_id, client_id) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("client {client_id} registered correctly")
                };
                self.log.push_back(new_log);
            }
            ServerEvent::ClientAlreadyRegistered(server_id, client_id) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("client {client_id} was already registered")
                };
                self.log.push_back(new_log);
            }
            ServerEvent::WrongCommandGiven(server_id, wrong_command) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("command {:?} is wrong", wrong_command )
                };
                self.log.push_back(new_log);
            }
            ServerEvent::ControllerShortcut(event) => {
                match event {
                    DroneEvent::PacketSent(_) => {}
                    DroneEvent::PacketDropped(_) => {}
                    DroneEvent::ControllerShortcut(packet) => {
                        let server_id = packet.routing_header.source().unwrap();
                        let new_log = LogEntry{
                            cause: Error,
                            node_id: server_id,
                            message: "starting controller shortcut".to_string()
                        };
                        self.log.push_back(new_log);
                    }
                }

            }
            ServerEvent::Flooding(server_id) => {
                let new_log = LogEntry{
                    cause: Error,
                    node_id: server_id,
                    message: format!("flooding...")
                };
                self.log.push_back(new_log);
            }
        }
    }


    pub fn msg_another_client(&self, src: NodeId, dst: NodeId, str: String){
        if Some(Client) == self.get_type(src){
            self.client_command_senders.get(&src).unwrap().send(ClientCommand::SendMSG(dst, str.clone())).unwrap();
            /*if DEBUG_MODE{
                println!("Sim Controller Forced {src} to send str {str} to {}", dst);
            }*/
        }
    }
    pub fn register_client_to_server(&self, src: NodeId, dst: NodeId){
        if Some(Client) == self.get_type(src){
            self.client_command_senders.get(&src).unwrap().send(ClientCommand::Register(dst)).unwrap();
            /*if DEBUG_MODE{
                println!("Sim Controller Forced {src} to register to {}", dst);
            }*/
        }
    }
    pub fn retrive_list_from_server(&self, src: NodeId, dst: NodeId){
        if Some(Client) == self.get_type(src){
            self.client_command_senders.get(&src).unwrap().send(ClientCommand::RetrieveList(dst)).unwrap();
            /*if DEBUG_MODE{
                println!("Sim Controller Forced {src} to retrive list from {}", dst);
            }*/
        }
    }

    pub fn get_text_file(&self, src: NodeId, text_file_id: u64){
        if Some(Client) == self.get_type(src){
            self.client_command_senders.get(&src).unwrap().send(ClientCommand::GetTextFile(text_file_id)).unwrap();
            /*if DEBUG_MODE{
                println!("Sim Controller Forced {src} to get text file {}",text_file_id);
            }*/
        }
    }
    pub fn force_client_get_media(&self, src: NodeId, file_id: u64){
        if Some(Client) == self.get_type(src){
            self.client_command_senders.get(&src).unwrap().send(ClientCommand::GetContent(file_id)).unwrap();
            /*if DEBUG_MODE{
                println!("Sim Controller Forced {src} to get text file {}",text_file_id);
            }*/
        }
    }
    pub fn get_media(&self, src: NodeId, media: u64){
        if Some(Client) == self.get_type(src){
            self.client_command_senders.get(&src).unwrap().send(ClientCommand::GetTextFile(media)).unwrap();
            /*if DEBUG_MODE{
                println!("Sim Controller Forced {src} to get media {}",media);
            }*/
        }
    }



    pub fn spawn_drone(&mut self, pdr: f32, connections: Vec<NodeId>) -> (JoinHandle<()>, NodeId) {
        let new_id = self.generate_id();
        //aggiorna network graph
        self.network_graph.insert(new_id, (NodeNature::Drone, HashSet::from_iter(connections.clone().into_iter())));

        let (control_sender, control_receiver) = unbounded(); //canale per il Sim che manda drone command al drone
        self.drone_command_senders
            .insert(new_id.clone(), control_sender.clone()); // do al sim il sender per questo drone

        let (packet_send, packet_recv) = unbounded(); //canale per il drone, il recv gli va dentro, il send va dato in copia a tutti i droni che vogliono comunicare con lui
        self.all_sender_packets.insert(new_id.clone(), packet_send.clone());

        let mut packet_send = HashMap::new();
        //fill hashmap
        for (id, sender) in &self.all_sender_packets {
            for i in connections.clone() {
                if i == *id {
                    packet_send.insert(*id, sender.clone());
                }
            }
        }

        let channel_clone = self.channel_for_drone.clone();

        //create thread
        let handle = thread::spawn(move || {
            let mut new_drone = SkyLinkDrone::new(
                new_id,
                channel_clone,
                control_receiver,
                packet_recv,
                packet_send,
                pdr,
            );
            new_drone.run();
        });

        for ids in connections.clone() {
            self.add_sender(new_id, ids);
        }

        (handle, new_id)
    }

    fn generate_id(&mut self) -> NodeId {
        //just a function to generate an id that is empty in our hashmap, if is 1-3-4, it should give 2, if it's 1-2-3, should give 4.
        for k in 0..=u8::MAX {
            //If k is not a key in the map, I return it.
            if !self.network_graph.contains_key(&k) {
                return k;
            }
        }
        unreachable!("No free key found");
    }

    pub fn crash_drone(&mut self, id: NodeId) {
        if let Some(sender) = self.drone_command_senders.get(&id) {
            if let Err(e) = sender.send(DroneCommand::Crash) {
                println!("error in crashing drone {}: {:?}", id, e);
            } else {
                println!("crash command sent do the drone {}", id);

                // remove the drone from the neighbour's sends
                let mut vec = self.network_graph.get(&id).unwrap().1.clone();
                    self.drone_command_senders.iter().for_each(|(neigh_id, sender)|{
                        match sender.try_send(RemoveSender(id)) {
                            Ok(_) => {}
                            Err(_e) => {
                                /*if DEBUG_MODE {
                                    println!("{neigh_id} is not in network, so he cannot remove {id}");
                                }*/
                            }
                        }
                    });
                    self.client_command_senders.iter().for_each(|(neigh_id, sender)|{
                        match sender.try_send(ClientCommand::RemoveSender(id)) {
                            Ok(_) => {}
                            Err(_e) => {
                                /*if DEBUG_MODE {
                                    println!("{neigh_id} is not in network, so he cannot remove {id}");
                                }*/
                            }
                        }
                    });
                    self.server_command_senders.iter().for_each(|(neigh_id, sender)|{
                        if vec.contains(neigh_id){
                           match sender.try_send(ServerCommand::RemoveSender(id)) {
                               Ok(_) => {}
                               Err(_e) => {
                                   /*if DEBUG_MODE {
                                       println!("{neigh_id} is not in network, so he cannot remove {id}");
                                   }*/
                               }
                           }
                        }
                    });
                }

                if let Some(to_be_dropped) = self.drone_command_senders.remove(&id) {
                    drop(to_be_dropped);
                }
                self.log.push_back(LogEntry::new(
                    Cause::Managing,
                    id,
                    "Node crashed".to_string(),
                ));
                self.network_graph.iter_mut().for_each(|(_, (_, connections))| {
                    connections.retain(|&rem| rem != id);
                });                self.network_graph.remove(&id);


        } else {
            println!("drone {} not found in the network.", id);
        }
    }

    pub(crate) fn crash_all(&mut self) {
        for (_, sender) in &self.drone_command_senders {
            sender.try_send(Crash).unwrap()
        }
        for (_, sender) in &self.server_command_senders {
            sender.try_send(ServerCommand::InstantCrash).unwrap()
        }
        for (_, sender) in &self.client_command_senders {
            sender.try_send(ClientCommand::InstantCrash).unwrap()
        }
    }

    pub fn remove_senders(&mut self, id: NodeId, id_to_remove: NodeId) {
        if !self.is_node_connected(id, id_to_remove){
            self.log.push_back(LogEntry::new(
                Error,
                id,
                format!("drone {} is not connected to {}", id_to_remove, id),
            ));
            return;
            //if not connected it does nothing
        }

        //I created get_type that gets the type of the node from the id,
        // depending on that, you send the correct drone/client/server command
        match self.get_type(id){
            None => {
                //if it returned None it wasn't saved inside the network, you shouldn't reach this anyway, but you never know
                self.log.push_back(LogEntry::new(
                    Error,
                    id,
                    format!("drone {id} is not in network",
                )));
                return;
            }
            Some(n_type) => {
                self.match_node_type_for_remove_senders(n_type, id, id_to_remove);
            }
        }

        match self.get_type(id_to_remove){
            None => {
                self.log.push_back(LogEntry::new(
                    Error,
                    id,
                    format!("drone {id_to_remove} is not in network",
                    )));
                return;
            }
            Some(n_type) => {
                self.match_node_type_for_remove_senders(n_type, id_to_remove, id);
            }
        }
    }

    pub fn flood_with(&mut self, node_id: NodeId){
        if !self.does_drone_exist(node_id) {
            self.log.push_back(LogEntry::new(
                Error,
                node_id,
                format!("drone {} does not exist in this network.", node_id),
            ));
            return;
        }
        match self.get_type(node_id) {
            None => {self.log.push_back(LogEntry::new(
                Error,
                node_id,
                format!("drone {node_id} is not in network",
                )));
                return;},

            Some(n_type) => {
                if Client == n_type{
                    if let Some(_sender) = self.client_command_senders.get(&node_id) {
                        if let Err(_e) = _sender.send(ClientCommand::Flood) {
                            /*if DEBUG_MODE {
                                println!("error flooding");
                            }*/
                        } else {
                           /* if DEBUG_MODE {
                                println!("flooded successfully");
                            }*/
                        }

                    }
                    else {
                        self.log.push_back(LogEntry::new(
                            Cause::Managing,
                            node_id,
                            "error flooding".to_string(),
                        ));
                    }
                }
            }
        }
    }

    pub fn add_sender(&mut self, id: NodeId, id_to_add: NodeId) {

        if !self.does_drone_exist(id_to_add) {
            self.log.push_back(LogEntry::new(
                Error,
                id,
                format!("drone {} does not exist in this network.", id_to_add),
            ));
            return;
        }

        match self.get_type(id) {
            None => {
                self.log.push_back(LogEntry::new(
                    Error,
                    id,
                    format!("drone {id} is not in network",
                    )));
                return;
            }
            Some(n_type) => {
                self.match_node_type_for_add_sender(n_type, id, id_to_add);
            }
        }

        match self.get_type(id_to_add) {
            None => {
                self.log.push_back(LogEntry::new(
                    Error,
                    id_to_add,
                    format!("drone {id_to_add} is not in network",
                    )));
                return;
            }
            Some(n_type) => {
                self.match_node_type_for_add_sender(n_type, id_to_add, id);
            }
        }
    }

    pub fn get_type(&self, id: NodeId) -> Option<NodeType> {
        let (node_type, _) = match self.network_graph.get(&id) {
            Some(node) => node,
            None => return None,
        };
        Some(node_type.simple_type())
    }

    pub fn set_pdr(&mut self, id: NodeId, pdr: f32) {
        let mut capped_pdr = pdr;
        if pdr >= 100.0{
            capped_pdr = 100.0;
            self.log.push_back(LogEntry::new(
                Cause::Managing,
                id,
                "Capped pdr to 100".to_string(),
            ));
        }

        if let Some(sender) = self.drone_command_senders.get(&id) {
            if let Err(_e) = sender.send(DroneCommand::SetPacketDropRate(capped_pdr)) {
                println!("error in setting drone {} pdr to {}", id, capped_pdr);
            } else {
                println!("setting drone {} pdr to {}", id, capped_pdr);
                self.log.push_back(LogEntry::new(
                    Cause::Managing,
                    id,
                    format!("drone now has pdr set to {}", capped_pdr),
                ));
            }
        }
    }

    pub fn does_drone_exist(&mut self, id: NodeId) -> bool {
        let mut exists = false;
        if self.network_graph.contains_key(&id){
            exists = true;
        }
        exists
    }

    pub fn is_node_connected (&self, id: NodeId, rhs: NodeId) -> bool {
        let mut out = true;
        if let Some((_node, vec)) = self.network_graph.get(&id){
            if !vec.contains(&rhs) {
                out = false;
            }
        }
        if let Some((_node, vec)) = self.network_graph.get(&rhs){
            if !vec.contains(&id) {
                out = false;
            }
        }
        out
    }

    // functions to process the adding of client events to log: (denoted with "c_")
    fn c_process_packet_sent(& mut self, packet: Packet){

        let message = self.get_message(packet.clone());

        let id_drone = match packet.clone().pack_type{
            PacketType::FloodRequest(flood) => {
                let (id, _) = flood.path_trace.last().unwrap();
                *id
            }
            _ => *{
                packet.routing_header.hops.get(packet.routing_header.hop_index).unwrap() //riguarda per type exchange
            },
        };

        let new_log = LogEntry {
            cause: Sent,
            node_id: id_drone,
            message
        };
        self.log.push_back(new_log);
    }
    fn c_process_packet_received(& mut self, packet: Packet){
        let id_drone = self.get_id_drone(packet.clone());

        let new_log = LogEntry {
            cause: Sent,
            node_id: id_drone,
            message: format!(
                "Received fragment {:?} moving from node {} to {}",
                packet.session_id, packet.routing_header.source().unwrap(), packet.routing_header.destination().unwrap()
            ),
        };
        self.log.push_back(new_log);
    }
    fn c_process_packet_sending_error(& mut self, packet: Packet){
        let id_drone = self.get_id_drone(packet.clone());
        let new_log = LogEntry {
            cause: Error,
            node_id: id_drone,
            message: format!(
                "Error in sending fragment {:?} from {} to {}",
                packet.session_id, packet.routing_header.source().unwrap(), packet.routing_header.destination().unwrap()
            ),
        };
        self.log.push_back(new_log);
    }
    fn c_process_ack_received(& mut self, packet: Packet){
        if let Some(ack_id) = packet.routing_header.destination(){
            match packet.pack_type {
                PacketType::Ack(ack) => {
                    let new_log = LogEntry{
                        cause: AckReceived,
                        node_id: ack_id,
                        message: format!(
                            "received Ack of fragment {} coming from {} in session {}"
                            , ack.fragment_index, packet.routing_header.source().unwrap() ,packet.session_id
                        )
                    };
                    self.log.push_back(new_log);
                }
                _ => {}
            }
        }
    }
    fn c_process_nack_received(& mut self, packet: Packet){
        if let Some(nack_id) = packet.routing_header.destination(){
            match packet.pack_type {
                PacketType::Nack(nack) => {
                    let new_log = LogEntry{
                        cause: NackReceived,
                        node_id: nack_id,
                        message: format!(
                            "received Nack of fragment {}, nack type:{:?} "
                            , nack.fragment_index, nack.nack_type
                        )
                    };
                    self.log.push_back(new_log);
                }
                _ => {

                }
            }
        }
    }
    fn c_process_missing_destination(& mut self, node_id: NodeId, node_id0: NodeId){
        let new_log = LogEntry{
            cause: MissingDestination,
            node_id,
            message: format!("Couldn't reach {} with a packet (missing destination) ",
                             node_id0),
        };
        self.log.push_back(new_log);
    }
    fn c_process_missing_route(& mut self, node_id: NodeId, node_id0: NodeId){
        let new_log = LogEntry{
            cause: MissingDestination,
            node_id,
            message: format!("Couldn't reach {} with a packet (missing route)",
                             node_id0),
        };
        self.log.push_back(new_log);
    }
    fn c_process_lost_message(&mut self, sess:u64, node_id: NodeId){
        let new_log = LogEntry{
            cause: LostMessage,
            node_id,
            message: format!("lost message from session {:?}", sess),
        };
        self.log.push_back(new_log);
    }
    fn c_process_lost_fragment(&mut self, sess:u64, node_id: NodeId, frag_index: u64){
        let new_log = LogEntry{
            cause: LostMessage,
            node_id,
            message: format!(
                " lost message from session {:?} of fragment index {:?}",
                sess, frag_index),
        };
        self.log.push_back(new_log);
    }
    fn c_process_drone_inside_destination(&mut self, node_id: NodeId){
        let new_log = LogEntry{
            cause: DroneInsideDestination,
            node_id,
            message: format!("destination removed because destination of id {} is a drone",
                             node_id)
        };
        self.log.push_back(new_log);
    }
    fn c_process_send_contacts(&mut self, src:NodeId){
        let new_log = LogEntry{
            cause: Flood,
            node_id: src,
            message: "Flood infos received.".to_string()
        };
        self.log.push_back(new_log);
    }

    // functions to process the adding of drone events to log: (denoted with "d_")
    fn d_process_packet_sent(&mut self, packet: Packet){

        let message = self.get_message(packet.clone());

        let id_drone = self.get_id_drone(packet.clone());

        let new_log = LogEntry {
            cause: Sent,
            node_id: id_drone,
            message
        };
        self.log.push_back(new_log);
    }
    fn d_process_packet_dropped(&mut self, packet: Packet){
        let id_drone = match packet.clone().pack_type{
            PacketType::FloodRequest(flood) => {
                let (id, _) = flood.path_trace.last().unwrap();
                *id
            }
            _ => {
                packet.routing_header.current_hop().unwrap()
            },
        };

        let new_log = LogEntry {
            cause: Cause::Dropped,
            node_id: id_drone,
            message: format!(
                "dropped fragment {:?} of packet: {}",
                packet.session_id, packet
            ),
        };
        self.log.push_back(new_log);
    }
    fn d_process_controller_shortcut(&mut self, packet: Packet){
        let id_drone = match packet.clone().pack_type{
            PacketType::FloodRequest(flood) => {
                let (id, _) = flood.path_trace.last().unwrap();
                *id
            }
            _ => {
                packet.routing_header.previous_hop().unwrap_or(255)
            },
        };
        let new_log = LogEntry {
            cause: Cause::Shortcut,
            node_id: id_drone,
            message: format!("Sent shortcut for packet {}", packet),
        };
        self.log.push_back(new_log);
    }

    // functions to process the adding of server events to log: (denoted with "s_")
    fn s_process_packet_sent(&mut self, packet: Packet){

        let message = self.get_message(packet.clone());

        let id_drone = self.get_id_drone(packet.clone());

        let new_log = LogEntry {
            cause: Sent,
            node_id: id_drone,
            message
        };
        self.log.push_back(new_log);
    }
    fn s_process_packet_received(&mut self, packet: Packet){
        let id_drone = self.get_id_drone(packet.clone());

        let new_log = LogEntry {
            cause: Sent,
            node_id: id_drone,
            message: format!(
                "Received fragment {:?} moving from node {} to {}",
                packet.session_id, packet.routing_header.source().unwrap(), packet.routing_header.destination().unwrap()
            ),
        };
        self.log.push_back(new_log);
    }
    fn s_process_packet_sending_error(&mut self, packet: Packet){
        let id_drone = self.get_id_drone(packet.clone());
        let new_log = LogEntry {
            cause: Error,
            node_id: id_drone,
            message: format!(
                "Error in sending fragment {:?} of packet: {}",
                packet.session_id, packet
            ),
        };
        self.log.push_back(new_log);
    }
    fn s_process_ack_received(&mut self, packet: Packet){
        self.c_process_ack_received(packet)
    }
    fn s_process_nack_received(&mut self, packet: Packet){
        if let Some(ack_id) = packet.routing_header.destination(){
            match packet.pack_type {
                PacketType::Ack(ack) => {
                    let new_log = LogEntry{
                        cause: AckReceived,
                        node_id: ack_id,
                        message: format!(
                            " received Nack of fragment {}"
                            ,ack.fragment_index
                        )
                    };
                    self.log.push_back(new_log);
                }
                _ => {}
            }
        }
    }

    fn get_id_drone(&mut self, packet: Packet) -> NodeId{
        let id_drone = match packet.clone().pack_type{
            PacketType::FloodRequest(flood) => {
                let (id, _) = flood.path_trace.last().unwrap();
                *id
            }
            _ => *{
                packet.routing_header.hops.get(packet.routing_header.hop_index - 1).unwrap()
            },
        };
        id_drone
    }

    fn get_message(&mut self, packet: Packet) -> String{
        let mut message = String::new();
        match packet.clone().pack_type{
            PacketType::MsgFragment(fragment) => {
                message = format!("sent fragment id: {}, to {}", fragment.fragment_index, packet.routing_header.destination().unwrap());
            }
            PacketType::Ack(ack) => {
                message = format!("forwarded ack with id: {} from initiator {} to {} of session {}", ack.fragment_index, packet.routing_header.source().unwrap(), packet.routing_header.destination().unwrap(), packet.session_id);
            }
            PacketType::Nack(nack) => {
                message = format!("sent nack id: {} to {}", nack.fragment_index, packet.routing_header.destination().unwrap());
            }
            PacketType::FloodRequest(rq) => {
                let path: Vec<NodeId> = rq.path_trace.iter().map(|x| x.0).collect();
                message = format!("sent flood request: (id: {}, initiator :{}) containing {:?}", rq.flood_id, rq.initiator_id, path);
            }
            PacketType::FloodResponse(rr) => {
                let path: Vec<NodeId> = rr.path_trace.iter().map(|x| x.0).collect();
                message = format!("sent flood response to {:?}, containing {:?}", packet.routing_header.destination(), path)
            }
        }
        message
    }

    //functions for remove_senders
    fn match_node_type_for_remove_senders(&mut self, n_type: NodeType, id:NodeId, id_to_remove:NodeId){
        match n_type {
            Client => {
                if let Some(sender) = self.client_command_senders.get(&id) {
                    if let Err(_e) = sender.try_send(ClientCommand::RemoveSender(id_to_remove)) {
                        println!(
                            "error in removing node {} from client {} senders",
                            id_to_remove, id
                        );
                        return
                    }
                }
            }
            Drone => {
                if let Some(sender) = self.drone_command_senders.get(&id) {
                    if let Err(_e) = sender.try_send(RemoveSender(id_to_remove)) {
                        println!(
                            "error in removing drone {} from drone {} senders",
                            id_to_remove, id
                        );
                        return
                    }
                }
            }
            Server => {
                if let Some(sender) = self.server_command_senders.get(&id) {
                    if let Err(_e) = sender.try_send(ServerCommand::RemoveSender(id_to_remove)) {
                        println!(
                            "error in removing node {} from server {} senders",
                            id_to_remove, id
                        );
                        return;
                    }
                }
            }
        }

        if let Some((_nodetype, ids)) = self.network_graph.get_mut(&id) {
            ids.retain(|id| {id != &id_to_remove});
        }
        self.log.push_back(LogEntry::new(
            Cause::Managing,
            id_to_remove,
            format!("node {} removed from senders", id),
        ));
    }

    //functions for add_sender
    fn match_node_type_for_add_sender(&mut self, n_type: NodeType, id: NodeId, id_to_add:NodeId){
        match n_type {
            Client => {
                if let Some(sender) = self.client_command_senders.get(&id) {
                    if let Some(senderpacket) = self.all_sender_packets.get(&id_to_add) {
                        if let Err(_e) = sender.try_send(ClientCommand::AddSender(id_to_add, senderpacket.clone())) {
                            println!("error adding drone {} to client {} senders", id_to_add, id);
                            return;
                        }
                    }
                }/*else if DEBUG_MODE{
                    println!("error adding drone {} to server {} senders", id_to_add, id);
                }*/
            }
            Drone => {
                if let Some(sender) = self.drone_command_senders.get(&id) {
                    if let Some(senderpacket) = self.all_sender_packets.get(&id_to_add) {
                        if let Err(_e) = sender.try_send(AddSender(id_to_add, senderpacket.clone())) {
                            println!("error adding drone {} to drone {} senders", id_to_add, id);
                            return;
                        }
                    }
                }/* else if DEBUG_MODE{
                    println!("error adding drone {} to server {} senders", id_to_add, id);
                }*/
            }
            Server => {
                if let Some(sender) = self.server_command_senders.get(&id) {
                    if let Some(senderpacket) = self.all_sender_packets.get(&id_to_add) {
                        if let Err(_e) = sender.try_send(ServerCommand::AddSender(id_to_add, senderpacket.clone())) {
                            println!("error adding drone {} to server {} senders", id_to_add, id);
                            return;
                        }
                    }
                }/* else if DEBUG_MODE{
                    println!("error adding drone {} to server {} senders", id_to_add, id);
                }*/
            }
        }
        if let Some((_, ids)) = self.network_graph.get_mut(&id) {
            ids.insert(id_to_add);
        }

        println!("drone {} added to client {} senders", id_to_add, id);
        self.log.push_back(LogEntry::new(
            Cause::Managing,
            id,
            format!("drone {} added to senders", id_to_add),
        ));
    }
}

pub enum Cause {
    Dropped,
    Sent,
    Shortcut,
    Managing, //this cause is for the log entry "caused" by manipulation of the SC
    Error,
    AckReceived,
    NackReceived,
    MissingDestination,
    LostMessage,
    LostFragment,
    DroneInsideDestination,
    Flood,
}

pub struct LogEntry {
    pub cause: Cause,
    pub node_id: NodeId,
    pub message: String,
}
impl LogEntry {
    pub fn new(cause: Cause, node_id: NodeId, message: String) -> LogEntry {
        LogEntry {
            cause,
            node_id,
            message,
        }
    }
    pub fn get_id(&self) -> NodeId {
        self.node_id
    }
}

impl Display for LogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Node {} notified:  {}", self.node_id, self.message)
    }
}

impl Debug for LogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "id: {}", self.node_id)
    }
}


























