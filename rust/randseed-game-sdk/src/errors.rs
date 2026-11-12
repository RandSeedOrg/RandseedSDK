//! Shared error types for the RandSeed game server SDK.
//!
//! Defines structured errors and conversions from common dependency errors.

use thiserror::Error;

/// Stable cross-language error codes used by the Room Host and Session Stream protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SdkErrorCode {
    /// The request JSON or required fields are invalid.
    InvalidRequest,
    /// The Session Stream message shape or field types are invalid.
    InvalidStreamMessage,
    /// The game manifest is invalid.
    InvalidManifest,
    /// Match options are invalid.
    InvalidOptions,
    /// The game state shape or version is invalid.
    InvalidState,
    /// An action or projection references a player outside the active roster.
    WrongRoster,
    /// An action payload or action identifier is invalid.
    InvalidAction,
    /// A stream tick is not the next contiguous tick.
    TickSequenceMismatch,
    /// Replay ticks or replayed state do not match the checkpoint contract.
    ReplayMismatch,
    /// A game callback failed or panicked.
    CallbackFailed,
    /// A game callback returned an invalid result.
    InvalidResult,
    /// The terminal result or next-match policy is invalid.
    InvalidGameOverPolicy,
    /// A request, frame, state, result, or view exceeded a size limit.
    ResourceLimitExceeded,
    /// The requested feature is not implemented.
    UnsupportedFeature,
    /// The endpoint or resource does not exist.
    NotFound,
    /// An internal or runtime error occurred.
    InternalError,
}

impl SdkErrorCode {
    /// Stable wire representation shared with the Node SDK.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::InvalidStreamMessage => "INVALID_STREAM_MESSAGE",
            Self::InvalidManifest => "INVALID_MANIFEST",
            Self::InvalidOptions => "INVALID_OPTIONS",
            Self::InvalidState => "INVALID_STATE",
            Self::WrongRoster => "WRONG_ROSTER",
            Self::InvalidAction => "INVALID_ACTION",
            Self::TickSequenceMismatch => "TICK_SEQUENCE_MISMATCH",
            Self::ReplayMismatch => "REPLAY_MISMATCH",
            Self::CallbackFailed => "CALLBACK_FAILED",
            Self::InvalidResult => "INVALID_RESULT",
            Self::InvalidGameOverPolicy => "INVALID_GAME_OVER_POLICY",
            Self::ResourceLimitExceeded => "RESOURCE_LIMIT_EXCEEDED",
            Self::UnsupportedFeature => "UNSUPPORTED_FEATURE",
            Self::NotFound => "NOT_FOUND",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    /// Whether retrying without changing the request or game code may succeed.
    pub fn retryable(self) -> bool {
        matches!(self, Self::InternalError)
    }

    /// Creator-facing guidance for correcting this error.
    pub fn creator_action(self) -> &'static str {
        match self {
            Self::InvalidRequest => "Correct the request JSON and required fields before retrying.",
            Self::InvalidStreamMessage => "Correct the Session Stream message shape and field types.",
            Self::InvalidManifest => "Correct the game manifest fields and restart the game server.",
            Self::InvalidOptions => "Correct match options to match the declared options schema.",
            Self::InvalidState => "Correct the game state shape and version before replaying or resuming.",
            Self::WrongRoster => {
                "Use only active roster player IDs and provide exactly one view per active seat."
            }
            Self::InvalidAction => {
                "Correct the action payload and action identifiers to match the declared action schema."
            }
            Self::TickSequenceMismatch => {
                "Send the next contiguous tick after the session's current tick."
            }
            Self::ReplayMismatch => {
                "Restore a checkpoint and replay each committed tick exactly once in order."
            }
            Self::CallbackFailed => {
                "Fix the game callback and redeploy; callback internals are intentionally not exposed."
            }
            Self::InvalidResult => {
                "Correct the game callback result to satisfy its documented schema."
            }
            Self::InvalidGameOverPolicy => {
                "Return a terminal result with a supported next-match policy and non-negative integer timeout."
            }
            Self::ResourceLimitExceeded => {
                "Reduce request, stream frame, state, result, or view size below the documented limit."
            }
            Self::UnsupportedFeature => {
                "Remove the unsupported feature or implement its required callback."
            }
            Self::NotFound => "Correct the endpoint or resource identifier.",
            Self::InternalError => {
                "Retry only after resolving a transient server or runtime failure."
            }
        }
    }
}

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

    /// A stable protocol error with creator-facing guidance.
    #[error("{message}")]
    Protocol {
        /// Stable protocol code.
        code: SdkErrorCode,
        /// Safe creator-facing message.
        message: String,
    },
}

impl SdkError {
    /// Create a protocol error using its default creator-facing guidance.
    pub fn protocol(code: SdkErrorCode) -> Self {
        Self::Protocol {
            code,
            message: code.creator_action().to_string(),
        }
    }

    /// Create a protocol error with safe, context-specific guidance.
    pub fn protocol_with_message(code: SdkErrorCode, message: impl Into<String>) -> Self {
        Self::Protocol {
            code,
            message: message.into(),
        }
    }

    /// Return the stable code associated with this error.
    pub fn code(&self) -> SdkErrorCode {
        match self {
            Self::Protocol { code, .. } => *code,
            Self::Validation(_) => SdkErrorCode::InvalidRequest,
            Self::NotImplemented(_) => SdkErrorCode::UnsupportedFeature,
            Self::Serialization(_)
            | Self::Network(_)
            | Self::PlatformHttp(_, _)
            | Self::Internal(_) => SdkErrorCode::InternalError,
        }
    }

    /// Whether retrying without changing the request or game code may succeed.
    pub fn retryable(&self) -> bool {
        self.code().retryable()
    }

    /// Return safe creator-facing guidance, never raw internal details.
    pub fn creator_message(&self) -> String {
        match self {
            Self::Protocol { message, .. } => message.clone(),
            Self::Validation(message) => message.clone(),
            _ => self.code().creator_action().to_string(),
        }
    }

    /// Return the original callback detail only for explicit validation errors.
    pub fn validation_message(&self) -> Option<&str> {
        match self {
            Self::Protocol { message, .. } | Self::Validation(message) => Some(message),
            _ => None,
        }
    }
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
