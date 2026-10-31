//! Deterministic replay contracts and storage tiers for the RandSeed SDK.
//!
//! Defines the `ReplayDataV2` format and its storage-tier thresholds.

use crate::errors::SdkError;
use crate::types::{MatchConfig, MatchSummary};
use serde::{Deserialize, Serialize};

/// One player input recorded in a replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayInputItem {
    /// Simulation tick when the input was applied.
    pub tick: u64,
    /// Player identifier.
    #[serde(rename = "playerId")]
    pub player_id: String,
    /// Game-defined input data.
    pub data: serde_json::Value,
    /// Optional client timestamp in epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

/// One trusted host command recorded in a replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayCommandItem {
    /// Simulation tick when the command was applied.
    pub tick: u64,
    /// Command type.
    #[serde(rename = "type")]
    pub command_type: String,
    /// Command data.
    pub data: serde_json::Value,
    /// Optional command timestamp in epoch milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

/// Version 2 format for deterministic match replays.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayDataV2 {
    /// Replay version, fixed at 2.
    pub version: u32,
    /// Game identifier.
    #[serde(rename = "gameId")]
    pub game_id: String,
    /// Match identifier.
    #[serde(rename = "matchId")]
    pub match_id: String,
    /// Copy of the match configuration.
    #[serde(rename = "matchConfig")]
    pub match_config: MatchConfig,
    /// Initial state snapshot.
    #[serde(rename = "initState")]
    pub init_state: serde_json::Value,
    /// All player inputs, ordered by non-decreasing tick.
    pub inputs: Vec<ReplayInputItem>,
    /// All trusted host commands.
    #[serde(default)]
    pub commands: Vec<ReplayCommandItem>,
    /// Match summary.
    pub summary: MatchSummary,
    /// Replay creation time in epoch milliseconds.
    #[serde(rename = "createdAt")]
    pub created_at: u64,
}

/// Storage tier selected for a compressed replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayStorageTier {
    /// Tier A: under 1.8 MB compressed.
    TierA,
    /// Tier B: at least 1.8 MB and under 10 MB compressed.
    TierB,
    /// Tier C: at least 10 MB compressed.
    TierC,
}

/// Tier A upper threshold in bytes (1.8 MiB).
pub const TIER_A_MAX_COMPRESSED_BYTES: usize = 1_887_436;

/// Tier B upper threshold in bytes (10 MiB).
pub const TIER_B_MAX_COMPRESSED_BYTES: usize = 10_485_760;

/// Validate the structure and tick ordering of a `ReplayDataV2`.
pub fn validate_replay_data_v2(replay: &ReplayDataV2) -> Result<(), SdkError> {
    if replay.version != 2 {
        return Err(SdkError::Validation(format!(
            "Replay version must be 2; received {}.",
            replay.version
        )));
    }
    if replay.game_id.trim().is_empty() {
        return Err(SdkError::Validation(
            "gameId must be a non-empty string.".into(),
        ));
    }
    if replay.match_id.trim().is_empty() {
        return Err(SdkError::Validation(
            "matchId must be a non-empty string.".into(),
        ));
    }
    if replay.created_at == 0 {
        return Err(SdkError::Validation(
            "createdAt must be a valid timestamp.".into(),
        ));
    }

    let mut prev_tick: i64 = -1;
    for input in &replay.inputs {
        if (input.tick as i64) < prev_tick {
            return Err(SdkError::Validation(format!(
                "inputs must be ordered by non-decreasing tick: {} < {}.",
                input.tick, prev_tick
            )));
        }
        if input.player_id.trim().is_empty() {
            return Err(SdkError::Validation(
                "input.playerId must be a non-empty string.".into(),
            ));
        }
        prev_tick = input.tick as i64;
    }

    for cmd in &replay.commands {
        if cmd.command_type.trim().is_empty() {
            return Err(SdkError::Validation(
                "command.type must be a non-empty string.".into(),
            ));
        }
    }

    Ok(())
}

/// Select a storage tier from the compressed replay size.
pub fn evaluate_replay_storage_tier(compressed_bytes: usize) -> ReplayStorageTier {
    if compressed_bytes < TIER_A_MAX_COMPRESSED_BYTES {
        ReplayStorageTier::TierA
    } else if compressed_bytes < TIER_B_MAX_COMPRESSED_BYTES {
        ReplayStorageTier::TierB
    } else {
        ReplayStorageTier::TierC
    }
}
