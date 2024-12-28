use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{Ack, Nack, Packet};

pub enum ServerCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    //SendPacket(Packet), // Not sure yet if I want this or not
}

pub enum ServerEvent {
    PacketSent(Packet),
    PacketReceived(Packet),
    PacketSendingError(Packet),
    AckReceived(Ack),
    NackReceived(Nack),
    // CreatedConnection(NodeId, NodeId),
}
