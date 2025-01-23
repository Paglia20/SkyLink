use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerType {
    Chat,
    Content(ContentServerType),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentServerType{
    Text,
    Media
}

