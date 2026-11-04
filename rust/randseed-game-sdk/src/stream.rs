//! Persistent WebSocket session stream (`Session Stream v1`) and zero-`Value`
//! strongly typed server traits for the RandSeed room-host protocol.

use crate::agent_capabilities::AgentGameCapabilities;
use crate::errors::SdkError;
use crate::server::GameServer;
use crate::types::{
    validate_game_result_units, CommandResult, GameCommand, GameManifest, GameResult,
    GameResultEvent, InitResult, IsGameOverRequest, MatchConfig, PlayerEventRequest,
    PlayerEventResult, TickContext, TickInput, TickResult,
};
use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{FutureExt, StreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
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
#[serde(tag = "type", rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
#[serde(tag = "type", rename_all = "camelCase")]
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
        /// Sanitized error message.
        error: String,
    },
}

/// Strongly typed `init` result avoiding intermediate `serde_json::Value` round-trips.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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

#[async_trait]
impl<T: TypedGameServer> GameServer for TypedGameAdapter<T> {
    async fn agent_capabilities(&self) -> Result<Option<AgentGameCapabilities>, SdkError> {
        self.0.agent_capabilities().await
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
        let mut state: T::State = serde_json::from_value(state_val)
            .map_err(|e| SdkError::Validation(format!("Invalid game state payload: {e}")))?;
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
            extra: HashMap::new(),
        })
        .await
    }

    async fn on_player_join_ext(
        &self,
        req: PlayerEventRequest,
    ) -> Result<PlayerEventResult, SdkError> {
        let mut state: T::State = serde_json::from_value(req.state)
            .map_err(|e| SdkError::Validation(format!("Invalid game state payload: {e}")))?;
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
            extra: HashMap::new(),
        })
        .await
    }

    async fn on_player_leave_ext(
        &self,
        req: PlayerEventRequest,
    ) -> Result<PlayerEventResult, SdkError> {
        let mut state: T::State = serde_json::from_value(req.state)
            .map_err(|e| SdkError::Validation(format!("Invalid game state payload: {e}")))?;
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
        let state: T::State = serde_json::from_value(req.state)
            .map_err(|e| SdkError::Validation(format!("Invalid game state payload: {e}")))?;
        self.0.is_game_over_typed(&state).await
    }

    async fn on_command(
        &self,
        command: GameCommand,
        state_val: Value,
    ) -> Result<CommandResult, SdkError> {
        let mut state: T::State = serde_json::from_value(state_val)
            .map_err(|e| SdkError::Validation(format!("Invalid game state payload: {e}")))?;
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
        Ok(res) => res,
        Err(_) => Err(SdkError::Internal("Game callback panicked.".into())),
    }
}

fn validate_game_over_contract(res: &GameResult) -> Result<(), SdkError> {
    validate_game_result_units(res.result_units.as_deref(), None)?;
    if res.over {
        let policy_ok = res.next_match_policy.as_ref().is_some_and(|p| {
            matches!(
                p.mode.as_str(),
                "auto_next_match" | "wait_for_ready" | "close_room"
            )
        });
        let summary_ok = res
            .summary
            .as_ref()
            .is_some_and(|s| s.duration_seconds.is_finite() && s.duration_seconds >= 0.0);
        if !policy_ok || !summary_ok {
            return Err(SdkError::Internal(
                "The game returned an invalid game-over result.".into(),
            ));
        }
    }
    Ok(())
}

fn error_frame(tick: Option<u64>, err: SdkError) -> StreamServerMessage {
    match err {
        SdkError::Validation(msg) => StreamServerMessage::Error {
            tick,
            code: "VALIDATION_ERROR".into(),
            error: msg,
        },
        _ => StreamServerMessage::Error {
            tick,
            code: "INTERNAL_ERROR".into(),
            error: "Internal server error.".into(),
        },
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
    game_over: bool,
}

struct ValueStreamSession<G: GameServer> {
    game: Arc<G>,
    active: Option<ValueSessionData>,
}

impl<G: GameServer> ValueStreamSession<G> {
    async fn apply_step_internal(
        game: &G,
        tick_rate_hz: f64,
        state: Value,
        tick: u64,
        inputs: Vec<TickInput>,
        events: Vec<StreamStepEvent>,
    ) -> Result<
        (
            Value,
            Vec<Value>,
            bool,
            Option<bool>,
            Option<HashMap<String, Value>>,
            Vec<GameResultEvent>,
            GameResult,
        ),
        SdkError,
    > {
        let mut cur_state = state;
        let mut broadcasts = Vec::new();
        let mut result_units = Vec::new();
        let mut joinable = None;
        let mut player_views = None;

        for ev in events {
            match ev {
                StreamStepEvent::Join { player_id } => {
                    if player_id.trim().is_empty() {
                        return Err(SdkError::Validation("playerId must be non-empty.".into()));
                    }
                    let req = PlayerEventRequest {
                        player_id,
                        state: cur_state,
                        extra: HashMap::new(),
                    };
                    let res = guard_unwind(game.on_player_join_ext(req)).await?;
                    validate_game_result_units(res.result_units.as_deref(), None)?;
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
                    if player_id.trim().is_empty() {
                        return Err(SdkError::Validation("playerId must be non-empty.".into()));
                    }
                    let req = PlayerEventRequest {
                        player_id,
                        state: cur_state,
                        extra: HashMap::new(),
                    };
                    let res = guard_unwind(game.on_player_leave_ext(req)).await?;
                    validate_game_result_units(res.result_units.as_deref(), None)?;
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
                    validate_game_result_units(res.result_units.as_deref(), None)?;
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

        let ctx = TickContext {
            tick,
            elapsed_seconds: tick as f64 / tick_rate_hz,
            inputs,
            commands: Vec::new(),
        };
        ctx.validate()?;
        let tick_res = guard_unwind(game.tick(ctx, cur_state)).await?;
        validate_game_result_units(tick_res.result_units.as_deref(), None)?;
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

        Ok((
            cur_state,
            broadcasts,
            tick_res.persist,
            joinable,
            player_views,
            result_units,
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
                replay,
                resync_players: _,
            } => {
                if !tick_rate_hz.is_finite() || !(1.0..=60.0).contains(&tick_rate_hz) {
                    return error_frame(
                        Some(tick),
                        SdkError::Validation("tickRateHz must be between 1 and 60.".into()),
                    );
                }
                let mut cur_tick = tick;
                let mut cur_state = state;
                let mut all_broadcasts = Vec::new();
                let mut all_units = Vec::new();
                let mut joinable = None;
                let mut last_views = None;
                let had_replay = !replay.is_empty();

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
                    if game_result.over {
                        break;
                    }
                    if step.tick != cur_tick + 1 {
                        return error_frame(
                            Some(step.tick),
                            SdkError::Validation("Replay ticks must be contiguous.".into()),
                        );
                    }
                    match Self::apply_step_internal(
                        &self.game,
                        tick_rate_hz,
                        cur_state,
                        step.tick,
                        step.inputs,
                        step.events,
                    )
                    .await
                    {
                        Ok((next_s, b, _persist, j, views, units, gr)) => {
                            cur_tick = step.tick;
                            cur_state = next_s;
                            all_broadcasts.extend(b);
                            all_units.extend(units);
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
                    game_result,
                }
            }
            StreamClientMessage::Step {
                tick,
                inputs,
                events,
                resync_players: _,
                want_state,
            } => {
                let Some(session) = self.active.as_mut() else {
                    return error_frame(
                        Some(tick),
                        SdkError::Validation("Stream session is not hydrated.".into()),
                    );
                };
                if session.game_over {
                    return StreamServerMessage::IgnoredAfterGameOver { tick };
                }
                if tick != session.tick + 1 {
                    return error_frame(
                        Some(tick),
                        SdkError::Validation(format!(
                            "Expected tick {}, got {tick}.",
                            session.tick + 1
                        )),
                    );
                }
                let had_events = !events.is_empty();
                let prev_state = std::mem::replace(&mut session.state, Value::Null);
                match Self::apply_step_internal(
                    &self.game,
                    session.tick_rate_hz,
                    prev_state,
                    tick,
                    inputs,
                    events,
                )
                .await
                {
                    Ok((next_state, broadcasts, persist, joinable, player_views, units, gr)) => {
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
                        StreamServerMessage::StepResult {
                            tick,
                            state: out_state,
                            player_views,
                            broadcasts,
                            persist,
                            joinable,
                            result_units: units,
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
        tick_rate_hz: f64,
        state: &mut T::State,
        view_ctx: &mut T::ViewContext,
        tick: u64,
        inputs: Vec<TickInput>,
        events: Vec<StreamStepEvent>,
        resync_players: &[String],
        emit_views: bool,
    ) -> Result<
        (
            Vec<Value>,
            bool,
            Option<bool>,
            Option<HashMap<String, Value>>,
            Vec<GameResultEvent>,
            GameResult,
        ),
        SdkError,
    > {
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
                    if player_id.trim().is_empty() {
                        return Err(SdkError::Validation("playerId must be non-empty.".into()));
                    }
                    game.reset_player_view(view_ctx, &player_id);
                    let out = guard_unwind(game.on_player_join_typed(&player_id, state, view_ctx))
                        .await?;
                    validate_game_result_units(out.result_units.as_deref(), None)?;
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
                    if player_id.trim().is_empty() {
                        return Err(SdkError::Validation("playerId must be non-empty.".into()));
                    }
                    game.reset_player_view(view_ctx, &player_id);
                    let out = guard_unwind(game.on_player_leave_typed(&player_id, state, view_ctx))
                        .await?;
                    validate_game_result_units(out.result_units.as_deref(), None)?;
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
                    validate_game_result_units(out.result_units.as_deref(), None)?;
                    if let Some(b) = out.broadcasts {
                        broadcasts.extend(b);
                    }
                    if let Some(u) = out.result_units {
                        result_units.extend(u);
                    }
                }
            }
        }

        let ctx = TickContext {
            tick,
            elapsed_seconds: tick as f64 / tick_rate_hz,
            inputs,
            commands: Vec::new(),
        };
        ctx.validate()?;
        let tick_out = guard_unwind(game.step_typed(ctx, state, view_ctx, emit_views)).await?;
        validate_game_result_units(tick_out.result_units.as_deref(), None)?;
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

        Ok((
            broadcasts,
            tick_out.persist,
            joinable,
            player_views,
            result_units,
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
                replay,
                resync_players,
            } => {
                if !tick_rate_hz.is_finite() || !(1.0..=60.0).contains(&tick_rate_hz) {
                    return error_frame(
                        Some(tick),
                        SdkError::Validation("tickRateHz must be between 1 and 60.".into()),
                    );
                }
                let mut typed_state: T::State = match serde_json::from_value(state_val) {
                    Ok(s) => s,
                    Err(e) => {
                        return error_frame(
                            Some(tick),
                            SdkError::Validation(format!("Invalid checkpoint state: {e}")),
                        )
                    }
                };
                let mut view_ctx = T::ViewContext::default();
                for pid in &resync_players {
                    self.game.reset_player_view(&mut view_ctx, pid);
                }
                let mut cur_tick = tick;
                let mut all_broadcasts = Vec::new();
                let mut all_units = Vec::new();
                let mut joinable = None;
                let mut last_views = None;
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
                    if game_result.over {
                        break;
                    }
                    if step.tick != cur_tick + 1 {
                        return error_frame(
                            Some(step.tick),
                            SdkError::Validation("Replay ticks must be contiguous.".into()),
                        );
                    }
                    let is_last = idx + 1 == replay_len;
                    match Self::apply_typed_step(
                        &self.game,
                        tick_rate_hz,
                        &mut typed_state,
                        &mut view_ctx,
                        step.tick,
                        step.inputs,
                        step.events,
                        &[],
                        is_last,
                    )
                    .await
                    {
                        Ok((b, _persist, j, views, units, gr)) => {
                            cur_tick = step.tick;
                            all_broadcasts.extend(b);
                            all_units.extend(units);
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
                        SdkError::Validation("Stream session is not hydrated.".into()),
                    );
                };
                if session.game_over {
                    return StreamServerMessage::IgnoredAfterGameOver { tick };
                }
                if tick != session.tick + 1 {
                    return error_frame(
                        Some(tick),
                        SdkError::Validation(format!(
                            "Expected tick {}, got {tick}.",
                            session.tick + 1
                        )),
                    );
                }
                let had_events = !events.is_empty();
                match Self::apply_typed_step(
                    &self.game,
                    session.tick_rate_hz,
                    &mut session.state,
                    &mut session.view_ctx,
                    tick,
                    inputs,
                    events,
                    &resync_players,
                    true,
                )
                .await
                {
                    Ok((broadcasts, persist, joinable, player_views, units, gr)) => {
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
                        session.game_over = gr.over;
                        StreamServerMessage::StepResult {
                            tick,
                            state: out_state,
                            player_views,
                            broadcasts,
                            persist,
                            joinable,
                            result_units: units,
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
                        &error_frame(None, SdkError::Validation("Expected UTF-8 JSON.".into())),
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
                    SdkError::Validation("Stream frame must not exceed 1 MiB.".into()),
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
                    &error_frame(
                        None,
                        SdkError::Validation("Invalid stream JSON message.".into()),
                    ),
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
