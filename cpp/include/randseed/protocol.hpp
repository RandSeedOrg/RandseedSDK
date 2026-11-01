#pragma once

#include <nlohmann/json.hpp>

#include <cstdint>
#include <map>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace randseed {

using Json = nlohmann::json;

class ValidationError final : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

enum class ResultMetricScope {
    Participant,
    Team
};

struct RatioMetricAggregation {
    std::string numerator_metric_id;
    std::string denominator_metric_id;
};

struct GameMetricAggregation {
    std::string type;
    std::optional<RatioMetricAggregation> ratio{std::nullopt};
};

struct GameMetricDefinition {
    std::string id;
    ResultMetricScope scope{ResultMetricScope::Participant};
    std::string unit;
    std::vector<std::string> unit_types;
    GameMetricAggregation aggregation;
};

struct GameResultDefinition {
    std::string version;
    std::vector<std::string> unit_types;
    std::vector<GameMetricDefinition> metrics;
};

enum class ResultOutcome {
    Win,
    Loss,
    Draw,
    Push
};

enum class GameResultCorrectionAction {
    Replace,
    Retract
};

struct GameResultCorrection {
    GameResultCorrectionAction action{GameResultCorrectionAction::Replace};
    std::string supersedes_event_id;
};

struct GameResultParticipant {
    std::string player_id;
    std::optional<std::string> team_id{std::nullopt};
    std::optional<ResultOutcome> outcome{std::nullopt};
    std::optional<std::uint64_t> rank{std::nullopt};
    std::map<std::string, double> metrics;
};

struct GameResultTeam {
    std::string team_id;
    std::optional<ResultOutcome> outcome{std::nullopt};
    std::optional<std::uint64_t> rank{std::nullopt};
    std::map<std::string, double> metrics;
};

struct GameResultEvent {
    std::uint32_t schema_version{1};
    std::string event_id;
    std::string match_id;
    std::string unit_id;
    std::string unit_type;
    std::string rules_version;
    std::string definition_version;
    std::optional<std::uint64_t> occurred_at{std::nullopt};
    std::vector<GameResultParticipant> participants;
    std::optional<std::vector<GameResultTeam>> teams{std::nullopt};
    std::optional<GameResultCorrection> correction{std::nullopt};
};

struct GameManifest {
    std::string title;
    std::string version;
    std::uint32_t min_players{1};
    std::uint32_t max_players{1};
    double tick_rate_hz{1.0};
    bool persistent_table{false};
    std::optional<Json> options_schema;
    std::optional<GameResultDefinition> result_definition{std::nullopt};
};

struct MatchConfig {
    std::string match_id;
    std::string game_id;
    std::vector<std::string> players;
    std::uint64_t match_seq{0};
    Json options = Json::object();
    Json carryover{nullptr};
    std::optional<double> tick_rate_hz;
};

struct TickInput {
    std::string player_id;
    Json data{nullptr};
    std::optional<std::uint64_t> timestamp;
};

struct TickContext {
    std::uint64_t tick{0};
    std::vector<TickInput> inputs;
    std::vector<Json> commands;
    double elapsed_seconds{0.0};
};

struct TickRequest {
    TickContext ctx;
    Json state{nullptr};
};

struct PlayerEventRequest {
    std::string player_id;
    Json state{nullptr};
};

struct IsGameOverRequest {
    Json state{nullptr};
};

struct CommandRequest {
    Json command{Json::object()};
    Json state{nullptr};
};

struct StateResult {
    Json state{nullptr};
    std::optional<Json> broadcasts;
    std::optional<std::vector<GameResultEvent>> result_units{std::nullopt};
};

using InitResult = StateResult;
using PlayerEventResult = StateResult;
using CommandResult = StateResult;

struct TickResult {
    Json state{nullptr};
    std::optional<Json> broadcasts;
    bool persist{false};
    std::optional<std::vector<GameResultEvent>> result_units{std::nullopt};
};

struct GameOverResult {
    bool over{false};
    std::optional<Json> summary;
    std::optional<Json> next_match_policy;
    std::optional<std::vector<GameResultEvent>> result_units{std::nullopt};
};

[[nodiscard]] GameManifest parse_game_manifest(const Json& value);
[[nodiscard]] GameResultDefinition parse_game_result_definition(const Json& value);
[[nodiscard]] GameResultEvent parse_game_result_event(
    const Json& value,
    const GameResultDefinition* definition = nullptr);
[[nodiscard]] MatchConfig parse_match_config(const Json& value);
[[nodiscard]] TickRequest parse_tick_request(const Json& value);
[[nodiscard]] PlayerEventRequest parse_player_event_request(const Json& value);
[[nodiscard]] IsGameOverRequest parse_is_game_over_request(const Json& value);
[[nodiscard]] CommandRequest parse_command_request(const Json& value);
void validate_game_manifest(const GameManifest& manifest);
void validate_game_result_definition(const GameResultDefinition& definition);
void validate_game_result_event(
    const GameResultEvent& event,
    const GameResultDefinition* definition = nullptr);
void validate_game_result_units(
    const std::optional<std::vector<GameResultEvent>>& events,
    const GameResultDefinition* definition = nullptr);

void to_json(Json& value, const GameManifest& manifest);
void from_json(const Json& value, GameManifest& manifest);
void to_json(Json& value, const GameResultDefinition& definition);
void from_json(const Json& value, GameResultDefinition& definition);
void to_json(Json& value, const GameMetricDefinition& metric);
void from_json(const Json& value, GameMetricDefinition& metric);
void to_json(Json& value, const GameMetricAggregation& aggregation);
void from_json(const Json& value, GameMetricAggregation& aggregation);
void to_json(Json& value, const RatioMetricAggregation& aggregation);
void from_json(const Json& value, RatioMetricAggregation& aggregation);
void to_json(Json& value, ResultMetricScope scope);
void from_json(const Json& value, ResultMetricScope& scope);
void to_json(Json& value, const GameResultEvent& event);
void from_json(const Json& value, GameResultEvent& event);
void to_json(Json& value, const GameResultParticipant& participant);
void from_json(const Json& value, GameResultParticipant& participant);
void to_json(Json& value, const GameResultTeam& team);
void from_json(const Json& value, GameResultTeam& team);
void to_json(Json& value, const GameResultCorrection& correction);
void from_json(const Json& value, GameResultCorrection& correction);
void to_json(Json& value, ResultOutcome outcome);
void from_json(const Json& value, ResultOutcome& outcome);
void to_json(Json& value, GameResultCorrectionAction action);
void from_json(const Json& value, GameResultCorrectionAction& action);
void to_json(Json& value, const MatchConfig& config);
void from_json(const Json& value, MatchConfig& config);
void to_json(Json& value, const TickInput& input);
void from_json(const Json& value, TickInput& input);
void to_json(Json& value, const TickContext& context);
void from_json(const Json& value, TickContext& context);
void to_json(Json& value, const TickRequest& request);
void from_json(const Json& value, TickRequest& request);
void to_json(Json& value, const PlayerEventRequest& request);
void from_json(const Json& value, PlayerEventRequest& request);
void to_json(Json& value, const IsGameOverRequest& request);
void from_json(const Json& value, IsGameOverRequest& request);
void to_json(Json& value, const CommandRequest& request);
void from_json(const Json& value, CommandRequest& request);

void to_json(Json& value, const StateResult& result);
void to_json(Json& value, const TickResult& result);
void to_json(Json& value, const GameOverResult& result);

class Game {
public:
    virtual ~Game() = default;

    [[nodiscard]] virtual GameManifest manifest() const = 0;
    [[nodiscard]] virtual InitResult init(const MatchConfig& config) const = 0;
    [[nodiscard]] virtual TickResult tick(
        const TickContext& context,
        const Json& state) const = 0;
    [[nodiscard]] virtual PlayerEventResult on_player_join(
        std::string_view player_id,
        const Json& state) const = 0;
    [[nodiscard]] virtual PlayerEventResult on_player_leave(
        std::string_view player_id,
        const Json& state) const = 0;
    [[nodiscard]] virtual GameOverResult is_game_over(const Json& state) const = 0;
    [[nodiscard]] virtual CommandResult on_command(
        const Json& command,
        const Json& state) const = 0;
};

} // namespace randseed
