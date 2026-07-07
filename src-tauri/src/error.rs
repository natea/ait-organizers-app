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
    /// The write guardrail rejected a mutation: no token, an unknown/expired/
    /// already-consumed token, or a token whose payload doesn't match the
    /// exact mutation requested (specs/rsvp-screening, design D2).
    #[error("{0}")]
    ConfirmationRequired(String),
    /// A bulk mutation's materialized selection exceeds the per-call ceiling
    /// (design D4) — the caller must split it into confirmed chunks.
    #[error("{0}")]
    CeilingExceeded(String),
    /// Client-side request timeout (generation calls only — specs/promotion-tools D5).
    #[error("{0}")]
    Timeout(String),
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
            "confirmation_required" => AppError::ConfirmationRequired(message),
            "ceiling_exceeded" => AppError::CeilingExceeded(message),
            _ => AppError::Other(format!("{code}: {message}")),
        }
    }

    /// Stable snake_case code for this variant, mirroring the API envelope so
    /// the frontend can branch degraded copy (e.g. group vs scope).
    pub fn code(&self) -> &'static str {
        match self {
            AppError::NoKey => "no_key",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::ForbiddenRole(_) => "forbidden_role",
            AppError::ForbiddenScope(_) => "forbidden_scope",
            AppError::ForbiddenApiGroup(_) => "forbidden_api_group",
            AppError::RateLimited(_) => "rate_limited",
            AppError::NotFound(_) => "not_found",
            AppError::ConfirmationRequired(_) => "confirmation_required",
            AppError::CeilingExceeded(_) => "ceiling_exceeded",
            AppError::Timeout(_) => "timeout",
            AppError::Network(_) => "network",
            AppError::Keychain(_) => "keychain",
            AppError::Db(_) => "db",
            AppError::Other(_) => "other",
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
        if e.is_timeout() {
            AppError::Timeout(e.to_string())
        } else {
            AppError::Network(e.to_string())
        }
    }
}

impl From<keyring::Error> for AppError {
    fn from(e: keyring::Error) -> Self {
        AppError::Keychain(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
