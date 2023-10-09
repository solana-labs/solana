use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Sending channel disconnected: {0}")]
    DisconnectedSendChannel(&'static str),
}
