use crossbeam_channel::Sender;
use wg_2024::controller::DroneEvent;
use wg_2024::network::NodeId;
use wg_2024::packet::Packet;

#[derive(Debug, Clone)]
pub enum ServerCommand {
    RemoveSender(NodeId),
    AddSender(NodeId, Sender<Packet>),
    Flood,
    AddFile(String),
    InstantCrash,
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    Flooding(NodeId),

    PacketSent(Packet),
    PacketReceived(Packet),
    PacketSendingError(Packet),
    AckReceived(Packet), // Packet with inside the ACK (so we can get the NodeIds in SC).
    NackReceived(Packet),
    
    MissingDestination(NodeId, NodeId), // First id is server one, second is missing destination.
    MissingRoute(NodeId, NodeId), // First id is server one, second is destination for which I don't have route.
    LostMessage(u64, NodeId, String), // session_id, NodeId of initiator and error String.
    LostFragment(u64, NodeId, u64), // session_id, NodeId and fragment_index.
    DiscardedMessage(NodeId, u64), // Server NodeId and session_id.
    DroneInsideDestination(NodeId, NodeId), // Received when a destination is removed because it's a drone; Server id and drone id.
    WrongDestinationType(NodeId, NodeId), // First NodeId thinks that second NodeId is of wrong type.
    WrongDestination(NodeId, Packet), // Server id and packet sent to wrong dst. Used for ACKs or NACKs that might create an error loop if resolved normally.
    
    FileNotFound(NodeId, u64), // Server id and file_id requested but not owned.
    IncompleteFile(NodeId, u64), // Server id and file_id of file whose at least one media is still missing.
    FilesState(NodeId, Vec<(u64, String)>, Vec<(u64, String)>), // Server id, completed file and files with still missing medias.
    FileNotReadable(NodeId, String, String), // Server id and file name and error; Used when a '.read' fails.
    MediaNotFound(NodeId, u64), // Server id and missing media id
    MediaState(NodeId, Vec<(u64, String)>), // server_id and ids and names of medias in it.
    
    ClientRegistered(NodeId, NodeId), // Server id and client id
    ClientAlreadyRegistered(NodeId, NodeId), // Server id and client id
    
    WrongCommandGiven(NodeId, ServerCommand), // Server id and wrong command it received.
    ControllerShortcut(DroneEvent), // In case I have problems sending an ACK or NACK.
}
