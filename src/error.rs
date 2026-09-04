use thiserror::Error;

#[derive(Debug, Error)]
pub enum MinerError {
    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Store error: {0}")]
    Store(String),

    #[error("All engines failed for query: {0}")]
    NoResults(String),
}

pub type Result<T> = std::result::Result<T, MinerError>;
