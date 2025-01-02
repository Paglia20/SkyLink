use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerType {
    Chat,
    Content,
    Media, //I'm not 100% sure these were the right ones.
}
