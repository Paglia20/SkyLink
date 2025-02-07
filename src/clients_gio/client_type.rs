use serde::{Deserialize, Serialize};

///There are two types of client, Browser and Chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientType {
    WebBrowser,
    ChatClient,
}
