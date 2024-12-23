use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

pub enum ServerCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    SendPacket(Packet),
}

pub enum ServerEvent {
    PacketSent(Packet),
    PacketReceived(Packet),
}