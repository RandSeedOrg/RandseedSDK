#pragma once

#include <nlohmann/json.hpp>

#include <cstdint>
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

struct GameManifest {
    std::string title;
    std::string version;
    std::uint32_t min_players{1};
    std::uint32_t max_players{1};
    std::uint32_t tick_rate_hz{1};
    bool persistent_table{false};
    std::optional<Json> options_schema;
};

struct MatchConfig {
    std::string match_id;
    std::string game_id;
    std::vector<std::string> players;
    std::uint64_t match_seq{0};
    Json options{Json::object()};
    Json carryover{nullptr};
    std::optional<std::uint32_t> tick_rate_hz;
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
};

using InitResult = StateResult;
using PlayerEventResult = StateResult;
using CommandResult = StateResult;

struct TickResult {
    Json state{nullptr};
    std::optional<Json> broadcasts;
    bool persist{false};
};

struct GameOverResult {
    bool over{false};
    std::optional<Json> summary;
    std::optional<Json> next_match_policy;
};

[[nodiscard]] GameManifest parse_game_manifest(const Json& value);
[[nodiscard]] MatchConfig parse_match_config(const Json& value);
[[nodiscard]] TickRequest parse_tick_request(const Json& value);
[[nodiscard]] PlayerEventRequest parse_player_event_request(const Json& value);
[[nodiscard]] IsGameOverRequest parse_is_game_over_request(const Json& value);
[[nodiscard]] CommandRequest parse_command_request(const Json& value);
void validate_game_manifest(const GameManifest& manifest);

void to_json(Json& value, const GameManifest& manifest);
void from_json(const Json& value, GameManifest& manifest);
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
