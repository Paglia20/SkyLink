use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{Packet};

#[derive(Debug, Clone)]
pub enum ClientCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    Flood,
    RetrieveList(NodeId), // Both for a chat list or a text/media list
    // For a webclient is a retrieve TextList

    // Special commands for chat client
    Register(NodeId), // dst id
    SendMSG(NodeId, String), // Contact id, not dst (that will be a server), nb: it's different from sendmessage

    // Special command for webclient
    GetTextFile(u64), // Get a TextFile full of media references, hence the response will be a mediareferences(..)
    GetContent(u64) // Get a Content from any server with that given id (the string)
}

#[derive(Debug, Clone)]
pub enum ClientEvent {
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

    // Chat client only
    SendContactsToSC(NodeId, NodeId), // First is src second is dst
    MissingContacts(NodeId, NodeId), // First is src second is dst
    ReceivedChatText(NodeId, NodeId, String), // From-dst-chat text
    RegisterSuccessfully(NodeId, NodeId),

    // Web client only
    SendTextList(NodeId, u64, String),
    SendCatalogue(NodeId, u64, String),
    SendMedia(NodeId, u64, String, Vec<u8>),
}


