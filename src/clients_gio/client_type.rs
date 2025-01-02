use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientType {
    WebBrowser,
    ChatClient,
}
