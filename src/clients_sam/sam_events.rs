// sam_events.rs
use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

#[derive(Debug, Clone)]
pub enum ClientCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    Flood,
    RetrieveList(NodeId), // Both for a chat list or a text/media list
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

// Our unique feature: explicit connection state tracking
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    WaitingForType,
    Ready,
    Failed
}

// Event compatibility verification layer
pub fn verify_event_compatibility(event: &ClientEvent) -> bool {
    match event {
        ClientEvent::SendMedia(src, media_id, name, data) => {
            !data.is_empty() && !name.is_empty() && *media_id > 0
        }
        ClientEvent::ReceivedChatText(from, to, msg) => {
            from != to && !msg.is_empty()
        }
        ClientEvent::SendTextList(src, id, name) => {
            !name.is_empty() && *id > 0
        }
        ClientEvent::SendCatalogue(src, id, name) => {
            !name.is_empty() && *id > 0
        }
        ClientEvent::MissingDestination(src, dst) |
        ClientEvent::MissingRoute(src, dst) |
        ClientEvent::SendDestinations(src, dst) |
        ClientEvent::SendContactsToSC(src, dst) |
        ClientEvent::MissingContacts(src, dst) => src != dst,
        _ => true
    }
}

// Helper for safe event sending
pub fn send_verified_event(event: ClientEvent, sender: &Sender<ClientEvent>) -> bool {
    if verify_event_compatibility(&event) {
        sender.send(event).is_ok()
    } else {
        false
    }
}

// Helper for type checking
pub fn is_valid_node_ids(src: NodeId, dst: NodeId) -> bool {
    src != dst && src != NodeId::MAX && dst != NodeId::MAX
}