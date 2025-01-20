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
    TypeRequest{
        from: NodeId,
    },
    TypeResponse{
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
pub enum TextRequest {
    TextList,
    Text(u64),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaRequest {
    MediaList,
    Media(u64),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatRequest {
    ClientList,
    Register(NodeId),
    SendMessage {
        from: NodeId,
        to: NodeId,
        message: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TextResponse {
    TextList(Vec<u64>),
    Text(String),
    NotFound,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaResponse {
    MediaList(Vec<u64>),
    Media(Vec<u8>), // should we use some other type? gio: maybe add not found? anyway I don't get the type inside MediaList
    // Leo: I've still no idea on how to use the medias, so we'll change these if needed.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatResponse {
    ClientList(Vec<NodeId>),
    MessageFrom { from: NodeId, message: String },
    MessageSent,
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
