use wg_2024::network::NodeId;
use crate::message::{Message, MessageType, Request, Response};

pub trait NetworkEdge {
    type RequestType: Request;
    type ResponseType: Response;

    fn compose_message(
        source_id: NodeId,
        session_id: u64,
        raw_content: String,
    ) -> Result<Message<Self::RequestType>, String> {
        let content = Self::RequestType::from_string(raw_content)?;
        Ok(Message {
            session_id,
            source_id,
            content,
        })
    }
}