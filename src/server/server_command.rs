use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

pub enum ServerCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    SendPacket(Packet),
} // I copied the one from client, but I need to change these

pub enum ServerEvent {
    PacketSent(Packet),
    PacketReceived(Packet),
}
