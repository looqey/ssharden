//! Error type and `Result` alias for `ssharden-core`.

use thiserror::Error;

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, CoreError>;

/// All errors surfaced by `ssharden-core`.
#[derive(Debug, Error)]
pub enum CoreError {
    /// An HTTP transport/protocol error talking to `bw serve`.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// An I/O error (process spawn, PTY, filesystem).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A JSON (de)serialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The `bw` CLI / `bw serve` adapter reported a failure.
    #[error("bw error: {0}")]
    Bw(String),

    /// A requested resource (host, session, …) was not found.
    #[error("not found")]
    NotFound,

    /// Failed to spawn a child process (`bw serve`, `ssh`, …).
    #[error("spawn error: {0}")]
    Spawn(String),

    /// An SFTP transport/protocol error (russh / russh-sftp).
    #[error("sftp error: {0}")]
    Sftp(String),
}
