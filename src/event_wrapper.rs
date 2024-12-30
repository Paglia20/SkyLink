use wg_2024::controller::DroneEvent;
use crate::clients_gio::client_command::ClientEvent;
use crate::server::server_command::ServerEvent;


pub enum Event {
    Drone(DroneEvent),
    Server(ServerEvent),
    Client(ClientEvent),
}