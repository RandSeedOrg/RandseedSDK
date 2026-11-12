//! Persistent WebSocket session stream (`Session Stream v1`) and zero-`Value`
//! strongly typed server traits for the RandSeed room-host protocol.

use crate::agent_capabilities::AgentGameCapabilities;
use crate::agent_player_view::{
    AgentPlayerContract, AgentPlayerViewContext, AgentPlayerViewProjection, AgentPlayerViewRequest,
};
use crate::errors::{SdkError, SdkErrorCode};
use crate::server::GameServer;
use crate::types::{
    validate_game_result_units, validate_player_views, CommandResult, GameActionResult,
    GameCommand, GameManifest, GameResult, GameResultEvent, InitResult, IsGameOverRequest,
    MatchConfig, PlayerEventRequest, PlayerEventResult, PlayerViewMode, TickContext, TickInput,
    TickResult,
};
use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{FutureExt, StreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

/// Protocol identifier advertised in `/manifest` for `Session Stream v1`.
pub const STREAM_PROTOCOL_V1: &str = "session-v1";

const MAX_WS_FRAME_BYTES: usize = 1024 * 1024;

/// Durability policy declared by a game server for stream sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamDurability {
    /// Persist checkpoints periodically (for example every 5s) and immediately
    /// on player roster events, `resultUnits`, or `gameOver`, while keeping
    /// intermediate step inputs in host memory for instant container recovery.
    Checkpoint,
    /// Persist every step's authoritative input batch in the durable journal
    /// before/alongside execution for turn-based or audit-critical games.
    Strict,
}

/// Optional server protocol capabilities advertised in `GET /manifest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestCapabilities {
    /// Supported container WebSocket session stream version (`"session-v1"`).
    pub stream: String,
}

impl Default for ManifestCapabilities {
    fn default() -> Self {
        Self {
            stream: STREAM_PROTOCOL_V1.to_string(),
        }
    }
}

/// Wire envelope returned by `GET /manifest` with auto-injected stream capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestWireResponse {
    /// Core game manifest fields.
    #[serde(flatten)]
    pub manifest: GameManifest,
    /// Protocol capabilities supported by this SDK runtime.
    pub capabilities: ManifestCapabilities,
    /// Optional durability hint (`"checkpoint"` or `"strict"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durability: Option<StreamDurability>,
}

/// Authoritative room lifecycle event bundled with a stream step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum StreamStepEvent {
    /// Player admitted or reconnected to the room.
    Join {
        /// Joining player identifier.
        #[serde(rename = "playerId")]
        player_id: String,
    },
    /// Player left or timed out from the room.
    Leave {
        /// Leaving player identifier.
        #[serde(rename = "playerId")]
        player_id: String,
    },
    /// Trusted room-host command.
    Command {
        /// Command payload.
        command: GameCommand,
    },
}

/// Deterministic step replayed during container session hydration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamReplayStep {
    /// One-based authoritative simulation tick.
    pub tick: u64,
    /// Player inputs sealed for `tick`.
    #[serde(default)]
    pub inputs: Vec<TickInput>,
    /// Lifecycle events sealed before `tick`.
    #[serde(default)]
    pub events: Vec<StreamStepEvent>,
}

/// Control messages sent from `GameRoom DO` to `ws://container:8080/stream`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum StreamClientMessage {
    /// Initialize or recover the container's in-memory match state from a checkpoint.
    Hydrate {
        /// Active match identifier.
        #[serde(rename = "matchId")]
        match_id: String,
        /// Authoritative tick rate in hertz.
        #[serde(rename = "tickRateHz")]
        tick_rate_hz: f64,
        /// Checkpoint tick corresponding to `state`.
        tick: u64,
        /// Full authoritative checkpoint state.
        state: Value,
        /// Active player roster at the checkpoint.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        players: Vec<String>,
        /// Per-player views stored alongside the checkpoint, required for private views without replay.
        #[serde(
            default,
            rename = "playerViews",
            skip_serializing_if = "Option::is_none"
        )]
        player_views: Option<HashMap<String, Value>>,
        /// Ordered post-checkpoint steps to fast-forward deterministically.
        #[serde(default)]
        replay: Vec<StreamReplayStep>,
        /// Players requiring a full baseline view on the next projection.
        #[serde(default, rename = "resyncPlayers")]
        resync_players: Vec<String>,
    },
    /// Advance one authoritative simulation tick on the hydrated session.
    Step {
        /// Expected next one-based simulation tick (`current_tick + 1`).
        tick: u64,
        /// Player inputs sealed for this tick.
        #[serde(default)]
        inputs: Vec<TickInput>,
        /// Room lifecycle events sealed for this tick.
        #[serde(default)]
        events: Vec<StreamStepEvent>,
        /// Players requiring a full baseline keyframe view on this tick.
        #[serde(default, rename = "resyncPlayers")]
        resync_players: Vec<String>,
        /// Whether the host explicitly requests the full serialized `state`.
        #[serde(default, rename = "wantState")]
        want_state: bool,
    },
}

/// Outbound messages emitted by the container over `ws://container:8080/stream`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamServerMessage {
    /// Confirms session hydration (and optional fast-forward replay) completed.
    Hydrated {
        /// Active match identifier.
        #[serde(rename = "matchId")]
        match_id: String,
        /// Current authoritative tick after hydration and replay.
        tick: u64,
        /// Serialized post-replay state when `replay` was non-empty.
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<Value>,
        /// Optional per-player projected views after hydration.
        #[serde(rename = "playerViews", skip_serializing_if = "Option::is_none")]
        player_views: Option<HashMap<String, Value>>,
        /// Broadcasts emitted during fast-forward replay.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        broadcasts: Vec<Value>,
        /// Updated dynamic joinability flag, if reported.
        #[serde(skip_serializing_if = "Option::is_none")]
        joinable: Option<bool>,
        /// Result units emitted during fast-forward replay.
        #[serde(default, rename = "resultUnits", skip_serializing_if = "Vec::is_empty")]
        result_units: Vec<GameResultEvent>,
        /// Action receipts emitted during fast-forward replay.
        #[serde(
            default,
            rename = "actionResults",
            skip_serializing_if = "Vec::is_empty"
        )]
        action_results: Vec<GameActionResult>,
        /// Terminal status after hydration/replay.
        #[serde(rename = "gameResult")]
        game_result: GameResult,
    },
    /// Output of a single pipelined `Step` transition.
    StepResult {
        /// Completed simulation tick.
        tick: u64,
        /// Full authoritative state, included only on checkpoints or critical transitions.
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<Value>,
        /// Per-player AOI or delta views for connected players.
        #[serde(rename = "playerViews", skip_serializing_if = "Option::is_none")]
        player_views: Option<HashMap<String, Value>>,
        /// Public broadcasts emitted during this tick.
        #[serde(default)]
        broadcasts: Vec<Value>,
        /// Whether the game requested a durable checkpoint.
        persist: bool,
        /// Updated dynamic joinability flag, if reported.
        #[serde(skip_serializing_if = "Option::is_none")]
        joinable: Option<bool>,
        /// Validated result units emitted during this tick.
        #[serde(default, rename = "resultUnits", skip_serializing_if = "Vec::is_empty")]
        result_units: Vec<GameResultEvent>,
        /// Explicit outcomes for action IDs submitted on this tick.
        #[serde(
            default,
            rename = "actionResults",
            skip_serializing_if = "Option::is_none"
        )]
        action_results: Option<Vec<GameActionResult>>,
        /// Inline `isGameOver` evaluation for this tick.
        #[serde(rename = "gameResult")]
        game_result: GameResult,
    },
    /// Indicates a pipelined `Step` was skipped because an earlier tick ended the match.
    IgnoredAfterGameOver {
        /// Skipped tick number.
        tick: u64,
    },
    /// Structured protocol or execution failure for a stream request.
    Error {
        /// Associated tick if applicable.
        #[serde(skip_serializing_if = "Option::is_none")]
        tick: Option<u64>,
        /// Machine-readable error code.
        code: String,
        /// Whether retrying without changing the request or game code may succeed.
        retryable: bool,
        /// Creator-facing correction guidance.
        creator_action: String,
        /// Sanitized error message.
        error: String,
    },
}

/// Strongly typed `init` result avoiding intermediate `serde_json::Value` round-trips.
#[derive(Debug, Clone, Default)]
pub struct TypedInitResult<S> {
    /// Initial strongly typed game state.
    pub state: S,
    /// Optional initial broadcasts.
    pub broadcasts: Option<Vec<Value>>,
    /// Optional initial result units.
    pub result_units: Option<Vec<GameResultEvent>>,
    /// Optional initial joinability flag.
    pub joinable: Option<bool>,
    /// Optional initial per-player views.
    pub player_views: Option<HashMap<String, Value>>,
}

/// Strongly typed single-tick transition output.
#[derive(Debug, Clone, Default)]
pub struct TypedTickResult {
    /// Optional public broadcasts.
    pub broadcasts: Option<Vec<Value>>,
    /// Whether the game requests a checkpoint.
    pub persist: bool,
    /// Optional result units emitted on this tick.
    pub result_units: Option<Vec<GameResultEvent>>,
    /// Optional updated joinability flag.
    pub joinable: Option<bool>,
    /// Optional per-player projected views.
    pub player_views: Option<HashMap<String, Value>>,
    /// Explicit results for inputs carrying action IDs.
    pub action_results: Option<Vec<GameActionResult>>,
}

/// Strongly typed player join/leave output (mutating `State` in place).
#[derive(Debug, Clone, Default)]
pub struct TypedEventOutcome {
    /// Optional public broadcasts.
    pub broadcasts: Option<Vec<Value>>,
    /// Optional result units.
    pub result_units: Option<Vec<GameResultEvent>>,
    /// Optional updated joinability flag.
    pub joinable: Option<bool>,
    /// Optional per-player projected views.
    pub player_views: Option<HashMap<String, Value>>,
}

/// High-performance typed game server trait that keeps native `State` in container RAM
/// across `/stream` ticks without per-tick `serde_json::Value` serialization.
#[async_trait]
pub trait TypedGameServer: Send + Sync + 'static {
    /// Native deterministic game state stored in memory during a stream session.
    type State: Serialize + DeserializeOwned + Clone + Send + Sync + 'static;

    /// Per-stream-connection ephemeral view state (for example per-player AOI delta sets).
    /// Must never influence authoritative `State` transitions.
    type ViewContext: Default + Send + Sync + 'static;

    /// Optional Agent support declaration.
    async fn agent_capabilities(&self) -> Result<Option<AgentGameCapabilities>, SdkError> {
        Ok(None)
    }

    /// Declare the authoritative per-seat Agent view and action contract.
    async fn agent_player_contract(&self) -> Result<Option<AgentPlayerContract>, SdkError> {
        Ok(None)
    }

    /// Project a recipient-safe Agent view from strongly typed state and committed events.
    async fn project_agent_player_view_typed(
        &self,
        _context: &AgentPlayerViewContext,
        _players: &[String],
        _state: &Self::State,
        _committed_events: &[Value],
    ) -> Result<AgentPlayerViewProjection, SdkError> {
        Err(SdkError::NotImplemented(
            "Agent player projection is not implemented.".into(),
        ))
    }

    /// Optional stream durability override (`None` selects platform default by `tickRateHz`).
    fn durability(&self) -> Option<StreamDurability> {
        None
    }

    /// Return game metadata for `GET /manifest`.
    async fn manifest(&self) -> Result<GameManifest, SdkError>;

    /// Initialize a match into strongly typed `Self::State`.
    async fn init_typed(
        &self,
        config: MatchConfig,
    ) -> Result<TypedInitResult<Self::State>, SdkError>;

    /// Reset per-player delta state in `ViewContext` so the next tick emits a full baseline view.
    fn reset_player_view(&self, _view_ctx: &mut Self::ViewContext, _player_id: &str) {}

    /// Advance one deterministic tick by mutating `state` in place.
    /// When `emit_views` is `false` (for example fast-forward replay), view projection can be skipped.
    async fn step_typed(
        &self,
        ctx: TickContext,
        state: &mut Self::State,
        view_ctx: &mut Self::ViewContext,
        emit_views: bool,
    ) -> Result<TypedTickResult, SdkError>;

    /// Handle player join or reconnect in place.
    async fn on_player_join_typed(
        &self,
        _player_id: &str,
        _state: &mut Self::State,
        _view_ctx: &mut Self::ViewContext,
    ) -> Result<TypedEventOutcome, SdkError> {
        Ok(TypedEventOutcome::default())
    }

    /// Handle player leave in place.
    async fn on_player_leave_typed(
        &self,
        _player_id: &str,
        _state: &mut Self::State,
        _view_ctx: &mut Self::ViewContext,
    ) -> Result<TypedEventOutcome, SdkError> {
        Ok(TypedEventOutcome::default())
    }

    /// Evaluate whether the match has reached a terminal state.
    async fn is_game_over_typed(&self, state: &Self::State) -> Result<GameResult, SdkError>;

    /// Handle a trusted platform command in place.
    async fn on_command_typed(
        &self,
        _command: GameCommand,
        _state: &mut Self::State,
        _view_ctx: &mut Self::ViewContext,
    ) -> Result<TypedEventOutcome, SdkError> {
        Ok(TypedEventOutcome::default())
    }
}

/// Blanket adapter exposing any [`TypedGameServer`] over the 8 stateless HTTP endpoints.
pub struct TypedGameAdapter<T: TypedGameServer>(pub Arc<T>);

fn deserialize_game_state<T: DeserializeOwned>(value: Value) -> Result<T, SdkError> {
    serde_json::from_value(value).map_err(|_| {
        SdkError::protocol_with_message(
            SdkErrorCode::InvalidState,
            "State does not match the game server's declared schema.",
        )
    })
}

#[async_trait]
impl<T: TypedGameServer> GameServer for TypedGameAdapter<T> {
    async fn agent_capabilities(&self) -> Result<Option<AgentGameCapabilities>, SdkError> {
        self.0.agent_capabilities().await
    }

    async fn agent_player_contract(&self) -> Result<Option<AgentPlayerContract>, SdkError> {
        self.0.agent_player_contract().await
    }

    async fn project_agent_player_view(
        &self,
        request: AgentPlayerViewRequest,
    ) -> Result<AgentPlayerViewProjection, SdkError> {
        let state: T::State = deserialize_game_state(request.state)?;
        self.0
            .project_agent_player_view_typed(
                &request.context,
                &request.players,
                &state,
                &request.committed_events,
            )
            .await
    }

    fn durability(&self) -> Option<StreamDurability> {
        self.0.durability()
    }

    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        self.0.manifest().await
    }

    async fn init(&self, config: MatchConfig) -> Result<InitResult, SdkError> {
        let out = self.0.init_typed(config).await?;
        let state = serde_json::to_value(&out.state)
            .map_err(|e| SdkError::Internal(format!("State serialization failed: {e}")))?;
        Ok(InitResult {
            state,
            broadcasts: out.broadcasts,
            result_units: out.result_units,
            joinable: out.joinable,
            player_views: out.player_views,
        })
    }

    async fn tick(&self, ctx: TickContext, state_val: Value) -> Result<TickResult, SdkError> {
        let mut state: T::State = deserialize_game_state(state_val)?;
        let mut view_ctx = T::ViewContext::default();
        let out = self
            .0
            .step_typed(ctx, &mut state, &mut view_ctx, true)
            .await?;
        let next_val = serde_json::to_value(&state)
            .map_err(|e| SdkError::Internal(format!("State serialization failed: {e}")))?;
        Ok(TickResult {
            state: next_val,
            broadcasts: out.broadcasts,
            persist: out.persist,
            result_units: out.result_units,
            joinable: out.joinable,
            player_views: out.player_views,
            action_results: out.action_results,
        })
    }

    async fn on_player_join(
        &self,
        player_id: String,
        state: Value,
    ) -> Result<PlayerEventResult, SdkError> {
        self.on_player_join_ext(PlayerEventRequest {
            player_id,
            state,
            players: None,
            extra: HashMap::new(),
        })
        .await
    }

    async fn on_player_join_ext(
        &self,
        req: PlayerEventRequest,
    ) -> Result<PlayerEventResult, SdkError> {
        let mut state: T::State = deserialize_game_state(req.state)?;
        let mut view_ctx = T::ViewContext::default();
        let out = self
            .0
            .on_player_join_typed(&req.player_id, &mut state, &mut view_ctx)
            .await?;
        let next_val = serde_json::to_value(&state)
            .map_err(|e| SdkError::Internal(format!("State serialization failed: {e}")))?;
        Ok(PlayerEventResult {
            state: next_val,
            broadcasts: out.broadcasts,
            result_units: out.result_units,
            joinable: out.joinable,
            player_views: out.player_views,
        })
    }

    async fn on_player_leave(
        &self,
        player_id: String,
        state: Value,
    ) -> Result<PlayerEventResult, SdkError> {
        self.on_player_leave_ext(PlayerEventRequest {
            player_id,
            state,
            players: None,
            extra: HashMap::new(),
        })
        .await
    }

    async fn on_player_leave_ext(
        &self,
        req: PlayerEventRequest,
    ) -> Result<PlayerEventResult, SdkError> {
        let mut state: T::State = deserialize_game_state(req.state)?;
        let mut view_ctx = T::ViewContext::default();
        let out = self
            .0
            .on_player_leave_typed(&req.player_id, &mut state, &mut view_ctx)
            .await?;
        let next_val = serde_json::to_value(&state)
            .map_err(|e| SdkError::Internal(format!("State serialization failed: {e}")))?;
        Ok(PlayerEventResult {
            state: next_val,
            broadcasts: out.broadcasts,
            result_units: out.result_units,
            joinable: out.joinable,
            player_views: out.player_views,
        })
    }

    async fn is_game_over(&self, state: Value) -> Result<GameResult, SdkError> {
        self.is_game_over_ext(IsGameOverRequest {
            state,
            extra: HashMap::new(),
        })
        .await
    }

    async fn is_game_over_ext(&self, req: IsGameOverRequest) -> Result<GameResult, SdkError> {
        let state: T::State = deserialize_game_state(req.state)?;
        self.0.is_game_over_typed(&state).await
    }

    async fn on_command(
        &self,
        command: GameCommand,
        state_val: Value,
    ) -> Result<CommandResult, SdkError> {
        let mut state: T::State = deserialize_game_state(state_val)?;
        let mut view_ctx = T::ViewContext::default();
        let out = self
            .0
            .on_command_typed(command, &mut state, &mut view_ctx)
            .await?;
        let next_val = serde_json::to_value(&state)
            .map_err(|e| SdkError::Internal(format!("State serialization failed: {e}")))?;
        Ok(CommandResult {
            state: next_val,
            broadcasts: out.broadcasts,
            result_units: out.result_units,
            joinable: out.joinable,
            player_views: out.player_views,
        })
    }
}

/// Per-connection session runner abstraction supporting both untyped [`GameServer`]
/// and zero-`Value` [`TypedGameServer`] implementations.
#[async_trait]
pub trait StreamSessionFactory: Send + Sync + 'static {
    /// Create a fresh per-WebSocket session instance.
    fn new_session(&self) -> Box<dyn ActiveStreamSession>;
}

/// Active stateful session bound to a single `/stream` WebSocket connection.
#[async_trait]
pub trait ActiveStreamSession: Send {
    /// Process one incoming client message and return the response frame.
    async fn handle_message(&mut self, msg: StreamClientMessage) -> StreamServerMessage;
}

async fn guard_unwind<T>(fut: impl Future<Output = Result<T, SdkError>>) -> Result<T, SdkError> {
    match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(SdkError::Protocol { code, message })) => {
            Err(SdkError::protocol_with_message(code, message))
        }
        Ok(Err(_)) | Err(_) => Err(SdkError::protocol(SdkErrorCode::CallbackFailed)),
    }
}

fn validate_action_results_contract(
    action_results: Option<&[GameActionResult]>,
    submitted_action_ids: &HashSet<String>,
) -> Result<(), SdkError> {
    if let Some(results) = action_results {
        let mut reported = HashSet::with_capacity(results.len());
        for result in results {
            if result.validate().is_err()
                || !submitted_action_ids.contains(&result.action_id)
                || !reported.insert(result.action_id.as_str())
            {
                return Err(SdkError::protocol(SdkErrorCode::InvalidResult));
            }
        }
    }
    Ok(())
}

fn validate_result_units(result_units: Option<&[GameResultEvent]>) -> Result<(), SdkError> {
    validate_game_result_units(result_units, None).map_err(|error| {
        if error.to_string().to_ascii_lowercase().contains("limit")
            || error.to_string().to_ascii_lowercase().contains("exceeds")
        {
            SdkError::protocol(SdkErrorCode::ResourceLimitExceeded)
        } else {
            SdkError::protocol(SdkErrorCode::InvalidResult)
        }
    })
}

fn validate_game_over_contract(res: &GameResult) -> Result<(), SdkError> {
    validate_result_units(res.result_units.as_deref())?;
    if res.over {
        let policy_ok = res.next_match_policy.as_ref().is_some_and(|p| {
            matches!(
                p.mode.as_str(),
                "auto_next_match" | "wait_for_ready" | "close_room"
            ) && p.timeout_ms <= 9_007_199_254_740_991
        });
        let summary_ok = res
            .summary
            .as_ref()
            .is_some_and(|s| s.duration_seconds.is_finite() && s.duration_seconds >= 0.0);
        if !policy_ok || !summary_ok {
            return Err(SdkError::protocol(SdkErrorCode::InvalidGameOverPolicy));
        }
    }
    Ok(())
}

fn private_views_required(manifest: &GameManifest) -> bool {
    manifest.player_view_mode == Some(PlayerViewMode::Private)
}

fn validate_active_roster(players: &[String], manifest: &GameManifest) -> Result<(), SdkError> {
    let unique = players.iter().collect::<HashSet<_>>();
    if players.is_empty()
        || players.len() > manifest.max_players as usize
        || players.len() > 100
        || unique.len() != players.len()
        || players.iter().any(|player_id| player_id.trim().is_empty())
    {
        return Err(SdkError::protocol(SdkErrorCode::WrongRoster));
    }
    Ok(())
}

fn projected_roster(
    current: &[String],
    events: &[StreamStepEvent],
    manifest: &GameManifest,
) -> Result<Vec<String>, SdkError> {
    let mut players = current.to_vec();
    for event in events {
        match event {
            StreamStepEvent::Join { player_id } => {
                if player_id.trim().is_empty() {
                    return Err(SdkError::protocol(SdkErrorCode::WrongRoster));
                }
                if !players.contains(player_id) {
                    players.push(player_id.clone());
                    validate_active_roster(&players, manifest)?;
                }
            }
            StreamStepEvent::Leave { player_id } => {
                let Some(index) = players.iter().position(|active| active == player_id) else {
                    return Err(SdkError::protocol(SdkErrorCode::WrongRoster));
                };
                players.remove(index);
            }
            StreamStepEvent::Command { command } => {
                if command.command_type.trim().is_empty() {
                    return Err(SdkError::protocol(SdkErrorCode::InvalidAction));
                }
            }
        }
    }
    Ok(players)
}

fn validate_inputs_against_roster(
    inputs: &[TickInput],
    players: &[String],
) -> Result<(), SdkError> {
    let mut action_ids = HashSet::new();
    for input in inputs {
        if !players.contains(&input.player_id) {
            return Err(SdkError::protocol(SdkErrorCode::WrongRoster));
        }
        if let Some(action_id) = &input.action_id {
            if !action_ids.insert(action_id.as_str()) {
                return Err(SdkError::protocol(SdkErrorCode::InvalidAction));
            }
        }
    }
    Ok(())
}

fn validate_resync_players(resync_players: &[String], players: &[String]) -> Result<(), SdkError> {
    let mut unique = HashSet::new();
    if resync_players
        .iter()
        .any(|player_id| !players.contains(player_id) || !unique.insert(player_id))
    {
        return Err(SdkError::protocol(SdkErrorCode::WrongRoster));
    }
    Ok(())
}

fn validate_step_payload(
    current_players: &[String],
    manifest: &GameManifest,
    tick_rate_hz: f64,
    tick: u64,
    inputs: &[TickInput],
    events: &[StreamStepEvent],
    resync_players: &[String],
) -> Result<(), SdkError> {
    let players = projected_roster(current_players, events, manifest)?;
    validate_inputs_against_roster(inputs, &players)?;
    validate_resync_players(resync_players, &players)?;
    TickContext {
        tick,
        elapsed_seconds: tick as f64 / tick_rate_hz,
        inputs: inputs.to_vec(),
        commands: Vec::new(),
    }
    .validate()
    .map_err(|_| SdkError::protocol(SdkErrorCode::InvalidAction))?;
    Ok(())
}

fn error_frame(tick: Option<u64>, err: SdkError) -> StreamServerMessage {
    let code = err.code();
    let error = match err {
        SdkError::Protocol { message, .. } | SdkError::Validation(message) => message,
        _ => code.creator_action().to_string(),
    };
    StreamServerMessage::Error {
        tick,
        code: code.as_str().into(),
        retryable: code.retryable(),
        creator_action: code.creator_action().into(),
        error,
    }
}

/// Session factory backed by any [`GameServer`] (keeps `Value` in memory across steps).
pub struct ValueStreamFactory<G: GameServer>(pub Arc<G>);

impl<G: GameServer> StreamSessionFactory for ValueStreamFactory<G> {
    fn new_session(&self) -> Box<dyn ActiveStreamSession> {
        Box::new(ValueStreamSession {
            game: self.0.clone(),
            active: None,
        })
    }
}

struct ValueSessionData {
    tick_rate_hz: f64,
    tick: u64,
    state: Value,
    players: Vec<String>,
    manifest: GameManifest,
    game_over: bool,
}

struct ValueStreamSession<G: GameServer> {
    game: Arc<G>,
    active: Option<ValueSessionData>,
}

impl<G: GameServer> ValueStreamSession<G> {
    async fn apply_step_internal(
        game: &G,
        manifest: &GameManifest,
        tick_rate_hz: f64,
        state: Value,
        current_players: &[String],
        tick: u64,
        inputs: Vec<TickInput>,
        events: Vec<StreamStepEvent>,
        resync_players: &[String],
    ) -> Result<
        (
            Value,
            Vec<String>,
            Vec<Value>,
            bool,
            Option<bool>,
            Option<HashMap<String, Value>>,
            Vec<GameResultEvent>,
            Option<Vec<GameActionResult>>,
            GameResult,
        ),
        SdkError,
    > {
        let mut cur_state = state;
        validate_step_payload(
            current_players,
            manifest,
            tick_rate_hz,
            tick,
            &inputs,
            &events,
            resync_players,
        )?;
        let private_views = private_views_required(manifest);
        let mut players = current_players.to_vec();
        let mut broadcasts = Vec::new();
        let mut result_units = Vec::new();
        let mut joinable = None;
        let mut player_views = None;

        for ev in events {
            match ev {
                StreamStepEvent::Join { player_id } => {
                    if !players.contains(&player_id) {
                        players.push(player_id.clone());
                    }
                    let req = PlayerEventRequest {
                        player_id,
                        state: cur_state,
                        players: Some(players.clone()),
                        extra: HashMap::new(),
                    };
                    let res = guard_unwind(game.on_player_join_ext(req)).await?;
                    validate_result_units(res.result_units.as_deref())?;
                    validate_player_views(
                        res.player_views.as_ref(),
                        Some(&players),
                        private_views,
                    )?;
                    cur_state = res.state;
                    if let Some(b) = res.broadcasts {
                        broadcasts.extend(b);
                    }
                    if let Some(u) = res.result_units {
                        result_units.extend(u);
                    }
                    if res.joinable.is_some() {
                        joinable = res.joinable;
                    }
                    if res.player_views.is_some() {
                        player_views = res.player_views;
                    }
                }
                StreamStepEvent::Leave { player_id } => {
                    if let Some(index) = players.iter().position(|active| active == &player_id) {
                        players.remove(index);
                    }
                    let req = PlayerEventRequest {
                        player_id,
                        state: cur_state,
                        players: Some(players.clone()),
                        extra: HashMap::new(),
                    };
                    let res = guard_unwind(game.on_player_leave_ext(req)).await?;
                    validate_result_units(res.result_units.as_deref())?;
                    validate_player_views(
                        res.player_views.as_ref(),
                        Some(&players),
                        private_views,
                    )?;
                    cur_state = res.state;
                    if let Some(b) = res.broadcasts {
                        broadcasts.extend(b);
                    }
                    if let Some(u) = res.result_units {
                        result_units.extend(u);
                    }
                    if res.joinable.is_some() {
                        joinable = res.joinable;
                    }
                    if res.player_views.is_some() {
                        player_views = res.player_views;
                    }
                }
                StreamStepEvent::Command { command } => {
                    if command.command_type.trim().is_empty() {
                        return Err(SdkError::Validation(
                            "command.type must be a non-empty string.".into(),
                        ));
                    }
                    let res = guard_unwind(game.on_command(command, cur_state)).await?;
                    validate_result_units(res.result_units.as_deref())?;
                    validate_player_views(
                        res.player_views.as_ref(),
                        Some(&players),
                        private_views,
                    )?;
                    cur_state = res.state;
                    if let Some(b) = res.broadcasts {
                        broadcasts.extend(b);
                    }
                    if let Some(u) = res.result_units {
                        result_units.extend(u);
                    }
                    if res.joinable.is_some() {
                        joinable = res.joinable;
                    }
                    if res.player_views.is_some() {
                        player_views = res.player_views;
                    }
                }
            }
        }

        let submitted_action_ids = inputs
            .iter()
            .filter_map(|input| input.action_id.clone())
            .collect::<HashSet<_>>();
        let ctx = TickContext {
            tick,
            elapsed_seconds: tick as f64 / tick_rate_hz,
            inputs,
            commands: Vec::new(),
        };
        let tick_res = guard_unwind(game.tick(ctx, cur_state)).await?;
        validate_result_units(tick_res.result_units.as_deref())?;
        validate_action_results_contract(
            tick_res.action_results.as_deref(),
            &submitted_action_ids,
        )?;
        validate_player_views(
            tick_res.player_views.as_ref(),
            Some(&players),
            private_views,
        )?;
        let action_results = tick_res.action_results.clone();
        cur_state = tick_res.state;
        if let Some(b) = tick_res.broadcasts {
            broadcasts.extend(b);
        }
        if let Some(u) = tick_res.result_units {
            result_units.extend(u);
        }
        if tick_res.joinable.is_some() {
            joinable = tick_res.joinable;
        }
        if tick_res.player_views.is_some() {
            player_views = tick_res.player_views;
        }

        let mut game_result = guard_unwind(game.is_game_over_ext(IsGameOverRequest {
            state: cur_state.clone(),
            extra: HashMap::new(),
        }))
        .await?;
        validate_game_over_contract(&game_result)?;
        if let Some(u) = game_result.result_units.take() {
            result_units.extend(u);
        }
        if game_result.over {
            joinable = joinable.map(|_| false);
        }
        validate_player_views(player_views.as_ref(), Some(&players), private_views)?;

        Ok((
            cur_state,
            players,
            broadcasts,
            tick_res.persist,
            joinable,
            player_views,
            result_units,
            action_results,
            game_result,
        ))
    }
}

#[async_trait]
impl<G: GameServer> ActiveStreamSession for ValueStreamSession<G> {
    async fn handle_message(&mut self, msg: StreamClientMessage) -> StreamServerMessage {
        match msg {
            StreamClientMessage::Hydrate {
                match_id,
                tick_rate_hz,
                tick,
                state,
                players,
                player_views,
                replay,
                resync_players,
            } => {
                self.active = None;
                if match_id.trim().is_empty() || tick > 9_007_199_254_740_991 {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol(SdkErrorCode::InvalidStreamMessage),
                    );
                }
                if !tick_rate_hz.is_finite() || !(1.0..=60.0).contains(&tick_rate_hz) {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol_with_message(
                            SdkErrorCode::InvalidStreamMessage,
                            "tickRateHz must be between 1 and 60.",
                        ),
                    );
                }
                let manifest = match guard_unwind(self.game.manifest()).await {
                    Ok(manifest) => manifest,
                    Err(error) => return error_frame(Some(tick), error),
                };
                if manifest.validate().is_err() {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol(SdkErrorCode::InvalidManifest),
                    );
                }
                if let Err(error) = validate_active_roster(&players, &manifest)
                    .and_then(|_| validate_resync_players(&resync_players, &players))
                {
                    return error_frame(Some(tick), error);
                }
                let had_replay = !replay.is_empty();
                if let Err(error) = validate_player_views(
                    player_views.as_ref(),
                    Some(&players),
                    private_views_required(&manifest) && !had_replay,
                ) {
                    return error_frame(Some(tick), error);
                }
                let mut cur_tick = tick;
                let mut cur_state = state;
                let mut cur_players = players;
                let mut all_broadcasts = Vec::new();
                let mut all_units = Vec::new();
                let mut all_action_results = Vec::new();
                let mut joinable = None;
                let mut last_views = if had_replay { None } else { player_views };

                let mut game_result =
                    match guard_unwind(self.game.is_game_over_ext(IsGameOverRequest {
                        state: cur_state.clone(),
                        extra: HashMap::new(),
                    }))
                    .await
                    {
                        Ok(mut gr) => {
                            if let Err(e) = validate_game_over_contract(&gr) {
                                return error_frame(Some(cur_tick), e);
                            }
                            if let Some(u) = gr.result_units.take() {
                                all_units.extend(u);
                            }
                            gr
                        }
                        Err(e) => return error_frame(Some(cur_tick), e),
                    };

                for step in replay {
                    if game_result.over || cur_tick.checked_add(1) != Some(step.tick) {
                        return error_frame(
                            Some(step.tick),
                            SdkError::protocol(SdkErrorCode::ReplayMismatch),
                        );
                    }
                    match Self::apply_step_internal(
                        &self.game,
                        &manifest,
                        tick_rate_hz,
                        cur_state,
                        &cur_players,
                        step.tick,
                        step.inputs,
                        step.events,
                        &[],
                    )
                    .await
                    {
                        Ok((next_s, next_players, b, _persist, j, views, units, actions, gr)) => {
                            cur_tick = step.tick;
                            cur_state = next_s;
                            cur_players = next_players;
                            all_broadcasts.extend(b);
                            all_units.extend(units);
                            if let Some(actions) = actions {
                                all_action_results.extend(actions);
                            }
                            if j.is_some() {
                                joinable = j;
                            }
                            last_views = views;
                            game_result = gr;
                        }
                        Err(e) => return error_frame(Some(step.tick), e),
                    }
                }

                let state_out = if had_replay {
                    Some(cur_state.clone())
                } else {
                    None
                };
                self.active = Some(ValueSessionData {
                    tick_rate_hz,
                    tick: cur_tick,
                    state: cur_state,
                    players: cur_players,
                    manifest,
                    game_over: game_result.over,
                });
                StreamServerMessage::Hydrated {
                    match_id,
                    tick: cur_tick,
                    state: state_out,
                    player_views: last_views,
                    broadcasts: all_broadcasts,
                    joinable,
                    result_units: all_units,
                    action_results: all_action_results,
                    game_result,
                }
            }
            StreamClientMessage::Step {
                tick,
                inputs,
                events,
                resync_players,
                want_state,
            } => {
                let Some(session) = self.active.as_mut() else {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol(SdkErrorCode::InvalidStreamMessage),
                    );
                };
                if session.game_over {
                    return StreamServerMessage::IgnoredAfterGameOver { tick };
                }
                if tick > 9_007_199_254_740_991 || session.tick.checked_add(1) != Some(tick) {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol_with_message(
                            SdkErrorCode::TickSequenceMismatch,
                            format!(
                                "Expected tick {}, got {tick}.",
                                session.tick.saturating_add(1)
                            ),
                        ),
                    );
                }
                let manifest = session.manifest.clone();
                if let Err(error) = validate_step_payload(
                    &session.players,
                    &manifest,
                    session.tick_rate_hz,
                    tick,
                    &inputs,
                    &events,
                    &resync_players,
                ) {
                    return error_frame(Some(tick), error);
                }
                let had_events = !events.is_empty();
                let prev_state = std::mem::replace(&mut session.state, Value::Null);
                let current_players = session.players.clone();
                match Self::apply_step_internal(
                    &self.game,
                    &manifest,
                    session.tick_rate_hz,
                    prev_state,
                    &current_players,
                    tick,
                    inputs,
                    events,
                    &resync_players,
                )
                .await
                {
                    Ok((
                        next_state,
                        players,
                        broadcasts,
                        persist,
                        joinable,
                        player_views,
                        units,
                        action_results,
                        gr,
                    )) => {
                        let include_state =
                            want_state || had_events || persist || !units.is_empty() || gr.over;
                        let out_state = if include_state {
                            Some(next_state.clone())
                        } else {
                            None
                        };
                        session.tick = tick;
                        session.game_over = gr.over;
                        session.state = next_state;
                        session.players = players;
                        StreamServerMessage::StepResult {
                            tick,
                            state: out_state,
                            player_views,
                            broadcasts,
                            persist,
                            joinable,
                            result_units: units,
                            action_results,
                            game_result: gr,
                        }
                    }
                    Err(e) => {
                        self.active = None;
                        error_frame(Some(tick), e)
                    }
                }
            }
        }
    }
}

/// Zero-`Value` session factory backed by a [`TypedGameServer`].
pub struct TypedStreamFactory<T: TypedGameServer>(pub Arc<T>);

impl<T: TypedGameServer> StreamSessionFactory for TypedStreamFactory<T> {
    fn new_session(&self) -> Box<dyn ActiveStreamSession> {
        Box::new(TypedStreamSession {
            game: self.0.clone(),
            active: None,
        })
    }
}

struct TypedSessionData<T: TypedGameServer> {
    tick_rate_hz: f64,
    tick: u64,
    state: T::State,
    players: Vec<String>,
    manifest: GameManifest,
    view_ctx: T::ViewContext,
    game_over: bool,
}

struct TypedStreamSession<T: TypedGameServer> {
    game: Arc<T>,
    active: Option<TypedSessionData<T>>,
}

impl<T: TypedGameServer> TypedStreamSession<T> {
    async fn apply_typed_step(
        game: &T,
        manifest: &GameManifest,
        tick_rate_hz: f64,
        state: &mut T::State,
        current_players: &[String],
        view_ctx: &mut T::ViewContext,
        tick: u64,
        inputs: Vec<TickInput>,
        events: Vec<StreamStepEvent>,
        resync_players: &[String],
        emit_views: bool,
    ) -> Result<
        (
            Vec<String>,
            Vec<Value>,
            bool,
            Option<bool>,
            Option<HashMap<String, Value>>,
            Vec<GameResultEvent>,
            Option<Vec<GameActionResult>>,
            GameResult,
        ),
        SdkError,
    > {
        validate_step_payload(
            current_players,
            manifest,
            tick_rate_hz,
            tick,
            &inputs,
            &events,
            resync_players,
        )?;
        let private_views = private_views_required(manifest);
        let mut players = current_players.to_vec();
        for pid in resync_players {
            game.reset_player_view(view_ctx, pid);
        }
        let mut broadcasts = Vec::new();
        let mut result_units = Vec::new();
        let mut joinable = None;
        let mut player_views = None;

        for ev in events {
            match ev {
                StreamStepEvent::Join { player_id } => {
                    if !players.contains(&player_id) {
                        players.push(player_id.clone());
                        validate_active_roster(&players, manifest)?;
                    }
                    game.reset_player_view(view_ctx, &player_id);
                    let out = guard_unwind(game.on_player_join_typed(&player_id, state, view_ctx))
                        .await?;
                    validate_result_units(out.result_units.as_deref())?;
                    validate_player_views(
                        out.player_views.as_ref(),
                        Some(&players),
                        private_views,
                    )?;
                    if let Some(b) = out.broadcasts {
                        broadcasts.extend(b);
                    }
                    if let Some(u) = out.result_units {
                        result_units.extend(u);
                    }
                    if out.joinable.is_some() {
                        joinable = out.joinable;
                    }
                    if out.player_views.is_some() {
                        player_views = out.player_views;
                    }
                }
                StreamStepEvent::Leave { player_id } => {
                    let Some(index) = players.iter().position(|active| active == &player_id) else {
                        return Err(SdkError::protocol(SdkErrorCode::WrongRoster));
                    };
                    players.remove(index);
                    game.reset_player_view(view_ctx, &player_id);
                    let out = guard_unwind(game.on_player_leave_typed(&player_id, state, view_ctx))
                        .await?;
                    validate_result_units(out.result_units.as_deref())?;
                    validate_player_views(
                        out.player_views.as_ref(),
                        Some(&players),
                        private_views,
                    )?;
                    if let Some(b) = out.broadcasts {
                        broadcasts.extend(b);
                    }
                    if let Some(u) = out.result_units {
                        result_units.extend(u);
                    }
                    if out.joinable.is_some() {
                        joinable = out.joinable;
                    }
                    if out.player_views.is_some() {
                        player_views = out.player_views;
                    }
                }
                StreamStepEvent::Command { command } => {
                    if command.command_type.trim().is_empty() {
                        return Err(SdkError::Validation(
                            "command.type must be a non-empty string.".into(),
                        ));
                    }
                    let out = guard_unwind(game.on_command_typed(command, state, view_ctx)).await?;
                    validate_result_units(out.result_units.as_deref())?;
                    validate_player_views(
                        out.player_views.as_ref(),
                        Some(&players),
                        private_views,
                    )?;
                    if let Some(b) = out.broadcasts {
                        broadcasts.extend(b);
                    }
                    if let Some(u) = out.result_units {
                        result_units.extend(u);
                    }
                    if out.joinable.is_some() {
                        joinable = out.joinable;
                    }
                    if out.player_views.is_some() {
                        player_views = out.player_views;
                    }
                }
            }
        }

        let submitted_action_ids = inputs
            .iter()
            .filter_map(|input| input.action_id.clone())
            .collect::<HashSet<_>>();
        let ctx = TickContext {
            tick,
            elapsed_seconds: tick as f64 / tick_rate_hz,
            inputs,
            commands: Vec::new(),
        };
        let tick_out = guard_unwind(game.step_typed(ctx, state, view_ctx, emit_views)).await?;
        validate_result_units(tick_out.result_units.as_deref())?;
        validate_action_results_contract(
            tick_out.action_results.as_deref(),
            &submitted_action_ids,
        )?;
        validate_player_views(
            tick_out.player_views.as_ref(),
            Some(&players),
            private_views,
        )?;
        let action_results = tick_out.action_results.clone();
        if let Some(b) = tick_out.broadcasts {
            broadcasts.extend(b);
        }
        if let Some(u) = tick_out.result_units {
            result_units.extend(u);
        }
        if tick_out.joinable.is_some() {
            joinable = tick_out.joinable;
        }
        if tick_out.player_views.is_some() {
            player_views = tick_out.player_views;
        }

        let mut game_result = guard_unwind(game.is_game_over_typed(state)).await?;
        validate_game_over_contract(&game_result)?;
        if let Some(u) = game_result.result_units.take() {
            result_units.extend(u);
        }
        if game_result.over {
            joinable = joinable.map(|_| false);
        }
        validate_player_views(player_views.as_ref(), Some(&players), private_views)?;

        Ok((
            players,
            broadcasts,
            tick_out.persist,
            joinable,
            player_views,
            result_units,
            action_results,
            game_result,
        ))
    }
}

#[async_trait]
impl<T: TypedGameServer> ActiveStreamSession for TypedStreamSession<T> {
    async fn handle_message(&mut self, msg: StreamClientMessage) -> StreamServerMessage {
        match msg {
            StreamClientMessage::Hydrate {
                match_id,
                tick_rate_hz,
                tick,
                state: state_val,
                players,
                player_views,
                replay,
                resync_players,
            } => {
                self.active = None;
                if match_id.trim().is_empty() || tick > 9_007_199_254_740_991 {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol(SdkErrorCode::InvalidStreamMessage),
                    );
                }
                if !tick_rate_hz.is_finite() || !(1.0..=60.0).contains(&tick_rate_hz) {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol_with_message(
                            SdkErrorCode::InvalidStreamMessage,
                            "tickRateHz must be between 1 and 60.",
                        ),
                    );
                }
                let manifest = match guard_unwind(self.game.manifest()).await {
                    Ok(manifest) => manifest,
                    Err(error) => return error_frame(Some(tick), error),
                };
                if manifest.validate().is_err() {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol(SdkErrorCode::InvalidManifest),
                    );
                }
                if let Err(error) = validate_active_roster(&players, &manifest)
                    .and_then(|_| validate_resync_players(&resync_players, &players))
                {
                    return error_frame(Some(tick), error);
                }
                let mut typed_state: T::State = match serde_json::from_value(state_val) {
                    Ok(s) => s,
                    Err(e) => {
                        return error_frame(
                            Some(tick),
                            SdkError::protocol_with_message(
                                SdkErrorCode::InvalidState,
                                format!("Invalid checkpoint state: {e}"),
                            ),
                        )
                    }
                };
                if let Err(error) = validate_player_views(
                    player_views.as_ref(),
                    Some(&players),
                    private_views_required(&manifest) && replay.is_empty(),
                ) {
                    return error_frame(Some(tick), error);
                }
                let mut view_ctx = T::ViewContext::default();
                for pid in &resync_players {
                    self.game.reset_player_view(&mut view_ctx, pid);
                }
                let mut cur_tick = tick;
                let mut cur_players = players;
                let mut all_broadcasts = Vec::new();
                let mut all_units = Vec::new();
                let mut all_action_results = Vec::new();
                let mut joinable = None;
                let mut last_views = if replay.is_empty() {
                    player_views
                } else {
                    None
                };
                let replay_len = replay.len();

                let mut game_result =
                    match guard_unwind(self.game.is_game_over_typed(&typed_state)).await {
                        Ok(mut gr) => {
                            if let Err(e) = validate_game_over_contract(&gr) {
                                return error_frame(Some(cur_tick), e);
                            }
                            if let Some(u) = gr.result_units.take() {
                                all_units.extend(u);
                            }
                            gr
                        }
                        Err(e) => return error_frame(Some(cur_tick), e),
                    };

                for (idx, step) in replay.into_iter().enumerate() {
                    if game_result.over || cur_tick.checked_add(1) != Some(step.tick) {
                        return error_frame(
                            Some(step.tick),
                            SdkError::protocol(SdkErrorCode::ReplayMismatch),
                        );
                    }
                    let is_last = idx + 1 == replay_len;
                    match Self::apply_typed_step(
                        &self.game,
                        &manifest,
                        tick_rate_hz,
                        &mut typed_state,
                        &cur_players,
                        &mut view_ctx,
                        step.tick,
                        step.inputs,
                        step.events,
                        &[],
                        is_last,
                    )
                    .await
                    {
                        Ok((next_players, b, _persist, j, views, units, actions, gr)) => {
                            cur_tick = step.tick;
                            cur_players = next_players;
                            all_broadcasts.extend(b);
                            all_units.extend(units);
                            if let Some(actions) = actions {
                                all_action_results.extend(actions);
                            }
                            if j.is_some() {
                                joinable = j;
                            }
                            if is_last {
                                last_views = views;
                            }
                            game_result = gr;
                        }
                        Err(e) => return error_frame(Some(step.tick), e),
                    }
                }

                let state_out = if replay_len > 0 {
                    match serde_json::to_value(&typed_state) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            return error_frame(
                                Some(cur_tick),
                                SdkError::Internal(format!("State serialization failed: {e}")),
                            )
                        }
                    }
                } else {
                    None
                };

                self.active = Some(TypedSessionData {
                    tick_rate_hz,
                    tick: cur_tick,
                    state: typed_state,
                    players: cur_players,
                    manifest,
                    view_ctx,
                    game_over: game_result.over,
                });

                StreamServerMessage::Hydrated {
                    match_id,
                    tick: cur_tick,
                    state: state_out,
                    player_views: last_views,
                    broadcasts: all_broadcasts,
                    joinable,
                    result_units: all_units,
                    action_results: all_action_results,
                    game_result,
                }
            }
            StreamClientMessage::Step {
                tick,
                inputs,
                events,
                resync_players,
                want_state,
            } => {
                let Some(session) = self.active.as_mut() else {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol(SdkErrorCode::InvalidStreamMessage),
                    );
                };
                if session.game_over {
                    return StreamServerMessage::IgnoredAfterGameOver { tick };
                }
                if tick > 9_007_199_254_740_991 || session.tick.checked_add(1) != Some(tick) {
                    return error_frame(
                        Some(tick),
                        SdkError::protocol_with_message(
                            SdkErrorCode::TickSequenceMismatch,
                            format!(
                                "Expected tick {}, got {tick}.",
                                session.tick.saturating_add(1)
                            ),
                        ),
                    );
                }
                let manifest = session.manifest.clone();
                if let Err(error) = validate_step_payload(
                    &session.players,
                    &manifest,
                    session.tick_rate_hz,
                    tick,
                    &inputs,
                    &events,
                    &resync_players,
                ) {
                    return error_frame(Some(tick), error);
                }
                let had_events = !events.is_empty();
                let current_players = session.players.clone();
                match Self::apply_typed_step(
                    &self.game,
                    &manifest,
                    session.tick_rate_hz,
                    &mut session.state,
                    &current_players,
                    &mut session.view_ctx,
                    tick,
                    inputs,
                    events,
                    &resync_players,
                    true,
                )
                .await
                {
                    Ok((
                        players,
                        broadcasts,
                        persist,
                        joinable,
                        player_views,
                        units,
                        action_results,
                        gr,
                    )) => {
                        let include_state =
                            want_state || had_events || persist || !units.is_empty() || gr.over;
                        let out_state = if include_state {
                            match serde_json::to_value(&session.state) {
                                Ok(v) => Some(v),
                                Err(e) => {
                                    self.active = None;
                                    return error_frame(
                                        Some(tick),
                                        SdkError::Internal(format!(
                                            "State serialization failed: {e}"
                                        )),
                                    );
                                }
                            }
                        } else {
                            None
                        };
                        session.tick = tick;
                        session.players = players;
                        session.game_over = gr.over;
                        StreamServerMessage::StepResult {
                            tick,
                            state: out_state,
                            player_views,
                            broadcasts,
                            persist,
                            joinable,
                            result_units: units,
                            action_results,
                            game_result: gr,
                        }
                    }
                    Err(e) => {
                        self.active = None;
                        error_frame(Some(tick), e)
                    }
                }
            }
        }
    }
}

/// Drive a single `/stream` WebSocket connection sequentially in FIFO order.
pub async fn serve_stream_socket(mut socket: WebSocket, factory: Arc<dyn StreamSessionFactory>) {
    let mut session = factory.new_session();
    while let Some(Ok(msg)) = socket.next().await {
        let text = match msg {
            Message::Text(utf8) => utf8,
            Message::Binary(bytes) => match std::str::from_utf8(&bytes) {
                Ok(s) => s.to_string().into(),
                Err(_) => {
                    let _ = send_frame(
                        &mut socket,
                        &error_frame(None, SdkError::protocol(SdkErrorCode::InvalidStreamMessage)),
                    )
                    .await;
                    continue;
                }
            },
            Message::Ping(payload) => {
                if socket.send(Message::Pong(payload)).await.is_err() {
                    break;
                }
                continue;
            }
            Message::Pong(_) => continue,
            Message::Close(_) => break,
        };

        if text.len() > MAX_WS_FRAME_BYTES {
            let _ = send_frame(
                &mut socket,
                &error_frame(
                    None,
                    SdkError::protocol(SdkErrorCode::ResourceLimitExceeded),
                ),
            )
            .await;
            continue;
        }

        let parsed: StreamClientMessage = match serde_json::from_str(&text) {
            Ok(req) => req,
            Err(_) => {
                let _ = send_frame(
                    &mut socket,
                    &error_frame(None, SdkError::protocol(SdkErrorCode::InvalidStreamMessage)),
                )
                .await;
                continue;
            }
        };

        let response = session.handle_message(parsed).await;
        if send_frame(&mut socket, &response).await.is_err() {
            break;
        }
    }
}

async fn send_frame(socket: &mut WebSocket, frame: &StreamServerMessage) -> Result<(), ()> {
    let encoded = serde_json::to_string(frame).map_err(|_| ())?;
    socket
        .send(Message::Text(encoded.into()))
        .await
        .map_err(|_| ())
}
