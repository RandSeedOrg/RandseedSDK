use async_trait::async_trait;
use randseed_game_sdk::{
    run_game_server, CommandResult, GameCommand, GameManifest, GameResult, GameServer, InitResult,
    MatchConfig, MatchSummary, NextMatchPolicy, PlayerEventResult, SdkError, TickContext,
    TickResult,
};
use serde_json::{json, Value};

struct CooperativeCounter;

#[async_trait]
impl GameServer for CooperativeCounter {
    async fn manifest(&self) -> Result<GameManifest, SdkError> {
        Ok(GameManifest {
            title: "Cooperative Counter".into(),
            version: "1.0.0".into(),
            min_players: 2,
            max_players: 4,
            tick_rate_hz: 5.0,
            persistent_table: false,
            options_schema: None,
        })
    }

    async fn init(&self, config: MatchConfig) -> Result<InitResult, SdkError> {
        let count = config
            .carryover
            .as_ref()
            .and_then(Value::as_i64)
            .unwrap_or(0);

        Ok(InitResult {
            state: json!({
                "count": count,
                "players": config.players,
            }),
            broadcasts: Some(vec![json!({ "type": "counter.started", "count": count })]),
        })
    }

    async fn tick(&self, ctx: TickContext, mut state: Value) -> Result<TickResult, SdkError> {
        let count = state.get("count").and_then(Value::as_i64).ok_or_else(|| {
            SdkError::Validation("Game state must contain an integer count.".into())
        })?;

        let mut next_count = count;
        for input in &ctx.inputs {
            let delta = input
                .data
                .get("delta")
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    SdkError::Validation("Each input must contain an integer delta.".into())
                })?;
            next_count = next_count
                .checked_add(delta)
                .ok_or_else(|| SdkError::Validation("The counter value is out of range.".into()))?;
        }

        state["count"] = json!(next_count);
        let broadcasts = if ctx.inputs.is_empty() {
            None
        } else {
            Some(vec![
                json!({ "type": "counter.changed", "count": next_count }),
            ])
        };

        Ok(TickResult {
            state,
            broadcasts,
            persist: false,
        })
    }

    async fn on_player_join(
        &self,
        player_id: String,
        mut state: Value,
    ) -> Result<PlayerEventResult, SdkError> {
        let players = state
            .get_mut("players")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                SdkError::Validation("Game state must contain a players array.".into())
            })?;
        if !players
            .iter()
            .any(|player| player.as_str() == Some(&player_id))
        {
            players.push(json!(player_id.clone()));
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
        mut state: Value,
    ) -> Result<PlayerEventResult, SdkError> {
        let players = state
            .get_mut("players")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                SdkError::Validation("Game state must contain a players array.".into())
            })?;
        players.retain(|player| player.as_str() != Some(&player_id));

        Ok(PlayerEventResult {
            state,
            broadcasts: Some(vec![
                json!({ "type": "player_left", "playerId": player_id }),
            ]),
        })
    }

    async fn is_game_over(&self, state: Value) -> Result<GameResult, SdkError> {
        let count = state.get("count").and_then(Value::as_i64).ok_or_else(|| {
            SdkError::Validation("Game state must contain an integer count.".into())
        })?;

        if count >= 20 {
            Ok(GameResult {
                over: true,
                summary: Some(MatchSummary {
                    winner_player_id: None,
                    scores: None,
                    duration_seconds: 0.0,
                    custom_metrics: Some(json!({ "count": count })),
                    extra: None,
                }),
                next_match_policy: Some(NextMatchPolicy {
                    mode: "close_room".into(),
                    timeout_ms: 0,
                    carryover: None,
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
        command: GameCommand,
        mut state: Value,
    ) -> Result<CommandResult, SdkError> {
        if command.command_type != "reset" {
            return Err(SdkError::Validation(
                "Only the reset command is supported.".into(),
            ));
        }
        state["count"] = json!(0);
        Ok(CommandResult {
            state,
            broadcasts: Some(vec![json!({ "type": "counter.reset" })]),
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    run_game_server(CooperativeCounter, port).await
}
