use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("encoding: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("certificate: {0}")]
    Cert(#[from] rcgen::Error),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("frame: {0}")]
    Frame(String),
    #[error("pairing: {0}")]
    Pairing(String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("clipboard: {0}")]
    Clipboard(String),
    #[error("transfer: {0}")]
    Transfer(String),
    #[error("unknown peer: {0}")]
    UnknownPeer(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
