use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use wg_2024::network::NodeId;

#[derive(Debug, Clone)]
pub struct Message {
    pub source_id: NodeId,
    pub session_id: u64,
    pub content: ContentType,
}
impl Message {
    pub fn stringify_content(&self) -> String {
        match &self.content {
            ContentType::TextRequest(inner) =>  inner.stringify(),
            ContentType::TextResponse(inner) => inner.stringify(),
            ContentType::MediaRequest(inner) => inner.stringify(),
            ContentType::MediaResponse(inner) =>  inner.stringify(),
            ContentType::ChatRequest(inner) =>  inner.stringify(),
            ContentType::ChatResponse(inner) => inner.stringify(),
        }
    }


}

#[derive(Clone, Debug)]
pub enum ContentType{
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
    MessageFrom { from: NodeId, message: Vec<u8> },
    MessageSent,
}

impl MessageType for TextRequest {}
impl MessageType for MediaRequest {}
impl MessageType for ChatRequest {}
impl MessageType for TextResponse {}
impl MessageType for MediaResponse {}
impl MessageType for ChatResponse {}
