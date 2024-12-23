use wg_2024::network::*;
use crate::network_edge::NetworkEdge;
use crate::server::server_type::ServerType;

pub trait Server: NetworkEdge {
    fn on_request_arrived(&mut self, source_id: NodeId, session_id: u64, raw_content: String) {
        if raw_content == "ServerType" {
            let _server_type = Self::get_sever_type();
            // send response
            return;
        }
        match Self::compose_message(source_id, session_id, raw_content) {
            Ok(message) => {
                let response = self.handle_request(message.content);
                self.send_response(response);
            }
            Err(str) => panic!("{}", str),
        }
    }

    fn send_response(&mut self, _response: Self::ResponseType) {
        // send response
    }

    fn handle_request(&mut self, request: Self::RequestType) -> Self::ResponseType;

    fn get_sever_type() -> ServerType;
}
