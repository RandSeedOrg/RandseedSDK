//! Integration and unit tests for the RandSeed Rust SDK.
//!
//! Covers the room-host protocol, Mulberry32, `PlatformClient`, and replay data.

use async_trait::async_trait;
use randseed_game_sdk::{
    create_game_router, evaluate_replay_storage_tier, validate_replay_data_v2, AgentControlMode,
    AgentGameCapabilities, CommandResult, GameCommand, GameManifest, GameResult, GameServer,
    HealthResponse, InitResult, MatchConfig, MatchOptions, MatchSummary, Mulberry32,
    NextMatchPolicy, PlatformClient, PlayerEventResult, ReplayDataV2, ReplayInputItem,
    ReplayStorageTier, SdkError, TickContext, TickResult,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn assert_json_roundtrip<T>(input: &serde_json::Value, expected: serde_json::Value)
where
    T: DeserializeOwned + Serialize,
{
    let decoded: T = serde_json::from_value(input.clone()).unwrap();
    let encoded = serde_json::to_value(&decoded).unwrap();
    let expected_decoded: T = serde_json::from_value(expected).unwrap();
    assert_eq!(encoded, serde_json::to_value(expected_decoded).unwrap());

    let decoded_again: T = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded_again).unwrap(), encoded);
}

#[test]
fn room_host_wire_roundtrips_opaque_carryover_and_frame_context() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/room-host-v1.json")).unwrap();

    assert_json_roundtrip::<GameManifest>(&fixture["manifest"], fixture["manifest"].clone());
    assert_json_roundtrip::<InitResult>(&fixture["initResponse"], fixture["initResponse"].clone());

    let config: MatchConfig = serde_json::from_value(fixture["config"].clone()).unwrap();
    config.validate().unwrap();
    assert_eq!(config.carryover, Some(json!(17)));
    assert_eq!(config.options.seed, u32::MAX as f64);
    assert_json_roundtrip::<MatchConfig>(&fixture["config"], fixture["config"].clone());
    let mut missing_options = fixture["config"].clone();
    missing_options.as_object_mut().unwrap().remove("options");
    let defaulted: MatchConfig = serde_json::from_value(missing_options.clone()).unwrap();
    assert_eq!(defaulted.options.seed, 0.0);
    missing_options["options"] = json!({});
    assert!(serde_json::from_value::<MatchConfig>(missing_options.clone()).is_ok());
    missing_options["options"] = json!(null);
    assert!(serde_json::from_value::<MatchConfig>(missing_options).is_err());
    let mut null_tick_rate = fixture["config"].clone();
    null_tick_rate["tickRateHz"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<MatchConfig>(null_tick_rate).is_err());
    let mut missing_tick_rate = fixture["config"].clone();
    missing_tick_rate
        .as_object_mut()
        .unwrap()
        .remove("tickRateHz");
    assert!(serde_json::from_value::<MatchConfig>(missing_tick_rate).is_ok());

    let request: randseed_game_sdk::TickRequest =
        serde_json::from_value(fixture["tickRequest"].clone()).unwrap();
    request.ctx.validate().unwrap();
    assert_eq!(request.ctx.tick, 1);
    assert_eq!(request.ctx.elapsed_seconds, 0.2);
    assert_eq!(request.ctx.inputs[0].player_id, "pairwise-a");
    assert!(request.ctx.commands.is_empty());
    let mut tick_request_expected = fixture["tickRequest"].clone();
    tick_request_expected["ctx"]
        .as_object_mut()
        .unwrap()
        .remove("commands");
    assert_json_roundtrip::<randseed_game_sdk::TickRequest>(
        &fixture["tickRequest"],
        tick_request_expected,
    );
    assert_json_roundtrip::<TickResult>(&fixture["tickResponse"], fixture["tickResponse"].clone());

    assert_json_roundtrip::<randseed_game_sdk::PlayerEventRequest>(
        &fixture["playerEventRequest"],
        fixture["playerEventRequest"].clone(),
    );
    assert_json_roundtrip::<PlayerEventResult>(
        &fixture["playerEventResponse"],
        fixture["playerEventResponse"].clone(),
    );
    assert_json_roundtrip::<randseed_game_sdk::IsGameOverRequest>(
        &fixture["isGameOverRequest"],
        fixture["isGameOverRequest"].clone(),
    );

    let result: GameResult = serde_json::from_value(fixture["gameOverResponse"].clone()).unwrap();
    assert_eq!(result.next_match_policy.unwrap().carryover, Some(json!(17)));
    assert_json_roundtrip::<GameResult>(
        &fixture["gameOverResponse"],
        fixture["gameOverResponse"].clone(),
    );
    assert_json_roundtrip::<randseed_game_sdk::CommandRequest>(
        &fixture["commandRequest"],
        fixture["commandRequest"].clone(),
    );
    assert_json_roundtrip::<CommandResult>(
        &fixture["commandResponse"],
        fixture["commandResponse"].clone(),
    );

    let health = json!({ "status": "ok", "gameId": "room-counter", "uptime": 1.0 });
    assert_json_roundtrip::<HealthResponse>(&health, health.clone());
}

#[test]
fn protocol_v1_validation_rejects_out_of_range_values() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/room-host-v1.json")).unwrap();
    let mut manifest: GameManifest = serde_json::from_value(fixture["manifest"].clone()).unwrap();
    assert!(manifest.validate().is_ok());
    manifest.tick_rate_hz = 0.0;
    assert!(manifest.validate().is_err());
    manifest.tick_rate_hz = 60.0;
    assert!(manifest.validate().is_ok());
    manifest.tick_rate_hz = f64::INFINITY;
    assert!(manifest.validate().is_err());
    manifest.tick_rate_hz = 5.0;
    manifest.max_players = 101;
    assert!(manifest.validate().is_err());

    let mut config: MatchConfig = serde_json::from_value(fixture["config"].clone()).unwrap();
    config.match_seq = 0;
    assert!(config.validate().is_err());
    config.match_seq = MAX_SAFE_INTEGER;
    assert!(config.validate().is_ok());
    config.match_seq = MAX_SAFE_INTEGER + 1;
    assert!(config.validate().is_err());
    config.match_seq = 2;
    config.players[1] = config.players[0].clone();
    assert!(config.validate().is_err());
    config.players = vec!["pairwise-a".into(), "pairwise-b".into()];
    config.options.seed = f64::NAN;
    assert!(config.validate().is_err());
    config.options.seed = 1.0;
    config.tick_rate_hz = Some(61.0);
    assert!(config.validate().is_err());

    let mut context: TickContext =
        serde_json::from_value(fixture["tickRequest"]["ctx"].clone()).unwrap();
    context.tick = MAX_SAFE_INTEGER;
    assert!(context.validate().is_ok());
    context.tick = MAX_SAFE_INTEGER + 1;
    assert!(context.validate().is_err());
    context.tick = 1;
    context.inputs[0].timestamp = Some(MAX_SAFE_INTEGER);
    assert!(context.validate().is_ok());
    context.inputs[0].timestamp = Some(MAX_SAFE_INTEGER + 1);
    assert!(context.validate().is_err());
    context.inputs[0].timestamp = None;
    context.commands.push(GameCommand {
        command_type: "trusted".into(),
        data: None,
        issuer: None,
        timestamp: Some(MAX_SAFE_INTEGER + 1),
    });
    assert!(context.validate().is_err());
    context.commands.clear();
    context.elapsed_seconds = f64::INFINITY;
    assert!(context.validate().is_err());
}

/// Mock game server used by integration tests.
struct MockGame;

#[async_trait]
impl GameServer for MockGame {
    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        Ok(GameManifest {
            title: "Rust Test Game".into(),
            version: "1.0.0".into(),
            min_players: 1,
            max_players: 4,
            tick_rate_hz: 20.0,
            persistent_table: true,
            options_schema: Some(json!({ "seed": { "type": "number" } })),
        })
    }

    async fn init(&self, config: MatchConfig) -> Result<InitResult, SdkError> {
        Ok(InitResult {
            state: json!({
                "matchId": config.match_id,
                "score": 0,
                "tickCount": 0,
                "players": config.players,
                "carryover": config.carryover,
            }),
            broadcasts: Some(vec![json!({
                "type": "game_initialized",
                "matchId": config.match_id,
            })]),
        })
    }

    async fn tick(
        &self,
        ctx: TickContext,
        mut state: serde_json::Value,
    ) -> Result<TickResult, SdkError> {
        let is_persist = ctx.tick % 50 == 0;
        let prev_score = state.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
        let added_score = if !ctx.inputs.is_empty() { 10 } else { 1 };
        let new_score = prev_score + added_score;

        state["tickCount"] = json!(ctx.tick);
        state["score"] = json!(new_score);

        let broadcasts = if !ctx.inputs.is_empty() {
            Some(vec![
                json!({ "type": "player_action", "count": ctx.inputs.len() }),
            ])
        } else {
            None
        };

        Ok(TickResult {
            state,
            broadcasts,
            persist: is_persist,
        })
    }

    async fn on_player_join(
        &self,
        player_id: String,
        mut state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError> {
        let players = state.get_mut("players").and_then(|v| v.as_array_mut());
        if let Some(list) = players {
            list.push(json!(player_id.clone()));
        }
        Ok(PlayerEventResult {
            state,
            broadcasts: Some(vec![
                json!({ "type": "player_joined", "playerId": player_id }),
            ]),
        })
    }

    async fn on_player_leave(
        &self,
        player_id: String,
        mut state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError> {
        if let Some(list) = state.get_mut("players").and_then(|v| v.as_array_mut()) {
            list.retain(|p| p.as_str() != Some(&player_id));
        }
        Ok(PlayerEventResult {
            state,
            broadcasts: Some(vec![
                json!({ "type": "player_left", "playerId": player_id }),
            ]),
        })
    }

    async fn is_game_over(&self, state: serde_json::Value) -> Result<GameResult, SdkError> {
        let tick_count = state.get("tickCount").and_then(|v| v.as_u64()).unwrap_or(0);
        if tick_count >= 100 {
            let mut scores = HashMap::new();
            scores.insert("player_1".into(), 100.0);

            Ok(GameResult {
                over: true,
                summary: Some(MatchSummary {
                    winner_player_id: Some("player_1".into()),
                    scores: Some(scores),
                    duration_seconds: match state["durationKind"].as_str() {
                        Some("negative") => -1.0,
                        Some("nan") => f64::NAN,
                        Some("infinite") => f64::INFINITY,
                        Some("zero") => 0.0,
                        _ => 5.0,
                    },
                    custom_metrics: None,
                    extra: None,
                }),
                next_match_policy: Some(NextMatchPolicy {
                    mode: "auto_next_match".into(),
                    timeout_ms: 0,
                    carryover: Some(json!({ "score": 100 })),
                }),
            })
        } else {
            Ok(GameResult {
                over: false,
                summary: None,
                next_match_policy: None,
            })
        }
    }

    async fn on_command(
        &self,
        cmd: GameCommand,
        mut state: serde_json::Value,
    ) -> Result<CommandResult, SdkError> {
        if cmd.command_type == "panic" {
            panic!("private callback panic detail");
        }
        if cmd.command_type == "internal_error" {
            return Err(SdkError::Internal(
                "Sensitive backend detail: DATABASE_PASSWORD=do-not-leak".into(),
            ));
        }
        if cmd.command_type == "reset" {
            state["count"] = cmd
                .data
                .as_ref()
                .and_then(|data| data.get("count"))
                .cloned()
                .unwrap_or(json!(0));
            return Ok(CommandResult {
                state,
                broadcasts: Some(vec![json!({ "type": "counter.reset" })]),
            });
        }

        state["lastCommand"] = json!(cmd);
        Ok(CommandResult {
            state,
            broadcasts: Some(vec![
                json!({ "type": "command_executed", "cmdType": cmd.command_type }),
            ]),
        })
    }
}

struct AgentGame(AgentGameCapabilities);

#[async_trait]
impl GameServer for AgentGame {
    async fn agent_capabilities(&self) -> Result<Option<AgentGameCapabilities>, SdkError> {
        Ok(Some(self.0.clone()))
    }
    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        MockGame.manifest().await
    }
    async fn init(&self, config: MatchConfig) -> Result<InitResult, SdkError> {
        MockGame.init(config).await
    }
    async fn tick(
        &self,
        ctx: TickContext,
        state: serde_json::Value,
    ) -> Result<TickResult, SdkError> {
        MockGame.tick(ctx, state).await
    }
    async fn on_player_join(
        &self,
        player_id: String,
        state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError> {
        MockGame.on_player_join(player_id, state).await
    }
    async fn on_player_leave(
        &self,
        player_id: String,
        state: serde_json::Value,
    ) -> Result<PlayerEventResult, SdkError> {
        MockGame.on_player_leave(player_id, state).await
    }
    async fn is_game_over(&self, state: serde_json::Value) -> Result<GameResult, SdkError> {
        MockGame.is_game_over(state).await
    }
}

#[tokio::test]
async fn agent_capability_endpoint_requires_a_valid_explicit_declaration() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get<G: GameServer>(game: G) -> (u16, serde_json::Value) {
        let request = Request::builder()
            .uri("/agent/capabilities")
            .body(Body::empty())
            .unwrap();
        let response = create_game_router(Arc::new(game), None)
            .oneshot(request)
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    let (status, _) = get(MockGame).await;
    assert_eq!(status, 404);
    let capabilities = AgentGameCapabilities {
        observation_protocol_version: 1,
        action_protocol_version: 1,
        modes: vec![AgentControlMode::Autoplay, AgentControlMode::Follow],
        max_decision_hz: 10.0,
        background_autoplay: false,
    };
    let (status, body) = get(AgentGame(capabilities.clone())).await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        json!({
            "observationProtocolVersion": 1, "actionProtocolVersion": 1,
            "modes": ["autoplay", "follow"], "maxDecisionHz": 10.0,
            "backgroundAutoplay": false
        })
    );
    let invalid = AgentGameCapabilities {
        observation_protocol_version: 2,
        ..capabilities
    };
    let (status, body) = get(AgentGame(invalid)).await;
    assert_eq!(status, 500);
    assert_eq!(
        body,
        json!({"error": "Agent capabilities are unavailable."})
    );
}

#[tokio::test]
async fn test_wire_protocol_endpoints_real_tcp() {
    // Start the HTTP server on an available local TCP port.
    let router = create_game_router(Arc::new(MockGame), Some("rust-card-game".into()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let client = reqwest::Client::new();

    for (duration_kind, expected_status) in [
        ("negative", 500),
        ("nan", 500),
        ("infinite", 500),
        ("zero", 200),
        ("finite", 200),
    ] {
        let response = client
            .post(format!("{}/isGameOver", base_url))
            .json(&json!({"state": {"tickCount": 100, "durationKind": duration_kind}}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected_status);
        if expected_status == 500 {
            let body: serde_json::Value = response.json().await.unwrap();
            assert_eq!(body["error"], "Internal server error.");
        }
    }

    // 1. GET /manifest
    let resp = client
        .get(format!("{}/manifest", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let manifest: GameManifest = resp.json().await.unwrap();
    assert_eq!(manifest.title, "Rust Test Game");
    assert_eq!(manifest.tick_rate_hz, 20.0);

    // 2. GET /health
    let resp = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let health: HealthResponse = resp.json().await.unwrap();
    assert_eq!(health.status, "ok");
    assert_eq!(health.game_id, "rust-card-game");
    assert!(health.uptime.is_some());

    // 3. POST /init
    let init_req = MatchConfig {
        match_id: "rust_match_01".into(),
        game_id: "rust-card-game".into(),
        players: vec!["p1".into(), "p2".into()],
        match_seq: 1,
        options: MatchOptions {
            seed: 987654.0,
            extra: HashMap::new(),
        },
        carryover: None,
        tick_rate_hz: Some(20.0),
    };
    let resp = client
        .post(format!("{}/init", base_url))
        .json(&init_req)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let init_res: randseed_game_sdk::InitResult = resp.json().await.unwrap();
    assert_eq!(init_res.state["matchId"], "rust_match_01");

    // 4. POST /tick
    let tick_req = json!({
        "ctx": {
            "tick": 50,
            "inputs": [{ "playerId": "p1", "data": { "act": "hit" } }],
            "elapsedSeconds": 2.5
        },
        "state": { "score": 0, "tickCount": 49 }
    });
    let resp = client
        .post(format!("{}/tick", base_url))
        .json(&tick_req)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let tick_res: randseed_game_sdk::TickResult = resp.json().await.unwrap();
    assert_eq!(tick_res.state["score"], 10);
    assert_eq!(tick_res.state["tickCount"], 50);
    assert!(tick_res.persist);

    // 5. POST /onPlayerJoin
    let join_req = json!({
        "playerId": "p3",
        "state": { "players": ["p1", "p2"] }
    });
    let resp = client
        .post(format!("{}/onPlayerJoin", base_url))
        .json(&join_req)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let join_res: randseed_game_sdk::PlayerEventResult = resp.json().await.unwrap();
    assert_eq!(join_res.state["players"].as_array().unwrap().len(), 3);

    // 6. POST /onPlayerLeave
    let leave_req = json!({
        "playerId": "p2",
        "state": { "players": ["p1", "p2", "p3"] }
    });
    let resp = client
        .post(format!("{}/onPlayerLeave", base_url))
        .json(&leave_req)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let leave_res: randseed_game_sdk::PlayerEventResult = resp.json().await.unwrap();
    assert_eq!(leave_res.state["players"].as_array().unwrap().len(), 2);

    // 7. POST /isGameOver: match is not over.
    let over_req_false = json!({ "state": { "tickCount": 20 } });
    let resp = client
        .post(format!("{}/isGameOver", base_url))
        .json(&over_req_false)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let over_res_false: randseed_game_sdk::GameResult = resp.json().await.unwrap();
    assert!(!over_res_false.over);

    // 8. POST /isGameOver: match is over.
    let over_req_true = json!({ "state": { "tickCount": 100 } });
    let resp = client
        .post(format!("{}/isGameOver", base_url))
        .json(&over_req_true)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let over_res_true: randseed_game_sdk::GameResult = resp.json().await.unwrap();
    assert!(over_res_true.over);
    assert_eq!(
        over_res_true.summary.unwrap().winner_player_id.unwrap(),
        "player_1"
    );
    assert_eq!(
        over_res_true.next_match_policy.unwrap().mode,
        "auto_next_match"
    );

    // 9. POST /onCommand.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/room-host-v1.json")).unwrap();
    let resp = client
        .post(format!("{}/onCommand", base_url))
        .json(&fixture["commandRequest"])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let command_response: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(command_response, fixture["commandResponse"]);

    let cmd_req = json!({
        "command": { "type": "pause", "data": { "reason": "debug" } },
        "state": { "score": 10 }
    });
    let resp = client
        .post(format!("{}/onCommand", base_url))
        .json(&cmd_req)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let cmd_res: randseed_game_sdk::CommandResult = resp.json().await.unwrap();
    assert_eq!(cmd_res.state["lastCommand"]["type"], "pause");

    let internal_error_req = json!({
        "command": { "type": "internal_error" },
        "state": {}
    });
    let resp = client
        .post(format!("{}/onCommand", base_url))
        .json(&internal_error_req)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);
    let error_body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(error_body, json!({ "error": "Internal server error." }));

    let panic_req = json!({
        "command": { "type": "panic", "data": {} },
        "state": {}
    });
    let resp = client
        .post(format!("{}/onCommand", base_url))
        .json(&panic_req)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);
    let panic_error: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(panic_error, json!({ "error": "Internal server error." }));

    let resp = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(!error_body
        .to_string()
        .contains("DATABASE_PASSWORD=do-not-leak"));

    // 10. Unknown endpoint returns 404.
    let resp = client
        .get(format!("{}/not_found", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // 11. Invalid input is rejected with 400 Bad Request.
    let invalid_init = json!({
        "matchId": "",
        "gameId": "rust-card-game",
        "players": ["p1"],
        "matchSeq": 1,
        "options": { "seed": 1.0 }
    });
    let resp = client
        .post(format!("{}/init", base_url))
        .json(&invalid_init)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let validation_error: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        validation_error,
        json!({ "error": "matchId must be a non-empty string." })
    );

    let negative_match_seq = json!({
        "matchId": "invalid-sequence",
        "gameId": "rust-card-game",
        "players": ["p1"],
        "matchSeq": -1,
        "options": { "seed": 1 }
    });
    let resp = client
        .post(format!("{}/init", base_url))
        .json(&negative_match_seq)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap(),
        json!({ "error": "Invalid JSON request body." })
    );

    let negative_tick = json!({
        "ctx": { "tick": -1, "inputs": [], "elapsedSeconds": 0.0 },
        "state": {}
    });
    let resp = client
        .post(format!("{}/tick", base_url))
        .json(&negative_tick)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap(),
        json!({ "error": "Invalid JSON request body." })
    );

    let negative_timestamp = json!({
        "ctx": {
            "tick": 1,
            "inputs": [{ "playerId": "p1", "data": null, "timestamp": -1 }],
            "elapsedSeconds": 0.0
        },
        "state": {}
    });
    let resp = client
        .post(format!("{}/tick", base_url))
        .json(&negative_timestamp)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap(),
        json!({ "error": "Invalid JSON request body." })
    );

    let unsafe_timestamp = json!({
        "ctx": {
            "tick": 1,
            "inputs": [{
                "playerId": "p1",
                "data": null,
                "timestamp": MAX_SAFE_INTEGER + 1
            }],
            "elapsedSeconds": 0.0
        },
        "state": {}
    });
    let resp = client
        .post(format!("{}/tick", base_url))
        .json(&unsafe_timestamp)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap(),
        json!({ "error": "input.timestamp must be a non-negative safe integer." })
    );

    let oversized_init = json!({
        "matchId": "large",
        "gameId": "rust-card-game",
        "players": ["p1"],
        "matchSeq": 1,
        "options": {
            "seed": 1,
            "largeValue": "x".repeat(1024 * 1024)
        }
    });
    let resp = client
        .post(format!("{}/init", base_url))
        .json(&oversized_init)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 413);
    assert_eq!(
        resp.json::<serde_json::Value>().await.unwrap(),
        json!({ "error": "Request body must not exceed 1 MiB." })
    );
}

#[test]
fn test_mulberry32_prng_determinism() {
    let mut rng1 = Mulberry32::new(987654321.0);
    let mut rng2 = Mulberry32::new(987654321.0);

    for _ in 0..100 {
        let v1 = rng1.next_f64();
        let v2 = rng2.next_f64();
        assert_eq!(v1, v2);
        assert!((0.0..1.0).contains(&v1));
    }

    // Boundary seeds.
    let mut rng_zero = Mulberry32::from_u32(0);
    assert!((0.0..1.0).contains(&rng_zero.next_f64()));

    let mut rng_neg = Mulberry32::from_i32(-9999);
    assert!((0.0..1.0).contains(&rng_neg.next_f64()));

    let mut rng_u32_max = Mulberry32::from_u32(0xffffffff);
    assert!((0.0..1.0).contains(&rng_u32_max.next_f64()));

    // Check the distribution over 5,000 samples.
    let mut rng_dist = Mulberry32::new(42.0);
    let mut buckets = [0u32; 10];
    for _ in 0..5000 {
        let v = rng_dist.next_f64();
        let idx = (v * 10.0).floor() as usize;
        buckets[idx.min(9)] += 1;
    }
    for count in buckets {
        assert!(
            count >= 350 && count <= 650,
            "Distribution outside the expected range: {count}"
        );
    }
}

#[test]
fn mulberry32_matches_every_shared_fixture_vector() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/room-host-v1.json")).unwrap();
    let vectors = fixture["prngVectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 3);

    for vector in vectors {
        let seed = vector["seed"].as_f64().unwrap();
        let expected = vector["u32"].as_array().unwrap();
        let mut rng = Mulberry32::new(seed);
        for expected_value in expected {
            assert_eq!(
                u64::from(rng.next_u32()),
                expected_value.as_u64().unwrap(),
                "Unexpected Mulberry32 output for seed {seed}"
            );
        }
    }

    assert_eq!(
        Mulberry32::new(4_294_967_297.0).next_u32(),
        Mulberry32::new(1.0).next_u32()
    );
    assert_eq!(
        Mulberry32::new(-1.0).next_u32(),
        Mulberry32::from_u32(u32::MAX).next_u32()
    );
}

#[test]
fn test_replay_data_v2_and_tier_evaluation() {
    let replay = ReplayDataV2 {
        version: 2,
        game_id: "card_game".into(),
        match_id: "match_1001".into(),
        match_config: MatchConfig {
            match_id: "match_1001".into(),
            game_id: "card_game".into(),
            players: vec!["p1".into()],
            match_seq: 1,
            options: MatchOptions::default(),
            carryover: None,
            tick_rate_hz: Some(20.0),
        },
        init_state: json!({ "score": 0 }),
        inputs: vec![
            ReplayInputItem {
                tick: 0,
                player_id: "p1".into(),
                data: json!({ "act": "ready" }),
                timestamp: None,
            },
            ReplayInputItem {
                tick: 5,
                player_id: "p1".into(),
                data: json!({ "act": "play" }),
                timestamp: None,
            },
        ],
        commands: vec![],
        summary: MatchSummary {
            winner_player_id: Some("p1".into()),
            scores: None,
            duration_seconds: 1.0,
            custom_metrics: None,
            extra: None,
        },
        created_at: 1700000000000,
    };

    assert!(validate_replay_data_v2(&replay).is_ok());

    // Construct an out-of-order tick sequence (10 followed by 5).
    let mut invalid_replay = replay.clone();
    invalid_replay.inputs[0].tick = 10;
    invalid_replay.inputs[1].tick = 5;
    assert!(validate_replay_data_v2(&invalid_replay).is_err());

    // Check replay storage tiers.
    assert_eq!(evaluate_replay_storage_tier(1000), ReplayStorageTier::TierA);
    assert_eq!(
        evaluate_replay_storage_tier(2_000_000),
        ReplayStorageTier::TierB
    );
    assert_eq!(
        evaluate_replay_storage_tier(15_000_000),
        ReplayStorageTier::TierC
    );
}

#[tokio::test]
async fn test_platform_client_custom_mock() {
    // Start mock virtual-host services.
    let mock_router = axum::Router::new()
        .route(
            "/sql",
            axum::routing::post(
                |axum::Json(payload): axum::Json<serde_json::Value>| async move {
                    assert_eq!(payload["sql"], "SELECT 1");
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(json!({ "rows": [{ "c": 1 }] })),
                    )
                },
            ),
        )
        .route(
            "/platform.broadcast/",
            axum::routing::post(|| async {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(json!({ "success": true, "recipientCount": 2 })),
                )
            }),
        )
        .route(
            "/platform.send/",
            axum::routing::post(|| async {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(json!({ "success": true, "delivered": true })),
                )
            }),
        )
        .route(
            "/platform.players/",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(json!({ "players": [{ "playerId": "p1", "joinedAt": 1000 }] })),
                )
            }),
        )
        .route(
            "/platform.kv/key1",
            axum::routing::get(|| async {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(json!({ "value": "val1" })),
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let base_url = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        axum::serve(listener, mock_router).await.unwrap();
    });

    let mut vhost_map = HashMap::new();
    vhost_map.insert("platform.db".into(), base_url.clone());
    vhost_map.insert(
        "platform.broadcast".into(),
        format!("{}/platform.broadcast", base_url),
    );
    vhost_map.insert(
        "platform.send".into(),
        format!("{}/platform.send", base_url),
    );
    vhost_map.insert(
        "platform.players".into(),
        format!("{}/platform.players", base_url),
    );
    vhost_map.insert("platform.kv".into(), format!("{}/platform.kv", base_url));

    let client = PlatformClient::with_vhost_map(vhost_map);

    let rows = client.sql("SELECT 1", vec![]).await.unwrap();
    assert_eq!(rows.len(), 1);

    let (b_ok, b_count) = client.broadcast(json!({ "m": 1 })).await.unwrap();
    assert!(b_ok);
    assert_eq!(b_count, 2);

    let (s_ok, s_deliv) = client.send("p1", json!({ "msg": "secret" })).await.unwrap();
    assert!(s_ok);
    assert!(s_deliv);

    let players = client.get_players().await.unwrap();
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].player_id, "p1");

    let val = client.kv_get("key1").await.unwrap();
    assert_eq!(val, Some(json!("val1")));
}
