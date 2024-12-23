use std::collections::HashMap;
use crossbeam_channel::{Receiver, Sender};
use crate::message::{ChatRequest, ChatResponse, Message, MessageType, Request, Response};
use wg_2024::network::*;
use wg_2024::packet::Packet;
use crate::clients_gio::command::{ClientCommand, ClientEvent};

pub enum ClientType{
    WebBrowser,
    ChatClient,
}

pub trait Client {
    type RequestType: Request;
    type ResponseType: Response;

    fn new(
        id: NodeId,
        event_send: Sender<ClientEvent>,
        command_recv: Receiver<ClientCommand>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    )-> Self;

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

    fn send_request(&mut self, _request: Self::RequestType);

    fn handle_response(&mut self, _response: Self::ResponseType);

    fn get_client_type(&self) -> ClientType;

}