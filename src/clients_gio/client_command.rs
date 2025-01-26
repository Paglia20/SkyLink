use crate::message::{Message};
use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{Packet};

#[derive(Debug, Clone)]
pub enum ClientCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    Flood,
    RetrieveList(NodeId), //both for a chat list or a text/media list
    //for a webclient is a retrieve TextList

    //special commands for chat client
    Register(NodeId), //dst id
    SendMSG(NodeId, String), //contact id, not dst (that will be a server), nb: it's different from sendmessage

    //special command for webclient
    GetTextFile(u64), //get a TextFile full of media references, hence the response will be a mediareferences(..)
    GetContent(u64) //get a Content from any server with that given id (the string)
}

//add a send packet for testing??

#[derive(Debug, Clone)]
pub enum ClientEvent {
    Flooding(NodeId),

    PacketSent(Packet),
    PacketReceived(Packet),
    PacketSendingError(Packet),
    AckReceived(Packet), //packet with inside the ack (so i can get the nodeid in SC)
    NackReceived(Packet),
    MissingDestination(NodeId, NodeId),
    MissingRoute(NodeId, NodeId),
    LostMessage(u64, NodeId), // session_id and NodeId that lost it
    LostFragment(u64, NodeId, u64), // session_id, NodeId that lost it and fragment_index
    DroneInsideDestination(NodeId), // Received when a destination is removed because it's a drone
    WrongDestinationType(NodeId, NodeId), //first node id think that second node id is of wrong type
    SendDestinations(NodeId, NodeId),

    //chat client only
    SendContactsToSC(NodeId, NodeId), //first is src second is dst
    MissingContacts(NodeId, NodeId), //first is src second is dst
    SendChatText(NodeId, NodeId, String), //src-dst-chat text

    //Web client only
    SendTextList(NodeId, u64, String),
    SendCatalogue(NodeId, u64, String),
    SendMedia(NodeId, u64, String, Vec<u8>),
}


