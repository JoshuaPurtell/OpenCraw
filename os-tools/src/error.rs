use thiserror::Error;

pub type Result<T> = std::result::Result<T, ToolError>;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    #[error("io error: {0}")]
    Io(String),
}

impl From<std::io::Error> for ToolError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
