use crate::message::{Message};
use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{Packet};

#[derive(Debug, Clone)]
pub enum ClientCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    SendMessage(NodeId, Message),
    Flood,
}

//add a send packet for testing??

#[derive(Debug, Clone)]
pub enum ClientEvent {
    PacketSent(Packet),
    PacketReceived(Packet),
    PacketSendingError(Packet),
    AckReceived(Packet), //packet with inside the ack (so i can get the nodeid in SC)
    NackReceived(Packet),
    MissingDestination(NodeId),
    MissingRoute(NodeId),
    LostMessage(u64, NodeId), // session_id and NodeId
    LostFragment(u64, NodeId, u64), // session_id, NodeId and fragment_index
    DroneInsideDestination(NodeId), // Received when a destination is removed because it's a drone
    // OpenedChat(NodeID),
    SendContacts(NodeId,NodeId), //first is src second is dst
    WrongDestinationType(NodeId, NodeId), //first node id think that second node id is of wrong type
}


// I want to add a WrongTypeDestination (node_id) for when I want to contact a destination that is not the right type