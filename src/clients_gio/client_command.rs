use std::collections::HashMap;
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
    OpenChat, //special command that make a chat client send the SC the messages with all the nodes
    OpenContent // as over
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
    WrongDestinationType(NodeId, NodeId), //first node id think that second node id is of wrong type

    SendDestination(NodeId, NodeId), //first is src second is dst: as soon as a client get a dst, advise SC with it

    SendChats(NodeId, HashMap<NodeId, Vec<(NodeId, String)>>), //first is src, second is dst, third is the complete chat
    SendContent(NodeId, HashMap<u8, Vec<u64>>), //first is src, second is a id, third a media

}


// i want to add a WrongTypeDestination (node_id) for when i want to contact a destination that is not the right type


/*
    the same procedure will be applied to server command
    some client events will be sent immediately when


*/