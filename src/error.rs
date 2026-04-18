use thiserror::Error;

/// Application-level errors.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] anyhow::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] hyper::Error),

    #[error("Not found")]
    NotFound,

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Upstream error: {0}")]
    Upstream(String),
}

impl AppError {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn upstream(msg: impl Into<String>) -> Self {
        Self::Upstream(msg.into())
    }
}

/// Convert an AppError into an HTTP status code.
impl AppError {
    pub fn status_code(&self) -> hyper::StatusCode {
        match self {
            Self::NotFound => hyper::StatusCode::NOT_FOUND,
            Self::BadRequest(_) => hyper::StatusCode::BAD_REQUEST,
            Self::Upstream(_) => hyper::StatusCode::BAD_GATEWAY,
            _ => hyper::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
