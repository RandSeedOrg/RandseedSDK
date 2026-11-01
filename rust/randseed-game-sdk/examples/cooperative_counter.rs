use async_trait::async_trait;
use randseed_game_sdk::{
    run_game_server, CommandResult, GameCommand, GameManifest, GameMetricAggregation,
    GameMetricDefinition, GameResult, GameResultDefinition, GameResultEvent, GameResultParticipant,
    GameServer, InitResult, MatchConfig, MatchSummary, NextMatchPolicy, PlayerEventResult,
    ResultMetricScope, SdkError, TickContext, TickResult,
};
use serde_json::{json, Value};
use std::collections::HashMap;

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
            result_definition: Some(GameResultDefinition {
                version: "counter-results-v1".into(),
                unit_types: vec!["milestone".into()],
                metrics: vec![GameMetricDefinition {
                    id: "milestonesReached".into(),
                    scope: ResultMetricScope::Participant,
                    unit: "milestones".into(),
                    unit_types: vec!["milestone".into()],
                    aggregation: GameMetricAggregation::Simple(
                        randseed_game_sdk::SimpleMetricAggregation::Sum,
                    ),
                }],
            }),
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
                "matchId": config.match_id,
                "count": count,
                "players": config.players,
            }),
            broadcasts: Some(vec![json!({ "type": "counter.started", "count": count })]),
            result_units: None,
        })
    }

    async fn tick(&self, ctx: TickContext, mut state: Value) -> Result<TickResult, SdkError> {
        let count = state.get("count").and_then(Value::as_i64).ok_or_else(|| {
            SdkError::Validation("Game state must contain an integer count.".into())
        })?;
        let match_id = state
            .get("matchId")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Validation("Game state must contain a matchId.".into()))?
            .to_string();
        let players = state
            .get("players")
            .and_then(Value::as_array)
            .ok_or_else(|| SdkError::Validation("Game state must contain a players array.".into()))?
            .iter()
            .map(|player| {
                player
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| SdkError::Validation("Player IDs must be strings.".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;

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

        let result_units = [10_i64, 20]
            .into_iter()
            .filter(|milestone| count < *milestone && next_count >= *milestone)
            .map(|milestone| GameResultEvent {
                schema_version: 1,
                event_id: format!("{match_id}:milestone:{milestone}"),
                match_id: match_id.clone(),
                unit_id: format!("milestone-{milestone}"),
                unit_type: "milestone".into(),
                rules_version: "counter-rules-v1".into(),
                definition_version: "counter-results-v1".into(),
                occurred_at: None,
                participants: players
                    .iter()
                    .map(|player_id| GameResultParticipant {
                        player_id: player_id.clone(),
                        team_id: None,
                        outcome: None,
                        rank: None,
                        metrics: HashMap::from([("milestonesReached".into(), 1.0)]),
                    })
                    .collect(),
                teams: None,
                correction: None,
            })
            .collect::<Vec<_>>();

        Ok(TickResult {
            state,
            broadcasts,
            persist: !result_units.is_empty(),
            result_units: (!result_units.is_empty()).then_some(result_units),
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
            result_units: None,
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
            result_units: None,
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
    run_game_server(CooperativeCounter, port).await
}
