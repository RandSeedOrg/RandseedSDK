//! Shared error types for the RandSeed game server SDK.
//!
//! Defines structured errors and conversions from common dependency errors.

use thiserror::Error;

/// Errors that can occur while running or communicating through the SDK.
#[derive(Debug, Error)]
pub enum SdkError {
    /// JSON serialization or deserialization failed.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Network transport or low-level I/O failed.
    #[error("Network error: {0}")]
    Network(String),

    /// A protocol contract or input value is invalid.
    #[error("Validation error: {0}")]
    Validation(String),

    /// An optional feature or method is not implemented.
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    /// A platform request returned a non-success HTTP response.
    #[error("Platform request failed (HTTP {0}): {1}")]
    PlatformHttp(u16, String),

    /// An unexpected internal or runtime error occurred.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<serde_json::Error> for SdkError {
    fn from(err: serde_json::Error) -> Self {
        SdkError::Serialization(err.to_string())
    }
}

impl From<reqwest::Error> for SdkError {
    fn from(err: reqwest::Error) -> Self {
        SdkError::Network(err.to_string())
    }
}
