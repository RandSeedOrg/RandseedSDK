//! Room-host protocol server implementation for the RandSeed game server SDK.
//!
//! Builds a container-side HTTP service with Axum and Tokio.

use crate::agent_capabilities::AgentGameCapabilities;
use crate::agent_player_view::{
    create_agent_player_view, AgentPlayerContract, AgentPlayerViewProjection,
    AgentPlayerViewRequest,
};
use crate::errors::SdkError;
use crate::stream::{
    serve_stream_socket, StreamDurability, StreamSessionFactory, TypedGameAdapter, TypedGameServer,
    TypedStreamFactory, ValueStreamFactory, STREAM_PROTOCOL_V1,
};
use crate::types::{
    validate_action_results_contract, validate_game_result_units, CommandRequest, CommandResult,
    GameCommand, GameManifest, GameResult, GameResultEvent, HealthResponse, InitResult,
    IsGameOverRequest, MatchConfig, PlayerEventRequest, PlayerEventResult, TickContext,
    TickRequest, TickResult,
};
use async_trait::async_trait;
use axum::{
    extract::{ws::WebSocketUpgrade, DefaultBodyLimit, FromRequest, Request, State},
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

    /// Declare the authoritative per-seat Agent view and action contract.
    async fn agent_player_contract(&self) -> Result<Option<AgentPlayerContract>, SdkError> {
        Ok(None)
    }

    /// Project a recipient-safe view from canonical state and committed events.
    async fn project_agent_player_view(
        &self,
        _request: AgentPlayerViewRequest,
    ) -> Result<AgentPlayerViewProjection, SdkError> {
        Err(SdkError::NotImplemented(
            "Agent player projection is not implemented.".into(),
        ))
    }

    /// Optional stream durability policy (`None` uses platform default by `tickRateHz`).
    fn durability(&self) -> Option<StreamDurability> {
        None
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

    /// Handle a player joining or reconnecting with full request context (including extension fields).
    async fn on_player_join_ext(
        &self,
        req: PlayerEventRequest,
    ) -> Result<PlayerEventResult, SdkError> {
        self.on_player_join(req.player_id, req.state).await
    }

    /// Handle a player leaving.
    async fn on_player_leave(
        &self,
        player_id: String,
        state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError>;

    /// Handle a player leaving with full request context (including extension fields).
    async fn on_player_leave_ext(
        &self,
        req: PlayerEventRequest,
    ) -> Result<PlayerEventResult, SdkError> {
        self.on_player_leave(req.player_id, req.state).await
    }

    /// Determine whether the match has ended and, if so, return a next-match policy.
    async fn is_game_over(&self, state: serde_json::Value) -> Result<GameResult, SdkError>;

    /// Determine whether the match has ended with full request context (including extension fields).
    async fn is_game_over_ext(&self, req: IsGameOverRequest) -> Result<GameResult, SdkError> {
        self.is_game_over(req.state).await
    }

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
    stream_factory: Arc<dyn StreamSessionFactory>,
    game_id: Option<String>,
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

fn invalid_action_results_response() -> Response {
    tracing::error!("Game callback returned invalid action results.");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "Game server returned invalid action results.".into(),
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
            let mut resp = (StatusCode::OK, Json(manifest)).into_response();
            resp.headers_mut().insert(
                "x-randseed-stream-protocol",
                axum::http::HeaderValue::from_static(STREAM_PROTOCOL_V1),
            );
            if let Some(durability) = state.game.durability() {
                let val = match durability {
                    StreamDurability::Checkpoint => "checkpoint",
                    StreamDurability::Strict => "strict",
                };
                resp.headers_mut().insert(
                    "x-randseed-stream-durability",
                    axum::http::HeaderValue::from_static(val),
                );
            }
            resp
        }
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_stream<G: GameServer>(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState<G>>>,
) -> impl IntoResponse {
    let factory = state.stream_factory.clone();
    let durability = state.game.durability();
    let mut resp = ws
        .max_message_size(MAX_REQUEST_BODY_BYTES)
        .on_upgrade(move |socket| serve_stream_socket(socket, factory))
        .into_response();
    resp.headers_mut().insert(
        "x-randseed-stream-protocol",
        axum::http::HeaderValue::from_static(STREAM_PROTOCOL_V1),
    );
    if let Some(mode) = durability {
        let val = match mode {
            StreamDurability::Checkpoint => "checkpoint",
            StreamDurability::Strict => "strict",
        };
        resp.headers_mut().insert(
            "x-randseed-stream-durability",
            axum::http::HeaderValue::from_static(val),
        );
    }
    resp
}

async fn handle_health<G: GameServer>(State(state): State<Arc<AppState<G>>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs_f64();
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok".into(),
            game_id: state
                .game_id
                .clone()
                .unwrap_or_else(|| "randseed-game".into()),
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

async fn handle_agent_player_contract<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
) -> impl IntoResponse {
    match call_game(state.game.agent_player_contract()).await {
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Authoritative Agent player views are not enabled for this game.".into(),
            }),
        )
            .into_response(),
        Ok(Some(contract)) if contract.validate().is_ok() => {
            (StatusCode::OK, Json(contract)).into_response()
        }
        _ => {
            tracing::error!("Agent player contract failed validation or execution.");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Agent player contract is unavailable.".into(),
                }),
            )
                .into_response()
        }
    }
}

async fn handle_agent_player_view<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(request): ProtocolJson<AgentPlayerViewRequest>,
) -> impl IntoResponse {
    if request.validate().is_err()
        || state
            .game_id
            .as_deref()
            .is_some_and(|game_id| request.context.scope.game_id != game_id)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid Agent player view request.".into(),
            }),
        )
            .into_response();
    }
    let contract = match call_game(state.game.agent_player_contract()).await {
        Ok(Some(contract)) if contract.validate().is_ok() => contract,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Authoritative Agent player views are not enabled for this game.".into(),
                }),
            )
                .into_response();
        }
        _ => {
            tracing::error!("Agent player contract failed validation or execution.");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Agent player contract is unavailable.".into(),
                }),
            )
                .into_response();
        }
    };
    let projection = match call_game(state.game.project_agent_player_view(request.clone())).await {
        Ok(projection) => projection,
        Err(_) => {
            tracing::error!("Agent player projection failed.");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Agent player view is unavailable.".into(),
                }),
            )
                .into_response();
        }
    };
    match create_agent_player_view(&contract, &request, projection) {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(_) => {
            tracing::error!("Agent player projection failed validation.");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Agent player view is unavailable.".into(),
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
    let expected_action_ids = req
        .ctx
        .inputs
        .iter()
        .filter_map(|input| input.action_id.clone())
        .collect::<Vec<_>>();
    match call_game(state.game.tick(req.ctx, req.state)).await {
        Ok(res) if !result_units_are_valid(res.result_units.as_deref()) => {
            invalid_result_units_response()
        }
        Ok(res)
            if validate_action_results_contract(
                res.action_results.as_deref(),
                &expected_action_ids,
            )
            .is_err() =>
        {
            invalid_action_results_response()
        }
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_player_join<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(mut req): ProtocolJson<PlayerEventRequest>,
) -> impl IntoResponse {
    let Some(resolved_id) = req.resolved_player_id() else {
        return map_error_to_response(SdkError::Validation(
            "playerId must be a non-empty string.".into(),
        ))
        .into_response();
    };
    req.player_id = resolved_id;
    match call_game(state.game.on_player_join_ext(req)).await {
        Ok(res) if result_units_are_valid(res.result_units.as_deref()) => {
            (StatusCode::OK, Json(res)).into_response()
        }
        Ok(_) => invalid_result_units_response(),
        Err(err) => map_error_to_response(err).into_response(),
    }
}

async fn handle_player_leave<G: GameServer>(
    State(state): State<Arc<AppState<G>>>,
    ProtocolJson(mut req): ProtocolJson<PlayerEventRequest>,
) -> impl IntoResponse {
    let Some(resolved_id) = req.resolved_player_id() else {
        return map_error_to_response(SdkError::Validation(
            "playerId must be a non-empty string.".into(),
        ))
        .into_response();
    };
    req.player_id = resolved_id;
    match call_game(state.game.on_player_leave_ext(req)).await {
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
    match call_game(state.game.is_game_over_ext(req)).await {
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

fn build_router_with_factory<G: GameServer>(
    game: Arc<G>,
    stream_factory: Arc<dyn StreamSessionFactory>,
    game_id: Option<String>,
) -> Router {
    let app_state = Arc::new(AppState {
        game,
        stream_factory,
        game_id,
        start_time: Instant::now(),
    });

    Router::new()
        .route("/manifest", get(handle_manifest::<G>))
        .route("/stream", get(handle_stream::<G>))
        .route("/agent/capabilities", get(handle_agent_capabilities::<G>))
        .route(
            "/agent/player-contract",
            get(handle_agent_player_contract::<G>),
        )
        .route("/agent/player-view", post(handle_agent_player_view::<G>))
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

/// Create the Axum router for the room-host protocol (both HTTP endpoints and `/stream`).
///
/// The returned router can be used in tests or served over TCP.
pub fn create_game_router<G: GameServer>(game: Arc<G>, game_id: Option<String>) -> Router {
    let factory = Arc::new(ValueStreamFactory(game.clone()));
    build_router_with_factory(game, factory, game_id)
}

/// Create the Axum router for a strongly typed [`TypedGameServer`].
///
/// Keeps native `T::State` in memory across `/stream` ticks without per-tick
/// `serde_json::Value` serialization while preserving all 8 HTTP endpoints.
pub fn create_typed_game_router<T: TypedGameServer>(
    game: Arc<T>,
    game_id: Option<String>,
) -> Router {
    let adapter = Arc::new(TypedGameAdapter(game.clone()));
    let factory = Arc::new(TypedStreamFactory(game));
    build_router_with_factory(adapter, factory, game_id)
}

/// Create the Axum router pairing a [`GameServer`] for HTTP endpoints with a
/// zero-`Value` [`TypedGameServer`] session runner for `/stream`.
pub fn create_game_router_with_stream<G, T>(
    http_game: Arc<G>,
    stream_game: Arc<T>,
    game_id: Option<String>,
) -> Router
where
    G: GameServer,
    T: TypedGameServer,
{
    let factory = Arc::new(TypedStreamFactory(stream_game));
    build_router_with_factory(http_game, factory, game_id)
}

/// Start the game server on `0.0.0.0:{port}`.
pub async fn run_game_server<G: GameServer>(game: G, port: u16) -> Result<(), SdkError> {
    let router = create_game_router(Arc::new(game), None);
    serve_router_on_port(router, port).await
}

/// Start a strongly typed [`TypedGameServer`] on `0.0.0.0:{port}`.
pub async fn run_typed_game_server<T: TypedGameServer>(game: T, port: u16) -> Result<(), SdkError> {
    let router = create_typed_game_router(Arc::new(game), None);
    serve_router_on_port(router, port).await
}

async fn serve_router_on_port(router: Router, port: u16) -> Result<(), SdkError> {
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
