//! Versioned, seat-scoped Agent player views and action contracts.

use crate::errors::SdkError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// Maximum serialized Agent view size, matching the isolated Skill observation bound.
pub const MAX_AGENT_PLAYER_VIEW_BYTES: usize = 6_000;
/// Maximum serialized canonical-state projection request accepted from the Room host.
pub const MAX_AGENT_PLAYER_REQUEST_BYTES: usize = 1_048_576;
/// Maximum serialized game action accepted from a Skill decision.
pub const MAX_AGENT_PLAYER_ACTION_BYTES: usize = 2_000;
/// Maximum serialized private memory returned by a Skill decision.
pub const MAX_AGENT_PLAYER_MEMORY_BYTES: usize = 16_000;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub(crate) fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

pub(crate) fn valid_schema_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

pub(crate) fn valid_action_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

/// Agent interaction mode supported by an immutable game release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentPlayerMode {
    /// Observe and record a human-controlled seat without autonomous game actions.
    Follow,
    /// Permit an explicitly authorized Skill to submit actions for the seat.
    Autoplay,
}

/// Server-declared support for the authoritative multiplayer Agent protocol.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlayerContract {
    /// Supported per-seat view protocol version.
    pub player_view_protocol_version: u8,
    /// Supported game action and receipt protocol version.
    pub action_protocol_version: u8,
    /// Immutable identifier for the shape of the player view.
    pub player_view_schema_id: String,
    /// Immutable identifier for the shape of game actions.
    pub action_schema_id: String,
    /// Modes supported by the game.
    pub modes: Vec<AgentPlayerMode>,
    /// Maximum supported decision frequency, capped at 10 Hz.
    pub max_decision_hz: f64,
    /// Whether an Agent may occupy an independent player seat.
    pub independent_seats: bool,
    /// Whether an authorized independent Agent may run without a browser.
    pub background_autoplay: bool,
}

impl AgentPlayerContract {
    /// Validate supported versions, schema IDs, modes, and decision limits.
    pub fn validate(&self) -> Result<(), SdkError> {
        let unique: HashSet<_> = self.modes.iter().collect();
        if self.player_view_protocol_version != 1
            || self.action_protocol_version != 1
            || !valid_schema_id(&self.player_view_schema_id)
            || !valid_schema_id(&self.action_schema_id)
            || self.modes.is_empty()
            || unique.len() != self.modes.len()
            || !self.max_decision_hz.is_finite()
            || self.max_decision_hz <= 0.0
            || self.max_decision_hz > 10.0
            || (self.background_autoplay
                && (!self.modes.contains(&AgentPlayerMode::Autoplay) || !self.independent_seats))
        {
            return Err(SdkError::Validation(
                "Invalid Agent player contract.".into(),
            ));
        }
        Ok(())
    }
}

/// Game and environment scope bound to a player view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlayerViewScope {
    /// Registered game identifier.
    pub game_id: String,
    /// Deployment environment.
    pub environment: String,
    /// Immutable deployed release identifier.
    pub release_version: String,
}

/// Trusted room context used to project one authorized seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlayerViewContext {
    /// Game and environment scope.
    pub scope: AgentPlayerViewScope,
    /// Room identifier.
    pub room_id: String,
    /// Current match identifier.
    pub match_id: String,
    /// Match sequence within the room.
    pub match_seq: u64,
    /// Recipient seat selected by the authenticated Room host.
    pub player_id: String,
    /// Monotonic sequence for this match and seat.
    pub sequence: u64,
    /// Current committed game tick.
    pub tick: u64,
}

/// Internal Room-to-game-server projection request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlayerViewRequest {
    /// Trusted scope, match, and recipient seat metadata.
    pub context: AgentPlayerViewContext,
    /// Current roster used to reject a non-member recipient.
    pub players: Vec<String>,
    /// Canonical game state; never returned directly to a player.
    pub state: Value,
    /// Canonical committed events that the projection must filter.
    pub committed_events: Vec<Value>,
}

impl AgentPlayerViewRequest {
    /// Reject invalid scope, sequence, roster, or oversized host requests.
    pub fn validate(&self) -> Result<(), SdkError> {
        let context = &self.context;
        let players: HashSet<_> = self.players.iter().collect();
        let bytes = serde_json::to_vec(self)
            .map_err(|_| SdkError::Validation("Invalid Agent player view request.".into()))?
            .len();
        if !valid_key(&context.scope.game_id)
            || !valid_key(&context.scope.environment)
            || !valid_key(&context.scope.release_version)
            || !valid_key(&context.room_id)
            || !valid_key(&context.match_id)
            || !valid_key(&context.player_id)
            || context.match_seq == 0
            || context.match_seq > MAX_SAFE_INTEGER
            || context.sequence == 0
            || context.sequence > MAX_SAFE_INTEGER
            || context.tick > MAX_SAFE_INTEGER
            || self.players.is_empty()
            || self.players.len() > 100
            || players.len() != self.players.len()
            || self.players.iter().any(|player| !valid_key(player))
            || !players.contains(&context.player_id)
            || bytes > MAX_AGENT_PLAYER_REQUEST_BYTES
        {
            return Err(SdkError::Validation(
                "Invalid Agent player view context or roster.".into(),
            ));
        }
        Ok(())
    }
}

/// Per-seat actionability metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlayerDecisionState {
    /// Whether this seat may act on the current view.
    pub can_act: bool,
    /// Optional epoch-millisecond action deadline.
    pub deadline_at: Option<u64>,
}

/// Game-authored state and events after recipient-specific filtering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlayerViewProjection {
    /// State visible to all players.
    pub public_state: Value,
    /// Private state visible only to the selected seat.
    pub private_state: Value,
    /// Semantic events visible only to the selected seat.
    pub visible_events: Vec<Value>,
    /// Actionability metadata for this seat.
    pub decision: AgentPlayerDecisionState,
}

/// Private partition in a serialized player view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlayerViewSelf {
    /// Game-scoped player seat ID.
    pub player_id: String,
    /// Private state belonging to this seat only.
    pub private_state: Value,
}

/// Versioned player view sent to a human client or its Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPlayerView {
    /// Player-view protocol version.
    pub protocol_version: u8,
    /// Scope and sequence metadata.
    #[serde(flatten)]
    pub context: AgentPlayerViewContext,
    /// Schema identifier declared by the immutable game artifact.
    pub schema_id: String,
    /// State visible to every player.
    pub public_state: Value,
    /// Private state for the selected seat.
    #[serde(rename = "self")]
    pub player: AgentPlayerViewSelf,
    /// Recipient-filtered semantic events.
    pub visible_events: Vec<Value>,
    /// Actionability metadata for this seat.
    pub decision: AgentPlayerDecisionState,
}

/// Build and bound a recipient-specific view from a trusted host request.
pub fn create_agent_player_view(
    contract: &AgentPlayerContract,
    request: &AgentPlayerViewRequest,
    projection: AgentPlayerViewProjection,
) -> Result<AgentPlayerView, SdkError> {
    contract.validate()?;
    request.validate()?;
    if projection
        .decision
        .deadline_at
        .is_some_and(|deadline| deadline > MAX_SAFE_INTEGER)
    {
        return Err(SdkError::Validation(
            "Invalid Agent player decision deadline.".into(),
        ));
    }
    let view = AgentPlayerView {
        protocol_version: 1,
        context: request.context.clone(),
        schema_id: contract.player_view_schema_id.clone(),
        public_state: projection.public_state,
        player: AgentPlayerViewSelf {
            player_id: request.context.player_id.clone(),
            private_state: projection.private_state,
        },
        visible_events: projection.visible_events,
        decision: projection.decision,
    };
    let bytes = serde_json::to_vec(&view)
        .map_err(|_| SdkError::Validation("Invalid Agent player projection.".into()))?
        .len();
    if bytes > MAX_AGENT_PLAYER_VIEW_BYTES {
        return Err(SdkError::Validation(
            "Agent player view exceeds the 6000-byte limit.".into(),
        ));
    }
    Ok(view)
}

/// Human or Agent identity holding a seat controller lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum AgentController {
    /// Human player controls the seat.
    Human,
    /// The named Agent run controls the seat.
    Agent {
        /// Server-pinned Agent run identifier.
        run_id: String,
    },
}

/// Revocable controller lease for one game seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentControllerLease {
    /// Room identifier.
    pub room_id: String,
    /// Match identifier.
    pub match_id: String,
    /// Controlled seat identifier.
    pub player_id: String,
    /// Current seat controller.
    pub controller: AgentController,
    /// Monotonic fencing epoch.
    pub epoch: u64,
    /// Lease expiry in epoch milliseconds.
    pub expires_at: u64,
}

impl AgentControllerLease {
    /// Validate the lease identity, fencing epoch, and expiry.
    pub fn validate(&self) -> Result<(), SdkError> {
        if !valid_key(&self.room_id)
            || !valid_key(&self.match_id)
            || !valid_key(&self.player_id)
            || matches!(&self.controller, AgentController::Agent { run_id } if !valid_key(run_id))
            || self.epoch == 0
            || self.epoch > MAX_SAFE_INTEGER
            || self.expires_at == 0
            || self.expires_at > MAX_SAFE_INTEGER
        {
            return Err(SdkError::Validation(
                "Invalid Agent controller lease.".into(),
            ));
        }
        Ok(())
    }
}

/// Lifecycle status of an action receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentActionReceiptStatus {
    /// The action was durably admitted but not yet committed by game logic.
    Queued,
    /// Game logic committed this specific action.
    Applied,
    /// Game logic rejected this specific action.
    Rejected,
    /// The action outcome is not yet known and must be reconciled.
    Unresolved,
}

/// Idempotent action request bound to the exact player view and controller lease.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentActionRequest {
    /// Action protocol version.
    pub protocol_version: u8,
    /// Game and environment scope.
    pub scope: AgentPlayerViewScope,
    /// Room identifier.
    pub room_id: String,
    /// Match identifier.
    pub match_id: String,
    /// Match sequence within the room.
    pub match_seq: u64,
    /// Target player seat.
    pub player_id: String,
    /// Stable idempotency key.
    pub action_id: String,
    /// Controller associated with the active lease.
    pub controller: AgentController,
    /// Fencing epoch from the current seat lease.
    pub controller_epoch: u64,
    /// Source player-view sequence.
    pub observation_sequence: u64,
    /// Game tick the action was computed against.
    pub expected_tick: u64,
    /// Immutable action schema identifier.
    pub action_schema_id: String,
    /// Game-defined action payload.
    pub action: Value,
}

impl AgentActionRequest {
    /// Validate action identity and safe-integer metadata.
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.protocol_version != 1
            || !valid_key(&self.scope.game_id)
            || !valid_key(&self.scope.environment)
            || !valid_key(&self.scope.release_version)
            || !valid_key(&self.room_id)
            || !valid_key(&self.match_id)
            || !valid_key(&self.player_id)
            || !valid_key(&self.action_id)
            || matches!(&self.controller, AgentController::Agent { run_id } if !valid_key(run_id))
            || self.match_seq == 0
            || self.match_seq > MAX_SAFE_INTEGER
            || self.controller_epoch == 0
            || self.controller_epoch > MAX_SAFE_INTEGER
            || self.observation_sequence == 0
            || self.observation_sequence > MAX_SAFE_INTEGER
            || self.expected_tick > MAX_SAFE_INTEGER
            || !valid_schema_id(&self.action_schema_id)
            || serde_json::to_vec(&self.action)
                .map_or(true, |bytes| bytes.len() > MAX_AGENT_PLAYER_ACTION_BYTES)
        {
            return Err(SdkError::Validation("Invalid Agent action request.".into()));
        }
        Ok(())
    }
}

/// Authoritative state of an action submitted by a player or Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentActionReceipt {
    /// Action protocol version.
    pub protocol_version: u8,
    /// Stable action idempotency key.
    pub action_id: String,
    /// Room identifier.
    pub room_id: String,
    /// Match identifier.
    pub match_id: String,
    /// Player seat.
    pub player_id: String,
    /// Fencing epoch that submitted the action.
    pub controller_epoch: u64,
    /// Tick against which the action was submitted.
    pub expected_tick: u64,
    /// Current terminal or pending action status.
    pub status: AgentActionReceiptStatus,
    /// Tick at which an applied or rejected result was committed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed_tick: Option<u64>,
    /// Sanitized machine-readable rejection or unresolved code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl AgentActionReceipt {
    /// Validate receipt identity and its status-specific committed tick.
    pub fn validate(&self) -> Result<(), SdkError> {
        let terminal_tick_valid = match self.status {
            AgentActionReceiptStatus::Queued | AgentActionReceiptStatus::Unresolved => self
                .committed_tick
                .is_none_or(|tick| tick <= MAX_SAFE_INTEGER),
            AgentActionReceiptStatus::Applied | AgentActionReceiptStatus::Rejected => self
                .committed_tick
                .is_some_and(|tick| tick <= MAX_SAFE_INTEGER),
        };
        if self.protocol_version != 1
            || !valid_key(&self.action_id)
            || !valid_key(&self.room_id)
            || !valid_key(&self.match_id)
            || !valid_key(&self.player_id)
            || self.controller_epoch == 0
            || self.controller_epoch > MAX_SAFE_INTEGER
            || self.expected_tick > MAX_SAFE_INTEGER
            || !terminal_tick_valid
            || self
                .code
                .as_ref()
                .is_some_and(|code| !valid_action_code(code))
        {
            return Err(SdkError::Validation("Invalid Agent action receipt.".into()));
        }
        Ok(())
    }
}

/// Complete result of one Skill invocation, including a bounded wait outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentSkillDecision {
    /// Submit a game action and retain the returned private memory.
    Act {
        /// Game-defined action payload.
        action: Value,
        /// Updated run-local private memory.
        memory: Value,
    },
    /// Submit no game action and resume at the requested bounded deadline.
    Wait {
        /// Optional epoch-millisecond time to resume.
        resume_at: Option<u64>,
        /// Updated run-local private memory.
        memory: Value,
    },
}

impl AgentSkillDecision {
    /// Validate action, wait deadline, and private memory size limits.
    pub fn validate(&self) -> Result<(), SdkError> {
        let (action_bytes, resume_at, memory) = match self {
            Self::Act { action, memory } => (
                Some(
                    serde_json::to_vec(action)
                        .map_err(|_| SdkError::Validation("Invalid Agent action.".into()))?
                        .len(),
                ),
                None,
                memory,
            ),
            Self::Wait { resume_at, memory } => (None, *resume_at, memory),
        };
        let memory_bytes = serde_json::to_vec(memory)
            .map_err(|_| SdkError::Validation("Invalid Agent memory.".into()))?
            .len();
        if action_bytes.is_some_and(|bytes| bytes > MAX_AGENT_PLAYER_ACTION_BYTES)
            || resume_at.is_some_and(|deadline| deadline > MAX_SAFE_INTEGER)
            || memory_bytes > MAX_AGENT_PLAYER_MEMORY_BYTES
        {
            return Err(SdkError::Validation(
                "Agent decision exceeds the supported limits.".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/agent-player-v1.json")).unwrap()
    }

    #[test]
    fn serializes_and_projects_the_shared_player_view_fixture() {
        let fixture = fixture();
        let contract: AgentPlayerContract =
            serde_json::from_value(fixture["contract"].clone()).unwrap();
        let request: AgentPlayerViewRequest =
            serde_json::from_value(fixture["request"].clone()).unwrap();
        let projection: AgentPlayerViewProjection =
            serde_json::from_value(fixture["projection"].clone()).unwrap();
        let view = create_agent_player_view(&contract, &request, projection).unwrap();
        let value = serde_json::to_value(view).unwrap();
        assert_eq!(value, fixture["view"]);
        assert!(!value.to_string().contains("B_SECRET"));
        assert!(!value.to_string().contains("randomSeed"));
    }

    #[test]
    fn validates_action_leases_receipts_and_explicit_wait_decisions() {
        let fixture = fixture();
        let lease: AgentControllerLease = serde_json::from_value(fixture["lease"].clone()).unwrap();
        let action: AgentActionRequest =
            serde_json::from_value(fixture["actionRequest"].clone()).unwrap();
        let queued: AgentActionReceipt =
            serde_json::from_value(fixture["queuedReceipt"].clone()).unwrap();
        let applied: AgentActionReceipt =
            serde_json::from_value(fixture["appliedReceipt"].clone()).unwrap();
        let rejected: AgentActionReceipt =
            serde_json::from_value(fixture["rejectedReceipt"].clone()).unwrap();
        let wait: AgentSkillDecision =
            serde_json::from_value(fixture["waitDecision"].clone()).unwrap();

        assert!(lease.validate().is_ok());
        assert!(action.validate().is_ok());
        assert!(queued.validate().is_ok());
        assert!(applied.validate().is_ok());
        assert!(rejected.validate().is_ok());
        assert!(wait.validate().is_ok());

        let mut invalid = action;
        invalid.controller_epoch = 0;
        assert!(invalid.validate().is_err());
        let mut invalid = applied;
        invalid.committed_tick = None;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn rejects_missing_recipient_private_state_and_oversized_views() {
        let fixture = fixture();
        let contract: AgentPlayerContract =
            serde_json::from_value(fixture["contract"].clone()).unwrap();
        let mut request: AgentPlayerViewRequest =
            serde_json::from_value(fixture["request"].clone()).unwrap();
        let mut projection: AgentPlayerViewProjection =
            serde_json::from_value(fixture["projection"].clone()).unwrap();
        request.players = vec!["seat-b".into()];
        assert!(request.validate().is_err());

        request = serde_json::from_value(fixture["request"].clone()).unwrap();
        projection.public_state = Value::String("x".repeat(MAX_AGENT_PLAYER_VIEW_BYTES));
        assert!(create_agent_player_view(&contract, &request, projection).is_err());
    }

    #[test]
    fn request_identifiers_enforce_utf8_bytes_and_all_control_characters() {
        let fixture = fixture();
        let mut request: AgentPlayerViewRequest =
            serde_json::from_value(fixture["request"].clone()).unwrap();
        request.context.room_id = "界".repeat(86);
        assert!(request.validate().is_err());

        request = serde_json::from_value(fixture["request"].clone()).unwrap();
        request.context.room_id = "room-\u{0085}".into();
        assert!(request.validate().is_err());
    }
}
