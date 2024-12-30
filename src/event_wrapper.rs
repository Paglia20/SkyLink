use std::fmt::Display;
use wg_2024::controller::DroneEvent;
use crate::clients_gio::client_command::ClientEvent;
use crate::server::server_command::ServerEvent;

#[derive(Clone)]
pub enum Event {
    Drone(DroneEvent),
    Server(ServerEvent),
    Client(ClientEvent),
}

impl Display for Event{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self{
            Event::Drone(drone_event) => write!(f, "drone event: {:?}", drone_event),
            Event::Server(server_event) => write!(f, "server event: {:?}", server_event),
            Event::Client(client_event) => write!(f, "client event: {:?}", client_event),
        }
    }
}