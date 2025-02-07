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
    ///Special commands for chat client, Send a Message to NodeId
    SendMSG(NodeId, String), // Contact id, not dst (that will be a server), nb: it's different from sendmessage


    ///WebClient only, Get a TextFile full of media references, hence the response will be a media references
    GetTextFile(u64),

    ///WebClient only, Get a Content from any server with that given id
    GetContent(u64)
}

#[derive(Debug, Clone)]
pub enum ClientEvent {
    ///Client is Successfully Flooding
    Flooding(NodeId),

    PacketSent(Packet),
    PacketReceived(Packet),
    PacketSendingError(Packet),
    AckReceived(Packet), // Packet with inside the ack (so I can get the node_id in SC)
    NackReceived(Packet),
    MissingDestination(NodeId, NodeId),
    MissingRoute(NodeId, NodeId),
    LostMessage(u64, NodeId), // session_id and NodeId that lost it
    LostFragment(u64, NodeId, u64), // session_id, NodeId that lost it and fragment_index
    DroneInsideDestination(NodeId), // Received when a destination is removed because it's a drone
    WrongDestinationType(NodeId, NodeId), //first node id think that second node id is of wrong type
    SendDestinations(NodeId, NodeId),
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


