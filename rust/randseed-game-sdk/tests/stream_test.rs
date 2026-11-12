//! Integration tests for `Session Stream v1` (`/stream`) and `TypedGameServer`.

use async_trait::async_trait;
use randseed_game_sdk::stream::{
    ActiveStreamSession, StreamSessionFactory, TypedStreamFactory, ValueStreamFactory,
};
use randseed_game_sdk::{
    create_game_router, create_typed_game_router, AgentPlayerContract, AgentPlayerDecisionState,
    AgentPlayerMode, AgentPlayerViewContext, AgentPlayerViewProjection, GameActionOutcome,
    GameActionResult, GameCommand, GameManifest, GameResult, GameServer, InitResult, MatchConfig,
    MatchSummary, NextMatchPolicy, PlayerEventResult, PlayerViewMode, SdkError,
    StreamClientMessage, StreamDurability, StreamReplayStep, StreamServerMessage, StreamStepEvent,
    TickContext, TickInput, TickResult, TypedEventOutcome, TypedGameServer, TypedInitResult,
    TypedTickResult, STREAM_PROTOCOL_V1,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn connect_raw_websocket(address: SocketAddr) -> TcpStream {
    let mut socket = TcpStream::connect(address).await.unwrap();
    let handshake = format!(
        "GET /stream HTTP/1.1\r\n\
         Host: {address}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    socket.write_all(handshake.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        let mut byte = [0; 1];
        socket.read_exact(&mut byte).await.unwrap();
        response.push(byte[0]);
        assert!(
            response.len() < 8192,
            "WebSocket handshake headers are too large"
        );
    }
    let response = String::from_utf8(response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 101 Switching Protocols"),
        "WebSocket upgrade failed: {response}"
    );
    assert!(response
        .to_ascii_lowercase()
        .contains("x-randseed-stream-protocol: session-v1"));
    socket
}

fn masked_text_frame(payload: &str) -> Vec<u8> {
    let payload = payload.as_bytes();
    let mask = [0x12, 0x34, 0x56, 0x78];
    let mut frame = Vec::with_capacity(payload.len() + 8);
    frame.push(0x81);
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| *byte ^ mask[index % 4]),
    );
    frame
}

async fn send_websocket_json(socket: &mut TcpStream, value: &serde_json::Value) {
    let payload = serde_json::to_string(value).unwrap();
    socket
        .write_all(&masked_text_frame(&payload))
        .await
        .unwrap();
}

async fn read_websocket_json(socket: &mut TcpStream) -> serde_json::Value {
    loop {
        let mut header = [0; 2];
        socket.read_exact(&mut header).await.unwrap();
        assert_eq!(
            header[1] & 0x80,
            0,
            "server WebSocket frames must not be masked"
        );
        let mut payload_length = (header[1] & 0x7f) as u64;
        if payload_length == 126 {
            let mut extended = [0; 2];
            socket.read_exact(&mut extended).await.unwrap();
            payload_length = u16::from_be_bytes(extended) as u64;
        } else if payload_length == 127 {
            let mut extended = [0; 8];
            socket.read_exact(&mut extended).await.unwrap();
            payload_length = u64::from_be_bytes(extended);
        }
        assert!(payload_length <= 1024 * 1024);
        let mut payload = vec![0; payload_length as usize];
        socket.read_exact(&mut payload).await.unwrap();
        match header[0] & 0x0f {
            0x01 => return serde_json::from_slice(&payload).unwrap(),
            0x08 => panic!("server closed the WebSocket while the preflight was running"),
            0x09 => continue,
            opcode => panic!("unexpected server WebSocket opcode: {opcode}"),
        }
    }
}

#[test]
fn session_stream_fixture_round_trips_in_rust() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/session-stream-v1.json")).unwrap();
    assert_eq!(fixture["protocolVersion"], 1);
    for key in ["hydrateRequest", "stepRequest"] {
        let message: StreamClientMessage = serde_json::from_value(fixture[key].clone()).unwrap();
        assert_eq!(serde_json::to_value(message).unwrap(), fixture[key]);
    }
    let response: StreamServerMessage =
        serde_json::from_value(fixture["stepResult"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(response).unwrap(),
        fixture["stepResult"]
    );
    assert!(serde_json::from_value::<StreamClientMessage>(json!({
        "type": "step",
        "tick": 1,
        "events": [{ "type": "join" }]
    }))
    .is_err());
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CounterState {
    tick: u64,
    score: i64,
    players: Vec<String>,
}

#[derive(Default)]
struct CounterViewCtx {
    initialized_players: HashSet<String>,
}

struct TypedCounterGame;

#[async_trait]
impl TypedGameServer for TypedCounterGame {
    type State = CounterState;
    type ViewContext = CounterViewCtx;

    fn durability(&self) -> Option<StreamDurability> {
        Some(StreamDurability::Checkpoint)
    }

    async fn agent_player_contract(&self) -> Result<Option<AgentPlayerContract>, SdkError> {
        Ok(Some(AgentPlayerContract {
            player_view_protocol_version: 1,
            action_protocol_version: 1,
            player_view_schema_id: "counter.view.v1".into(),
            action_schema_id: "counter.action.v1".into(),
            modes: vec![AgentPlayerMode::Follow, AgentPlayerMode::Autoplay],
            max_decision_hz: 5.0,
            independent_seats: true,
            background_autoplay: false,
        }))
    }

    async fn project_agent_player_view_typed(
        &self,
        context: &AgentPlayerViewContext,
        _players: &[String],
        state: &Self::State,
        committed_events: &[serde_json::Value],
    ) -> Result<AgentPlayerViewProjection, SdkError> {
        Ok(AgentPlayerViewProjection {
            public_state: json!({
                "tick": state.tick,
                "score": state.score,
            }),
            private_state: json!({
                "seat": context.player_id,
            }),
            visible_events: committed_events.to_vec(),
            decision: AgentPlayerDecisionState {
                can_act: state.score < 10,
                deadline_at: None,
            },
        })
    }

    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        Ok(GameManifest {
            title: "Typed Stream Counter".into(),
            version: "1.0.0".into(),
            min_players: 1,
            max_players: 8,
            tick_rate_hz: 20.0,
            persistent_table: false,
            options_schema: None,
            player_view_mode: None,
            result_definition: None,
        })
    }

    async fn init_typed(
        &self,
        config: MatchConfig,
    ) -> Result<TypedInitResult<Self::State>, SdkError> {
        Ok(TypedInitResult {
            state: CounterState {
                tick: 0,
                score: 0,
                players: config.players,
            },
            broadcasts: None,
            result_units: None,
            joinable: Some(true),
            player_views: None,
        })
    }

    fn reset_player_view(&self, view_ctx: &mut Self::ViewContext, player_id: &str) {
        view_ctx.initialized_players.remove(player_id);
    }

    async fn step_typed(
        &self,
        ctx: TickContext,
        state: &mut Self::State,
        view_ctx: &mut Self::ViewContext,
        emit_views: bool,
    ) -> Result<TypedTickResult, SdkError> {
        state.tick = ctx.tick;
        let mut action_results = Vec::new();
        for input in &ctx.inputs {
            if let Some(delta) = input.data.get("delta").and_then(|v| v.as_i64()) {
                state.score += delta;
                if let Some(action_id) = &input.action_id {
                    action_results.push(GameActionResult {
                        action_id: action_id.clone(),
                        status: GameActionOutcome::Applied,
                        code: None,
                    });
                }
            } else if let Some(action_id) = &input.action_id {
                action_results.push(GameActionResult {
                    action_id: action_id.clone(),
                    status: GameActionOutcome::Rejected,
                    code: Some("missing_delta".into()),
                });
            }
        }
        let player_views = if emit_views {
            let mut views = HashMap::new();
            for p in &state.players {
                let is_full = view_ctx.initialized_players.insert(p.clone());
                views.insert(
                    p.clone(),
                    json!({
                        "tick": state.tick,
                        "score": state.score,
                        "mode": if is_full { "full" } else { "delta" },
                    }),
                );
            }
            Some(views)
        } else {
            None
        };
        Ok(TypedTickResult {
            broadcasts: Some(Vec::new()),
            persist: state.tick % 5 == 0,
            result_units: None,
            joinable: Some(state.players.len() < 8),
            player_views,
            action_results: if action_results.is_empty() {
                None
            } else {
                Some(action_results)
            },
        })
    }

    async fn on_player_join_typed(
        &self,
        player_id: &str,
        state: &mut Self::State,
        _view_ctx: &mut Self::ViewContext,
    ) -> Result<TypedEventOutcome, SdkError> {
        if !state.players.iter().any(|p| p == player_id) {
            state.players.push(player_id.to_string());
        }
        Ok(TypedEventOutcome {
            joinable: Some(state.players.len() < 8),
            ..Default::default()
        })
    }

    async fn is_game_over_typed(&self, state: &Self::State) -> Result<GameResult, SdkError> {
        if state.score >= 10 {
            Ok(GameResult {
                over: true,
                summary: Some(MatchSummary {
                    winner_player_id: None,
                    scores: None,
                    duration_seconds: state.tick as f64 / 20.0,
                    custom_metrics: None,
                    extra: None,
                }),
                next_match_policy: Some(NextMatchPolicy {
                    mode: "close_room".into(),
                    timeout_ms: 0,
                    carryover: None,
                }),
                result_units: None,
            })
        } else {
            Ok(GameResult {
                over: false,
                summary: None,
                next_match_policy: None,
                result_units: None,
            })
        }
    }
    async fn on_command_typed(
        &self,
        command: GameCommand,
        state: &mut Self::State,
        _view_ctx: &mut Self::ViewContext,
    ) -> Result<TypedEventOutcome, SdkError> {
        if command.command_type == "lock_room" {
            let mut views = HashMap::new();
            for p in &state.players {
                views.insert(
                    p.clone(),
                    json!({
                        "tick": state.tick,
                        "score": state.score,
                        "locked": true,
                    }),
                );
            }
            return Ok(TypedEventOutcome {
                broadcasts: Some(vec![json!({ "event": "locked" })]),
                joinable: Some(false),
                player_views: Some(views),
                ..Default::default()
            });
        }
        Ok(TypedEventOutcome::default())
    }
}

struct PrivateViewGame;

fn private_views(state: &serde_json::Value) -> HashMap<String, serde_json::Value> {
    let players = state["players"].as_array().expect("players array");
    players
        .iter()
        .filter_map(|player| player.as_str())
        .map(|player_id| {
            (
                player_id.to_string(),
                json!({
                    "ownSecret": state["secrets"][player_id],
                    "playerId": player_id,
                }),
            )
        })
        .collect()
}

#[async_trait]
impl GameServer for PrivateViewGame {
    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        Ok(GameManifest {
            title: "Private View Test".into(),
            version: "1.0.0".into(),
            min_players: 2,
            max_players: 2,
            tick_rate_hz: 10.0,
            persistent_table: false,
            options_schema: None,
            player_view_mode: Some(PlayerViewMode::Private),
            result_definition: None,
        })
    }

    async fn init(&self, config: MatchConfig) -> Result<InitResult, SdkError> {
        let state = json!({
            "players": config.players,
            "secrets": { "p1": "P1_CANARY", "p2": "P2_CANARY" },
            "tick": 0,
        });
        Ok(InitResult {
            player_views: Some(private_views(&state)),
            state,
            ..Default::default()
        })
    }

    async fn tick(
        &self,
        ctx: TickContext,
        mut state: serde_json::Value,
    ) -> Result<TickResult, SdkError> {
        state["tick"] = json!(ctx.tick);
        let action_results = ctx
            .inputs
            .iter()
            .filter_map(|input| {
                input.action_id.as_ref().map(|action_id| GameActionResult {
                    action_id: action_id.clone(),
                    status: if input.data["reject"].as_bool().unwrap_or(false) {
                        GameActionOutcome::Rejected
                    } else {
                        GameActionOutcome::Applied
                    },
                    code: input.data["reject"]
                        .as_bool()
                        .filter(|rejected| *rejected)
                        .map(|_| "INVALID_MOVE".into()),
                })
            })
            .collect::<Vec<_>>();
        Ok(TickResult {
            player_views: Some(private_views(&state)),
            state,
            persist: false,
            action_results: (!action_results.is_empty()).then_some(action_results),
            ..Default::default()
        })
    }

    async fn on_player_join(
        &self,
        player_id: String,
        mut state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError> {
        let players = state["players"].as_array_mut().expect("players array");
        if !players
            .iter()
            .any(|entry| entry.as_str() == Some(&player_id))
        {
            players.push(json!(player_id));
        }
        Ok(PlayerEventResult {
            player_views: Some(private_views(&state)),
            state,
            ..Default::default()
        })
    }

    async fn on_player_leave(
        &self,
        player_id: String,
        mut state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError> {
        let players = state["players"].as_array_mut().expect("players array");
        players.retain(|entry| entry.as_str() != Some(&player_id));
        Ok(PlayerEventResult {
            player_views: Some(private_views(&state)),
            state,
            ..Default::default()
        })
    }

    async fn is_game_over(&self, state: serde_json::Value) -> Result<GameResult, SdkError> {
        let over = state["tick"].as_u64().unwrap_or_default() >= 3;
        Ok(GameResult {
            over,
            summary: over.then_some(MatchSummary {
                winner_player_id: None,
                scores: None,
                duration_seconds: state["tick"].as_u64().unwrap_or_default() as f64 / 10.0,
                custom_metrics: None,
                extra: None,
            }),
            next_match_policy: over.then_some(NextMatchPolicy {
                mode: "close_room".into(),
                timeout_ms: 0,
                carryover: None,
            }),
            result_units: None,
        })
    }
}

#[tokio::test]
async fn sdk_migration_preflight_covers_http_roster_and_private_stream_recovery() {
    let router = create_game_router(Arc::new(PrivateViewGame), Some("private-game".into()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let client = reqwest::Client::new();
    let manifest: serde_json::Value = client
        .get(format!("http://{address}/manifest"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(manifest["playerViewMode"], "private");

    let config = json!({
        "matchId": "preflight-match",
        "gameId": "private-game",
        "players": ["p1", "p2"],
        "matchSeq": 1
    });
    let invalid_options = client
        .post(format!("http://{address}/init"))
        .json(&json!({
            "matchId": "invalid-options",
            "gameId": "private-game",
            "players": ["p1", "p2"],
            "matchSeq": 1,
            "options": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid_options.status(), 400);
    assert_eq!(
        invalid_options.json::<serde_json::Value>().await.unwrap()["code"],
        "INVALID_OPTIONS"
    );
    let init_response = client
        .post(format!("http://{address}/init"))
        .json(&config)
        .send()
        .await
        .unwrap();
    assert_eq!(init_response.status(), 200);
    let init: serde_json::Value = init_response.json().await.unwrap();
    assert_eq!(init["playerViews"].as_object().unwrap().len(), 2);

    let tick_request = json!({
        "ctx": {
            "tick": 1,
            "elapsedSeconds": 0.1,
            "inputs": [{
                "playerId": "p1",
                "data": { "delta": 1 },
                "actionId": "preflight-http-action",
                "actionSchemaId": "private.action.v1"
            }]
        },
        "state": init["state"],
        "players": ["p1", "p2"]
    });
    let tick_response = client
        .post(format!("http://{address}/tick"))
        .json(&tick_request)
        .send()
        .await
        .unwrap();
    assert_eq!(tick_response.status(), 200);
    let tick: serde_json::Value = tick_response.json().await.unwrap();
    assert_eq!(tick["actionResults"][0]["status"], "applied");
    assert_eq!(tick["playerViews"].as_object().unwrap().len(), 2);

    let wrong_roster = client
        .post(format!("http://{address}/tick"))
        .json(&json!({
            "ctx": {
                "tick": 2,
                "elapsedSeconds": 0.2,
                "inputs": [{ "playerId": "p3", "data": {} }]
            },
            "state": tick["state"],
            "players": ["p1", "p2"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_roster.status(), 400);
    let wrong_roster_body: serde_json::Value = wrong_roster.json().await.unwrap();
    assert_eq!(wrong_roster_body["code"], "WRONG_ROSTER");

    let reconnect = client
        .post(format!("http://{address}/onPlayerJoin"))
        .json(&json!({
            "playerId": "p2",
            "state": tick["state"],
            "players": ["p1", "p2"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reconnect.status(), 200);
    let leave = client
        .post(format!("http://{address}/onPlayerLeave"))
        .json(&json!({
            "playerId": "p2",
            "state": reconnect.json::<serde_json::Value>().await.unwrap()["state"],
            "players": ["p1"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(leave.status(), 200);
    assert_eq!(
        leave.json::<serde_json::Value>().await.unwrap()["playerViews"]
            .as_object()
            .unwrap()
            .len(),
        1
    );

    server.abort();

    let factory = ValueStreamFactory(Arc::new(PrivateViewGame));
    let mut session = factory.new_session();
    let hydrated = session
        .handle_message(StreamClientMessage::Hydrate {
            match_id: "private-1".into(),
            tick_rate_hz: 10.0,
            tick: 0,
            state: json!({
                "players": ["p1", "p2"],
                "secrets": { "p1": "P1_CANARY", "p2": "P2_CANARY" },
                "tick": 0
            }),
            players: vec!["p1".into(), "p2".into()],
            player_views: Some(HashMap::from([
                ("p1".into(), json!({ "ownSecret": "P1_CANARY" })),
                ("p2".into(), json!({ "ownSecret": "P2_CANARY" })),
            ])),
            replay: vec![],
            resync_players: vec![],
        })
        .await;
    match hydrated {
        StreamServerMessage::Hydrated {
            state,
            player_views,
            ..
        } => {
            assert!(state.is_none());
            let views = player_views.unwrap();
            assert_eq!(views["p1"]["ownSecret"], "P1_CANARY");
            assert!(!views["p1"].to_string().contains("P2_CANARY"));
        }
        other => panic!("unexpected private hydrate response: {other:?}"),
    }

    let wrong_roster = session
        .handle_message(StreamClientMessage::Step {
            tick: 1,
            inputs: vec![TickInput {
                player_id: "p3".into(),
                data: json!({}),
                ..Default::default()
            }],
            events: vec![],
            resync_players: vec![],
            want_state: false,
        })
        .await;
    assert!(matches!(
        wrong_roster,
        StreamServerMessage::Error { ref code, retryable: false, .. } if code == "WRONG_ROSTER"
    ));

    let sequence_mismatch = session
        .handle_message(StreamClientMessage::Step {
            tick: 2,
            inputs: vec![],
            events: vec![],
            resync_players: vec![],
            want_state: false,
        })
        .await;
    assert!(matches!(
        sequence_mismatch,
        StreamServerMessage::Error { ref code, retryable: false, .. }
            if code == "TICK_SEQUENCE_MISMATCH"
    ));

    let step = session
        .handle_message(StreamClientMessage::Step {
            tick: 1,
            inputs: vec![TickInput {
                player_id: "p1".into(),
                data: json!({}),
                action_id: Some("act-1".into()),
                action_schema_id: Some("private.action.v1".into()),
                ..Default::default()
            }],
            events: vec![
                StreamStepEvent::Join {
                    player_id: "p2".into(),
                },
                StreamStepEvent::Leave {
                    player_id: "p2".into(),
                },
                StreamStepEvent::Join {
                    player_id: "p2".into(),
                },
            ],
            resync_players: vec![],
            want_state: true,
        })
        .await;
    match step {
        StreamServerMessage::StepResult {
            action_results,
            player_views,
            state,
            ..
        } => {
            assert_eq!(
                action_results,
                Some(vec![GameActionResult {
                    action_id: "act-1".into(),
                    status: GameActionOutcome::Applied,
                    code: None,
                }])
            );
            assert!(!player_views.unwrap()["p1"]
                .to_string()
                .contains("P2_CANARY"));
            let checkpoint = state.expect("requested state checkpoint");
            let mut recovered = factory.new_session();
            let replayed = recovered
                .handle_message(StreamClientMessage::Hydrate {
                    match_id: "private-recovered".into(),
                    tick_rate_hz: 10.0,
                    tick: 1,
                    state: checkpoint.clone(),
                    players: vec!["p1".into(), "p2".into()],
                    player_views: Some(HashMap::from([
                        ("p1".into(), json!({ "ownSecret": "P1_CANARY" })),
                        ("p2".into(), json!({ "ownSecret": "P2_CANARY" })),
                    ])),
                    replay: vec![StreamReplayStep {
                        tick: 2,
                        inputs: vec![TickInput {
                            player_id: "p2".into(),
                            data: json!({ "reject": true }),
                            action_id: Some("act-rejected".into()),
                            action_schema_id: Some("private.action.v1".into()),
                            ..Default::default()
                        }],
                        events: vec![],
                    }],
                    resync_players: vec![],
                })
                .await;
            match replayed {
                StreamServerMessage::Hydrated {
                    state,
                    tick,
                    action_results,
                    ..
                } => {
                    assert_eq!(tick, 2);
                    assert_eq!(state.unwrap()["tick"], 2);
                    assert_eq!(
                        action_results,
                        vec![GameActionResult {
                            action_id: "act-rejected".into(),
                            status: GameActionOutcome::Rejected,
                            code: Some("INVALID_MOVE".into()),
                        }]
                    );
                }
                other => panic!("unexpected recovered hydrate response: {other:?}"),
            }
            let terminal = recovered
                .handle_message(StreamClientMessage::Step {
                    tick: 3,
                    inputs: vec![],
                    events: vec![],
                    resync_players: vec![],
                    want_state: false,
                })
                .await;
            assert!(matches!(
                terminal,
                StreamServerMessage::StepResult {
                    game_result: GameResult { over: true, .. },
                    ..
                }
            ));
        }
        other => panic!("unexpected private step response: {other:?}"),
    }

    let mut incomplete = factory.new_session();
    let rejected = incomplete
        .handle_message(StreamClientMessage::Hydrate {
            match_id: "private-2".into(),
            tick_rate_hz: 10.0,
            tick: 0,
            state: json!({ "players": ["p1", "p2"], "secrets": {}, "tick": 0 }),
            players: vec!["p1".into(), "p2".into()],
            player_views: Some(HashMap::from([("p1".into(), json!({ "ownSecret": "P1" }))])),
            replay: vec![],
            resync_players: vec![],
        })
        .await;
    assert!(matches!(
        rejected,
        StreamServerMessage::Error { ref code, retryable: false, .. } if code == "INVALID_RESULT"
    ));

    let mut invalid_replay = factory.new_session();
    let replay_error = invalid_replay
        .handle_message(StreamClientMessage::Hydrate {
            match_id: "private-replay-gap".into(),
            tick_rate_hz: 10.0,
            tick: 0,
            state: json!({
                "players": ["p1", "p2"],
                "secrets": { "p1": "P1_CANARY", "p2": "P2_CANARY" },
                "tick": 0
            }),
            players: vec!["p1".into(), "p2".into()],
            player_views: None,
            replay: vec![StreamReplayStep {
                tick: 2,
                inputs: vec![],
                events: vec![],
            }],
            resync_players: vec![],
        })
        .await;
    assert!(matches!(
        replay_error,
        StreamServerMessage::Error { ref code, retryable: false, .. }
            if code == "REPLAY_MISMATCH"
    ));
}

#[tokio::test]
async fn sdk_migration_preflight_websocket_transport_covers_recovery_and_errors() {
    let router = create_game_router(Arc::new(PrivateViewGame), Some("private-game".into()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let initial_state = json!({
        "players": ["p1", "p2"],
        "secrets": { "p1": "P1_CANARY", "p2": "P2_CANARY" },
        "tick": 0
    });
    let initial_views = json!({
        "p1": { "ownSecret": "P1_CANARY" },
        "p2": { "ownSecret": "P2_CANARY" }
    });
    let mut socket = connect_raw_websocket(address).await;

    send_websocket_json(
        &mut socket,
        &json!({
            "type": "hydrate",
            "matchId": "ws-preflight",
            "tickRateHz": 10,
            "tick": 0,
            "state": initial_state,
            "players": ["p1", "p2"],
            "playerViews": initial_views
        }),
    )
    .await;
    let hydrated = read_websocket_json(&mut socket).await;
    assert_eq!(hydrated["type"], "hydrated");
    assert_eq!(hydrated["playerViews"]["p1"]["ownSecret"], "P1_CANARY");
    assert!(!hydrated["playerViews"]["p1"]
        .to_string()
        .contains("P2_CANARY"));

    send_websocket_json(
        &mut socket,
        &json!({
            "type": "step",
            "tick": 1,
            "inputs": [{
                "playerId": "p1",
                "data": { "delta": 1 },
                "actionId": "ws-applied-1",
                "actionSchemaId": "private.action.v1"
            }],
            "events": [
                { "type": "join", "playerId": "p2" },
                { "type": "leave", "playerId": "p2" },
                { "type": "join", "playerId": "p2" }
            ],
            "wantState": true
        }),
    )
    .await;
    let step_one = read_websocket_json(&mut socket).await;
    assert_eq!(step_one["type"], "stepResult");
    assert_eq!(step_one["tick"], 1);
    assert_eq!(step_one["actionResults"][0]["status"], "applied");
    assert_eq!(step_one["playerViews"].as_object().unwrap().len(), 2);

    send_websocket_json(&mut socket, &json!({ "type": "step", "tick": 3 })).await;
    let gap = read_websocket_json(&mut socket).await;
    assert_eq!(gap["type"], "error");
    assert_eq!(gap["code"], "TICK_SEQUENCE_MISMATCH");
    assert_eq!(gap["retryable"], false);

    let rejected_input = json!({
        "playerId": "p2",
        "data": { "reject": true },
        "actionId": "ws-rejected-2",
        "actionSchemaId": "private.action.v1"
    });
    send_websocket_json(
        &mut socket,
        &json!({
            "type": "step",
            "tick": 2,
            "inputs": [rejected_input],
            "wantState": true
        }),
    )
    .await;
    let step_two = read_websocket_json(&mut socket).await;
    assert_eq!(step_two["actionResults"][0]["status"], "rejected");
    assert_eq!(step_two["actionResults"][0]["code"], "INVALID_MOVE");
    drop(socket);

    let mut recovered_socket = connect_raw_websocket(address).await;
    send_websocket_json(
        &mut recovered_socket,
        &json!({
            "type": "hydrate",
            "matchId": "ws-preflight",
            "tickRateHz": 10,
            "tick": 1,
            "state": step_one["state"],
            "players": ["p1", "p2"],
            "playerViews": step_one["playerViews"],
            "replay": [{
                "tick": 2,
                "inputs": [rejected_input],
                "events": []
            }]
        }),
    )
    .await;
    let recovered = read_websocket_json(&mut recovered_socket).await;
    assert_eq!(recovered["type"], "hydrated");
    assert_eq!(recovered["tick"], 2);
    assert_eq!(recovered["state"], step_two["state"]);
    assert_eq!(recovered["actionResults"][0]["status"], "rejected");

    send_websocket_json(
        &mut recovered_socket,
        &json!({
            "type": "step",
            "tick": 3,
            "inputs": [{
                "playerId": "p1",
                "data": { "delta": 1 },
                "actionId": "ws-terminal-3",
                "actionSchemaId": "private.action.v1"
            }]
        }),
    )
    .await;
    let terminal = read_websocket_json(&mut recovered_socket).await;
    assert_eq!(terminal["type"], "stepResult");
    assert_eq!(terminal["gameResult"]["over"], true);
    assert_eq!(
        terminal["gameResult"]["nextMatchPolicy"]["mode"],
        "close_room"
    );

    send_websocket_json(
        &mut recovered_socket,
        &json!({
            "type": "hydrate",
            "matchId": "ws-invalid-replay",
            "tickRateHz": 10,
            "tick": 0,
            "state": initial_state,
            "players": ["p1", "p2"],
            "replay": [{ "tick": 2, "inputs": [], "events": [] }]
        }),
    )
    .await;
    let replay_error = read_websocket_json(&mut recovered_socket).await;
    assert_eq!(replay_error["code"], "REPLAY_MISMATCH");

    send_websocket_json(
        &mut recovered_socket,
        &json!({
            "type": "hydrate",
            "matchId": "ws-missing-view",
            "tickRateHz": 10,
            "tick": 0,
            "state": initial_state,
            "players": ["p1", "p2"],
            "playerViews": { "p1": { "ownSecret": "P1_CANARY" } }
        }),
    )
    .await;
    let missing_view = read_websocket_json(&mut recovered_socket).await;
    assert_eq!(missing_view["code"], "INVALID_RESULT");
    assert_eq!(missing_view["retryable"], false);
    drop(recovered_socket);
    server.abort();
}

#[tokio::test]
async fn typed_stream_session_hydrates_replays_pipelines_and_stops_at_game_over() {
    let factory = TypedStreamFactory(Arc::new(TypedCounterGame));
    let mut session: Box<dyn ActiveStreamSession> = factory.new_session();

    let hydrate_resp = session
        .handle_message(StreamClientMessage::Hydrate {
            match_id: "m1".into(),
            tick_rate_hz: 20.0,
            tick: 0,
            state: json!({ "tick": 0, "score": 0, "players": ["p1"] }),
            players: vec!["p1".into()],
            player_views: None,
            replay: vec![
                StreamReplayStep {
                    tick: 1,
                    inputs: vec![TickInput {
                        player_id: "p1".into(),
                        data: json!({ "delta": 2 }),
                        ..Default::default()
                    }],
                    events: vec![],
                },
                StreamReplayStep {
                    tick: 2,
                    inputs: vec![TickInput {
                        player_id: "p1".into(),
                        data: json!({ "delta": 3 }),
                        ..Default::default()
                    }],
                    events: vec![StreamStepEvent::Join {
                        player_id: "p2".into(),
                    }],
                },
            ],
            resync_players: vec!["p1".into()],
        })
        .await;

    match hydrate_resp {
        StreamServerMessage::Hydrated {
            tick,
            state,
            player_views,
            game_result,
            ..
        } => {
            assert_eq!(tick, 2);
            assert_eq!(state.unwrap()["score"], 5);
            let views = player_views.unwrap();
            assert_eq!(views["p1"]["mode"], "full");
            assert_eq!(views["p2"]["mode"], "full");
            assert!(!game_result.over);
        }
        other => panic!("unexpected hydrate response: {other:?}"),
    }

    // Ordinary tick (tick 3): want_state = false, persist = false => state omitted!
    let step3 = session
        .handle_message(StreamClientMessage::Step {
            tick: 3,
            inputs: vec![TickInput {
                player_id: "p1".into(),
                data: json!({ "delta": 1 }),
                action_id: Some("act-step3".into()),
                action_schema_id: Some("counter.action.v1".into()),
                ..Default::default()
            }],
            events: vec![],
            resync_players: vec![],
            want_state: false,
        })
        .await;
    match step3 {
        StreamServerMessage::StepResult {
            tick,
            state,
            player_views,
            persist,
            action_results,
            ..
        } => {
            assert_eq!(tick, 3);
            assert!(state.is_none(), "ordinary tick must omit full state");
            assert!(!persist);
            let views = player_views.unwrap();
            assert_eq!(views["p1"]["mode"], "delta");
            assert_eq!(views["p2"]["mode"], "delta");
            assert_eq!(views["p1"]["score"], 6);
            assert_eq!(
                action_results,
                Some(vec![GameActionResult {
                    action_id: "act-step3".into(),
                    status: GameActionOutcome::Applied,
                    code: None,
                }])
            );
        }
        other => panic!("unexpected step3: {other:?}"),
    }

    // Resync p2 + reach game over at tick 4 => state MUST be included automatically!
    let step4 = session
        .handle_message(StreamClientMessage::Step {
            tick: 4,
            inputs: vec![TickInput {
                player_id: "p2".into(),
                data: json!({ "delta": 5 }),
                ..Default::default()
            }],
            events: vec![],
            resync_players: vec!["p2".into()],
            want_state: false,
        })
        .await;
    match step4 {
        StreamServerMessage::StepResult {
            tick,
            state,
            player_views,
            game_result,
            ..
        } => {
            assert_eq!(tick, 4);
            assert!(game_result.over);
            assert_eq!(state.expect("gameOver must include state")["score"], 11);
            let views = player_views.unwrap();
            assert_eq!(views["p1"]["mode"], "delta");
            assert_eq!(views["p2"]["mode"], "full");
        }
        other => panic!("unexpected step4: {other:?}"),
    }

    // Pipelined in-flight tick 5 arriving after game over is cleanly ignored.
    let step5 = session
        .handle_message(StreamClientMessage::Step {
            tick: 5,
            inputs: vec![],
            events: vec![],
            resync_players: vec![],
            want_state: false,
        })
        .await;
    assert!(matches!(
        step5,
        StreamServerMessage::IgnoredAfterGameOver { tick: 5 }
    ));
}

#[tokio::test]
async fn manifest_advertises_stream_capability_and_ws_endpoint_upgrades_over_tcp() {
    let router = create_typed_game_router(Arc::new(TypedCounterGame), Some("counter".into()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/manifest"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.headers()
            .get("x-randseed-stream-protocol")
            .and_then(|v| v.to_str().ok()),
        Some(STREAM_PROTOCOL_V1)
    );
    assert_eq!(
        resp.headers()
            .get("x-randseed-stream-durability")
            .and_then(|v| v.to_str().ok()),
        Some("checkpoint")
    );
    let manifest_val: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(manifest_val["title"], "Typed Stream Counter");

    // Verify real RFC 6455 HTTP -> WebSocket upgrade on GET /stream
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let handshake = format!(
        "GET /stream HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    );
    tcp.write_all(handshake.as_bytes()).await.unwrap();
    let mut buf = [0u8; 512];
    let n = tcp.read(&mut buf).await.unwrap();
    let header_str = std::str::from_utf8(&buf[..n]).unwrap();
    assert!(
        header_str.starts_with("HTTP/1.1 101 Switching Protocols"),
        "expected 101 Switching Protocols, got: {header_str}"
    );
}

#[tokio::test]
async fn typed_stream_session_command_propagates_joinable_and_player_views() {
    struct QuietStepCounterGame;

    #[async_trait]
    impl TypedGameServer for QuietStepCounterGame {
        type State = CounterState;
        type ViewContext = CounterViewCtx;

        async fn manifest(&self) -> Result<GameManifest, SdkError> {
            TypedCounterGame.manifest().await
        }

        async fn init_typed(
            &self,
            config: MatchConfig,
        ) -> Result<TypedInitResult<Self::State>, SdkError> {
            TypedCounterGame.init_typed(config).await
        }

        async fn step_typed(
            &self,
            ctx: TickContext,
            state: &mut Self::State,
            _view_ctx: &mut Self::ViewContext,
            _emit_views: bool,
        ) -> Result<TypedTickResult, SdkError> {
            state.tick = ctx.tick;
            // Deliberately leave `joinable` and `player_views` as `None` so command overrides survive.
            Ok(TypedTickResult::default())
        }

        async fn on_command_typed(
            &self,
            command: GameCommand,
            state: &mut Self::State,
            view_ctx: &mut Self::ViewContext,
        ) -> Result<TypedEventOutcome, SdkError> {
            TypedCounterGame
                .on_command_typed(command, state, view_ctx)
                .await
        }

        async fn is_game_over_typed(&self, state: &Self::State) -> Result<GameResult, SdkError> {
            TypedCounterGame.is_game_over_typed(state).await
        }
    }

    let factory = TypedStreamFactory(Arc::new(QuietStepCounterGame));
    let mut session = factory.new_session();
    let _ = session
        .handle_message(StreamClientMessage::Hydrate {
            match_id: "m-cmd".into(),
            tick_rate_hz: 20.0,
            tick: 0,
            state: json!({ "tick": 0, "score": 0, "players": ["p1"] }),
            players: vec!["p1".into()],
            player_views: None,
            replay: vec![],
            resync_players: vec![],
        })
        .await;

    let step = session
        .handle_message(StreamClientMessage::Step {
            tick: 1,
            inputs: vec![],
            events: vec![StreamStepEvent::Command {
                command: GameCommand {
                    command_type: "lock_room".into(),
                    data: None,
                    issuer: Some("host".into()),
                    timestamp: None,
                },
            }],
            resync_players: vec![],
            want_state: false,
        })
        .await;

    match step {
        StreamServerMessage::StepResult {
            tick,
            joinable,
            player_views,
            broadcasts,
            ..
        } => {
            assert_eq!(tick, 1);
            assert_eq!(joinable, Some(false));
            assert_eq!(broadcasts, vec![json!({ "event": "locked" })]);
            let views = player_views.expect("command player_views must be preserved");
            assert_eq!(views["p1"]["locked"], json!(true));
        }
        other => panic!("unexpected step response: {other:?}"),
    }
}

#[tokio::test]
async fn typed_game_router_serves_agent_contract_projection_and_tick_action_results() {
    let router = create_typed_game_router(Arc::new(TypedCounterGame), Some("counter".into()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let client = reqwest::Client::new();

    let contract_resp = client
        .get(format!("http://{addr}/agent/player-contract"))
        .send()
        .await
        .unwrap();
    assert_eq!(contract_resp.status(), 200);
    let contract_json: serde_json::Value = contract_resp.json().await.unwrap();
    assert_eq!(contract_json["actionSchemaId"], "counter.action.v1");

    let tick_resp = client
        .post(format!("http://{addr}/tick"))
        .json(&json!({
            "ctx": {
                "tick": 1,
                "elapsedSeconds": 0.05,
                "inputs": [
                    {
                        "playerId": "p1",
                        "delta": 4,
                        "actionId": "act-100",
                        "actionSchemaId": "counter.action.v1"
                    }
                ]
            },
            "state": { "tick": 0, "score": 0, "players": ["p1"] },
            "players": ["p1"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(tick_resp.status(), 200);
    let tick_json: serde_json::Value = tick_resp.json().await.unwrap();
    assert_eq!(tick_json["state"]["score"], 4);
    assert_eq!(
        tick_json["actionResults"],
        json!([{ "actionId": "act-100", "status": "applied" }])
    );

    let view_resp = client
        .post(format!("http://{addr}/agent/player-view"))
        .json(&json!({
            "context": {
                "scope": {
                    "gameId": "counter",
                    "environment": "staging",
                    "releaseVersion": "1.0.0"
                },
                "roomId": "room-1",
                "matchId": "match-1",
                "matchSeq": 1,
                "playerId": "p1",
                "sequence": 1,
                "tick": 1
            },
            "players": ["p1"],
            "state": tick_json["state"],
            "committedEvents": [{ "type": "scored" }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(view_resp.status(), 200);
    let view_json: serde_json::Value = view_resp.json().await.unwrap();
    assert_eq!(view_json["schemaId"], "counter.view.v1");
    assert_eq!(view_json["publicState"]["score"], 4);
    assert_eq!(view_json["self"]["playerId"], "p1");
    assert_eq!(view_json["self"]["privateState"]["seat"], "p1");
}
