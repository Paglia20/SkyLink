use std::collections::HashMap;
use crossbeam_channel::TrySendError;
use wg_2024::network::NodeId;
use wg_2024::packet::*;
use crate::message::{Message, MessageType, Request, Response};

pub trait NetworkEdge {
    type RequestType: Request;
    type ResponseType: Response;

    fn compose_message<M:MessageType>(
        source_id: NodeId,
        session_id: u64,
        raw_content: String,
    ) -> Result<Message<M>, String> {
        let content = M::from_string(raw_content)?;
        Ok(Message {
            session_id,
            source_id,
            content,
        })
    }

    fn send_message<M:MessageType>(&mut self, message: Message<M>, destination: NodeId) -> Result<(), String>;

    fn fragment_message<M:MessageType> (message: &Message<M>) -> Vec<Fragment>{
        let all_bytes = message.content.stringify().into_bytes();
        let total_n_fragments = (all_bytes.len() as u64).div_ceil(128);
        let mut out = Vec::new();

        let mut padded_chunk = [0u8; 128];
        for (frag_id,chunk) in all_bytes.chunks(128).enumerate() {
            let len = chunk.len();

            // i have to pad it i thinl
            padded_chunk[..len].copy_from_slice(chunk);

            let fragment = Fragment::new(
                frag_id as u64,
                total_n_fragments,
                padded_chunk,
            );

            out.push(fragment);
        }
        out
    }

    //questa potrei averla fuckappata, ho fatto l'assumption che questi packet siano di tipo msgFragment, todo check che ci siano tutti i fragment
    fn reassemble_message<M:MessageType>(packets: Vec<Packet>) -> Result<Message<M>, String> {
        let source_id = packets[0].routing_header.hops[0];
        let session_id = packets[0].session_id;
        let mut to_content = HashMap::new();

        for packet in packets {
            match packet.pack_type{
                PacketType::MsgFragment(frag) =>{
                    let help = frag.data[0..frag.length as usize].to_vec();
                    to_content.insert(frag.fragment_index, help);
                }
                _ => unreachable!() //{return Err("ziopera???".to_string());}
            }
        }

        let mut keys: Vec<u64> = to_content.keys().cloned().collect();
        keys.sort();
        let mut string_to_cont = String::new();
        for key in keys {
            if let Some(values) = to_content.get(&key) {
                for u8 in values {
                    string_to_cont.push_str(&u8.to_string());
                }
            }
        }
        //there is probably a way far more efficient

        let content = match M::from_string(string_to_cont){
            Ok(content) => content,
            Err(e) => {return Err(e);}
        };

        Ok(Message{
            source_id,
            session_id,
            content,
        })
    }
}