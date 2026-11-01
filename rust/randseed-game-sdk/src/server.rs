//! Room-host protocol server implementation for the RandSeed game server SDK.
//!
//! Builds a container-side HTTP service with Axum and Tokio.

use crate::agent_capabilities::AgentGameCapabilities;
use crate::errors::SdkError;
use crate::types::{
    validate_game_result_units, CommandRequest, CommandResult, GameCommand, GameManifest,
    GameResult, GameResultEvent, HealthResponse, InitResult, IsGameOverRequest, MatchConfig,
    PlayerEventRequest, PlayerEventResult, TickContext, TickRequest, TickResult,
};
use async_trait::async_trait;
use axum::{
    extract::{DefaultBodyLimit, FromRequest, Request, State},
    http::{Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::FutureExt;
use serde::de::DeserializeOwned;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// Interface implemented by a game server.
///
/// Lifecycle methods should calculate results from the supplied state and
/// context so the room host can replay them deterministically.
#[async_trait]
pub trait GameServer: Send + Sync + 'static {
    /// Explicit Agent support; games without a declaration are not opted in.
    async fn agent_capabilities(&self) -> Result<Option<AgentGameCapabilities>, SdkError> {
        Ok(None)
    }

    /// Return game metadata for `GET /manifest`.
    async fn manifest(&self) -> Result<GameManifest, SdkError>;

    /// Initialize a match or a reused room.
    async fn init(&self, config: MatchConfig) -> Result<InitResult, SdkError>;

    /// Process one deterministic simulation tick.
    async fn tick(
        &self,
        ctx: TickContext,
        state: serde_json::Value,
    ) -> Result<TickResult, SdkError>;

    /// Handle a player joining or reconnecting.
    async fn on_player_join(
        &self,
        player_id: String,
        state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError>;

    /// Handle a player leaving.
    async fn on_player_leave(
        &self,
        player_id: String,
        state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError>;

    /// Determine whether the match has ended and, if so, return a next-match policy.
    async fn is_game_over(&self, state: serde_json::Value) -> Result<GameResult, SdkError>;

    /// Handle a trusted host command. The default implementation is unsupported.
    async fn on_command(
        &self,
        _cmd: GameCommand,
        _state: serde_json::Value,
    ) -> Result<CommandResult, SdkError> {
        Err(SdkError::NotImplemented(
            "The on_command handler is not implemented.".into(),
        ))
    }
}

/// Shared state used by the internal routes.
struct AppState<G: GameServer> {
    game: Arc<G>,
    game_id: String,
    start_time: Instant,
}

/// Error response returned by protocol endpoints.
#[derive(serde::Serialize)]
struct ErrorResponse {
    error: String,
}

struct ProtocolJson<T>(T);

impl<S, T> FromRequest<S> for ProtocolJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = Response;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|rejection| {
                let status = rejection.into_response().status();
                if status == StatusCode::PAYLOAD_TOO_LARGE {
                    (
                        StatusCode::PAYLOAD_TOO_LARGE,
                        Json(ErrorResponse {
                            error: "Request body must not exceed 1 MiB.".into(),
                        }),
                    )
                        .into_response()
                } else {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "Invalid JSON request body.".into(),
                        }),
                    )
                        .into_response()
                }
            })
    }
}

fn map_error_to_response(err: SdkError) -> (StatusCode, Json<ErrorResponse>) {
    match err {
        SdkError::Validation(msg) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: msg })),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error.".into(),
            }),
        ),
    }
}

fn invalid_result_units_response() -> Response {
    tracing::error!("Game callback returned invalid result units.");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "Game server returned invalid result units.".into(),
        }),
    )
        .into_response()
}

fn result_units_are_valid(result_units: Option<&[GameResultEvent]>) -> bool {
    validate_game_result_units(result_units, None).is_ok()
}

async fn call_game<T>(future: impl Future<Output = Result<T, SdkError>>) -> Result<T, SdkError> {
    match AssertUnwindSafe(future).catch_unwind().await {
        Ok(result) => result,
        Err(_) => Err(SdkError::Internal("Game callback panicked.".into())),
    }
}

async fn handle_manifest<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
) -> impl IntoResponse {
    match call_game(state.game.manifest()).await {
        Ok(manifest) => {
            if manifest.validate().is_err() {
                return map_error_to_response(SdkError::Internal(
                    "The game returned an invalid manifest.".into(),
                ))
                .into_response();
            }
            (StatusCode::OK, Json(manifest)).into_response()
        }
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_health<G: GameServer>(State(state): State<Arc<AppState<G>>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs_f64();
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok".into(),
            game_id: state.game_id.clone(),
            uptime: Some(uptime),
        }),
    )
        .into_response()
}

async fn handle_agent_capabilities<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
) -> impl IntoResponse {
    match call_game(state.game.agent_capabilities()).await {
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Agent support is not enabled for this game.".into(),
            }),
        )
            .into_response(),
        Ok(Some(capabilities)) if capabilities.validate().is_ok() => {
            (StatusCode::OK, Json(capabilities)).into_response()
        }
        _ => {
            tracing::error!("Agent capability declaration failed validation or execution.");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Agent capabilities are unavailable.".into(),
                }),
            )
                .into_response()
        }
    }
}

async fn handle_init<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(config): ProtocolJson<MatchConfig>,
) -> impl IntoResponse {
    if let Err(e) = config.validate() {
        return map_error_to_response(e).into_response();
    }
    match call_game(state.game.init(config)).await {
        Ok(res) if result_units_are_valid(res.result_units.as_deref()) => {
            (StatusCode::OK, Json(res)).into_response()
        }
        Ok(_) => invalid_result_units_response(),
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_tick<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(req): ProtocolJson<TickRequest>,
) -> impl IntoResponse {
    if let Err(e) = req.ctx.validate() {
        return map_error_to_response(e).into_response();
    }
    match call_game(state.game.tick(req.ctx, req.state)).await {
        Ok(res) if result_units_are_valid(res.result_units.as_deref()) => {
            (StatusCode::OK, Json(res)).into_response()
        }
        Ok(_) => invalid_result_units_response(),
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_player_join<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(req): ProtocolJson<PlayerEventRequest>,
) -> impl IntoResponse {
    if req.player_id.trim().is_empty() {
        return map_error_to_response(SdkError::Validation(
            "playerId must be a non-empty string.".into(),
        ))
        .into_response();
    }
    match call_game(state.game.on_player_join(req.player_id, req.state)).await {
        Ok(res) if result_units_are_valid(res.result_units.as_deref()) => {
            (StatusCode::OK, Json(res)).into_response()
        }
        Ok(_) => invalid_result_units_response(),
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_player_leave<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(req): ProtocolJson<PlayerEventRequest>,
) -> impl IntoResponse {
    if req.player_id.trim().is_empty() {
        return map_error_to_response(SdkError::Validation(
            "playerId must be a non-empty string.".into(),
        ))
        .into_response();
    }
    match call_game(state.game.on_player_leave(req.player_id, req.state)).await {
        Ok(res) if result_units_are_valid(res.result_units.as_deref()) => {
            (StatusCode::OK, Json(res)).into_response()
        }
        Ok(_) => invalid_result_units_response(),
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_is_game_over<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(req): ProtocolJson<IsGameOverRequest>,
) -> impl IntoResponse {
    match call_game(state.game.is_game_over(req.state)).await {
        Ok(res) => {
            if !result_units_are_valid(res.result_units.as_deref()) {
                return invalid_result_units_response();
            }
            if res.over {
                let policy_is_supported = res.next_match_policy.as_ref().is_some_and(|policy| {
                    matches!(
                        policy.mode.as_str(),
                        "auto_next_match" | "wait_for_ready" | "close_room"
                    )
                });
                let summary_is_valid = res.summary.as_ref().is_some_and(|summary| {
                    summary.duration_seconds.is_finite() && summary.duration_seconds >= 0.0
                });
                if !summary_is_valid || !policy_is_supported {
                    return map_error_to_response(SdkError::Internal(
                        "The game returned an invalid game-over result.".into(),
                    ))
                    .into_response();
                }
            }
            (StatusCode::OK, Json(res)).into_response()
        }
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_command<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(req): ProtocolJson<CommandRequest>,
) -> impl IntoResponse {
    if req.command.command_type.trim().is_empty() {
        return map_error_to_response(SdkError::Validation(
            "command.type must be a non-empty string.".into(),
        ))
        .into_response();
    }
    match call_game(state.game.on_command(req.command, req.state)).await {
        Ok(res) if result_units_are_valid(res.result_units.as_deref()) => {
            (StatusCode::OK, Json(res)).into_response()
        }
        Ok(_) => invalid_result_units_response(),
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_fallback(method: Method, uri: Uri) -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("Endpoint not found: {} {}", method, uri.path()),
        }),
    )
}

/// Create the Axum router for the room-host protocol.
///
/// The returned router can be used in tests or served over TCP.
pub fn create_game_router<G: GameServer>(game: Arc<G>, game_id: Option<String>) -> Router {
    let app_state = Arc::new(AppState {
        game,
        game_id: game_id.unwrap_or_else(|| "randseed-game".into()),
        start_time: Instant::now(),
    });

    Router::new()
        .route("/manifest", get(handle_manifest::<G>))
        .route("/agent/capabilities", get(handle_agent_capabilities::<G>))
        .route("/health", get(handle_health::<G>))
        .route("/init", post(handle_init::<G>))
        .route("/tick", post(handle_tick::<G>))
        .route("/onPlayerJoin", post(handle_player_join::<G>))
        .route("/onPlayerLeave", post(handle_player_leave::<G>))
        .route("/isGameOver", post(handle_is_game_over::<G>))
        .route("/onCommand", post(handle_command::<G>))
        .fallback(handle_fallback)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(app_state)
}

/// Start the game server on `0.0.0.0:{port}`.
pub async fn run_game_server<G: GameServer>(game: G, port: u16) -> Result<(), SdkError> {
    let router = create_game_router(Arc::new(game), None);
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| SdkError::Network(format!("Failed to bind port {port}: {e}")))?;

    tracing::info!("RandSeed game server listening on {addr}");

    axum::serve(listener, router)
        .await
        .map_err(|e| SdkError::Internal(format!("The game server failed: {e}")))?;

    Ok(())
}
