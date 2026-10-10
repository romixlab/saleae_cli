//! The error type every fallible call in this crate returns.

use std::path::Path;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Transport(#[from] tonic::transport::Error),
    /// A gRPC call failed; `message` is the server's, which usually says exactly what is wrong.
    #[error("server: {message} ({code:?})")]
    Rpc { code: tonic::Code, message: String },
    /// A path, id or device the caller named does not exist (or is not available right now).
    #[error("{0}")]
    NotFound(String),
    /// Bad input: a setting, channel, trigger or other value that does not make sense.
    #[error("{0}")]
    InvalidInput(String),
    /// The server (or a subprocess the crate ran, like `curl` or `kill`) did not do what was asked.
    #[error("{0}")]
    Server(String),
}

impl Error {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Error::NotFound(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Error::InvalidInput(msg.into())
    }

    pub fn server(msg: impl Into<String>) -> Self {
        Error::Server(msg.into())
    }
}

impl From<tonic::Status> for Error {
    fn from(s: tonic::Status) -> Self {
        Error::Rpc {
            code: s.code(),
            message: s.message().to_string(),
        }
    }
}

/// Shorthand for `Error::Io` with a path in the message (`std::io::Error` alone drops it).
pub(crate) fn io_at(e: std::io::Error, path: &Path) -> Error {
    Error::Io(std::io::Error::new(
        e.kind(),
        format!("{}: {e}", path.display()),
    ))
}
