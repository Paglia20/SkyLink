use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

#[derive(Debug, Clone)]
pub enum ServerCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    //SendPacket(Packet), // Not sure yet if I want this or not
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    PacketSent(Packet),
    PacketReceived(Packet),
    PacketSendingError(Packet),
    AckReceived(Packet), //packet with inside the ack (so I can get the NodeId in SC)
    NackReceived(Packet),
    // CreatedConnection(NodeId, NodeId),
    MissingDestination(NodeId),
    MissingRoute(NodeId),
    LostMessage(u64, NodeId), // session_id and NodeId
    LostFragment(u64, NodeId, u64), // session_id, NodeId and fragment_index
    DroneInsideDestination(NodeId), // Received when a destination is removed because it's a drone
    // OpenedChat(NodeID),
    WrongDestinationType(NodeId, NodeId), //first node id think that second node id is of wrong type
}
