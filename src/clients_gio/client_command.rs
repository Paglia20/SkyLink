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

    //special commands for chat client
    Register(NodeId), //dst id
    SendMSG(NodeId, String), //contact id, not dst (that will be a server), nb: it's different from sendmessage

    //special command for webclient
    GetContent(String) //get a Content from any server with that given id (the string)
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

    //chat client only
    SendContactsToSC(NodeId, NodeId), //first is src second is dst
    MissingContacts(NodeId, NodeId), //first is src second is dst
    SendDestinations(NodeId, NodeId),
    SendChatText(NodeId, NodeId, String) //src-dst-chat text


}



/*
idee per dire al sc robe tipo chats ecc...

1) clientevent messagesent, messagereceived, content received,



*/

