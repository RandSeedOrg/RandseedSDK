//! Integration tests for `Session Stream v1` (`/stream`) and `TypedGameServer`.

use async_trait::async_trait;
use randseed_game_sdk::stream::{ActiveStreamSession, StreamSessionFactory, TypedStreamFactory};
use randseed_game_sdk::{
    create_typed_game_router, GameManifest, GameResult, MatchConfig, MatchSummary, NextMatchPolicy,
    SdkError, StreamClientMessage, StreamDurability, StreamReplayStep, StreamServerMessage,
    StreamStepEvent, TickContext, TickInput, TypedEventOutcome, TypedGameServer, TypedInitResult,
    TypedTickResult, STREAM_PROTOCOL_V1,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        Ok(GameManifest {
            title: "Typed Stream Counter".into(),
            version: "1.0.0".into(),
            min_players: 1,
            max_players: 8,
            tick_rate_hz: 20.0,
            persistent_table: false,
            options_schema: None,
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
        for input in &ctx.inputs {
            if let Some(delta) = input.data.get("delta").and_then(|v| v.as_i64()) {
                state.score += delta;
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
            replay: vec![
                StreamReplayStep {
                    tick: 1,
                    inputs: vec![TickInput {
                        player_id: "p1".into(),
                        data: json!({ "delta": 2 }),
                        timestamp: None,
                    }],
                    events: vec![],
                },
                StreamReplayStep {
                    tick: 2,
                    inputs: vec![TickInput {
                        player_id: "p1".into(),
                        data: json!({ "delta": 3 }),
                        timestamp: None,
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
                timestamp: None,
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
            ..
        } => {
            assert_eq!(tick, 3);
            assert!(state.is_none(), "ordinary tick must omit full state");
            assert!(!persist);
            let views = player_views.unwrap();
            assert_eq!(views["p1"]["mode"], "delta");
            assert_eq!(views["p2"]["mode"], "delta");
            assert_eq!(views["p1"]["score"], 6);
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
                timestamp: None,
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
