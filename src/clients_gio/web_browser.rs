use crate::clients_gio::client_command::{ClientCommand, ClientEvent};
use crate::clients_gio::client_struct::ClientStruct;
use crate::clients_gio::client_trait::ClientTrait;
use crate::clients_gio::client_type::ClientType;
use crate::message::MediaRequest::MediaList;
use crate::message::TextRequest::TextList;
use crate::message::{ContentType, Message};
use crate::network_edge::{NetworkEdge, NetworkEdgeErrors};
use crate::DEBUG_MODE;
use crossbeam_channel::{select_biased, Receiver, Sender};
use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;
use wg_2024::network::NodeId;
use wg_2024::packet::{Fragment, Nack, NackType, Packet};

pub struct WebBrowser{
    comm: ClientStruct, //common client duh

    //web browser specks
    arrived_content: HashMap<NodeId, Vec<Vec<u8>>>,
    catalogue: HashMap<NodeId, (u8, Vec<String>)>, //the u8 represent if it's a text server(1) or a media server(2)
    /*
    this is necessary because path will only distinguish if it's a good server for us, not the type,
    hence when we get the typexchange we also create the catalogue to remember if that server is a text or media
    */
}

impl NetworkEdge for WebBrowser {
    fn send_message(&mut self, message: Message, destination: NodeId) {
        self.comm.send_message(message, destination)
    }

    fn handle_packet(&mut self, _packet: Packet) {
        unimplemented!()
    }

    fn handle_message(&mut self,_message: Message ) {
        unimplemented!()
    }

    fn send_fragment(&mut self, fragment: Fragment, destination: NodeId, session_id: u64) {
        self.comm.send_fragment(fragment, destination, session_id)
    }

    fn add_unsent_fragment(&mut self, fragment: Fragment, session_id: u64, destination: NodeId) {
        self.comm.add_unsent_fragment(fragment, session_id, destination);
    }

    fn send_fragment_after_nack(&mut self, packet: Packet, nack: Nack) {
        self.comm.send_fragment_after_nack(packet, nack)
    }

    fn send_ack(&mut self, packet: Packet, fragment_index: u64) {
        self.comm.send_ack(packet, fragment_index);
    }

    fn flood(&mut self) {
        self.comm.flood();
    }

    fn get_flood_id(&mut self) -> u64 {
        self.comm.get_flood_id()
    }

    fn get_session_id(&mut self) -> u64 {
        self.comm.get_session_id()
    }

    fn get_src_id(&self) -> NodeId {
        self.comm.get_src_id()
    }
}

impl NetworkEdgeErrors for WebBrowser {
    fn check_type(&mut self, id: NodeId) {
        self.comm.check_type(id);
    }

    fn is_state_ok(&self, node_id: NodeId) -> bool {
        self.comm.is_state_ok(node_id)
    }

    fn send_nack_message(&mut self, dst: NodeId, nack: Message) {
        self.comm.send_nack_message(dst, nack);
    }

    fn send_drone_nack(&mut self, dst: NodeId, nack: NackType) {
        self.comm.send_drone_nack(dst, nack);
    }
}

impl ClientTrait for WebBrowser {
    fn new(
        node_id: NodeId,
        command_recv: Receiver<ClientCommand>,
        event_send: Sender<ClientEvent>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
    ) -> Self {
        WebBrowser {
            comm: ClientStruct::new(node_id, command_recv, event_send, packet_recv, packet_send),
            arrived_content: Default::default(),
            catalogue: Default::default(),
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.comm.command_recv) -> cmd => {
                    if let Ok(command) = cmd {
                        self.handle_command(command);
                    }
                }
                recv(self.comm.packet_recv) -> pkt => {
                    if let Ok(packet) = pkt {
                        self.handle_packet(packet);
                    }
                }
                default => {
                     sleep(Duration::from_millis(10));
                    // Wait a second before going on.
                }
            }
            // I check a counter, so that I don't try to send all the fragments every loop.
            if self.comm.unsent_fragments.0 >= 150 {
                //if I have some unchecked nodes I try to check them

                self.comm.paths.clone().iter().for_each(|(dst, (state, path))| {
                    if *state == 0{
                        self.check_type(dst.clone());
                    }
                });

                // I create a temporary copy of the fragments that needs to be processed.
                let mut to_process = Vec::new();
                for (identifier, content) in self.comm.unsent_fragments.1.iter() {
                    for fragment in content.iter() {
                        to_process.push((fragment.clone(), identifier.clone()));
                    }
                }
                // I then empty the HashMap to not have any duplicate.as
                self.comm.unsent_fragments.1 = HashMap::new();
                self.comm.unsent_fragments.0 = 0; for (fragment, identifier) in to_process {
                    self.send_fragment(fragment.clone(), identifier.2, identifier.0);
                }

                //uncomment to check flood periodically

                // let mut path_printable = String::new();
                // self.paths.clone().iter_mut().for_each(|(dst, (state, path))| {
                //     let destination = format!("Node {}, State: {}, path: *not now* \n", dst, state);
                //     path_printable.push_str(destination.as_str());
                // });
                // println!("{} has paths: {:?}",self.node_id, path_printable);



            } else {
                self.comm.unsent_fragments.0 += 1;
            }
        }
    }

    fn handle_command(&mut self, command: ClientCommand) {
        match command {
            ClientCommand::RemoveSender(node_id) => {
                if self.comm.packet_send.contains_key(&node_id) {
                    if let Some(to_be_dropped) = self.comm.packet_send.remove(&node_id) {
                        drop(to_be_dropped);
                        //println!("Client {} no more has a connection to {}!", self.node_id, node_id);
                    }
                }
            }
            ClientCommand::AddSender(node_id, sender) => {
                self.comm.packet_send.insert(node_id, sender);
            }

            ClientCommand::Flood =>{
                self.flood();
            }
            ClientCommand::RetrieveList(id) => {
                self.get_list(id);
            }

            //commands for WebClient
            ClientCommand::GetContent(id) => {
                self.get_content(id);
            }


            //ignore other commands cause are chat clients commands
            _ =>{

            }
        }
    }


    fn get_client_type(&self) -> ClientType {
        ClientType::WebBrowser
    }

    fn send_event(&self, ce: ClientEvent) {
        self.comm.send_event(ce);
    }
}

impl WebBrowser{
    fn get_list(&mut self, id: NodeId) {
        let src = self.comm.get_src_id();
        let session = self.comm.get_session_id();

        if let Some((state, _catalogue)) = self.catalogue.get(&id) {
            let content = match *state {
                1 => ContentType::TextRequest(TextList),
                2 => ContentType::MediaRequest(MediaList),
                _ => unreachable!("Invalid state in catalogue."),
            };
            let msg = Message::new(src, session, content);
            self.comm.send_message(msg, id);

            if DEBUG_MODE {
                println!("Sent content list request from {src} to server {id}");
            }
        } else {
            // Handle the case where the catalogue entry is not found in flood
            if DEBUG_MODE {
                println!("Catalogue entry for {id} not found.");
            }
            // Add event?
        }
    }
    fn get_content(&mut self, _cont_id: String) {
        //todo

        //come distinguiamo se stiamo cercando un client o un server dall'id diobest
    }


}
