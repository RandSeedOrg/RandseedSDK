//! Explicit, versioned Agent capabilities declared by a game.

use crate::errors::SdkError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// An Agent control mode supported by a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentControlMode {
    /// Advisory assistance.
    Copilot,
    /// Agent-controlled play.
    Autoplay,
    /// Capture of the owner's play.
    Follow,
}

/// Agent support explicitly declared by a game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentGameCapabilities {
    /// Observation protocol version supported by this game.
    pub observation_protocol_version: u8,
    /// Action protocol version supported by this game.
    pub action_protocol_version: u8,
    /// Supported modes.
    pub modes: Vec<AgentControlMode>,
    /// Maximum supported Agent decision frequency in hertz.
    pub max_decision_hz: f64,
    /// Whether this game permits authorized background Autoplay.
    pub background_autoplay: bool,
}

impl AgentGameCapabilities {
    /// Validate a declaration before advertising it.
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.observation_protocol_version != 1 || self.action_protocol_version != 1 {
            return Err(SdkError::Validation(
                "Unsupported Agent protocol version.".into(),
            ));
        }

        let unique: HashSet<_> = self.modes.iter().collect();
        if self.modes.is_empty() || unique.len() != self.modes.len() {
            return Err(SdkError::Validation(
                "Agent modes must be non-empty and unique.".into(),
            ));
        }

        if !self.max_decision_hz.is_finite() || self.max_decision_hz <= 0.0 {
            return Err(SdkError::Validation(
                "maxDecisionHz must be a positive finite number.".into(),
            ));
        }
        if self.background_autoplay && !self.modes.contains(&AgentControlMode::Autoplay) {
            return Err(SdkError::Validation(
                "Background Autoplay requires Autoplay support.".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> AgentGameCapabilities {
        AgentGameCapabilities {
            observation_protocol_version: 1,
            action_protocol_version: 1,
            modes: vec![AgentControlMode::Autoplay, AgentControlMode::Follow],
            max_decision_hz: 10.0,
            background_autoplay: false,
        }
    }

    #[test]
    fn rejects_invalid_capabilities() {
        let mut value = valid();
        assert!(value.validate().is_ok());
        value.observation_protocol_version = 2;
        assert!(value.validate().is_err());
        value = valid();
        value.action_protocol_version = 2;
        assert!(value.validate().is_err());
        value = valid();
        value.modes.clear();
        assert!(value.validate().is_err());
        value.modes = vec![AgentControlMode::Follow, AgentControlMode::Follow];
        assert!(value.validate().is_err());
        value.modes = vec![AgentControlMode::Follow];
        value.background_autoplay = true;
        assert!(value.validate().is_err());
        value = valid();
        value.max_decision_hz = f64::INFINITY;
        assert!(value.validate().is_err());
        value.max_decision_hz = 0.0;
        assert!(value.validate().is_err());
    }

    #[test]
    fn serializes_to_shared_wire_format() {
        assert_eq!(
            serde_json::to_value(valid()).unwrap(),
            serde_json::json!({
                "observationProtocolVersion": 1, "actionProtocolVersion": 1,
                "modes": ["autoplay", "follow"], "maxDecisionHz": 10.0,
                "backgroundAutoplay": false
            })
        );
    }
}
