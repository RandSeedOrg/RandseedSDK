#include "randseed/protocol.hpp"

#include <algorithm>
#include <cmath>
#include <initializer_list>
#include <limits>
#include <map>
#include <set>
#include <string>
#include <unordered_set>
#include <utility>

namespace randseed {
namespace {

constexpr std::uint64_t max_safe_integer = 9007199254740991ULL;
constexpr std::size_t max_identifier_code_points = 128;
constexpr std::size_t max_result_event_bytes = 256U * 1024U;
constexpr std::size_t max_result_participants = 1000;
constexpr std::size_t max_result_teams = 256;
constexpr std::size_t max_metrics_per_entity = 128;
constexpr std::size_t max_result_events_per_transition = 32;

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

void reject_unknown_fields(
    const Json& value,
    std::initializer_list<std::string_view> allowed,
    std::string_view label) {
    require_object(value, label);
    for (auto item = value.begin(); item != value.end(); ++item) {
        const auto known = std::find(allowed.begin(), allowed.end(), item.key());
        if (known == allowed.end()) {
            throw ValidationError(
                std::string(label) + " contains unsupported field " + item.key());
        }
    }
}

bool is_trim_whitespace(std::uint32_t code_point) {
    return (code_point >= 0x0009 && code_point <= 0x000d) ||
        code_point == 0x0020 || code_point == 0x00a0 ||
        code_point == 0x1680 ||
        (code_point >= 0x2000 && code_point <= 0x200a) ||
        code_point == 0x2028 || code_point == 0x2029 ||
        code_point == 0x202f || code_point == 0x205f ||
        code_point == 0x3000 || code_point == 0xfeff;
}

std::pair<std::uint32_t, std::size_t> decode_utf8_code_point(
    std::string_view value,
    std::size_t offset) {
    const auto first = static_cast<unsigned char>(value[offset]);
    if (first <= 0x7f) {
        return {first, 1};
    }

    std::size_t length = 0;
    std::uint32_t code_point = 0;
    if (first >= 0xc2 && first <= 0xdf) {
        length = 2;
        code_point = first & 0x1fU;
    } else if (first >= 0xe0 && first <= 0xef) {
        length = 3;
        code_point = first & 0x0fU;
    } else if (first >= 0xf0 && first <= 0xf4) {
        length = 4;
        code_point = first & 0x07U;
    } else {
        throw ValidationError("Identifier is not valid UTF-8");
    }
    if (offset + length > value.size()) {
        throw ValidationError("Identifier is not valid UTF-8");
    }

    for (std::size_t index = 1; index < length; ++index) {
        const auto next = static_cast<unsigned char>(value[offset + index]);
        if ((next & 0xc0U) != 0x80U) {
            throw ValidationError("Identifier is not valid UTF-8");
        }
        code_point = (code_point << 6U) | (next & 0x3fU);
    }
    if ((length == 3 && code_point < 0x800U) ||
        (length == 4 && code_point < 0x10000U) ||
        (code_point >= 0xd800U && code_point <= 0xdfffU) ||
        code_point > 0x10ffffU) {
        throw ValidationError("Identifier is not valid UTF-8");
    }
    return {code_point, length};
}

std::string require_identifier(const Json& value, std::string_view field) {
    const std::string result = require_string(value, field, false);
    std::size_t code_points = 0;
    bool has_non_whitespace = false;
    for (std::size_t offset = 0; offset < result.size();) {
        const auto [code_point, length] = decode_utf8_code_point(result, offset);
        ++code_points;
        if (code_points > max_identifier_code_points) {
            throw ValidationError(
                std::string(field) + " exceeds 128 Unicode code points");
        }
        has_non_whitespace = has_non_whitespace ||
            !is_trim_whitespace(code_point);
        offset += length;
    }
    if (!has_non_whitespace) {
        throw ValidationError(std::string(field) + " must be a non-empty string");
    }
    return result;
}

void validate_identifier(std::string_view value, std::string_view field) {
    static_cast<void>(require_identifier(Json(value), field));
}

std::uint64_t require_safe_integer(
    const Json& value,
    std::string_view field,
    bool positive) {
    std::uint64_t result = 0;
    if (value.is_number_unsigned()) {
        result = value.get<std::uint64_t>();
    } else if (value.is_number_integer()) {
        const auto signed_value = value.get<std::int64_t>();
        if (signed_value < 0) {
            throw ValidationError(
                std::string(field) + " must be a positive safe integer");
        }
        result = static_cast<std::uint64_t>(signed_value);
    } else if (value.is_number_float()) {
        const double number = value.get<double>();
        if (!std::isfinite(number) || std::floor(number) != number ||
            number < 0.0 || number > static_cast<double>(max_safe_integer)) {
            throw ValidationError(
                std::string(field) + " must be a positive safe integer");
        }
        result = static_cast<std::uint64_t>(number);
    } else {
        throw ValidationError(
            std::string(field) + " must be a positive safe integer");
    }
    if (result > max_safe_integer || (positive && result == 0)) {
        throw ValidationError(
            std::string(field) + " must be a positive safe integer");
    }
    return result;
}

std::vector<std::string> parse_identifier_array(
    const Json& value,
    std::string_view field,
    bool must_be_non_empty) {
    const auto& array = require_array(value, field);
    if (must_be_non_empty && array.empty()) {
        throw ValidationError(std::string(field) + " must be a non-empty string array");
    }
    std::vector<std::string> result;
    result.reserve(array.size());
    std::unordered_set<std::string> identifiers;
    for (const auto& item : array) {
        auto identifier = require_identifier(item, field);
        if (!identifiers.emplace(identifier).second) {
            throw ValidationError(std::string(field) + " must not contain duplicates");
        }
        result.push_back(std::move(identifier));
    }
    return result;
}

void validate_identifier_array(
    const std::vector<std::string>& values,
    std::string_view field,
    bool must_be_non_empty) {
    if (must_be_non_empty && values.empty()) {
        throw ValidationError(std::string(field) + " must be a non-empty string array");
    }
    std::unordered_set<std::string> identifiers;
    for (const auto& value : values) {
        validate_identifier(value, field);
        if (!identifiers.emplace(value).second) {
            throw ValidationError(std::string(field) + " must not contain duplicates");
        }
    }
}

ResultMetricScope parse_metric_scope(const Json& value) {
    if (!value.is_string()) {
        throw ValidationError("Result metric scope must be participant or team");
    }
    const auto scope = value.get<std::string>();
    if (scope == "participant") {
        return ResultMetricScope::Participant;
    }
    if (scope == "team") {
        return ResultMetricScope::Team;
    }
    throw ValidationError("Result metric scope must be participant or team");
}

std::string metric_scope_name(ResultMetricScope scope) {
    switch (scope) {
    case ResultMetricScope::Participant:
        return "participant";
    case ResultMetricScope::Team:
        return "team";
    }
    throw ValidationError("Result metric scope must be participant or team");
}

ResultOutcome parse_result_outcome(const Json& value) {
    if (!value.is_string()) {
        throw ValidationError("Result outcome must be win, loss, draw, or push");
    }
    const auto outcome = value.get<std::string>();
    if (outcome == "win") {
        return ResultOutcome::Win;
    }
    if (outcome == "loss") {
        return ResultOutcome::Loss;
    }
    if (outcome == "draw") {
        return ResultOutcome::Draw;
    }
    if (outcome == "push") {
        return ResultOutcome::Push;
    }
    throw ValidationError("Result outcome must be win, loss, draw, or push");
}

GameResultCorrectionAction parse_correction_action(const Json& value) {
    if (!value.is_string()) {
        throw ValidationError("Correction action must be replace or retract");
    }
    const auto action = value.get<std::string>();
    if (action == "replace") {
        return GameResultCorrectionAction::Replace;
    }
    if (action == "retract") {
        return GameResultCorrectionAction::Retract;
    }
    throw ValidationError("Correction action must be replace or retract");
}

std::map<std::string, double> parse_metric_values(
    const Json& value,
    std::string_view field) {
    require_object(value, field);
    if (value.size() > max_metrics_per_entity) {
        throw ValidationError("Result metrics exceed the 128-metric per-entity limit");
    }
    std::map<std::string, double> result;
    for (auto item = value.begin(); item != value.end(); ++item) {
        validate_identifier(item.key(), "Result metric ID");
        if (!item.value().is_number()) {
            throw ValidationError(
                std::string(field) + "." + item.key() +
                " must be a finite IEEE-754 number");
        }
        const double metric_value = item.value().get<double>();
        if (!std::isfinite(metric_value)) {
            throw ValidationError(
                std::string(field) + "." + item.key() +
                " must be a finite IEEE-754 number");
        }
        result.emplace(item.key(), metric_value);
    }
    return result;
}

GameResultParticipant parse_result_participant(const Json& value) {
    reject_unknown_fields(
        value,
        {"playerId", "teamId", "outcome", "rank", "metrics"},
        "GameResultParticipant");
    GameResultParticipant participant;
    participant.player_id = require_identifier(
        require_field(value, "playerId", "GameResultParticipant"),
        "GameResultParticipant.playerId");
    if (value.contains("teamId")) {
        participant.team_id = require_identifier(
            value.at("teamId"), "GameResultParticipant.teamId");
    }
    if (value.contains("outcome")) {
        participant.outcome = parse_result_outcome(value.at("outcome"));
    }
    if (value.contains("rank")) {
        participant.rank = require_safe_integer(
            value.at("rank"), "GameResultParticipant.rank", true);
    }
    participant.metrics = parse_metric_values(
        require_field(value, "metrics", "GameResultParticipant"),
        "GameResultParticipant.metrics");
    return participant;
}

GameResultTeam parse_result_team(const Json& value) {
    reject_unknown_fields(
        value, {"teamId", "outcome", "rank", "metrics"}, "GameResultTeam");
    GameResultTeam team;
    team.team_id = require_identifier(
        require_field(value, "teamId", "GameResultTeam"),
        "GameResultTeam.teamId");
    if (value.contains("outcome")) {
        team.outcome = parse_result_outcome(value.at("outcome"));
    }
    if (value.contains("rank")) {
        team.rank = require_safe_integer(
            value.at("rank"), "GameResultTeam.rank", true);
    }
    team.metrics = parse_metric_values(
        require_field(value, "metrics", "GameResultTeam"),
        "GameResultTeam.metrics");
    return team;
}

GameResultCorrection parse_result_correction(const Json& value) {
    reject_unknown_fields(
        value,
        {"action", "supersedesEventId"},
        "GameResultCorrection");
    return GameResultCorrection{
        .action = parse_correction_action(
            require_field(value, "action", "GameResultCorrection")),
        .supersedes_event_id = require_identifier(
            require_field(value, "supersedesEventId", "GameResultCorrection"),
            "GameResultCorrection.supersedesEventId")};
}

GameMetricDefinition parse_metric_definition(const Json& value) {
    reject_unknown_fields(
        value,
        {"id", "scope", "unit", "unitTypes", "aggregation"},
        "GameMetricDefinition");
    GameMetricDefinition metric;
    metric.id = require_identifier(
        require_field(value, "id", "GameMetricDefinition"),
        "GameMetricDefinition.id");
    metric.scope = parse_metric_scope(
        require_field(value, "scope", "GameMetricDefinition"));
    metric.unit = require_identifier(
        require_field(value, "unit", "GameMetricDefinition"),
        "GameMetricDefinition.unit");
    metric.unit_types = parse_identifier_array(
        require_field(value, "unitTypes", "GameMetricDefinition"),
        "GameMetricDefinition.unitTypes",
        true);
    metric.aggregation = require_field(
        value, "aggregation", "GameMetricDefinition").get<GameMetricAggregation>();
    return metric;
}

GameResultDefinition parse_result_definition(const Json& value) {
    reject_unknown_fields(
        value, {"version", "unitTypes", "metrics"}, "GameResultDefinition");
    GameResultDefinition definition;
    definition.version = require_identifier(
        require_field(value, "version", "GameResultDefinition"),
        "GameResultDefinition.version");
    definition.unit_types = parse_identifier_array(
        require_field(value, "unitTypes", "GameResultDefinition"),
        "GameResultDefinition.unitTypes",
        true);
    const auto& metrics = require_array(
        require_field(value, "metrics", "GameResultDefinition"),
        "GameResultDefinition.metrics");
    if (metrics.size() > max_metrics_per_entity) {
        throw ValidationError("GameResultDefinition exceeds the 128-metric limit");
    }
    definition.metrics.reserve(metrics.size());
    for (const auto& metric : metrics) {
        definition.metrics.push_back(parse_metric_definition(metric));
    }
    validate_game_result_definition(definition);
    return definition;
}

GameResultEvent parse_result_event(
    const Json& value,
    const GameResultDefinition* definition) {
    reject_unknown_fields(
        value,
        {"schemaVersion", "eventId", "matchId", "unitId", "unitType",
         "rulesVersion", "definitionVersion", "occurredAt", "participants",
         "teams", "correction"},
        "GameResultEvent");

    const auto schema_version = require_safe_integer(
        require_field(value, "schemaVersion", "GameResultEvent"),
        "GameResultEvent.schemaVersion",
        false);
    if (schema_version != 1) {
        throw ValidationError("GameResultEvent.schemaVersion must be 1");
    }

    GameResultEvent event;
    event.schema_version = static_cast<std::uint32_t>(schema_version);
    event.event_id = require_identifier(
        require_field(value, "eventId", "GameResultEvent"),
        "GameResultEvent.eventId");
    event.match_id = require_identifier(
        require_field(value, "matchId", "GameResultEvent"),
        "GameResultEvent.matchId");
    event.unit_id = require_identifier(
        require_field(value, "unitId", "GameResultEvent"),
        "GameResultEvent.unitId");
    event.unit_type = require_identifier(
        require_field(value, "unitType", "GameResultEvent"),
        "GameResultEvent.unitType");
    event.rules_version = require_identifier(
        require_field(value, "rulesVersion", "GameResultEvent"),
        "GameResultEvent.rulesVersion");
    event.definition_version = require_identifier(
        require_field(value, "definitionVersion", "GameResultEvent"),
        "GameResultEvent.definitionVersion");
    if (value.contains("occurredAt")) {
        event.occurred_at = require_safe_integer(
            value.at("occurredAt"), "GameResultEvent.occurredAt", true);
    }

    const auto& participants = require_array(
        require_field(value, "participants", "GameResultEvent"),
        "GameResultEvent.participants");
    event.participants.reserve(participants.size());
    for (const auto& participant : participants) {
        event.participants.push_back(parse_result_participant(participant));
    }
    if (value.contains("teams")) {
        const auto& teams = require_array(
            value.at("teams"), "GameResultEvent.teams");
        std::vector<GameResultTeam> parsed_teams;
        parsed_teams.reserve(teams.size());
        for (const auto& team : teams) {
            parsed_teams.push_back(parse_result_team(team));
        }
        event.teams = std::move(parsed_teams);
    }
    if (value.contains("correction")) {
        event.correction = parse_result_correction(value.at("correction"));
    }
    validate_game_result_event(event, definition);
    return event;
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

void validate_game_result_definition(const GameResultDefinition& definition) {
    validate_identifier(definition.version, "GameResultDefinition.version");
    validate_identifier_array(
        definition.unit_types, "GameResultDefinition.unitTypes", true);
    if (definition.metrics.size() > max_metrics_per_entity) {
        throw ValidationError("GameResultDefinition exceeds the 128-metric limit");
    }

    std::unordered_set<std::string> unit_types(
        definition.unit_types.begin(), definition.unit_types.end());
    std::map<std::string, const GameMetricDefinition*> metrics;
    for (const auto& metric : definition.metrics) {
        validate_identifier(metric.id, "GameMetricDefinition.id");
        validate_identifier(metric.unit, "GameMetricDefinition.unit");
        if (metric.scope != ResultMetricScope::Participant &&
            metric.scope != ResultMetricScope::Team) {
            throw ValidationError(
                "GameMetricDefinition.scope must be participant or team");
        }
        validate_identifier_array(
            metric.unit_types, "GameMetricDefinition.unitTypes", true);
        for (const auto& unit_type : metric.unit_types) {
            if (!unit_types.contains(unit_type)) {
                throw ValidationError(
                    "GameMetricDefinition.unitTypes must be declared result-unit types");
            }
        }
        if (!metrics.emplace(metric.id, &metric).second) {
            throw ValidationError("Duplicate metric ID: " + metric.id);
        }
        const auto& aggregation = metric.aggregation;
        if (aggregation.type == "ratio") {
            if (!aggregation.ratio) {
                throw ValidationError("Ratio metric aggregation is invalid");
            }
        } else if (aggregation.type == "sum" || aggregation.type == "min" ||
                   aggregation.type == "max") {
            if (aggregation.ratio) {
                throw ValidationError("Simple metric aggregation is invalid");
            }
        } else {
            throw ValidationError("Metric aggregation is unsupported");
        }
    }

    for (const auto& metric : definition.metrics) {
        if (metric.aggregation.type != "ratio") {
            continue;
        }
        const auto& ratio = *metric.aggregation.ratio;
        validate_identifier(
            ratio.numerator_metric_id, "ratio.numeratorMetricId");
        validate_identifier(
            ratio.denominator_metric_id, "ratio.denominatorMetricId");
        if (ratio.numerator_metric_id == metric.id ||
            ratio.denominator_metric_id == metric.id) {
            throw ValidationError(
                "Ratio metric " + metric.id + " cannot reference itself");
        }
        const auto numerator = metrics.find(ratio.numerator_metric_id);
        const auto denominator = metrics.find(ratio.denominator_metric_id);
        if (numerator == metrics.end() || denominator == metrics.end() ||
            numerator->second->scope != metric.scope ||
            denominator->second->scope != metric.scope ||
            numerator->second->aggregation.type != "sum" ||
            denominator->second->aggregation.type != "sum") {
            throw ValidationError(
                "Ratio metric " + metric.id +
                " must reference sum metrics with the same scope");
        }
        for (const auto& unit_type : metric.unit_types) {
            const auto supports = [&unit_type](const GameMetricDefinition* source) {
                return std::find(
                           source->unit_types.begin(),
                           source->unit_types.end(),
                           unit_type) != source->unit_types.end();
            };
            if (!supports(numerator->second) || !supports(denominator->second)) {
                throw ValidationError(
                    "Ratio metric " + metric.id +
                    " unit types must be supported by both numerator and denominator");
            }
        }
    }
}

void validate_game_result_event(
    const GameResultEvent& event,
    const GameResultDefinition* definition) {
    if (event.schema_version != 1) {
        throw ValidationError("GameResultEvent.schemaVersion must be 1");
    }
    validate_identifier(event.event_id, "GameResultEvent.eventId");
    validate_identifier(event.match_id, "GameResultEvent.matchId");
    validate_identifier(event.unit_id, "GameResultEvent.unitId");
    validate_identifier(event.unit_type, "GameResultEvent.unitType");
    validate_identifier(event.rules_version, "GameResultEvent.rulesVersion");
    validate_identifier(
        event.definition_version, "GameResultEvent.definitionVersion");
    if (event.occurred_at && *event.occurred_at > max_safe_integer) {
        throw ValidationError(
            "GameResultEvent.occurredAt must be a non-negative safe integer timestamp");
    }

    const std::vector<GameResultTeam> no_teams;
    const auto& teams = event.teams ? *event.teams : no_teams;
    if (event.participants.size() > max_result_participants) {
        throw ValidationError(
            "GameResultEvent exceeds the 1000-participant limit");
    }
    if (teams.size() > max_result_teams) {
        throw ValidationError("GameResultEvent exceeds the 256-team limit");
    }

    const bool is_retraction = event.correction &&
        event.correction->action == GameResultCorrectionAction::Retract;
    if (event.correction) {
        if (event.correction->action != GameResultCorrectionAction::Replace &&
            event.correction->action != GameResultCorrectionAction::Retract) {
            throw ValidationError(
                "GameResultEvent.correction.action must be replace or retract");
        }
        validate_identifier(
            event.correction->supersedes_event_id,
            "GameResultEvent.correction.supersedesEventId");
        if (event.correction->supersedes_event_id == event.event_id) {
            throw ValidationError("A correction cannot supersede itself");
        }
    }
    if (event.participants.empty() && teams.empty() && !is_retraction) {
        throw ValidationError(
            "GameResultEvent must include a participant or team");
    }
    if (is_retraction && (!event.participants.empty() || !teams.empty())) {
        throw ValidationError(
            "A retraction must not include participants or teams");
    }

    if (definition) {
        validate_game_result_definition(*definition);
        if (event.definition_version != definition->version) {
            throw ValidationError(
                "GameResultEvent.definitionVersion does not match the published definition");
        }
        if (std::find(
                definition->unit_types.begin(),
                definition->unit_types.end(),
                event.unit_type) == definition->unit_types.end()) {
            throw ValidationError("GameResultEvent.unitType is not declared");
        }
    }

    const auto validate_metrics = [&](const std::map<std::string, double>& values,
                                      ResultMetricScope scope,
                                      std::string_view label) {
        if (values.size() > max_metrics_per_entity) {
            throw ValidationError(
                "Result metrics exceed the 128-metric per-entity limit");
        }
        for (const auto& [id, metric_value] : values) {
            validate_identifier(id, "Result metric ID");
            if (!std::isfinite(metric_value)) {
                throw ValidationError(
                    std::string(label) +
                    " metric values must be finite IEEE-754 numbers");
            }
            if (!definition) {
                continue;
            }
            const auto metric = std::find_if(
                definition->metrics.begin(),
                definition->metrics.end(),
                [&id](const GameMetricDefinition& item) {
                    return item.id == id;
                });
            if (metric == definition->metrics.end() ||
                metric->scope != scope ||
                metric->aggregation.type == "ratio" ||
                std::find(
                    metric->unit_types.begin(),
                    metric->unit_types.end(),
                    event.unit_type) == metric->unit_types.end()) {
                throw ValidationError(
                    std::string(label) +
                    " contains undeclared or inapplicable metric " + id);
            }
        }
    };

    std::unordered_set<std::string> participant_ids;
    for (const auto& participant : event.participants) {
        validate_identifier(participant.player_id, "participant.playerId");
        if (!participant_ids.emplace(participant.player_id).second) {
            throw ValidationError(
                "Player appears more than once in a result unit: " +
                participant.player_id);
        }
        if (participant.team_id) {
            validate_identifier(*participant.team_id, "participant.teamId");
        }
        if (participant.outcome &&
            *participant.outcome != ResultOutcome::Win &&
            *participant.outcome != ResultOutcome::Loss &&
            *participant.outcome != ResultOutcome::Draw &&
            *participant.outcome != ResultOutcome::Push) {
            throw ValidationError("participant.outcome is invalid");
        }
        if (participant.rank &&
            (*participant.rank == 0 || *participant.rank > max_safe_integer)) {
            throw ValidationError(
                "participant.rank must be a positive safe integer");
        }
        validate_metrics(
            participant.metrics,
            ResultMetricScope::Participant,
            "participant");
    }

    std::unordered_set<std::string> team_ids;
    for (const auto& team : teams) {
        validate_identifier(team.team_id, "team.teamId");
        if (!team_ids.emplace(team.team_id).second) {
            throw ValidationError(
                "Team appears more than once in a result unit: " + team.team_id);
        }
        if (team.outcome && *team.outcome != ResultOutcome::Win &&
            *team.outcome != ResultOutcome::Loss &&
            *team.outcome != ResultOutcome::Draw &&
            *team.outcome != ResultOutcome::Push) {
            throw ValidationError("team.outcome is invalid");
        }
        if (team.rank && (*team.rank == 0 || *team.rank > max_safe_integer)) {
            throw ValidationError("team.rank must be a positive safe integer");
        }
        validate_metrics(team.metrics, ResultMetricScope::Team, "team");
    }
    if (!team_ids.empty()) {
        for (const auto& participant : event.participants) {
            if (participant.team_id &&
                !team_ids.contains(*participant.team_id)) {
                throw ValidationError(
                    "Participant references undeclared teamId: " +
                    *participant.team_id);
            }
        }
    }

    std::string serialized;
    try {
        serialized = Json(event).dump(
            -1, ' ', false, Json::error_handler_t::strict);
    } catch (const Json::exception&) {
        throw ValidationError(
            "GameResultEvent could not be serialized as valid UTF-8 JSON");
    }
    if (serialized.size() > max_result_event_bytes) {
        throw ValidationError(
            "GameResultEvent exceeds the 262144-byte limit");
    }
}

void validate_game_result_units(
    const std::optional<std::vector<GameResultEvent>>& events,
    const GameResultDefinition* definition) {
    if (!events) {
        return;
    }
    if (events->size() > max_result_events_per_transition) {
        throw ValidationError(
            "resultUnits exceeds the 32-event per-transition limit");
    }
    std::unordered_set<std::string> event_ids;
    std::set<std::pair<std::string, std::string>> unit_ids;
    for (const auto& event : *events) {
        validate_game_result_event(event, definition);
        if (!event_ids.emplace(event.event_id).second) {
            throw ValidationError(
                "Duplicate result eventId in one state transition: " +
                event.event_id);
        }
        if (!unit_ids.emplace(event.match_id, event.unit_id).second) {
            throw ValidationError(
                "Duplicate result unit in one state transition: " +
                event.match_id + "/" + event.unit_id);
        }
    }
}

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
    if (manifest.result_definition) {
        validate_game_result_definition(*manifest.result_definition);
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
    if (value.contains("resultDefinition")) {
        manifest.result_definition =
            parse_game_result_definition(value.at("resultDefinition"));
    }
    return manifest;
}

GameResultDefinition parse_game_result_definition(const Json& value) {
    return parse_result_definition(value);
}

GameResultEvent parse_game_result_event(
    const Json& value,
    const GameResultDefinition* definition) {
    return parse_result_event(value, definition);
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
    if (manifest.result_definition) {
        value["resultDefinition"] = *manifest.result_definition;
    }
}

void from_json(const Json& value, GameManifest& manifest) {
    manifest = parse_game_manifest(value);
}

void to_json(Json& value, const GameResultDefinition& definition) {
    validate_game_result_definition(definition);
    value = Json{
        {"version", definition.version},
        {"unitTypes", definition.unit_types},
        {"metrics", definition.metrics}};
}

void from_json(const Json& value, GameResultDefinition& definition) {
    definition = parse_game_result_definition(value);
}

void to_json(Json& value, const GameMetricDefinition& metric) {
    value = Json{
        {"id", metric.id},
        {"scope", metric.scope},
        {"unit", metric.unit},
        {"unitTypes", metric.unit_types},
        {"aggregation", metric.aggregation}};
}

void from_json(const Json& value, GameMetricDefinition& metric) {
    metric = parse_metric_definition(value);
}

void to_json(Json& value, const GameMetricAggregation& aggregation) {
    if (aggregation.type == "ratio" && aggregation.ratio) {
        value = Json{
            {"type", "ratio"},
            {"numeratorMetricId", aggregation.ratio->numerator_metric_id},
            {"denominatorMetricId", aggregation.ratio->denominator_metric_id}};
        return;
    }
    if ((aggregation.type == "sum" || aggregation.type == "min" ||
         aggregation.type == "max") &&
        !aggregation.ratio) {
        value = aggregation.type;
        return;
    }
    throw ValidationError("Metric aggregation is invalid");
}

void from_json(const Json& value, GameMetricAggregation& aggregation) {
    if (value.is_string()) {
        aggregation.type = value.get<std::string>();
        if (aggregation.type != "sum" && aggregation.type != "min" &&
            aggregation.type != "max") {
            throw ValidationError("Metric aggregation is unsupported");
        }
        aggregation.ratio.reset();
        return;
    }
    reject_unknown_fields(
        value,
        {"type", "numeratorMetricId", "denominatorMetricId"},
        "RatioMetricAggregation");
    const auto type = require_string(
        require_field(value, "type", "RatioMetricAggregation"),
        "RatioMetricAggregation.type");
    if (type != "ratio") {
        throw ValidationError("Ratio metric aggregation is invalid");
    }
    aggregation.type = "ratio";
    aggregation.ratio = RatioMetricAggregation{
        .numerator_metric_id = require_identifier(
            require_field(
                value, "numeratorMetricId", "RatioMetricAggregation"),
            "RatioMetricAggregation.numeratorMetricId"),
        .denominator_metric_id = require_identifier(
            require_field(
                value, "denominatorMetricId", "RatioMetricAggregation"),
            "RatioMetricAggregation.denominatorMetricId")};
}

void to_json(Json& value, const RatioMetricAggregation& aggregation) {
    value = Json{
        {"type", "ratio"},
        {"numeratorMetricId", aggregation.numerator_metric_id},
        {"denominatorMetricId", aggregation.denominator_metric_id}};
}

void from_json(const Json& value, RatioMetricAggregation& aggregation) {
    const auto parsed = value.get<GameMetricAggregation>();
    if (parsed.type != "ratio" || !parsed.ratio) {
        throw ValidationError("Ratio metric aggregation is invalid");
    }
    aggregation = *parsed.ratio;
}

void to_json(Json& value, ResultMetricScope scope) {
    value = metric_scope_name(scope);
}

void from_json(const Json& value, ResultMetricScope& scope) {
    scope = parse_metric_scope(value);
}

void to_json(Json& value, ResultOutcome outcome) {
    switch (outcome) {
    case ResultOutcome::Win:
        value = "win";
        return;
    case ResultOutcome::Loss:
        value = "loss";
        return;
    case ResultOutcome::Draw:
        value = "draw";
        return;
    case ResultOutcome::Push:
        value = "push";
        return;
    }
    throw ValidationError("Result outcome is invalid");
}

void from_json(const Json& value, ResultOutcome& outcome) {
    outcome = parse_result_outcome(value);
}

void to_json(Json& value, GameResultCorrectionAction action) {
    switch (action) {
    case GameResultCorrectionAction::Replace:
        value = "replace";
        return;
    case GameResultCorrectionAction::Retract:
        value = "retract";
        return;
    }
    throw ValidationError("Correction action is invalid");
}

void from_json(const Json& value, GameResultCorrectionAction& action) {
    action = parse_correction_action(value);
}

void to_json(Json& value, const GameResultCorrection& correction) {
    value = Json{
        {"action", correction.action},
        {"supersedesEventId", correction.supersedes_event_id}};
}

void from_json(const Json& value, GameResultCorrection& correction) {
    correction = parse_result_correction(value);
}

void to_json(Json& value, const GameResultParticipant& participant) {
    value = Json{{"playerId", participant.player_id}, {"metrics", participant.metrics}};
    if (participant.team_id) {
        value["teamId"] = *participant.team_id;
    }
    if (participant.outcome) {
        value["outcome"] = *participant.outcome;
    }
    if (participant.rank) {
        value["rank"] = *participant.rank;
    }
}

void from_json(const Json& value, GameResultParticipant& participant) {
    participant = parse_result_participant(value);
}

void to_json(Json& value, const GameResultTeam& team) {
    value = Json{{"teamId", team.team_id}, {"metrics", team.metrics}};
    if (team.outcome) {
        value["outcome"] = *team.outcome;
    }
    if (team.rank) {
        value["rank"] = *team.rank;
    }
}

void from_json(const Json& value, GameResultTeam& team) {
    team = parse_result_team(value);
}

void to_json(Json& value, const GameResultEvent& event) {
    value = Json{
        {"schemaVersion", event.schema_version},
        {"eventId", event.event_id},
        {"matchId", event.match_id},
        {"unitId", event.unit_id},
        {"unitType", event.unit_type},
        {"rulesVersion", event.rules_version},
        {"definitionVersion", event.definition_version},
        {"participants", event.participants}};
    if (event.occurred_at) {
        value["occurredAt"] = *event.occurred_at;
    }
    if (event.teams) {
        value["teams"] = *event.teams;
    }
    if (event.correction) {
        value["correction"] = *event.correction;
    }
}

void from_json(const Json& value, GameResultEvent& event) {
    event = parse_game_result_event(value);
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
    validate_game_result_units(result.result_units);
    value = Json{{"state", result.state}};
    if (result.broadcasts) {
        value["broadcasts"] = *result.broadcasts;
    }
    if (result.result_units) {
        value["resultUnits"] = *result.result_units;
    }
}

void to_json(Json& value, const TickResult& result) {
    validate_broadcasts(result.broadcasts);
    validate_game_result_units(result.result_units);
    value = Json{{"state", result.state}, {"persist", result.persist}};
    if (result.broadcasts) {
        value["broadcasts"] = *result.broadcasts;
    }
    if (result.result_units) {
        value["resultUnits"] = *result.result_units;
    }
}

void to_json(Json& value, const GameOverResult& result) {
    validate_game_result_units(result.result_units);
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
    if (result.result_units) {
        value["resultUnits"] = *result.result_units;
    }
    if (result.over && (!result.summary || !result.next_match_policy)) {
        throw std::invalid_argument(
            "A completed game requires summary and next-match policy");
    }
}

} // namespace randseed
