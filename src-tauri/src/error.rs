use serde::Serialize;

/// Typed errors surfaced to the frontend. `code` mirrors the AI Tinkerers
/// Agents API error envelope codes so the UI can degrade per-feature
/// (see specs/api-auth, specs/background-sync).
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum AppError {
    #[error("no API key stored")]
    NoKey,
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    ForbiddenRole(String),
    #[error("{0}")]
    ForbiddenScope(String),
    #[error("{0}")]
    ForbiddenApiGroup(String),
    #[error("{0}")]
    RateLimited(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Network(String),
    #[error("{0}")]
    Keychain(String),
    #[error("{0}")]
    Db(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// Map an API envelope `error.code` string to a typed variant.
    pub fn from_api_code(code: &str, message: String) -> Self {
        match code {
            "forbidden_role" => AppError::ForbiddenRole(message),
            "forbidden_scope" => AppError::ForbiddenScope(message),
            "forbidden_api_group" => AppError::ForbiddenApiGroup(message),
            "rate_limited" => AppError::RateLimited(message),
            "not_found" => AppError::NotFound(message),
            "unauthorized" | "invalid_api_key" => AppError::Unauthorized(message),
            _ => AppError::Other(format!("{code}: {message}")),
        }
    }

    /// True when the feature behind this endpoint is unavailable for this
    /// caller and further polling should stop (background-sync degradation).
    pub fn is_capability_block(&self) -> bool {
        matches!(
            self,
            AppError::ForbiddenApiGroup(_) | AppError::ForbiddenScope(_) | AppError::ForbiddenRole(_)
        )
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Network(e.to_string())
    }
}

impl From<keyring::Error> for AppError {
    fn from(e: keyring::Error) -> Self {
        AppError::Keychain(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
