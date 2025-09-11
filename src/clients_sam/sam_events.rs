// Events and commands exchanged between the simulation controller (SC) and clients.

use crossbeam_channel::Sender;
use wg_2024::network::NodeId;
use wg_2024::packet::{Packet};

// Commands sent by the SC to clients.
#[derive(Debug, Clone)]
pub enum ClientCommand {
    // Remove a sender from the available map.
    RemoveSender(NodeId),
    // Add a sender channel to a client.
    AddSender(NodeId, Sender<Packet>),
    // Trigger a flood discovery from this client.
    Flood,
    // Force the client to crash immediately.
    InstantCrash,
    // Ask a client to retrieve a list (contacts or texts).
    RetrieveList(NodeId),
    // Chat client: register with a server.
    Register(NodeId), // dst id
    // Chat client: send a message to a peer (via a server).
    SendMSG(NodeId, String),
    // Web client: fetch a text file of media references.
    GetTextFile(u64),
    // Web client: fetch content by ID.
    GetContent(u64),
}

#[derive(Debug, Clone)]
pub enum ClientEvent {
    // Client has initiated a flood.
    Flooding(NodeId),
    // A packet was sent.
    PacketSent(Packet),
    // A packet was received.
    PacketReceived(Packet),
    // Error while sending a packet.
    PacketSendingError(Packet),
    // Received an ACK packet (includes the node ID).
    AckReceived(Packet),
    // Received a NACK packet (includes the node ID).
    NackReceived(Packet),
    // Destination missing in the network view (may trigger flood).
    MissingDestination(NodeId, NodeId),
    // Route missing to reach a destination (may trigger flood).
    MissingRoute(NodeId, NodeId),
    // Message lost (identified by session ID).
    LostMessage(u64, NodeId),
    // Fragment lost (session ID, node, fragment index).
    LostFragment(u64, NodeId, u64), // session_id, NodeId that lost it and fragment_index
    // Destination removed because it was a drone.
    DroneInsideDestination(NodeId),
    // Attempted to send to a node of the wrong type.
    WrongDestinationType(NodeId, NodeId), //first node id think that second node id is of wrong type
    // Newly available destination learned.
    SendDestinations(NodeId, NodeId),
    // Failed to reassemble a message.
    ErrorReassembling(NodeId),

    // Chat client only
    // Chat client: report discovered contacts.
    // (src, dst).
    SendContactsToSC(NodeId, NodeId),
    // Chat client: missing contact.
    MissingContacts(NodeId, NodeId),
    // Chat client: deliver received chat text.
    // (from, dst, message).
    ReceivedChatText(NodeId, NodeId, String),
    // Chat client: successful registration.
    RegisterSuccessfully(NodeId, NodeId),

    // Web client only
    // Web client: report retrieved text list.
    SendTextList(NodeId, u64, String),
    // Web client: report catalogue updates.
    SendCatalogue(NodeId, u64, String),
    // Web client: report retrieved media.
    SendMedia(NodeId, u64, String, Vec<u8>),
    // Web client: missing destination for media.
    MissingDestForMedia(NodeId, u64),
    // Web client: missing text list.
    MissingTextList(NodeId, u64),
}

