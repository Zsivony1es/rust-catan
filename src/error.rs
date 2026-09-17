//! Typed game errors shared by the engine (`lib`) and the HTTP layer.
//!
//! The engine returns [`GameError`]; the server maps each variant to an HTTP
//! status code plus a `{ "error": { "code", "message" } }` JSON body.

use serde::{Deserialize, Serialize};

/// Machine-readable error code, also used as the JSON `code` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    BadRequest,
    Forbidden,
    Conflict,
}

/// All validation / state errors produced while mutating a [`crate::GameSession`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GameError {
    /// Unknown game id, player color, hex, intersection, edge, trade, ...
    #[error("not found: {0}")]
    NotFound(String),
    /// Malformed request or illegal move (wrong phase, placement rule, cost, ...).
    #[error("bad request: {0}")]
    BadRequest(String),
    /// Valid token but not allowed to perform this action (wrong player/token).
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// Action conflicts with current state (duplicate join, game full, ...).
    #[error("conflict: {0}")]
    Conflict(String),
}

impl GameError {
    /// Machine-readable code for JSON responses.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::BadRequest(_) => ErrorCode::BadRequest,
            Self::Forbidden(_) => ErrorCode::Forbidden,
            Self::Conflict(_) => ErrorCode::Conflict,
        }
    }

    /// HTTP status code for the Axum response.
    #[must_use]
    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::BadRequest(_) => 400,
            Self::Forbidden(_) => 403,
            Self::Conflict(_) => 409,
        }
    }

    #[must_use]
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    #[must_use]
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    #[must_use]
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }

    #[must_use]
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }
}
