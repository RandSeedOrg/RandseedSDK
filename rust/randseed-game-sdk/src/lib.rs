//! RandSeed game server SDK for Rust.
//!
//! Provides the room-host HTTP protocol runtime, deterministic Mulberry32,
//! replay contracts, and a client for room-host platform services.

#![warn(missing_docs)]

pub mod agent_capabilities;
pub mod agent_observation;
pub mod errors;
pub mod platform;
pub mod prng;
pub mod replay;
pub mod server;
pub mod types;

// Re-export the primary public traits, types, and functions.
pub use agent_capabilities::{AgentControlMode, AgentGameCapabilities};
pub use agent_observation::{
    create_agent_observation, AgentObservation, AgentObservationContext, AgentObservationSelf,
};
pub use errors::SdkError;
pub use platform::{PlatformClient, PlayerInfo};
pub use prng::Mulberry32;
pub use replay::{
    evaluate_replay_storage_tier, validate_replay_data_v2, ReplayCommandItem, ReplayDataV2,
    ReplayInputItem, ReplayStorageTier, TIER_A_MAX_COMPRESSED_BYTES, TIER_B_MAX_COMPRESSED_BYTES,
};
pub use server::{create_game_router, run_game_server, GameServer};
pub use types::{
    validate_game_result_units, CommandRequest, CommandResult, GameCommand, GameManifest,
    GameMetricAggregation, GameMetricDefinition, GameResult, GameResultCorrection,
    GameResultCorrectionAction, GameResultDefinition, GameResultEvent, GameResultParticipant,
    GameResultTeam, HealthResponse, InitResult, IsGameOverRequest, MatchConfig, MatchOptions,
    MatchSummary, NextMatchPolicy, PlayerEventRequest, PlayerEventResult, RatioMetricAggregation,
    ResultMetricScope, ResultOutcome, SimpleMetricAggregation, TickContext, TickInput, TickRequest,
    TickResult,
};
