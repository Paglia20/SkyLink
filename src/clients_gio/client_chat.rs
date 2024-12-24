use std::collections::{HashMap, HashSet};
use crossbeam_channel::{select_biased, Receiver, Sender, TrySendError};
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Packet, PacketType};
use crate::clients_gio::client_trait::{Client};
use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_type::ClientType;
use crate::message::{ChatRequest, ChatResponse, Message, MessageType};
use crate::network_edge::NetworkEdge;
use crate::routing::RouteList;

pub struct ChatClient {
    node_id: NodeId,
    command_recv: Receiver<ClientCommand>,
    event_send: Sender<ClientEvent>,
    packet_recv: Receiver<Packet>,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    flood_ids: HashSet<(u64, NodeId)>, //just like drones
    used_session_id: HashSet<u64>,

    paths: HashMap<NodeId, RouteList>, //lui ha bisogno di parlare solo con chat server (il node id è di un chatserver)
    contact_list: HashMap<NodeId, Vec<NodeId>>,  //first NodeId is the client he want to comunicate to, the second one is the server he has to write to, this two hash might be merged in future
    fragments: HashMap<u64, Vec<Fragment>>,
}

impl NetworkEdge for ChatClient {
    type RequestType = ChatRequest;
    type ResponseType = ChatResponse;

    fn send_message<M: MessageType>(&mut self, message: Message<M>, destination: NodeId) -> Result<(), String> {
        self.fragments.insert(message.session_id, Self::fragment_message(&message));

        Ok(())
    }
}

impl Client for ChatClient  {

    fn new(id: NodeId, event_send: Sender<ClientEvent>, command_recv: Receiver<ClientCommand>, packet_recv: Receiver<Packet>, packet_send: HashMap<NodeId, Sender<Packet>>) -> Self {
        ChatClient{
            node_id: id,
            command_recv,
            event_send,
            packet_recv,
            packet_send,
            flood_ids: HashSet::new(),
            used_session_id: HashSet::new(),
            paths: HashMap::new(),
            contact_list: HashMap::new(),
            fragments: Default::default(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased!{
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

    fn handle_packet(&mut self, packet: Packet) {
        match packet.pack_type{
            PacketType::MsgFragment(_) => {}
            PacketType::Ack(_) => {}
            PacketType::Nack(_) => {}
            PacketType::FloodRequest(_) => {}
            PacketType::FloodResponse(_) => {}
        }
    }

    fn handle_command(&mut self, command: ClientCommand) {
        match command{
            ClientCommand::RemoveSender(_) => {}
            ClientCommand::AddSender(_, _) => {}
            ClientCommand::SendPacket(packet) => {}
        }
    }

    fn get_client_type(&self) -> ClientType {
        ClientType::ChatClient
    }
}