// sam_client_type.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClientType {
    WebBrowser,
    ChatClient,
}