use egui::ahash::HashMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use wg_2024::network::NodeId;
use crate::network_edge::EdgeType;

#[derive(Debug, Clone)]
pub struct Message {
    pub source_id: NodeId,
    pub session_id: u64,
    pub content: ContentType,
}
impl Message {
    pub fn new(source_id: NodeId, session_id: u64, content: ContentType) -> Self {
        Self{
            source_id,
            session_id,
            content,
        }
    }
    pub fn stringify_content(&self) -> String {
        match &self.content {
            ContentType::TextRequest(inner) =>  inner.stringify(),
            ContentType::TextResponse(inner) => inner.stringify(),
            ContentType::MediaRequest(inner) => inner.stringify(),
            ContentType::MediaResponse(inner) =>  inner.stringify(),
            ContentType::ChatRequest(inner) =>  inner.stringify(),
            ContentType::ChatResponse(inner) => inner.stringify(),
            ContentType::TypeExchange(inner) => inner.stringify(),
            ContentType::EdgeNack(inner) => inner.stringify(),
        }
    }

}

#[derive(Clone, Debug)]
pub enum ContentType{
    TypeExchange(TypeExchange),
    TextRequest(TextRequest),
    TextResponse(TextResponse),
    MediaRequest(MediaRequest),
    MediaResponse(MediaResponse),
    ChatRequest(ChatRequest),
    ChatResponse(ChatResponse),
    EdgeNack(EdgeNackType),
}

impl Default for ContentType {
    fn default() -> Self {
        Self::TypeExchange(TypeExchange::default())
    }
}

pub trait MessageType: Serialize + DeserializeOwned {
    fn stringify(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
    fn from_string(raw: String) -> Result<Self, String> {
        serde_json::from_str(raw.as_str()).map_err(|e| e.to_string())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeExchange {
    TypeRequest {
        from: NodeId,
    },
    TypeResponse {
        edge_type: EdgeType,
        from: NodeId,
    },
}
impl Default for TypeExchange {
    fn default() -> Self {
        Self::TypeRequest{from: Default::default() }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatRequest {
    ClientList, // From client to server.
    Register(NodeId), // From client to server.
    SendMessage { from: NodeId, to: NodeId, message: String}, // From client to server.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatResponse {
    ClientList(Vec<NodeId>), // From server to client.
    MessageFrom { from: NodeId, message: String }, // From server to client.
    ClientNotFound(NodeId), // If you try to send a message to a client, but I can't communicate with it.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TextRequest {
    TextList, // Only to text severs
    TextFile(u64), // Only to text severs
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaRequest {
    MediaList, // Used only between servers.
    Media(u64), // Only from client to media server.
}

// solo dai text server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TextResponse {
    TextList(HashMap<u64, String>), // ID and name of all texts contained in the server.
    MediaReferences(HashMap<u64, (String, Vec<NodeId>)>), // List of media contained inside a specific text, and which server has them.
    Incomplete(u64), // If we don't know where some of the media are.
    NotFound(u64), // If we didn't find that ID.
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaResponse {
    MediaList(Vec<(u64, String)>), // Used only between servers.
    Media(u64, String, Vec<u8>), // Sent back to clients; media_id, media_name and the media itself;
    NotFound(u64), // If we didn't find that ID.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeNackType{
    //..
    UnexpectedMessage, //tipo se un chat client trova un messaggio da text client...
}

impl MessageType for TextRequest {}
impl MessageType for MediaRequest {}
impl MessageType for ChatRequest {}
impl MessageType for TextResponse {}
impl MessageType for MediaResponse {}
impl MessageType for ChatResponse {}
impl MessageType for TypeExchange {}
impl MessageType for EdgeNackType {}
