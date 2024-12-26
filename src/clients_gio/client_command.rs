use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{Ack, Nack, Packet};
use crate::message::{Message, MessageType};

pub enum ClientCommand<M:MessageType> {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    SendMessage(NodeId,Message<M>),
}

pub enum ClientEvent{
    PacketSent(Packet),
    PacketReceived(Packet),
    PacketSendingError(Packet),
    AckReceived(Ack),
    NackReceived(Nack),
}
