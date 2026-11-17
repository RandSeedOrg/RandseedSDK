use async_trait::async_trait;
use randseed_game_sdk::{
    create_game_router, CommandRequest, CommandResult, GameCommand, GameManifest, GameResult,
    GameServer, InitResult, MatchConfig, PlayerEventRequest, PlayerEventResult, PlayerViewMode,
    SdkError, TickContext, TickRequest, TickResult,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task::JoinHandle;

#[derive(Clone, Copy)]
enum ViewBehavior {
    Valid,
    Missing,
    MissingSeat,
    Extra,
    EmptyKey,
    OversizedView,
    OversizedMap,
}

struct PrivateViewGame {
    behavior: ViewBehavior,
}

impl PrivateViewGame {
    fn views(&self, players: &[String]) -> Option<HashMap<String, Value>> {
        if matches!(self.behavior, ViewBehavior::Missing) {
            return None;
        }
        let players = if matches!(self.behavior, ViewBehavior::MissingSeat) {
            players.iter().take(1).cloned().collect::<Vec<_>>()
        } else {
            players.to_vec()
        };
        let mut views = HashMap::new();
        for player_id in players {
            let view = match self.behavior {
                ViewBehavior::OversizedView => Value::String("x".repeat(65 * 1024)),
                ViewBehavior::OversizedMap => Value::String("x".repeat(62_000)),
                _ => json!({
                    "viewer": player_id,
                    "hand": if player_id == "seat-a" { vec!["ACE_OF_SPADES"] } else { vec!["KING_OF_CLUBS"] }
                }),
            };
            views.insert(player_id, view);
        }
        match self.behavior {
            ViewBehavior::Extra => {
                views.insert("seat-extra".into(), json!({"viewer":"seat-extra"}));
            }
            ViewBehavior::EmptyKey => {
                views.insert(String::new(), json!({"bad":true}));
            }
            _ => {}
        }
        Some(views)
    }
}

#[async_trait]
impl GameServer for PrivateViewGame {
    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        Ok(GameManifest {
            title: "Private fixture".into(),
            version: "1.0.0".into(),
            min_players: 1,
            max_players: 20,
            tick_rate_hz: 5.0,
            persistent_table: false,
            options_schema: None,
            player_view_mode: Some(PlayerViewMode::Private),
            result_definition: None,
        })
    }

    async fn init(&self, config: MatchConfig) -> Result<InitResult, SdkError> {
        Ok(InitResult {
            state: json!({"canonicalHands":{"seat-a":["ACE_OF_SPADES"],"seat-b":["KING_OF_CLUBS"]}}),
            player_views: self.views(&config.players),
            ..Default::default()
        })
    }

    async fn tick(&self, _ctx: TickContext, state: Value) -> Result<TickResult, SdkError> {
        Ok(TickResult {
            state,
            ..Default::default()
        })
    }

    async fn tick_ext(&self, request: TickRequest) -> Result<TickResult, SdkError> {
        Ok(TickResult {
            state: json!({"tick": request.ctx.tick}),
            player_views: self.views(&request.players),
            persist: false,
            ..Default::default()
        })
    }

    async fn on_player_join(
        &self,
        _player_id: String,
        state: Value,
    ) -> Result<PlayerEventResult, SdkError> {
        Ok(PlayerEventResult {
            state,
            ..Default::default()
        })
    }

    async fn on_player_join_ext(
        &self,
        request: PlayerEventRequest,
    ) -> Result<PlayerEventResult, SdkError> {
        Ok(PlayerEventResult {
            state: json!({"joined": request.player_id}),
            player_views: self.views(request.players.as_deref().unwrap_or_default()),
            ..Default::default()
        })
    }

    async fn on_player_leave(
        &self,
        _player_id: String,
        state: Value,
    ) -> Result<PlayerEventResult, SdkError> {
        Ok(PlayerEventResult {
            state,
            ..Default::default()
        })
    }

    async fn on_player_leave_ext(
        &self,
        request: PlayerEventRequest,
    ) -> Result<PlayerEventResult, SdkError> {
        Ok(PlayerEventResult {
            state: json!({"left": request.player_id}),
            player_views: self.views(request.players.as_deref().unwrap_or_default()),
            ..Default::default()
        })
    }

    async fn is_game_over(&self, _state: Value) -> Result<GameResult, SdkError> {
        Ok(GameResult {
            over: false,
            summary: None,
            next_match_policy: None,
            result_units: None,
        })
    }

    async fn on_command_ext(&self, request: CommandRequest) -> Result<CommandResult, SdkError> {
        Ok(CommandResult {
            state: json!({"command": request.command.command_type}),
            player_views: self.views(request.players.as_deref().unwrap_or_default()),
            ..Default::default()
        })
    }

    async fn on_command(
        &self,
        _command: GameCommand,
        state: Value,
    ) -> Result<CommandResult, SdkError> {
        Ok(CommandResult {
            state,
            ..Default::default()
        })
    }
}

async fn start_server(behavior: ViewBehavior) -> (String, JoinHandle<()>) {
    let router = create_game_router(
        Arc::new(PrivateViewGame { behavior }),
        Some("fixture-game_1".into()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{address}"), task)
}

fn fixture_config() -> MatchConfig {
    MatchConfig {
        match_id: "match-private-v1".into(),
        game_id: "fixture-game_1".into(),
        players: vec!["seat-a".into(), "seat-b".into()],
        match_seq: 1,
        options: Default::default(),
        carryover: None,
        tick_rate_hz: None,
    }
}

#[tokio::test]
async fn real_rust_http_lifecycle_returns_private_views_and_preserves_legacy_wire() {
    let (base_url, task) = start_server(ViewBehavior::Valid).await;
    let client = reqwest::Client::new();
    let manifest: Value = client
        .get(format!("{base_url}/manifest"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(manifest["playerViewMode"], "private");
    assert_eq!(manifest["playerViewSchemaVersion"], 1);

    let health: Value = client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["gameId"], "fixture-game_1");

    let fixture: Value = serde_json::from_str(include_str!(
        "../../../protocol/fixtures/room-host-private-v1.json"
    ))
    .unwrap();
    let init: Value = client
        .post(format!("{base_url}/init"))
        .json(&fixture_config())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(init["playerViews"], fixture["playerViews"]);
    assert_eq!(init["playerViews"]["seat-a"]["hand"][0], "ACE_OF_SPADES");
    assert_eq!(init["playerViews"]["seat-b"]["hand"][0], "KING_OF_CLUBS");
    assert!(init["playerViews"]["seat-a"]
        .to_string()
        .find("KING_OF_CLUBS")
        .is_none());

    let roster = vec!["seat-a", "seat-b"];
    let tick: Value = client
        .post(format!("{base_url}/tick"))
        .json(&json!({
            "ctx":{"tick":1,"inputs":[],"commands":[],"elapsedSeconds":0.2},
            "state":{"round":1},"players":roster
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(tick["playerViews"]["seat-a"]["viewer"], "seat-a");

    let empty_tick = client
        .post(format!("{base_url}/tick"))
        .json(&json!({
            "ctx":{"tick":2,"inputs":[],"commands":[],"elapsedSeconds":0.4},
            "state":{"round":2},"players":[]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(empty_tick.status(), reqwest::StatusCode::OK);
    let empty_tick: Value = empty_tick.json().await.unwrap();
    assert_eq!(empty_tick["playerViews"], json!({}));
    let empty_leave: Value = client
        .post(format!("{base_url}/onPlayerLeave"))
        .json(&json!({
            "playerId":"seat-b","state":{"round":2},"players":[]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(empty_leave["playerViews"], json!({}));
    let empty_command: Value = client
        .post(format!("{base_url}/onCommand"))
        .json(&json!({
            "command":{"type":"reset","data":{}},"state":{"round":2},"players":[]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(empty_command["playerViews"], json!({}));

    let join: Value = client
        .post(format!("{base_url}/onPlayerJoin"))
        .json(&json!({
            "playerId":"seat-c","state":{"round":1},"players":["seat-a","seat-b","seat-c"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(join["playerViews"].get("seat-c").is_some());
    let leave: Value = client
        .post(format!("{base_url}/onPlayerLeave"))
        .json(&json!({
            "playerId":"seat-b","state":{"round":1},"players":["seat-a"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(leave["playerViews"].as_object().unwrap().len(), 1);
    let command: Value = client
        .post(format!("{base_url}/onCommand"))
        .json(&json!({
            "command":{"type":"reset","data":{}},"state":{"round":1},"players":roster
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(command["playerViews"].as_object().unwrap().len(), 2);
    task.abort();
}

#[tokio::test]
async fn actual_gamecreator_host_requests_round_trip_through_rust_http_routes() {
    let (base_url, task) = start_server(ViewBehavior::Valid).await;
    let client = reqwest::Client::new();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../protocol/fixtures/gamecreator-room-host-private-requests-v1.json"
    ))
    .unwrap();
    let requests = [
        ("/tick", fixture["tickActive"].clone()),
        ("/onCommand", fixture["command"].clone()),
        ("/onPlayerJoin", fixture["join"].clone()),
        ("/onPlayerLeave", fixture["leaveJoined"].clone()),
        ("/onPlayerLeave", fixture["leaveFirst"].clone()),
        ("/onPlayerLeave", fixture["leaveLast"].clone()),
        ("/tick", fixture["tickEmpty"].clone()),
    ];
    let mut last_leave = Value::Null;
    let mut empty_tick = Value::Null;
    for (index, (route, body)) in requests.iter().enumerate() {
        let response = client
            .post(format!("{base_url}{route}"))
            .json(body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::OK,
            "{route}: {body}"
        );
        let result: Value = response.json().await.unwrap();
        if index == 5 {
            last_leave = result;
        } else if index == 6 {
            empty_tick = result;
        }
    }
    assert_eq!(last_leave["playerViews"], json!({}));
    assert_eq!(empty_tick["playerViews"], json!({}));
    task.abort();
}

#[tokio::test]
async fn rust_private_http_routes_reject_incomplete_rosters_and_bad_views() {
    let client = reqwest::Client::new();
    let (base_url, task) = start_server(ViewBehavior::Valid).await;
    let missing_roster = client
        .post(format!("{base_url}/tick"))
        .json(&json!({
            "ctx":{"tick":1,"inputs":[],"commands":[],"elapsedSeconds":0.2},"state":{}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing_roster.status(), reqwest::StatusCode::BAD_REQUEST);
    let outsider_on_empty = client.post(format!("{base_url}/tick")).json(&json!({
        "ctx":{"tick":1,"inputs":[{"playerId":"seat-a","data":{}}],"commands":[],"elapsedSeconds":0.2},
        "state":{},"players":[]
    })).send().await.unwrap();
    assert_eq!(outsider_on_empty.status(), reqwest::StatusCode::BAD_REQUEST);
    let null_roster = client.post(format!("{base_url}/tick")).json(&json!({
        "ctx":{"tick":1,"inputs":[],"commands":[],"elapsedSeconds":0.2},"state":{},"players":null
    })).send().await.unwrap();
    assert_eq!(null_roster.status(), reqwest::StatusCode::BAD_REQUEST);
    let bad_join = client
        .post(format!("{base_url}/onPlayerJoin"))
        .json(&json!({
            "playerId":"seat-c","state":{},"players":["seat-a","seat-b"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_join.status(), reqwest::StatusCode::BAD_REQUEST);
    let malformed = client
        .post(format!("{base_url}/tick"))
        .body("{")
        .send()
        .await
        .unwrap();
    assert_eq!(malformed.status(), reqwest::StatusCode::BAD_REQUEST);
    task.abort();

    for (behavior, expected) in [
        (
            ViewBehavior::Missing,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            ViewBehavior::MissingSeat,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            ViewBehavior::Extra,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            ViewBehavior::EmptyKey,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            ViewBehavior::OversizedView,
            reqwest::StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let (base_url, task) = start_server(behavior).await;
        let response = client
            .post(format!("{base_url}/init"))
            .json(&fixture_config())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        task.abort();
    }

    let (base_url, task) = start_server(ViewBehavior::OversizedMap).await;
    let mut config = fixture_config();
    config.players = (0..17).map(|index| format!("seat-{index:02}")).collect();
    let response = client
        .post(format!("{base_url}/init"))
        .json(&config)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    task.abort();
}

#[test]
fn rust_manifest_deserializes_explicit_v1_without_breaking_public_struct_literals() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../protocol/fixtures/room-host-private-v1.json"
    ))
    .unwrap();
    let explicit: GameManifest =
        serde_json::from_value(fixture["manifestCases"][2]["manifest"].clone()).unwrap();
    assert_eq!(explicit.player_view_mode, Some(PlayerViewMode::Private));
    assert_eq!(
        serde_json::to_value(explicit).unwrap()["playerViewSchemaVersion"],
        1
    );
}

#[test]
fn manifest_options_schema_accepts_only_json_schema_object_or_boolean() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../protocol/fixtures/room-host-private-v1.json"
    ))
    .unwrap();
    let mut manifest: GameManifest =
        serde_json::from_value(fixture["manifestCases"][0]["manifest"].clone()).unwrap();

    for schema in [
        json!({"type":"object", "properties":{"mode":{"type":"string"}}}),
        json!(true),
        json!(false),
    ] {
        manifest.options_schema = Some(schema);
        assert!(manifest.validate().is_ok());
    }
    for schema in [json!(null), json!([]), json!("object"), json!(1)] {
        manifest.options_schema = Some(schema);
        assert!(manifest.validate().is_err());
    }
}
