//! RandSeed game server SDK for Rust.
//!
//! Provides the room-host HTTP protocol runtime, deterministic ChaCha20Rng (plus
//! legacy Mulberry32), replay contracts, and a client for room-host platform services.

#![warn(missing_docs)]

pub mod agent_capabilities;
pub mod agent_observation;
pub mod agent_player_view;
pub mod errors;
pub mod game_data;
pub mod platform;
pub mod prng;
pub mod replay;
pub mod server;
pub mod stream;
pub mod types;

// Re-export the primary public traits, types, and functions.
pub use agent_capabilities::{AgentControlMode, AgentGameCapabilities};
pub use agent_observation::{
    create_agent_observation, AgentObservation, AgentObservationContext, AgentObservationSelf,
};
pub use agent_player_view::{
    create_agent_player_view, AgentActionReceipt, AgentActionReceiptStatus, AgentActionRequest,
    AgentController, AgentControllerLease, AgentPlayerContract, AgentPlayerDecisionState,
    AgentPlayerMode, AgentPlayerView, AgentPlayerViewContext, AgentPlayerViewProjection,
    AgentPlayerViewRequest, AgentPlayerViewScope, AgentPlayerViewSelf, AgentSkillDecision,
    MAX_AGENT_PLAYER_ACTION_BYTES, MAX_AGENT_PLAYER_MEMORY_BYTES, MAX_AGENT_PLAYER_REQUEST_BYTES,
    MAX_AGENT_PLAYER_VIEW_BYTES,
};
pub use errors::{SdkError, SdkErrorCode};
pub use game_data::{
    GameDataAggregate, GameDataAggregateMetric, GameDataAggregateOperator, GameDataAggregateResult,
    GameDataClient, GameDataFieldType, GameDataFilter, GameDataFilterOperator, GameDataMutation,
    GameDataQuery, GameDataQueryResult, GameDataRecord, GameDataSchema, GameDataSort,
    GameDataSortDirection, GameDataWriteReceipt, GameDataWriteReceiptItem, GameDataWriteRequest,
    GAME_DATA_PROTOCOL_VERSION,
};
pub use platform::{PlatformClient, PlayerInfo};
pub use prng::{
    derive_seed, derive_seed_hex, derive_seed_strs, encode_seed_hex, is_valid_seed_hex,
    legacy_seed_from_bytes, parse_seed_hex, ChaCha20Rng, ChaCha20State, Mulberry32,
    DERIVE_SEED_DOMAIN_PREFIX, MAX_SAFE_CURSOR, RANDOM_SEED_BYTES, RANDOM_SEED_HEX_LEN,
};
pub use replay::{
    evaluate_replay_storage_tier, validate_replay_data_v2, ReplayCommandItem, ReplayDataV2,
    ReplayInputItem, ReplayStorageTier, TIER_A_MAX_COMPRESSED_BYTES, TIER_B_MAX_COMPRESSED_BYTES,
};
pub use server::{
    create_game_router, create_game_router_with_stream, create_typed_game_router, run_game_server,
    run_typed_game_server, GameServer,
};
pub use stream::{
    ManifestCapabilities, ManifestWireResponse, StreamClientMessage, StreamDurability,
    StreamReplayStep, StreamServerMessage, StreamStepEvent, TypedEventOutcome, TypedGameAdapter,
    TypedGameServer, TypedInitResult, TypedTickResult, STREAM_PROTOCOL_V1,
};
pub use types::{
    validate_game_result_units, validate_player_views, CommandRequest, CommandResult,
    GameActionOutcome, GameActionResult, GameCommand, GameManifest, GameMetricAggregation,
    GameMetricDefinition, GameResult, GameResultCorrection, GameResultCorrectionAction,
    GameResultDefinition, GameResultEvent, GameResultParticipant, GameResultTeam, HealthResponse,
    InitResult, IsGameOverRequest, MatchConfig, MatchOptions, MatchSummary, NextMatchPolicy,
    PlayerEventRequest, PlayerEventResult, PlayerViewMode, RatioMetricAggregation,
    ResultMetricScope, ResultOutcome, SimpleMetricAggregation, TickContext, TickInput, TickRequest,
    TickRequestWithPlayers, TickResult, MAX_PLAYER_VIEW_BYTES,
};
