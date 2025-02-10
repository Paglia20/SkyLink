use crate::message::{ChatRequest, ChatResponse, ContentType, EdgeNackType, MediaRequest, MediaResponse, Message, MessageType, TextRequest, TextResponse, TypeExchange};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::*;
use crate::clients_gio::client_type::ClientType;
use crate::server::server_type::ServerType;

pub trait NetworkEdge {
    fn send_message(&mut self, message: Message, destination: NodeId);

    fn fragment_message(message: &Message) -> Vec<Fragment> {
        let all_bytes = message.stringify_content().into_bytes();
        let total_n_fragments = (all_bytes.len() as u64).div_ceil(128);

        let mut out = Vec::new();
        for (frag_id, chunk) in all_bytes.chunks(128).enumerate() {
            let mut padded_chunk = [0u8; 128];
            let len = chunk.len();

            // I use a padded_chunk initially at 0, where I put the fragment up to its length.
            padded_chunk[..len].copy_from_slice(chunk);

            let fragment = Fragment::new(frag_id as u64, total_n_fragments, padded_chunk);

            out.push(fragment);
        }
        out
    }

    // We assume that this function will be called only when the client or server has
    // already collected all fragments of a message and sent the Ack.
    fn reassemble_message(session_id: u64, source_id: NodeId,packets: &Vec<Fragment>) -> Result<Message, String> {
        let mut to_content = HashMap::new();

        for frag in packets {
            let help = frag.data[0..frag.length as usize].to_vec();
            to_content.insert(frag.fragment_index, help);
        }
        // We have all fragments, but we first put them in an HashMap to be able to order them.

        let keys_cap = to_content.len() as u64;
        let mut vec_cont = Vec::new();
        for key in 0..keys_cap {
            if let Some(values) = to_content.get(&key) {
                for u8_value in values {
                    vec_cont.push(*u8_value);
                    // We add the fragment to the vec that will be converted to content.
                }
            }
        }
        // We repeat for every fragment of the HashMap (Since we have all of them,
        // we can just use an incremental counter).

        let string_to_cont = match String::from_utf8(vec_cont){
            Ok(v) => v,
            Err(e) => return Err(e.to_string()),
        };

        // Attempt to deserialize into each possible type
        if let Ok(content) = MediaRequest::from_string(string_to_cont.clone()) {
            return Ok(Message {
                source_id,
                session_id,
                content: ContentType::MediaRequest(content),
            });
        }

        if let Ok(content) = MediaResponse::from_string(string_to_cont.clone()) {
            return Ok(Message {
                source_id,
                session_id,
                content: ContentType::MediaResponse(content),
            });
        }

        if let Ok(content) = TextRequest::from_string(string_to_cont.clone()) {
            return Ok(Message {
                source_id,
                session_id,
                content: ContentType::TextRequest(content),
            });
        }

        if let Ok(content) = TextResponse::from_string(string_to_cont.clone()) {
            return Ok(Message {
                source_id,
                session_id,
                content: ContentType::TextResponse(content),
            });
        }

        if let Ok(content) = ChatRequest::from_string(string_to_cont.clone()) {
            return Ok(Message {
                source_id,
                session_id,
                content: ContentType::ChatRequest(content),
            });
        }

        if let Ok(content) = ChatResponse::from_string(string_to_cont.clone()) {
            return Ok(Message {
                source_id,
                session_id,
                content: ContentType::ChatResponse(content),
            });
        }

        if let Ok(content) = TypeExchange::from_string(string_to_cont.clone()) {
            return Ok(Message {
                source_id,
                session_id,
                content: ContentType::TypeExchange(content),
            });
        }
        if let Ok(content) = EdgeNackType::from_string(string_to_cont.clone()) {
            return Ok(Message {
                source_id,
                session_id,
                content: ContentType::EdgeNack(content),
            });
        }

        // If no deserialization succeeds
        let err=  format!("Failed to determine content type for {}", string_to_cont);

        Err(err)
    }

    fn handle_packet(&mut self, packet: Packet);

    fn handle_message(&mut self, message: Message);

    fn edge_send_flood_response(&mut self, flood: FloodRequest) {
        //take a flood req, generate the response, send it

        let flood_resp = FloodResponse {
            flood_id: flood.flood_id,
            path_trace: flood.path_trace.clone(), //I put a copy of path trace done by the flood
        };

        let mut hops = flood
            .path_trace
            .iter()
            .rev()
            .map(|(id, _)| *id)
            .collect::<Vec<NodeId>>(); //I take only the ID's from the path trace and reverse them.
        if flood.path_trace[0].0 != flood.initiator_id {
            hops.push(flood.initiator_id);
        }

        let resp = Packet {
            pack_type: PacketType::FloodResponse(flood_resp),
            routing_header: SourceRoutingHeader { hop_index: 0, hops },
            session_id: flood.flood_id,
        };
        self.handle_packet(resp);
        //self.controller_send.send(DroneEvent::PacketSent(resp)).unwrap(); //Should be set by handle_packet.
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64);

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId);

    fn send_fragment_after_nack(&mut self, packet_session_id: u64, nack: Nack) ;

    fn send_ack(&mut self, packet: Packet, fragment_index: u64);

    fn flood(&mut self);

    fn get_flood_id(&mut self) -> u64;

    fn get_session_id(&mut self) -> u64;

    fn get_src_id(&self) -> NodeId;

    fn remove_sender(&mut self, id: NodeId);
}

pub trait NetworkEdgeErrors: NetworkEdge {
    fn check_type(&mut self, id: NodeId);

    fn is_state_ok(&self, node_id: NodeId) -> bool;

    fn send_nack_message(&mut self, dst: NodeId, nack: Message); //for edges nack

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType, session_id: u64);  //for drone nack

    fn create_nack(&mut self, nack_type: EdgeNackType) -> Message {
        Message{
            source_id: self.get_src_id(),
            session_id: self.get_session_id(),
            content: ContentType::EdgeNack(nack_type),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeType{
    Client(ClientType),
    Server(ServerType),
}