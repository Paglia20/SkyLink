use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{Packet};

///Client Commands by the SC
#[derive(Debug, Clone)]
pub enum ClientCommand {
    ///remove a sender from the available
    RemoveSender(NodeId),
    ///add a sender to a client
    AddSender(NodeId, Sender<Packet>),
    ///flood network with client
    Flood,
    ///Retrieve List for a Client: a Contact List or a TextList
    RetrieveList(NodeId),

    ///Special commands for chat client, Register to a Server
    Register(NodeId), // dst id
    ///Special commands for chat client, Send a Message to NodeId (the actual destination will be a server)
    SendMSG(NodeId, String),

    ///WebClient only, Get a TextFile full of media references, hence the response will be a media references
    GetTextFile(u64),
    ///WebClient only, Get a Content from any server with that given id
    GetContent(u64),
    ///Instant Crash for Client
    InstantCrash,

}

#[derive(Debug, Clone)]
pub enum ClientEvent {
    ///Client is Successfully Flooding
    Flooding(NodeId),

    ///Client Sent Packet
    PacketSent(Packet),
    ///Client received Packet
    PacketReceived(Packet),
    ///Client encountered an error sending the Packet
    PacketSendingError(Packet),
    ///Client received Ack, inside there is the Packet with inside the ack (so I can get the node_id in SC)
    AckReceived(Packet),
    ///Client received Nack, inside there is the Packet with inside the nack (so I can get the node_id in SC)
    NackReceived(Packet),
    ///Client is Missing this Destination in his network, might cause new flood
    MissingDestination(NodeId, NodeId),
    ///Client is Missing a route to get to destination in his network, might cause new flood
    MissingRoute(NodeId, NodeId),
    ///Client Lost Message of session id (u64)
    LostMessage(u64, NodeId),
    ///Client Lost Fragment number (second u64) of session id (first u64)
    LostFragment(u64, NodeId, u64), // session_id, NodeId that lost it and fragment_index
    ///Client sent this when a destination is removed because it's a drone
    DroneInsideDestination(NodeId),
    ///Client tried to send something to a node of wrong type
    WrongDestinationType(NodeId, NodeId), //first node id think that second node id is of wrong type
    ///Client has a new Destination available
    SendDestinations(NodeId, NodeId),
    ///Client didn't reassemble correctly a message
    ErrorReassembling(NodeId),

    // Chat client only
    ///Chat Client needs to send the found contacts to SC
    ///First is src second is dst
    SendContactsToSC(NodeId, NodeId),
    ///Chat Client is missing the contact
    MissingContacts(NodeId, NodeId),
    ///Chat Client needs to send the arrived texts
    ///From-dst-chat text
    ReceivedChatText(NodeId, NodeId, String),
    ///Chat Client is successfully registered to dst
    RegisterSuccessfully(NodeId, NodeId),

    // Web client only
    ///Web Client needs to send the found text list to SC
    SendTextList(NodeId, u64, String),
    ///Web Client needs to send the updates he did to his catalogue
    SendCatalogue(NodeId, u64, String),
    ///Web Client needs to send the found Medias to SC
    SendMedia(NodeId, u64, String, Vec<u8>),

    MissingDestForMedia(NodeId, u64),
    MissingTextList(NodeId, u64),
}


