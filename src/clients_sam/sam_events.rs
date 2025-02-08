
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    WaitingForType,
    Ready,
    Failed
}

