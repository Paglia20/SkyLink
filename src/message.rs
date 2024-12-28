use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use wg_2024::network::NodeId;

#[derive(Debug, Clone)]
pub struct Message {
    pub source_id: NodeId,
    pub session_id: u64,
    pub content: ContenType,
}
impl Message {
    pub fn stringify_content(&self) -> String {
        match &self.content {
            ContenType::TextRequest(inner) =>  inner.stringify(),
            ContenType::TextResponse(inner) => inner.stringify(),
            ContenType::MediaRequest(inner) => inner.stringify(),
            ContenType::MediaResponse(inner) =>  inner.stringify(),
            ContenType::ChatRequest(inner) =>  inner.stringify(),
            ContenType::ChatResponse(inner) => inner.stringify(),
        }
    }


}

#[derive(Clone, Debug)]
pub enum ContenType{
    TextRequest(TextRequest),
    TextResponse(TextResponse),
    MediaRequest(MediaRequest),
    MediaResponse(MediaResponse),
    ChatRequest(ChatRequest),
    ChatResponse(ChatResponse),
}

pub trait MessageType: Serialize + DeserializeOwned {
    fn stringify(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
    fn from_string(raw: String) -> Result<Self, String> {
        serde_json::from_str(raw.as_str()).map_err(|e| e.to_string())
    }
}
pub trait Request: MessageType {}
pub trait Response: MessageType {}

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
    Media(Vec<u8>), // should we use some other type? gio: maybe add not found? anyway i don't get the type inside MediaList
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatResponse {
    ClientList(Vec<NodeId>),
    MessageFrom { from: NodeId, message: Vec<u8> },
    MessageSent,
}

impl MessageType for TextRequest {}
impl MessageType for MediaRequest {}
impl MessageType for ChatRequest {}
impl MessageType for TextResponse {}
impl MessageType for MediaResponse {}
impl MessageType for ChatResponse {}

impl Request for TextRequest {}
impl Request for MediaRequest {}
impl Request for ChatRequest {}

impl Response for TextResponse {}
impl Response for MediaResponse {}
impl Response for ChatResponse {}
