use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("git: {0}")]
    Git(String),
    #[error("store: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("pty: {0}")]
    Pty(String),
    #[error("agent binary not found: {0}")]
    AgentBinaryMissing(String),
    #[error("setup: {0}")]
    Setup(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    UserInput(String),
    #[error("cancelled")]
    Cancelled,
}
