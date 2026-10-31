#include "randseed/protocol.hpp"

#include <cmath>
#include <limits>
#include <string>
#include <unordered_set>
#include <utility>

namespace randseed {
namespace {

constexpr std::uint64_t max_safe_integer = 9007199254740991ULL;

const Json& require_object(const Json& value, std::string_view field) {
    if (!value.is_object()) {
        throw ValidationError(std::string(field) + " must be an object");
    }
    return value;
}

const Json& require_field(
    const Json& object,
    std::string_view field,
    std::string_view parent = "request") {
    if (!object.contains(std::string(field))) {
        throw ValidationError(
            std::string(parent) + " is missing " + std::string(field));
    }
    return object.at(std::string(field));
}

std::string require_string(
    const Json& value,
    std::string_view field,
    bool non_empty = true) {
    if (!value.is_string()) {
        throw ValidationError(std::string(field) + " must be a string");
    }
    std::string result = value.get<std::string>();
    if (non_empty && result.empty()) {
        throw ValidationError(std::string(field) + " must not be empty");
    }
    return result;
}

std::uint64_t require_nonnegative_integer(
    const Json& value,
    std::string_view field) {
    std::uint64_t result = 0;
    if (value.is_number_unsigned()) {
        result = value.get<std::uint64_t>();
    } else if (value.is_number_integer()) {
        const auto signed_value = value.get<std::int64_t>();
        if (signed_value < 0) {
            throw ValidationError(
                std::string(field) + " must be a non-negative integer");
        }
        result = static_cast<std::uint64_t>(signed_value);
    } else {
        throw ValidationError(
            std::string(field) + " must be a non-negative integer");
    }

    if (result > max_safe_integer) {
        throw ValidationError(
            std::string(field) + " exceeds the safe integer range");
    }
    return result;
}

std::uint64_t require_positive_safe_integer(
    const Json& value,
    std::string_view field) {
    const std::uint64_t result = require_nonnegative_integer(value, field);
    if (result == 0) {
        throw ValidationError(
            std::string(field) + " must be a positive safe integer");
    }
    return result;
}

std::uint32_t require_bounded_integer(
    const Json& value,
    std::string_view field,
    std::uint32_t minimum,
    std::uint32_t maximum) {
    const std::uint64_t result = require_nonnegative_integer(value, field);
    if (result < minimum || result > maximum) {
        throw ValidationError(std::string(field) + " is out of range");
    }
    return static_cast<std::uint32_t>(result);
}

double require_bounded_number(
    const Json& value,
    std::string_view field,
    double minimum,
    double maximum) {
    if (!value.is_number()) {
        throw ValidationError(std::string(field) + " must be a number");
    }
    const double result = value.get<double>();
    if (!std::isfinite(result) || result < minimum || result > maximum) {
        throw ValidationError(std::string(field) + " is out of range");
    }
    return result;
}

double require_finite_nonnegative_number(
    const Json& value,
    std::string_view field) {
    if (!value.is_number()) {
        throw ValidationError(std::string(field) + " must be a number");
    }
    const double result = value.get<double>();
    if (!std::isfinite(result) || result < 0.0) {
        throw ValidationError(
            std::string(field) + " must be finite and non-negative");
    }
    return result;
}

const Json& require_array(const Json& value, std::string_view field) {
    if (!value.is_array()) {
        throw ValidationError(std::string(field) + " must be an array");
    }
    return value;
}

TickInput parse_tick_input(const Json& value) {
    require_object(value, "input");
    TickInput input;
    input.player_id = require_string(
        require_field(value, "playerId", "input"), "input.playerId");
    input.data = require_field(value, "data", "input");
    if (value.contains("timestamp")) {
        input.timestamp = require_nonnegative_integer(
            value.at("timestamp"), "input.timestamp");
    }
    return input;
}

void validate_broadcasts(const std::optional<Json>& broadcasts) {
    if (broadcasts && !broadcasts->is_array()) {
        throw std::invalid_argument("Game broadcasts must be an array");
    }
}

void validate_policy(const Json& policy) {
    if (!policy.is_object()) {
        throw std::invalid_argument("Next-match policy must be an object");
    }

    const auto mode = policy.find("mode");
    if (mode == policy.end() || !mode->is_string()) {
        throw std::invalid_argument("Next-match policy has an invalid mode");
    }
    const auto mode_value = mode->get<std::string>();
    if (mode_value != "auto_next_match" && mode_value != "wait_for_ready" &&
        mode_value != "close_room") {
        throw std::invalid_argument("Next-match policy has an invalid mode");
    }

    const auto timeout = policy.find("timeoutMs");
    if (timeout == policy.end()) {
        throw std::invalid_argument("Next-match policy is missing timeoutMs");
    }
    try {
        static_cast<void>(
            require_nonnegative_integer(*timeout, "nextMatchPolicy.timeoutMs"));
    } catch (const ValidationError&) {
        throw std::invalid_argument("Next-match policy has an invalid timeoutMs");
    }
}

} // namespace

void validate_game_manifest(const GameManifest& manifest) {
    if (manifest.title.empty()) {
        throw ValidationError("manifest.title must not be empty");
    }
    if (manifest.version.empty()) {
        throw ValidationError("manifest.version must not be empty");
    }
    if (manifest.min_players < 1 || manifest.min_players > 100) {
        throw ValidationError("manifest.minPlayers is out of range");
    }
    if (manifest.max_players < manifest.min_players ||
        manifest.max_players > 100) {
        throw ValidationError("manifest.maxPlayers is out of range");
    }
    if (!std::isfinite(manifest.tick_rate_hz) ||
        manifest.tick_rate_hz < 1 || manifest.tick_rate_hz > 60) {
        throw ValidationError("manifest.tickRateHz is out of range");
    }
}

GameManifest parse_game_manifest(const Json& value) {
    require_object(value, "manifest");
    GameManifest manifest;
    manifest.title =
        require_string(require_field(value, "title", "manifest"), "title");
    manifest.version =
        require_string(require_field(value, "version", "manifest"), "version");
    manifest.min_players = require_bounded_integer(
        require_field(value, "minPlayers", "manifest"), "minPlayers", 1, 100);
    manifest.max_players = require_bounded_integer(
        require_field(value, "maxPlayers", "manifest"),
        "maxPlayers",
        manifest.min_players,
        100);
    manifest.tick_rate_hz = require_bounded_number(
        require_field(value, "tickRateHz", "manifest"), "tickRateHz", 1, 60);
    const auto& persistent =
        require_field(value, "persistentTable", "manifest");
    if (!persistent.is_boolean()) {
        throw ValidationError("persistentTable must be a boolean");
    }
    manifest.persistent_table = persistent.get<bool>();
    if (value.contains("optionsSchema")) {
        manifest.options_schema = value.at("optionsSchema");
    }
    return manifest;
}

MatchConfig parse_match_config(const Json& value) {
    require_object(value, "request");
    MatchConfig config;
    config.match_id = require_string(
        require_field(value, "matchId"), "matchId");
    config.game_id =
        require_string(require_field(value, "gameId"), "gameId");
    config.match_seq = require_positive_safe_integer(
        require_field(value, "matchSeq"), "matchSeq");

    const auto& players = require_array(
        require_field(value, "players"), "players");
    if (players.empty() || players.size() > 100) {
        throw ValidationError("players must contain between 1 and 100 entries");
    }
    std::unordered_set<std::string> unique_players;
    config.players.reserve(players.size());
    for (const auto& player : players) {
        auto player_id = require_string(player, "players[]");
        if (!unique_players.emplace(player_id).second) {
            throw ValidationError("players must contain unique player IDs");
        }
        config.players.push_back(std::move(player_id));
    }

    if (value.contains("options")) {
        const auto& options = require_object(value.at("options"), "options");
        if (options.contains("seed")) {
            const auto& seed = options.at("seed");
            if (!seed.is_number() || !std::isfinite(seed.get<double>())) {
                throw ValidationError("options.seed must be a finite number");
            }
        }
        config.options = options;
    }
    config.carryover = require_field(value, "carryover");

    if (value.contains("tickRateHz")) {
        config.tick_rate_hz = require_bounded_number(
            value.at("tickRateHz"), "tickRateHz", 1, 60);
    }
    return config;
}

TickRequest parse_tick_request(const Json& value) {
    require_object(value, "request");
    const auto& context = require_object(
        require_field(value, "ctx"), "ctx");

    TickRequest request;
    request.ctx.tick = require_nonnegative_integer(
        require_field(context, "tick", "ctx"), "ctx.tick");
    request.ctx.elapsed_seconds = require_finite_nonnegative_number(
        require_field(context, "elapsedSeconds", "ctx"),
        "ctx.elapsedSeconds");

    const auto& inputs = require_array(
        require_field(context, "inputs", "ctx"), "ctx.inputs");
    request.ctx.inputs.reserve(inputs.size());
    for (const auto& input : inputs) {
        request.ctx.inputs.push_back(parse_tick_input(input));
    }

    if (context.contains("commands")) {
        const auto& commands = require_array(
            context.at("commands"), "ctx.commands");
        request.ctx.commands.assign(commands.begin(), commands.end());
    }
    request.state = require_field(value, "state");
    return request;
}

PlayerEventRequest parse_player_event_request(const Json& value) {
    require_object(value, "request");
    PlayerEventRequest request;
    request.player_id = require_string(
        require_field(value, "playerId"), "playerId");
    request.state = require_field(value, "state");
    return request;
}

IsGameOverRequest parse_is_game_over_request(const Json& value) {
    require_object(value, "request");
    IsGameOverRequest request;
    request.state = require_field(value, "state");
    return request;
}

CommandRequest parse_command_request(const Json& value) {
    require_object(value, "request");
    CommandRequest request;
    request.command = require_object(
        require_field(value, "command"), "command");
    require_string(
        require_field(request.command, "type", "command"),
        "command.type");
    static_cast<void>(require_field(request.command, "data", "command"));
    request.state = require_field(value, "state");
    return request;
}

void to_json(Json& value, const GameManifest& manifest) {
    validate_game_manifest(manifest);
    value = Json{
        {"title", manifest.title},
        {"version", manifest.version},
        {"minPlayers", manifest.min_players},
        {"maxPlayers", manifest.max_players},
        {"tickRateHz", manifest.tick_rate_hz},
        {"persistentTable", manifest.persistent_table}};
    if (manifest.options_schema) {
        value["optionsSchema"] = *manifest.options_schema;
    }
}

void from_json(const Json& value, GameManifest& manifest) {
    manifest = parse_game_manifest(value);
}

void to_json(Json& value, const MatchConfig& config) {
    value = Json{
        {"matchId", config.match_id},
        {"gameId", config.game_id},
        {"players", config.players},
        {"matchSeq", config.match_seq},
        {"options", config.options},
        {"carryover", config.carryover}};
    if (config.tick_rate_hz) {
        value["tickRateHz"] = *config.tick_rate_hz;
    }
}

void from_json(const Json& value, MatchConfig& config) {
    config = parse_match_config(value);
}

void to_json(Json& value, const TickInput& input) {
    value = Json{{"playerId", input.player_id}, {"data", input.data}};
    if (input.timestamp) {
        value["timestamp"] = *input.timestamp;
    }
}

void from_json(const Json& value, TickInput& input) {
    input = parse_tick_input(value);
}

void to_json(Json& value, const TickContext& context) {
    value = Json{
        {"tick", context.tick},
        {"inputs", context.inputs},
        {"commands", context.commands},
        {"elapsedSeconds", context.elapsed_seconds}};
}

void from_json(const Json& value, TickContext& context) {
    const Json wrapped = Json{{"ctx", value}, {"state", nullptr}};
    context = parse_tick_request(wrapped).ctx;
}

void to_json(Json& value, const TickRequest& request) {
    value = Json{{"ctx", request.ctx}, {"state", request.state}};
}

void from_json(const Json& value, TickRequest& request) {
    request = parse_tick_request(value);
}

void to_json(Json& value, const PlayerEventRequest& request) {
    value = Json{{"playerId", request.player_id}, {"state", request.state}};
}

void from_json(const Json& value, PlayerEventRequest& request) {
    request = parse_player_event_request(value);
}

void to_json(Json& value, const IsGameOverRequest& request) {
    value = Json{{"state", request.state}};
}

void from_json(const Json& value, IsGameOverRequest& request) {
    request = parse_is_game_over_request(value);
}

void to_json(Json& value, const CommandRequest& request) {
    value = Json{{"command", request.command}, {"state", request.state}};
}

void from_json(const Json& value, CommandRequest& request) {
    request = parse_command_request(value);
}

void to_json(Json& value, const StateResult& result) {
    validate_broadcasts(result.broadcasts);
    value = Json{{"state", result.state}};
    if (result.broadcasts) {
        value["broadcasts"] = *result.broadcasts;
    }
}

void to_json(Json& value, const TickResult& result) {
    validate_broadcasts(result.broadcasts);
    value = Json{{"state", result.state}, {"persist", result.persist}};
    if (result.broadcasts) {
        value["broadcasts"] = *result.broadcasts;
    }
}

void to_json(Json& value, const GameOverResult& result) {
    value = Json{{"over", result.over}};
    if (result.over) {
        if (!result.summary || !result.summary->is_object() ||
            !result.summary->contains("durationSeconds")) {
            throw std::invalid_argument("A completed game requires a valid summary");
        }
        try {
            static_cast<void>(require_finite_nonnegative_number(
                result.summary->at("durationSeconds"), "summary.durationSeconds"));
        } catch (const ValidationError&) {
            throw std::invalid_argument("A completed game requires a valid summary");
        }
    }
    if (result.summary) {
        value["summary"] = *result.summary;
    }
    if (result.next_match_policy) {
        validate_policy(*result.next_match_policy);
        value["nextMatchPolicy"] = *result.next_match_policy;
    }
    if (result.over && (!result.summary || !result.next_match_policy)) {
        throw std::invalid_argument(
            "A completed game requires summary and next-match policy");
    }
}

} // namespace randseed
