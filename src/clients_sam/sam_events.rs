// sam_events.rs
use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

#[derive(Debug, Clone)]
pub enum ClientCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    Flood,
    RetrieveList(NodeId),
    Register(NodeId),
    SendMSG(NodeId, String),
    GetTextFile(u64),
    GetContent(u64)
}

#[derive(Debug, Clone)]
pub enum ClientEvent {
    PacketSent(Packet),
    PacketReceived(Packet),
    PacketSendingError(Packet),
    AckReceived(Packet),
    NackReceived(Packet),

    MissingDestination(NodeId, NodeId),
    MissingRoute(NodeId, NodeId),
    LostMessage(u64, NodeId),
    LostFragment(u64, NodeId, u64),
    DroneInsideDestination(NodeId),
    WrongDestinationType(NodeId, NodeId),

    SendDestinations(NodeId, NodeId),
    SendContactsToSC(NodeId, NodeId),
    MissingContacts(NodeId, NodeId),
    ReceivedChatText(NodeId, NodeId, String),
    RegisterSuccessfully(NodeId, NodeId),

    SendTextList(NodeId, u64, String),
    SendCatalogue(NodeId, u64, String),
    SendMedia(NodeId, u64, String, Vec<u8>),
    MissingDestForMedia(NodeId, u64),
    MissingTextList(NodeId, u64),
    ErrorReassembling(NodeId),
    Flooding(NodeId)
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    WaitingForType,
    Ready,
    Failed
}