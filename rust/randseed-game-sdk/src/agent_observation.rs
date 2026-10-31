//! Player-scoped Agent observations for games that explicitly partition their state.

use crate::errors::SdkError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Metadata supplied by the trusted room for one player's observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentObservationContext {
    /// Current match identifier.
    pub match_id: String,
    /// Monotonically increasing observation sequence.
    pub sequence: u64,
    /// Game tick corresponding to the observation.
    pub tick: u64,
    /// Player selected by the room's authenticated authorization context.
    pub player_id: String,
}

/// The selected player's private view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentObservationSelf {
    /// Player identifier within this game.
    pub player_id: String,
    /// State visible only to this player.
    pub private_state: Value,
}

/// Versioned player-scoped observation.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentObservation {
    /// Observation protocol version, currently 1.
    pub protocol_version: u8,
    /// Current match identifier.
    pub match_id: String,
    /// Observation sequence.
    pub sequence: u64,
    /// Game tick.
    pub tick: u64,
    /// State explicitly classified as visible to all players.
    pub public_state: Value,
    /// The authorized player's own view.
    #[serde(rename = "self")]
    pub player: AgentObservationSelf,
}

/// Build a snapshot from explicitly partitioned game state.
///
/// The caller must authorize the player and correctly classify data before
/// calling this function; the SDK cannot infer a game's visibility rules.
pub fn create_agent_observation(
    context: &AgentObservationContext,
    public_state: &Value,
    private_states: &HashMap<String, Value>,
) -> Result<AgentObservation, SdkError> {
    if context.match_id.trim().is_empty() {
        return Err(SdkError::Validation(
            "matchId must be a non-empty string.".into(),
        ));
    }
    if context.player_id.trim().is_empty() {
        return Err(SdkError::Validation(
            "playerId must be a non-empty string.".into(),
        ));
    }
    if context.sequence > MAX_SAFE_INTEGER || context.tick > MAX_SAFE_INTEGER {
        return Err(SdkError::Validation(
            "sequence and tick must be safe integers.".into(),
        ));
    }
    let private_state = private_states
        .get(&context.player_id)
        .ok_or_else(|| SdkError::Validation("The player's private view is unavailable.".into()))?;
    Ok(AgentObservation {
        protocol_version: 1,
        match_id: context.match_id.clone(),
        sequence: context.sequence,
        tick: context.tick,
        public_state: public_state.clone(),
        player: AgentObservationSelf {
            player_id: context.player_id.clone(),
            private_state: private_state.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context(player_id: &str) -> AgentObservationContext {
        AgentObservationContext {
            match_id: "match-1".into(),
            sequence: 3,
            tick: 2,
            player_id: player_id.into(),
        }
    }

    #[test]
    fn each_player_receives_only_their_own_private_partition() {
        let public_state = json!({"turn": "alice", "positions": {"alice": [1, 2], "bob": [3, 4]}});
        let mut private_states = HashMap::from([
            ("alice".into(), json!({"hand": ["ALICE_SECRET"]})),
            ("bob".into(), json!({"hand": ["BOB_SECRET"]})),
        ]);
        let alice =
            create_agent_observation(&context("alice"), &public_state, &private_states).unwrap();
        let bob =
            create_agent_observation(&context("bob"), &public_state, &private_states).unwrap();
        let alice_json = serde_json::to_value(&alice).unwrap();
        assert_eq!(
            alice_json,
            json!({
                "protocolVersion": 1,
                "matchId": "match-1",
                "sequence": 3,
                "tick": 2,
                "publicState": public_state,
                "self": {"playerId": "alice", "privateState": {"hand": ["ALICE_SECRET"]}}
            })
        );
        assert!(!alice_json.to_string().contains("BOB_SECRET"));
        assert!(!serde_json::to_string(&bob)
            .unwrap()
            .contains("ALICE_SECRET"));
        private_states.insert("alice".into(), json!({"hand": ["CHANGED"]}));
        assert_eq!(
            alice.player.private_state,
            json!({"hand": ["ALICE_SECRET"]})
        );
    }

    #[test]
    fn missing_private_view_and_invalid_metadata_fail_closed() {
        let mut states = HashMap::new();
        assert!(create_agent_observation(&context("alice"), &Value::Null, &states).is_err());
        states.insert("alice".into(), Value::Null);
        assert!(create_agent_observation(&context("alice"), &Value::Null, &states).is_ok());
        let mut invalid = context("alice");
        invalid.match_id = " ".into();
        assert!(create_agent_observation(&invalid, &Value::Null, &states).is_err());
        invalid = context(" ");
        assert!(create_agent_observation(&invalid, &Value::Null, &states).is_err());
        invalid = context("alice");
        invalid.sequence = MAX_SAFE_INTEGER + 1;
        assert!(create_agent_observation(&invalid, &Value::Null, &states).is_err());
        invalid.sequence = 0;
        invalid.tick = MAX_SAFE_INTEGER + 1;
        assert!(create_agent_observation(&invalid, &Value::Null, &states).is_err());
    }
}
