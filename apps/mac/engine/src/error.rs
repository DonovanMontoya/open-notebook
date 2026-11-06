use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Stopped")]
    Cancelled,
    #[error("{0} not found")]
    NotFound(String),
    /// A provider answered with an error. 401 and 403 read "API key rejected: …".
    #[error("{provider}: {message}")]
    Provider { provider: String, status: u16, message: String },
    /// The Open Notebook server answered with an error.
    #[error("Server: {message}")]
    Server { status: u16, message: String },
    /// Something the user has to set up first, such as a model or an API key.
    #[error("{0}")]
    Setup(String),
    #[error("Network: {0}")]
    Network(String),
    #[error("Database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(message: impl Into<String>) -> Self {
        Error::Other(message.into())
    }
}
