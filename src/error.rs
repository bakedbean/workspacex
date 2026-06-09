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
    #[error("{msg}")]
    Usage {
        group: Option<&'static str>,
        msg: String,
    },
    #[error("cancelled")]
    Cancelled,
}

/// Adapt the `sessionx` parsing crate's error into wsx's, preserving the
/// underlying io/serde cause so `?` works across the boundary.
impl From<sessionx::Error> for Error {
    fn from(e: sessionx::Error) -> Self {
        match e {
            sessionx::Error::Io(io) => Error::Io(io),
            sessionx::Error::Serde(s) => Error::Serde(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_displays_only_msg() {
        let e = Error::Usage {
            group: Some("agent"),
            msg: "missing arguments".into(),
        };
        assert_eq!(e.to_string(), "missing arguments");
    }
}
