//! Core data contracts for the RandSeed game server SDK.
//!
//! Structs and serialization names follow the room-host wire protocol.

use crate::errors::SdkError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_RESULT_EVENT_BYTES: usize = 256 * 1024;
const MAX_RESULT_PARTICIPANTS: usize = 1000;
const MAX_RESULT_TEAMS: usize = 256;
const MAX_METRICS_PER_ENTITY: usize = 128;
const MAX_RESULT_EVENTS_PER_TRANSITION: usize = 32;
const MAX_RESULT_METRICS: usize = 128;
const MAX_IDENTIFIER_CODE_POINTS: usize = 128;

/// Scope at which a result metric is recorded.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResultMetricScope {
    /// One value per participating player.
    Participant,
    /// One value per participating team.
    Team,
}

/// Direct aggregation for a metric.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SimpleMetricAggregation {
    /// Add values across eligible result units.
    Sum,
    /// Keep the smallest submitted value.
    Min,
    /// Keep the largest submitted value.
    Max,
}

/// Definition for a ratio derived from two sum metrics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RatioMetricAggregation {
    /// Discriminator for a derived ratio.
    #[serde(rename = "type")]
    pub aggregation_type: String,
    /// ID of the numerator sum metric.
    #[serde(rename = "numeratorMetricId")]
    pub numerator_metric_id: String,
    /// ID of the denominator sum metric.
    #[serde(rename = "denominatorMetricId")]
    pub denominator_metric_id: String,
}

/// Direct or derived metric aggregation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum GameMetricAggregation {
    /// Direct sum, minimum, or maximum aggregation.
    Simple(SimpleMetricAggregation),
    /// Ratio derived from cumulative numerator and denominator sums.
    Ratio(RatioMetricAggregation),
}

/// A registered result metric.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameMetricDefinition {
    /// Stable metric identifier.
    pub id: String,
    /// Whether values belong to players or teams.
    pub scope: ResultMetricScope,
    /// Display and semantic unit.
    pub unit: String,
    /// Result-unit types that may submit or derive this metric.
    #[serde(rename = "unitTypes")]
    pub unit_types: Vec<String>,
    /// Aggregation rule. Ratio metrics are derived and cannot be submitted.
    pub aggregation: GameMetricAggregation,
}

/// Versioned result-unit types and metric declaration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameResultDefinition {
    /// Stable definition version.
    pub version: String,
    /// Result-unit types accepted for this definition.
    #[serde(rename = "unitTypes")]
    pub unit_types: Vec<String>,
    /// Registered metrics.
    pub metrics: Vec<GameMetricDefinition>,
}

impl GameResultDefinition {
    /// Validate identifiers, scopes, units, and derived ratio references.
    pub fn validate(&self) -> Result<(), SdkError> {
        validate_result_identifier(&self.version, "GameResultDefinition.version")?;
        validate_result_identifiers(&self.unit_types, "GameResultDefinition.unitTypes")?;
        if self.metrics.len() > MAX_RESULT_METRICS {
            return Err(result_validation_error(format!(
                "GameResultDefinition exceeds the {MAX_RESULT_METRICS}-metric limit"
            )));
        }
        let unit_types: HashSet<&str> = self.unit_types.iter().map(String::as_str).collect();
        let mut metrics = HashMap::new();
        for metric in &self.metrics {
            validate_result_identifier(&metric.id, "metric.id")?;
            validate_result_identifier(&metric.unit, "metric.unit")?;
            validate_result_identifiers(&metric.unit_types, "metric.unitTypes")?;
            if metric
                .unit_types
                .iter()
                .any(|unit_type| !unit_types.contains(unit_type.as_str()))
            {
                return Err(result_validation_error(format!(
                    "Metric {} unitTypes must be declared result-unit types",
                    metric.id
                )));
            }
            if metrics.insert(metric.id.as_str(), metric).is_some() {
                return Err(result_validation_error(format!(
                    "Duplicate metric ID: {}",
                    metric.id
                )));
            }
        }
        for metric in &self.metrics {
            let GameMetricAggregation::Ratio(ratio) = &metric.aggregation else {
                continue;
            };
            if ratio.aggregation_type != "ratio"
                || ratio.numerator_metric_id == metric.id
                || ratio.denominator_metric_id == metric.id
            {
                return Err(result_validation_error(format!(
                    "Ratio metric {} definition is invalid",
                    metric.id
                )));
            }
            validate_result_identifier(&ratio.numerator_metric_id, "ratio.numeratorMetricId")?;
            validate_result_identifier(&ratio.denominator_metric_id, "ratio.denominatorMetricId")?;
            let (Some(numerator), Some(denominator)) = (
                metrics.get(ratio.numerator_metric_id.as_str()),
                metrics.get(ratio.denominator_metric_id.as_str()),
            ) else {
                return Err(result_validation_error(format!(
                    "Ratio metric {} must reference declared sum metrics",
                    metric.id
                )));
            };
            let is_sum = |candidate: &&GameMetricDefinition| {
                candidate.scope == metric.scope
                    && candidate.aggregation
                        == GameMetricAggregation::Simple(SimpleMetricAggregation::Sum)
            };
            if !is_sum(numerator) || !is_sum(denominator) {
                return Err(result_validation_error(format!(
                    "Ratio metric {} must reference sum metrics with the same scope",
                    metric.id
                )));
            }
            if metric.unit_types.iter().any(|unit_type| {
                !numerator.unit_types.contains(unit_type)
                    || !denominator.unit_types.contains(unit_type)
            }) {
                return Err(result_validation_error(format!(
                    "Ratio metric {} unit types must be supported by both numerator and denominator",
                    metric.id
                )));
            }
        }
        Ok(())
    }
}

/// Outcome for a player or team in one result unit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ResultOutcome {
    /// Won this result unit.
    Win,
    /// Lost this result unit.
    Loss,
    /// Tied or otherwise drew this result unit.
    Draw,
    /// No win or loss, such as a push.
    Push,
}

/// Action taken by an append-only correction event.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GameResultCorrectionAction {
    /// Replace the referenced event.
    Replace,
    /// Retract the referenced event.
    Retract,
}

/// Reference to an earlier result event being corrected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameResultCorrection {
    /// Correction operation.
    pub action: GameResultCorrectionAction,
    /// Event being superseded.
    #[serde(rename = "supersedesEventId")]
    pub supersedes_event_id: String,
}

/// One player's outcome, rank, and direct metrics in a result unit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameResultParticipant {
    /// Player identity bound by the trusted host roster.
    #[serde(rename = "playerId")]
    pub player_id: String,
    /// Optional team identity.
    #[serde(
        rename = "teamId",
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub team_id: Option<String>,
    /// Optional game-defined outcome.
    #[serde(
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub outcome: Option<ResultOutcome>,
    /// Optional one-based rank; equal ranks represent a tie.
    #[serde(
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub rank: Option<u64>,
    /// Submitted direct metric values. Derived ratio metrics are excluded.
    pub metrics: HashMap<String, f64>,
}

/// One team's outcome, rank, and direct metrics in a result unit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameResultTeam {
    /// Team identity.
    #[serde(rename = "teamId")]
    pub team_id: String,
    /// Optional game-defined outcome.
    #[serde(
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub outcome: Option<ResultOutcome>,
    /// Optional one-based rank; equal ranks represent a tie.
    #[serde(
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub rank: Option<u64>,
    /// Submitted direct metric values. Derived ratio metrics are excluded.
    pub metrics: HashMap<String, f64>,
}

/// Immutable result event emitted by authoritative game logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameResultEvent {
    /// Version of this wire schema.
    #[serde(rename = "schemaVersion")]
    pub schema_version: u8,
    /// Stable idempotency key scoped by the host-bound game ID.
    #[serde(rename = "eventId")]
    pub event_id: String,
    /// Host-bound match identity.
    #[serde(rename = "matchId")]
    pub match_id: String,
    /// Stable result-unit identity within the match.
    #[serde(rename = "unitId")]
    pub unit_id: String,
    /// Game-declared type, such as `round`, `lap`, or `match`.
    #[serde(rename = "unitType")]
    pub unit_type: String,
    /// Version of the rules that produced this result.
    #[serde(rename = "rulesVersion")]
    pub rules_version: String,
    /// Version of the metric and result definition used.
    #[serde(rename = "definitionVersion")]
    pub definition_version: String,
    /// Optional Unix epoch milliseconds from a deterministic game context.
    #[serde(
        rename = "occurredAt",
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub occurred_at: Option<u64>,
    /// Player results; may be empty when the unit is team-only.
    pub participants: Vec<GameResultParticipant>,
    /// Optional team results.
    #[serde(
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub teams: Option<Vec<GameResultTeam>>,
    /// Optional append-only replacement or retraction metadata.
    #[serde(
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub correction: Option<GameResultCorrection>,
}

impl GameResultEvent {
    /// Validate the event structure without a published game definition.
    pub fn validate(&self) -> Result<(), SdkError> {
        self.validate_with_definition(None)
    }

    /// Validate the event and ensure its unit and metric IDs match a definition.
    pub fn validate_against(&self, definition: &GameResultDefinition) -> Result<(), SdkError> {
        self.validate_with_definition(Some(definition))
    }

    fn validate_with_definition(
        &self,
        definition: Option<&GameResultDefinition>,
    ) -> Result<(), SdkError> {
        if self.schema_version != 1 {
            return Err(result_validation_error(
                "GameResultEvent.schemaVersion must be 1",
            ));
        }
        for (field, value) in [
            ("eventId", self.event_id.as_str()),
            ("matchId", self.match_id.as_str()),
            ("unitId", self.unit_id.as_str()),
            ("unitType", self.unit_type.as_str()),
            ("rulesVersion", self.rules_version.as_str()),
            ("definitionVersion", self.definition_version.as_str()),
        ] {
            validate_result_identifier(value, &format!("GameResultEvent.{field}"))?;
        }
        if self
            .occurred_at
            .is_some_and(|timestamp| timestamp > MAX_SAFE_INTEGER)
        {
            return Err(result_validation_error(
                "GameResultEvent.occurredAt must be a non-negative safe integer timestamp in milliseconds",
            ));
        }
        if let Some(correction) = &self.correction {
            validate_result_identifier(
                &correction.supersedes_event_id,
                "GameResultEvent.correction.supersedesEventId",
            )?;
            if correction.supersedes_event_id == self.event_id {
                return Err(result_validation_error(
                    "A correction cannot supersede itself",
                ));
            }
        }

        let teams = self.teams.as_deref().unwrap_or_default();
        if self.participants.len() > MAX_RESULT_PARTICIPANTS {
            return Err(result_validation_error(format!(
                "GameResultEvent exceeds the {MAX_RESULT_PARTICIPANTS}-participant limit"
            )));
        }
        if teams.len() > MAX_RESULT_TEAMS {
            return Err(result_validation_error(format!(
                "GameResultEvent exceeds the {MAX_RESULT_TEAMS}-team limit"
            )));
        }
        let retraction = self
            .correction
            .as_ref()
            .is_some_and(|correction| correction.action == GameResultCorrectionAction::Retract);
        if self.participants.is_empty() && teams.is_empty() && !retraction {
            return Err(result_validation_error(
                "GameResultEvent must include a participant or team",
            ));
        }
        if retraction && (!self.participants.is_empty() || !teams.is_empty()) {
            return Err(result_validation_error(
                "A retraction must not include participants or teams",
            ));
        }

        if let Some(definition) = definition {
            definition.validate()?;
            if self.definition_version != definition.version {
                return Err(result_validation_error(
                    "GameResultEvent.definitionVersion does not match the published definition",
                ));
            }
            if !definition.unit_types.contains(&self.unit_type) {
                return Err(result_validation_error(
                    "GameResultEvent.unitType is not declared",
                ));
            }
        }

        let mut player_ids = HashSet::new();
        for participant in &self.participants {
            validate_result_identifier(&participant.player_id, "participant.playerId")?;
            if participant
                .team_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty())
            {
                return Err(result_validation_error(
                    "participant.teamId must be a non-empty string",
                ));
            }
            if let Some(team_id) = &participant.team_id {
                validate_result_identifier(team_id, "participant.teamId")?;
            }
            if !player_ids.insert(participant.player_id.as_str()) {
                return Err(result_validation_error(format!(
                    "Player appears more than once in a result unit: {}",
                    participant.player_id
                )));
            }
            validate_result_entity(
                participant.outcome,
                participant.rank,
                &participant.metrics,
                "participant",
                &self.unit_type,
                definition,
            )?;
        }

        let mut team_ids = HashSet::new();
        for team in teams {
            validate_result_identifier(&team.team_id, "team.teamId")?;
            if !team_ids.insert(team.team_id.as_str()) {
                return Err(result_validation_error(format!(
                    "Team appears more than once in a result unit: {}",
                    team.team_id
                )));
            }
            validate_result_entity(
                team.outcome,
                team.rank,
                &team.metrics,
                "team",
                &self.unit_type,
                definition,
            )?;
        }
        if !team_ids.is_empty() {
            for participant in &self.participants {
                if participant
                    .team_id
                    .as_ref()
                    .is_some_and(|team_id| !team_ids.contains(team_id.as_str()))
                {
                    return Err(result_validation_error(format!(
                        "Participant references undeclared teamId: {}",
                        participant.team_id.as_deref().unwrap_or_default()
                    )));
                }
            }
        }

        let serialized = serde_json::to_vec(self).map_err(|_| {
            result_validation_error("GameResultEvent could not be serialized as JSON")
        })?;
        if serialized.len() > MAX_RESULT_EVENT_BYTES {
            return Err(result_validation_error(format!(
                "GameResultEvent exceeds the {MAX_RESULT_EVENT_BYTES}-byte limit"
            )));
        }
        Ok(())
    }
}

/// Validate emitted events and ensure they do not duplicate IDs or units in one transition.
pub fn validate_game_result_units(
    events: Option<&[GameResultEvent]>,
    definition: Option<&GameResultDefinition>,
) -> Result<(), SdkError> {
    let Some(events) = events else {
        return Ok(());
    };
    if events.len() > MAX_RESULT_EVENTS_PER_TRANSITION {
        return Err(result_validation_error(format!(
            "resultUnits exceeds the {MAX_RESULT_EVENTS_PER_TRANSITION}-event per-transition limit"
        )));
    }
    let mut event_ids = HashSet::new();
    let mut unit_ids = HashSet::new();
    for event in events {
        match definition {
            Some(definition) => event.validate_against(definition)?,
            None => event.validate()?,
        }
        if !event_ids.insert(event.event_id.as_str()) {
            return Err(result_validation_error(format!(
                "Duplicate result eventId in one state transition: {}",
                event.event_id
            )));
        }
        if !unit_ids.insert((event.match_id.as_str(), event.unit_id.as_str())) {
            return Err(result_validation_error(format!(
                "Duplicate result unit in one state transition: {}/{}",
                event.match_id, event.unit_id
            )));
        }
    }
    Ok(())
}

fn validate_result_entity(
    outcome: Option<ResultOutcome>,
    rank: Option<u64>,
    metrics: &HashMap<String, f64>,
    scope: &str,
    unit_type: &str,
    definition: Option<&GameResultDefinition>,
) -> Result<(), SdkError> {
    let _ = outcome;
    if rank.is_some_and(|rank| rank == 0 || rank > MAX_SAFE_INTEGER) {
        return Err(result_validation_error(format!(
            "{scope}.rank must be a positive safe integer"
        )));
    }
    if metrics.len() > MAX_METRICS_PER_ENTITY {
        return Err(result_validation_error(format!(
            "{scope}.metrics exceeds the {MAX_METRICS_PER_ENTITY}-metric limit"
        )));
    }
    for (id, value) in metrics {
        validate_result_identifier(id, &format!("{scope}.metrics metric ID"))?;
        if !value.is_finite() {
            return Err(result_validation_error(format!(
                "{scope}.metrics.{id} must be a finite number"
            )));
        }
        if let Some(definition) = definition {
            let metric = definition.metrics.iter().find(|metric| metric.id == *id);
            let valid = metric.is_some_and(|metric| {
                metric.scope.as_str() == scope
                    && metric
                        .unit_types
                        .iter()
                        .any(|candidate| candidate == unit_type)
                    && matches!(metric.aggregation, GameMetricAggregation::Simple(_))
            });
            if !valid {
                return Err(result_validation_error(format!(
                    "{scope}.metrics.{id} is not a declared direct {scope} metric for {unit_type}"
                )));
            }
        }
    }
    Ok(())
}

impl ResultMetricScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Participant => "participant",
            Self::Team => "team",
        }
    }
}

fn validate_result_identifier(value: &str, label: &str) -> Result<(), SdkError> {
    if value.trim().is_empty() {
        return Err(result_validation_error(format!(
            "{label} must be a non-empty string"
        )));
    }
    if value.chars().count() > MAX_IDENTIFIER_CODE_POINTS {
        return Err(result_validation_error(format!(
            "{label} exceeds {MAX_IDENTIFIER_CODE_POINTS} Unicode code points"
        )));
    }
    Ok(())
}

fn validate_result_identifiers(values: &[String], label: &str) -> Result<(), SdkError> {
    if values.is_empty() {
        return Err(result_validation_error(format!(
            "{label} must be a non-empty string array"
        )));
    }
    let mut seen = HashSet::new();
    for value in values {
        validate_result_identifier(value, label)?;
        if !seen.insert(value.as_str()) {
            return Err(result_validation_error(format!(
                "{label} must not contain duplicates"
            )));
        }
    }
    Ok(())
}

fn result_validation_error(message: impl Into<String>) -> SdkError {
    SdkError::Validation(message.into())
}

fn deserialize_present_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

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
    /// Optional versioned result-unit and metric declaration.
    #[serde(
        rename = "resultDefinition",
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub result_definition: Option<GameResultDefinition>,
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
        if let Some(definition) = &self.result_definition {
            definition.validate()?;
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
    /// Optional result units emitted by initialization.
    #[serde(
        rename = "resultUnits",
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub result_units: Option<Vec<GameResultEvent>>,
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
    /// Optional result units emitted by this tick.
    #[serde(
        rename = "resultUnits",
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub result_units: Option<Vec<GameResultEvent>>,
}

/// Result returned after a player lifecycle event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerEventResult {
    /// Updated game state.
    pub state: serde_json::Value,
    /// Optional messages to broadcast.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcasts: Option<Vec<serde_json::Value>>,
    /// Optional result units emitted by this player event.
    #[serde(
        rename = "resultUnits",
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub result_units: Option<Vec<GameResultEvent>>,
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
    /// Optional result units emitted when checking match completion.
    #[serde(
        rename = "resultUnits",
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub result_units: Option<Vec<GameResultEvent>>,
}

/// Result returned after processing a trusted command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandResult {
    /// Updated game state.
    pub state: serde_json::Value,
    /// Optional messages to broadcast.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcasts: Option<Vec<serde_json::Value>>,
    /// Optional result units emitted by this trusted command.
    #[serde(
        rename = "resultUnits",
        default,
        deserialize_with = "deserialize_present_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub result_units: Option<Vec<GameResultEvent>>,
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
