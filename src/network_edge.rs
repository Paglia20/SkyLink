use crate::message::{Message, MessageType, Request, Response};
use std::collections::HashMap;
use wg_2024::network::NodeId;
use wg_2024::packet::*;

pub trait NetworkEdge {
    type RequestType: Request;
    type ResponseType: Response;

    fn send_message<M: MessageType>(
        &mut self,
        message: Message<M>,
        destination: NodeId,
    ) -> Result<(), String>;

    fn fragment_message<M: MessageType>(message: &Message<M>) -> Vec<Fragment> {
        let all_bytes = message.content.stringify().into_bytes();
        let total_n_fragments = (all_bytes.len() as u64).div_ceil(128);
        // I divide rounding up with div_ceil

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
    fn reassemble_message<M: MessageType>(packets: Vec<Packet>) -> Result<Message<M>, String> {
        let source_id = packets[0].routing_header.hops[0];
        let session_id = packets[0].session_id;
        let mut to_content = HashMap::new();

        for packet in packets {
            match packet.pack_type {
                PacketType::MsgFragment(frag) => {
                    let help = frag.data[0..frag.length as usize].to_vec();
                    to_content.insert(frag.fragment_index, help);
                }
                _ => {
                    return Err("Error: Wrong packet type".to_string());
                }
            }
        }
        // We have all fragments, but we first put them in an HashMap to be able to order them.

        let keys_cap = to_content.len() as u64;
        let mut string_to_cont = String::new();
        for key in 0..keys_cap {
            if let Some(values) = to_content.get(&key) {
                for u8_value in values {
                    string_to_cont.push_str(&u8_value.to_string());
                    // We add the fragment to the string that will be converted to content.
                }
            }
        }
        // We repeat for every fragment of the HashMap (Since we have all of them,
        // we can just use an incremental counter).

        let content = match M::from_string(string_to_cont) {
            Ok(content) => content,
            Err(e) => {
                return Err(e);
            }
        };

        Ok(Message {
            source_id,
            session_id,
            content,
        })
    }
}
