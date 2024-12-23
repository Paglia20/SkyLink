use crate::message::{ChatRequest, ChatResponse};
use crate::network_edge::NetworkEdge;
use crate::server::server_trait::{Server};
use crate::server::server_type::ServerType;

pub struct ChatServer {}

impl NetworkEdge for ChatServer {
    type RequestType = ChatRequest;
    type ResponseType = ChatResponse;

}
impl Server for ChatServer {
    fn handle_request(&mut self, request: Self::RequestType) -> Self::ResponseType {
        match request {
            ChatRequest::ClientList => {
                println!("Sending ClientList");
                ChatResponse::ClientList(vec![1, 2])
            }
            ChatRequest::Register(id) => {
                println!("Registering {}", id);
                ChatResponse::ClientList(vec![1, 2])
            }
            ChatRequest::SendMessage {
                message,
                to,
                from: _,
            } => {
                println!("Sending message \"{}\" to {}", message, to);
                // effectively forward message
                ChatResponse::MessageSent
            }
        }
    }

    fn get_sever_type() -> ServerType {
        ServerType::Chat
    }
}
