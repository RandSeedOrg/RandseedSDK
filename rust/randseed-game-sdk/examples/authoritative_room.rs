use async_trait::async_trait;
use randseed_game_sdk::{
    run_game_server, GameActionOutcome, GameActionResult, GameManifest, GameResult, GameServer,
    InitResult, MatchConfig, MatchSummary, NextMatchPolicy, PlayerEventResult, PlayerViewMode,
    SdkError, SdkErrorCode, TickContext, TickResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CounterState {
    version: u32,
    tick: u64,
    count: i64,
    target_score: i64,
    players: Vec<String>,
    private_scores: HashMap<String, i64>,
}

impl CounterState {
    fn player_views(&self) -> HashMap<String, Value> {
        self.players
            .iter()
            .map(|player_id| {
                (
                    player_id.clone(),
                    json!({
                        "tick": self.tick,
                        "count": self.count,
                        "ownScore": self.private_scores.get(player_id).copied().unwrap_or_default(),
                    }),
                )
            })
            .collect()
    }
}

struct AuthoritativeCounter;

#[async_trait]
impl GameServer for AuthoritativeCounter {
    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        Ok(GameManifest {
            title: "Authoritative Counter".into(),
            version: "1.0.0".into(),
            min_players: 2,
            max_players: 4,
            tick_rate_hz: 10.0,
            persistent_table: true,
            options_schema: Some(json!({
                "type": "object",
                "properties": {
                    "targetScore": { "type": "integer", "minimum": 1, "maximum": 100 }
                },
                "additionalProperties": false
            })),
            player_view_mode: Some(PlayerViewMode::Private),
            result_definition: None,
        })
    }

    async fn init(&self, config: MatchConfig) -> Result<InitResult, SdkError> {
        let target_score = match config.options.extra.get("targetScore") {
            None => 5,
            Some(value) => value.as_i64().ok_or_else(|| {
                SdkError::protocol_with_message(
                    SdkErrorCode::InvalidOptions,
                    "targetScore must be an integer from 1 to 100.",
                )
            })?,
        };
        if !(1..=100).contains(&target_score) {
            return Err(SdkError::protocol_with_message(
                SdkErrorCode::InvalidOptions,
                "targetScore must be an integer from 1 to 100.",
            ));
        }
        let state = CounterState {
            version: 1,
            tick: 0,
            count: 0,
            target_score,
            private_scores: config
                .players
                .iter()
                .map(|player_id| (player_id.clone(), 0))
                .collect(),
            players: config.players,
        };
        Ok(InitResult {
            state: serde_json::to_value(&state)?,
            player_views: Some(state.player_views()),
            joinable: Some(true),
            ..Default::default()
        })
    }

    async fn tick(&self, ctx: TickContext, state: Value) -> Result<TickResult, SdkError> {
        let mut state: CounterState = serde_json::from_value(state)
            .map_err(|_| SdkError::protocol(SdkErrorCode::InvalidState))?;
        state.tick = ctx.tick;
        let mut action_results = Vec::new();
        for input in &ctx.inputs {
            let delta = input.data.get("delta").and_then(Value::as_i64);
            let status = if !state.players.contains(&input.player_id) || delta.is_none() {
                GameActionOutcome::Rejected
            } else {
                let delta = delta.expect("checked above");
                state.count += delta;
                *state
                    .private_scores
                    .entry(input.player_id.clone())
                    .or_default() += delta;
                GameActionOutcome::Applied
            };
            if let Some(action_id) = &input.action_id {
                action_results.push(GameActionResult {
                    action_id: action_id.clone(),
                    status,
                    code: (status == GameActionOutcome::Rejected)
                        .then(|| "INVALID_MOVE".to_string()),
                });
            }
        }
        Ok(TickResult {
            state: serde_json::to_value(&state)?,
            player_views: Some(state.player_views()),
            persist: state.count >= state.target_score,
            action_results: (!action_results.is_empty()).then_some(action_results),
            ..Default::default()
        })
    }

    async fn on_player_join(
        &self,
        player_id: String,
        state: Value,
    ) -> Result<PlayerEventResult, SdkError> {
        let mut state: CounterState = serde_json::from_value(state)
            .map_err(|_| SdkError::protocol(SdkErrorCode::InvalidState))?;
        if !state.players.contains(&player_id) {
            state.players.push(player_id.clone());
        }
        state.private_scores.entry(player_id).or_default();
        Ok(PlayerEventResult {
            state: serde_json::to_value(&state)?,
            player_views: Some(state.player_views()),
            joinable: Some(state.players.len() < 4),
            ..Default::default()
        })
    }

    async fn on_player_leave(
        &self,
        player_id: String,
        state: Value,
    ) -> Result<PlayerEventResult, SdkError> {
        let mut state: CounterState = serde_json::from_value(state)
            .map_err(|_| SdkError::protocol(SdkErrorCode::InvalidState))?;
        state.players.retain(|active_id| active_id != &player_id);
        state.private_scores.remove(&player_id);
        Ok(PlayerEventResult {
            state: serde_json::to_value(&state)?,
            player_views: Some(state.player_views()),
            joinable: Some(true),
            ..Default::default()
        })
    }

    async fn is_game_over(&self, state: Value) -> Result<GameResult, SdkError> {
        let state: CounterState = serde_json::from_value(state)
            .map_err(|_| SdkError::protocol(SdkErrorCode::InvalidState))?;
        let over = state.count >= state.target_score;
        Ok(GameResult {
            over,
            summary: over.then_some(MatchSummary {
                winner_player_id: None,
                scores: None,
                duration_seconds: state.tick as f64 / 10.0,
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

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    run_game_server(AuthoritativeCounter, port).await
}
