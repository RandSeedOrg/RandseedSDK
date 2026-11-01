//! Core data contracts for the RandSeed game server SDK.
//!
//! Structs and serialization names follow the room-host wire protocol.

use crate::errors::SdkError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Game metadata returned by `GET /manifest`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameManifest {
    /// Display title.
    pub title: String,
    /// Semantic version.
    pub version: String,
    /// Minimum number of players in a room; must be at least one.
    #[serde(rename = "minPlayers")]
    pub min_players: u32,
    /// Maximum number of players in a room; must not be less than `min_players`.
    #[serde(rename = "maxPlayers")]
    pub max_players: u32,
    /// Simulation tick frequency in hertz, from 1 through 60.
    #[serde(rename = "tickRateHz")]
    pub tick_rate_hz: f64,
    /// Whether this game supports persistent rooms across matches.
    #[serde(rename = "persistentTable")]
    pub persistent_table: bool,
    /// Optional JSON Schema for match options.
    #[serde(rename = "optionsSchema", skip_serializing_if = "Option::is_none")]
    pub options_schema: Option<serde_json::Value>,
}

impl GameManifest {
    /// Validate the manifest against the room-host protocol limits.
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.title.trim().is_empty() {
            return Err(SdkError::Validation(
                "title must be a non-empty string.".into(),
            ));
        }
        if self.version.trim().is_empty() {
            return Err(SdkError::Validation(
                "version must be a non-empty string.".into(),
            ));
        }
        if self.min_players < 1 {
            return Err(SdkError::Validation(
                "minPlayers must be at least 1.".into(),
            ));
        }
        if self.max_players < self.min_players {
            return Err(SdkError::Validation(
                "maxPlayers must not be less than minPlayers.".into(),
            ));
        }
        if self.max_players > 100 {
            return Err(SdkError::Validation(
                "maxPlayers must not exceed 100.".into(),
            ));
        }
        if !self.tick_rate_hz.is_finite() || !(1.0..=60.0).contains(&self.tick_rate_hz) {
            return Err(SdkError::Validation(
                "tickRateHz must be a finite number between 1 and 60.".into(),
            ));
        }
        Ok(())
    }
}

/// Match options, including the numeric seed used by deterministic games.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchOptions {
    /// Initial seed for a deterministic pseudo-random number generator.
    #[serde(default)]
    pub seed: f64,
    /// Additional game-specific options.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Default for MatchOptions {
    fn default() -> Self {
        Self {
            seed: 0.0,
            extra: HashMap::new(),
        }
    }
}

/// Match configuration passed to `POST /init`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchConfig {
    /// Unique match identifier.
    #[serde(rename = "matchId")]
    pub match_id: String,
    /// Unique game identifier.
    #[serde(rename = "gameId")]
    pub game_id: String,
    /// Unique player identifiers in this match.
    pub players: Vec<String>,
    /// Positive safe-integer match sequence number within a room.
    #[serde(rename = "matchSeq")]
    pub match_seq: u64,
    /// Game-specific match options.
    #[serde(default)]
    pub options: MatchOptions,
    /// Opaque carryover data from a previous match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carryover: Option<serde_json::Value>,
    /// Optional match-specific tick rate in hertz, from 1 through 60.
    #[serde(
        rename = "tickRateHz",
        default,
        deserialize_with = "deserialize_optional_tick_rate",
        skip_serializing_if = "Option::is_none"
    )]
    pub tick_rate_hz: Option<f64>,
}

fn deserialize_optional_tick_rate<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    f64::deserialize(deserializer).map(Some)
}

impl MatchConfig {
    /// Validate the match configuration.
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.match_id.trim().is_empty() {
            return Err(SdkError::Validation(
                "matchId must be a non-empty string.".into(),
            ));
        }
        if self.game_id.trim().is_empty() {
            return Err(SdkError::Validation(
                "gameId must be a non-empty string.".into(),
            ));
        }
        if self.match_seq == 0 || self.match_seq > MAX_SAFE_INTEGER {
            return Err(SdkError::Validation(
                "matchSeq must be a positive safe integer.".into(),
            ));
        }
        if self.players.is_empty() {
            return Err(SdkError::Validation(
                "players must be a non-empty array.".into(),
            ));
        }
        if self.players.len() > 100 {
            return Err(SdkError::Validation(
                "players must not contain more than 100 entries.".into(),
            ));
        }
        let mut unique_players = HashSet::with_capacity(self.players.len());
        for player_id in &self.players {
            if player_id.trim().is_empty() {
                return Err(SdkError::Validation(
                    "player IDs must be non-empty strings.".into(),
                ));
            }
            if !unique_players.insert(player_id) {
                return Err(SdkError::Validation(
                    "players must contain unique player IDs.".into(),
                ));
            }
        }
        if !self.options.seed.is_finite() {
            return Err(SdkError::Validation(
                "options.seed must be a finite number.".into(),
            ));
        }
        if let Some(tick_rate_hz) = self.tick_rate_hz {
            if !tick_rate_hz.is_finite() || !(1.0..=60.0).contains(&tick_rate_hz) {
                return Err(SdkError::Validation(
                    "tickRateHz must be a finite number between 1 and 60.".into(),
                ));
            }
        }
        Ok(())
    }
}

/// One player's input submitted for a simulation tick.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TickInput {
    /// Player identifier.
    #[serde(rename = "playerId")]
    pub player_id: String,
    /// Game-defined input data.
    pub data: serde_json::Value,
    /// Optional non-negative safe-integer client timestamp in epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

/// A trusted command issued by the room host or an administrator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameCommand {
    /// Command type.
    #[serde(rename = "type")]
    pub command_type: String,
    /// Optional command data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Optional command issuer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    /// Optional non-negative safe-integer command timestamp in epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

/// Context supplied for one deterministic simulation tick.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TickContext {
    /// Non-negative safe-integer simulation tick.
    pub tick: u64,
    /// Player inputs collected for this tick.
    pub inputs: Vec<TickInput>,
    /// Trusted commands applied during this tick.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<GameCommand>,
    /// Non-negative elapsed simulation time in seconds.
    #[serde(rename = "elapsedSeconds")]
    pub elapsed_seconds: f64,
}

impl TickContext {
    /// Validate the tick context.
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.tick > MAX_SAFE_INTEGER {
            return Err(SdkError::Validation(
                "tick must be a non-negative safe integer.".into(),
            ));
        }
        if !self.elapsed_seconds.is_finite() || self.elapsed_seconds < 0.0 {
            return Err(SdkError::Validation(
                "elapsedSeconds must be a finite, non-negative number.".into(),
            ));
        }
        for item in &self.inputs {
            if item.player_id.trim().is_empty() {
                return Err(SdkError::Validation(
                    "input.playerId must be a non-empty string.".into(),
                ));
            }
            if item
                .timestamp
                .is_some_and(|timestamp| timestamp > MAX_SAFE_INTEGER)
            {
                return Err(SdkError::Validation(
                    "input.timestamp must be a non-negative safe integer.".into(),
                ));
            }
        }
        for command in &self.commands {
            if command
                .timestamp
                .is_some_and(|timestamp| timestamp > MAX_SAFE_INTEGER)
            {
                return Err(SdkError::Validation(
                    "command.timestamp must be a non-negative safe integer.".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Result returned from match initialization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InitResult {
    /// Initial game state.
    pub state: serde_json::Value,
    /// Optional messages to broadcast when the match starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcasts: Option<Vec<serde_json::Value>>,
}

/// Result returned after processing one simulation tick.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TickResult {
    /// Updated game state.
    pub state: serde_json::Value,
    /// Optional messages to broadcast.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcasts: Option<Vec<serde_json::Value>>,
    /// Whether the room host should persist a state snapshot.
    pub persist: bool,
}

/// Result returned after a player lifecycle event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerEventResult {
    /// Updated game state.
    pub state: serde_json::Value,
    /// Optional messages to broadcast.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcasts: Option<Vec<serde_json::Value>>,
}

/// Policy for what the room host should do after a match ends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NextMatchPolicy {
    /// One of `auto_next_match`, `wait_for_ready`, or `close_room`.
    pub mode: String,
    /// Delay before applying the policy, in milliseconds.
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: u64,
    /// Opaque data to pass to the next match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carryover: Option<serde_json::Value>,
}

/// Match summary returned when a game ends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MatchSummary {
    /// Optional winning player identifier.
    #[serde(rename = "winnerPlayerId", skip_serializing_if = "Option::is_none")]
    pub winner_player_id: Option<String>,
    /// Optional player scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scores: Option<HashMap<String, f64>>,
    /// Match duration in seconds.
    #[serde(rename = "durationSeconds")]
    pub duration_seconds: f64,
    /// Optional game-specific metrics.
    #[serde(rename = "customMetrics", skip_serializing_if = "Option::is_none")]
    pub custom_metrics: Option<serde_json::Value>,
    /// Optional additional data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// Result of checking whether a match has ended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameResult {
    /// Whether the match has ended.
    pub over: bool,
    /// Required when `over` is `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<MatchSummary>,
    /// Required when `over` is `true`.
    #[serde(rename = "nextMatchPolicy", skip_serializing_if = "Option::is_none")]
    pub next_match_policy: Option<NextMatchPolicy>,
}

/// Result returned after processing a trusted command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandResult {
    /// Updated game state.
    pub state: serde_json::Value,
    /// Optional messages to broadcast.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcasts: Option<Vec<serde_json::Value>>,
}

/// Request payload for `POST /tick`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickRequest {
    /// Tick context.
    pub ctx: TickContext,
    /// Current game state.
    pub state: serde_json::Value,
}

/// Request payload for a player lifecycle event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerEventRequest {
    /// Player identifier.
    #[serde(rename = "playerId")]
    pub player_id: String,
    /// Current game state.
    pub state: serde_json::Value,
}

/// Request payload for `POST /isGameOver`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsGameOverRequest {
    /// Current game state.
    pub state: serde_json::Value,
}

/// Request payload for `POST /onCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandRequest {
    /// Trusted command.
    pub command: GameCommand,
    /// Current game state.
    pub state: serde_json::Value,
}

/// Response payload for `GET /health`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthResponse {
    /// Health status, normally `"ok"`.
    pub status: String,
    /// Game identifier.
    #[serde(rename = "gameId")]
    pub game_id: String,
    /// Container uptime in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<f64>,
}
