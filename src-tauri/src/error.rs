use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "name")]
pub enum AppError {
    #[error("database error: {message}")]
    Db { message: String },

    #[error("lock poisoned")]
    Lock { message: String },

    #[error("not found: {message}")]
    NotFound { message: String },

    #[error("validation: {message}")]
    Validation { message: String },
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db { message: e.to_string() }
    }
}
