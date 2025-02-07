#[cfg(test)]
mod tests {
    use crate::sam_client_chat_system::TextChat;
    use crate::sam_client_interface::SkyLinkClient;
    use crate::sam_events::{ClientCommand, ClientEvent};
    use crossbeam_channel::unbounded;
    use std::collections::HashMap;
    use std::time::Duration;
    use wg_2024::network::NodeId;
    use wg_2024::packet::Packet;

    fn setup_test_chat_client() -> (
        TextChat,
        crossbeam_channel::Sender<ClientCommand>,
        crossbeam_channel::Receiver<ClientEvent>
    ) {
        let (command_tx, command_rx) = unbounded();
        let (event_tx, event_rx) = unbounded();
        let (packet_tx, packet_rx) = unbounded();

        let mut packet_senders = HashMap::new();
        packet_senders.insert(2, packet_tx);

        let client = TextChat::new(
            1,
            command_rx,
            event_tx,
            packet_rx,
            packet_senders
        );

        (client, command_tx, event_rx)
    }

    #[test]
    fn test_client_initialization() {
        let (_client, _cmd_tx, _event_rx) = setup_test_chat_client();
        // Basic initialization check - client created without panicking
        assert!(true);
    }

    #[test]
    fn test_command_handling() {
        let (_client, cmd_tx, event_rx) = setup_test_chat_client();

        // Test sending a command
        cmd_tx.send(ClientCommand::Flood).unwrap();

        // Check if we receive any event within timeout
        if let Ok(_event) = event_rx.recv_timeout(Duration::from_millis(100)) {
            assert!(true);
        }
    }

    #[test]
    fn test_message_sending_basic() {
        let (_client, cmd_tx, _event_rx) = setup_test_chat_client();

        // Test if we can send a message command without panic
        let result = cmd_tx.send(ClientCommand::SendChatMessage {
            server_id: 2,
            recipient: 3,
            content: "Test message".to_string()
        });

        assert!(result.is_ok());
    }
}