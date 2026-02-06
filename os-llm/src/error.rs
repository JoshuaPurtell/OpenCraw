use thiserror::Error;

pub type Result<T> = std::result::Result<T, LlmError>;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("http error: {0}")]
    Http(String),

    #[error("unexpected response format: {0}")]
    ResponseFormat(String),

    #[error("stream parse error: {0}")]
    StreamParse(String),
}

impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.to_string())
    }
}

impl From<serde_json::Error> for LlmError {
    fn from(e: serde_json::Error) -> Self {
        Self::ResponseFormat(e.to_string())
    }
}
