use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ServerType {
    Chat,
    Content(ContentServerType),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContentServerType{
    Text,
    Media
}

//i added partial eq e Hash to insert the type in hashsets