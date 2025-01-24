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
pub enum ChatResponse {
    ClientList(Vec<NodeId>),
    MessageFrom { from: NodeId, message: String },
    MessageSent,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TextRequest {
    TextList, // solo ai text server
    TextFile(u64), // solo ai text server
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaRequest {
    MediaList, // solo dai text server ai media server
    Media(u64), // solo da webclient ai media server !!
}

// solo dai text server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TextResponse {
    //la prima stringa indica il nome del textfile,
    //la seconda è il nome di ogni media associato all'id
    TextLists(HashMap<u64, (String, Vec<(u64, String)>)>),

    MediaReferences(HashMap<u64, NodeId>), //chi ha quel media
    NotFound(u64), //i didn't find that id.
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaResponse {
    MediaList(Vec<(u64, String)>), // solo tra server viene usata !!
    Media(((u64, String), Vec<u8>)),
}

/*

Ricapitolando:
id media > 1000
id texfiles < 1000


web client              text server                     media server
                            ---------------medialist? --->
                            <---------------medialist! ---
                         (process)

  ----------textlist? --->
  <--------TextLists! -----
  -------textfile(u64) --->
  <--------MediaReferences! ---

  ----------------------------------------media(u64)? --->
  <----------------------------------------media(..)! ---


 */


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
