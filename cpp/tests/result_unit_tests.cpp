#include "randseed/protocol.hpp"

#include <cstdint>
#include <fstream>
#include <functional>
#include <iostream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace {

using randseed::GameManifest;
using randseed::GameResultDefinition;
using randseed::GameResultEvent;
using randseed::Json;

constexpr std::uint64_t max_safe_integer = 9007199254740991ULL;

void require(bool condition, const char* message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

void expect_invalid(const std::function<void()>& operation, const char* message) {
    bool rejected = false;
    try {
        operation();
    } catch (const randseed::ValidationError&) {
        rejected = true;
    } catch (const Json::exception&) {
        rejected = true;
    }
    require(rejected, message);
}

Json read_fixture(const char* path) {
    std::ifstream input(path);
    if (!input) {
        throw std::runtime_error("Unable to read canonical Result Unit fixture");
    }
    Json fixture;
    input >> fixture;
    return fixture;
}

void test_fixture_round_trip(const Json& fixture) {
    const GameResultDefinition definition =
        randseed::parse_game_result_definition(fixture.at("definition"));
    const GameResultEvent event =
        randseed::parse_game_result_event(fixture.at("event"), &definition);
    require(
        Json(definition) == fixture.at("definition"),
        "result definition serialization differs from canonical fixture");
    require(
        Json(event) == fixture.at("event"),
        "result event serialization differs from canonical fixture");

    Json manifest_json{
        {"title", "Test game"},
        {"version", "1.0.0"},
        {"minPlayers", 1},
        {"maxPlayers", 4},
        {"tickRateHz", 5},
        {"persistentTable", false}};
    const GameManifest original_manifest =
        randseed::parse_game_manifest(manifest_json);
    require(
        Json(original_manifest) == manifest_json,
        "manifest without resultDefinition changed its wire representation");

    manifest_json["resultDefinition"] = fixture.at("definition");
    const GameManifest manifest = randseed::parse_game_manifest(manifest_json);
    require(
        manifest.result_definition.has_value(),
        "manifest resultDefinition was not parsed");
    require(
        Json(manifest) == manifest_json,
        "manifest resultDefinition serialization differs from fixture");
}

void test_definition_validation(const Json& fixture) {
    Json definition = fixture.at("definition");
    static_cast<void>(randseed::parse_game_result_definition(definition));

    Json unknown = definition;
    unknown["gameId"] = "host-bound";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(unknown)); },
        "unknown result-definition field was accepted");

    unknown = definition;
    unknown["metrics"][0]["custom"] = true;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(unknown)); },
        "unknown metric-definition field was accepted");

    unknown = definition;
    unknown["metrics"][2]["aggregation"]["gameId"] = "host-bound";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(unknown)); },
        "unknown ratio-aggregation field was accepted");

    Json invalid = definition;
    invalid["unitTypes"] = Json::array({"round", "round"});
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(invalid)); },
        "duplicate result unit type was accepted");

    invalid = definition;
    invalid["metrics"][0]["id"] = " \t";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(invalid)); },
        "blank metric ID was accepted");

    invalid = definition;
    invalid["metrics"][2]["aggregation"]["denominatorMetricId"] = "missing";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(invalid)); },
        "ratio with an undeclared sum metric was accepted");

    invalid = definition;
    invalid["metrics"][2]["aggregation"]["denominatorMetricId"] = "fastestLapMs";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(invalid)); },
        "ratio referencing a non-sum metric was accepted");

    invalid = definition;
    invalid["metrics"][2]["aggregation"]["numeratorMetricId"] = "teamPoints";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(invalid)); },
        "ratio referencing a different metric scope was accepted");

    invalid = definition;
    invalid["metrics"][2]["unitTypes"] = Json::array({"match"});
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(invalid)); },
        "ratio with incompatible unit types was accepted");

    Json at_limit = definition;
    Json metrics = Json::array();
    for (std::size_t index = 0; index < 128; ++index) {
        Json metric = definition.at("metrics")[0];
        metric["id"] = "metric-" + std::to_string(index);
        metrics.push_back(std::move(metric));
    }
    at_limit["metrics"] = std::move(metrics);
    static_cast<void>(randseed::parse_game_result_definition(at_limit));
    at_limit["metrics"].push_back(at_limit["metrics"][0]);
    at_limit["metrics"][128]["id"] = "metric-over-limit";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_definition(at_limit)); },
        "definition over 128 metrics was accepted");
}

Json valid_event_with_definition(const Json& fixture) {
    return fixture.at("event");
}

void test_event_validation(const Json& fixture) {
    const auto definition =
        randseed::parse_game_result_definition(fixture.at("definition"));
    const Json base = valid_event_with_definition(fixture);
    static_cast<void>(randseed::parse_game_result_event(base, &definition));

    Json optional_fields = base;
    optional_fields.erase("occurredAt");
    optional_fields.erase("teams");
    static_cast<void>(randseed::parse_game_result_event(optional_fields, &definition));

    Json invalid = base;
    invalid["gameId"] = "host-bound";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid, &definition)); },
        "host-bound gameId was accepted on a result event");
    invalid = base;
    invalid["roomId"] = "host-bound";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid, &definition)); },
        "host-bound roomId was accepted on a result event");
    invalid = base;
    invalid["participants"][0]["source"] = "host";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid, &definition)); },
        "unknown participant field was accepted");
    invalid = base;
    invalid["teams"][0]["gameId"] = "host-bound";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid, &definition)); },
        "host-bound team field was accepted");
    invalid = base;
    invalid["correction"] = {
        {"action", "replace"},
        {"supersedesEventId", "old-event"},
        {"roomId", "host-bound"}};
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "unknown correction field was accepted");

    for (const std::string field : {
             "eventId", "matchId", "unitId", "unitType",
             "rulesVersion", "definitionVersion"}) {
        invalid = base;
        invalid[field] = " \u00a0 ";
        expect_invalid(
            [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
            "blank identifier was accepted");
    }
    const std::string supplementary = "\xf0\x9f\x98\x80";
    invalid = base;
    invalid["eventId"] = std::string(129, 'e');
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "identifier over 128 code points was accepted");
    invalid["eventId"] = "";
    for (std::size_t index = 0; index < 129; ++index) {
        invalid["eventId"].get_ref<std::string&>() += supplementary;
    }
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "supplementary Unicode identifier over 128 code points was accepted");

    invalid = base;
    invalid["schemaVersion"] = 2;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "unsupported result schema version was accepted");
    invalid = base;
    invalid["definitionVersion"] = "unknown";
    expect_invalid(
        [&] {
            static_cast<void>(
                randseed::parse_game_result_event(invalid, &definition));
        },
        "event with mismatched definition version was accepted");
    invalid = base;
    invalid["unitType"] = "unknown";
    expect_invalid(
        [&] {
            static_cast<void>(
                randseed::parse_game_result_event(invalid, &definition));
        },
        "event with undeclared unit type was accepted");
    invalid = base;
    invalid["unitType"] = "match";
    expect_invalid(
        [&] {
            static_cast<void>(
                randseed::parse_game_result_event(invalid, &definition));
        },
        "metric on an inapplicable declared unit type was accepted");

    invalid = base;
    invalid["occurredAt"] = 0;
    static_cast<void>(randseed::parse_game_result_event(invalid));
    invalid["occurredAt"] = -1;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "negative timestamp was accepted");
    invalid["occurredAt"] = max_safe_integer;
    static_cast<void>(randseed::parse_game_result_event(invalid));
    invalid["occurredAt"] = max_safe_integer + 1;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "timestamp above the safe integer limit was accepted");
    invalid["occurredAt"] = 1.5;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "fractional timestamp was accepted");

    invalid = base;
    invalid["participants"][0]["rank"] = 0;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "zero participant rank was accepted");
    invalid["participants"][0]["rank"] = max_safe_integer;
    static_cast<void>(randseed::parse_game_result_event(invalid));
    invalid["participants"][0]["rank"] = max_safe_integer + 1;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "participant rank above the safe integer limit was accepted");
    invalid = base;
    invalid["teams"][0]["rank"] = -1;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "negative team rank was accepted");

    invalid = base;
    invalid["participants"][0]["outcome"] = "unknown";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "unsupported participant outcome was accepted");
    invalid = base;
    invalid["participants"][0]["metrics"]["teamPoints"] = 1.0;
    expect_invalid(
        [&] {
            static_cast<void>(
                randseed::parse_game_result_event(invalid, &definition));
        },
        "team metric on participant was accepted");
    invalid = base;
    invalid["participants"][0]["metrics"]["winRate"] = 1.0;
    expect_invalid(
        [&] {
            static_cast<void>(
                randseed::parse_game_result_event(invalid, &definition));
        },
        "derived ratio metric submission was accepted");
    invalid = base;
    invalid["participants"][0]["metrics"]["undeclared"] = 1.0;
    expect_invalid(
        [&] {
            static_cast<void>(
                randseed::parse_game_result_event(invalid, &definition));
        },
        "undeclared metric was accepted");
    invalid = base;
    invalid["participants"][0]["metrics"]["eligibleHands"] = nullptr;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "non-numeric metric value was accepted");
    invalid = base;
    invalid["participants"][0]["metrics"] = Json::array();
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "non-object metric map was accepted");

    invalid = base;
    invalid["participants"][1]["playerId"] =
        invalid["participants"][0]["playerId"];
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "duplicate player IDs were accepted");
    invalid = base;
    invalid["teams"][1]["teamId"] = invalid["teams"][0]["teamId"];
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "duplicate team IDs were accepted");
    invalid = base;
    invalid["participants"][0]["teamId"] = "missing-team";
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "participant reference to an undeclared team was accepted");

    invalid = base;
    invalid["participants"] = Json::array();
    invalid["teams"] = Json::array();
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "empty non-retraction event was accepted");
    invalid["correction"] = {
        {"action", "retract"}, {"supersedesEventId", "old-event"}};
    static_cast<void>(randseed::parse_game_result_event(invalid));
    invalid["participants"] = base["participants"];
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "retraction containing participants was accepted");
    invalid = base;
    invalid["correction"] = {
        {"action", "replace"}, {"supersedesEventId", "old-event"}};
    const auto replacement = randseed::parse_game_result_event(invalid);
    require(
        Json(replacement) == invalid,
        "replace correction serialization differs from its input");
    invalid["participants"] = Json::array();
    invalid["teams"] = Json::array();
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "empty replacement correction was accepted");
    invalid = base;
    invalid["correction"] = {
        {"action", "replace"},
        {"supersedesEventId", invalid["eventId"]}};
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "self-superseding correction was accepted");
    invalid = base;
    invalid["correction"] = {
        {"action", "remove"},
        {"supersedesEventId", "old-event"}};
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(invalid)); },
        "unsupported correction action was accepted");
    invalid = base;
    invalid["participants"] = Json::array();
    invalid["teams"] = Json::array();
    invalid["correction"] = {
        {"action", "retract"}, {"supersedesEventId", "old-event"}};
    const auto retraction = randseed::parse_game_result_event(invalid);
    require(
        Json(retraction) == invalid,
        "retract correction serialization differs from its input");

    Json count_limited = base;
    count_limited["teams"] = Json::array();
    count_limited["participants"] = Json::array();
    for (std::size_t index = 0; index < 1000; ++index) {
        count_limited["participants"].push_back({
            {"playerId", "player-" + std::to_string(index)},
            {"metrics", Json::object()}});
    }
    static_cast<void>(randseed::parse_game_result_event(count_limited));
    count_limited["participants"].push_back({
        {"playerId", "player-over-limit"}, {"metrics", Json::object()}});
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(count_limited)); },
        "event with over 1000 participants was accepted");

    Json team_limited = base;
    team_limited["participants"] = Json::array();
    team_limited["teams"] = Json::array();
    for (std::size_t index = 0; index < 256; ++index) {
        team_limited["teams"].push_back({
            {"teamId", "team-" + std::to_string(index)},
            {"metrics", Json::object()}});
    }
    static_cast<void>(randseed::parse_game_result_event(team_limited));
    team_limited["teams"].push_back({
        {"teamId", "team-over-limit"}, {"metrics", Json::object()}});
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(team_limited)); },
        "event with over 256 teams was accepted");

    Json metric_limited = base;
    metric_limited["participants"] = Json::array({
        {{"playerId", "player-one"}, {"metrics", Json::object()}}});
    for (std::size_t index = 0; index < 128; ++index) {
        metric_limited["participants"][0]["metrics"]
            ["metric-" + std::to_string(index)] = static_cast<double>(index);
    }
    static_cast<void>(randseed::parse_game_result_event(metric_limited));
    metric_limited["participants"][0]["metrics"]["metric-over-limit"] = 1.0;
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(metric_limited)); },
        "entity with over 128 metrics was accepted");

    constexpr std::size_t max_event_bytes = 256U * 1024U;
    const auto make_sized_event = [&base](std::size_t metric_id_length) {
        Json sized = base;
        sized["participants"] = Json::array();
        sized["teams"] = Json::array();
        const std::string metric_id(metric_id_length, 'm');
        for (std::size_t index = 0; index < 1000; ++index) {
            const std::string prefix = "p-" + std::to_string(index) + "-";
            const std::string player_id =
                prefix + std::string(128 - prefix.size(), 'x');
            sized["participants"].push_back({
                {"playerId", player_id},
                {"metrics", Json{{metric_id, 1.0}}}});
        }
        return sized;
    };
    std::size_t best_metric_id_length = 0;
    std::size_t best_serialized_size = 0;
    Json event_at_limit;
    for (std::size_t length = 1; length <= 128; ++length) {
        Json candidate = make_sized_event(length);
        const std::size_t serialized_size = candidate.dump().size();
        if (serialized_size > max_event_bytes) {
            break;
        }
        best_metric_id_length = length;
        best_serialized_size = serialized_size;
        event_at_limit = std::move(candidate);
    }
    require(
        best_metric_id_length > 0 && best_metric_id_length < 128,
        "could not construct an exact Result Unit event-size boundary case");
    const std::size_t padding = max_event_bytes - best_serialized_size;
    require(
        padding <= event_at_limit.at("participants").size(),
        "Result Unit event-size boundary adjustment exceeds participant count");
    const std::string short_metric_id(best_metric_id_length, 'm');
    const std::string padded_metric_id(best_metric_id_length + 1, 'm');
    for (std::size_t index = 0; index < padding; ++index) {
        auto& metrics = event_at_limit["participants"][index]["metrics"];
        metrics.erase(short_metric_id);
        metrics[padded_metric_id] = 1.0;
    }
    require(
        event_at_limit.dump().size() == max_event_bytes,
        "failed to construct an event exactly at the 256 KiB limit");
    static_cast<void>(randseed::parse_game_result_event(event_at_limit));

    Json large_event = base;
    large_event["participants"] = Json::array();
    large_event["teams"] = Json::array();
    for (std::size_t participant_index = 0; participant_index < 20;
         ++participant_index) {
        Json metrics = Json::object();
        for (std::size_t metric_index = 0; metric_index < 128; ++metric_index) {
            const auto digits = std::to_string(metric_index);
            const std::string metric_id =
                "m" + digits + std::string(126 - digits.size(), 'x');
            metrics[metric_id] = 1.0;
        }
        large_event["participants"].push_back({
            {"playerId", "player-" + std::to_string(participant_index)},
            {"metrics", std::move(metrics)}});
    }
    expect_invalid(
        [&] { static_cast<void>(randseed::parse_game_result_event(large_event)); },
        "event over 256 KiB serialized UTF-8 was accepted");

    Json unicode_size_event = base;
    unicode_size_event["participants"] = Json::array();
    unicode_size_event["teams"] = Json::array();
    for (std::size_t index = 0; index < 600; ++index) {
        const std::string prefix = "p-" + std::to_string(index) + "-";
        std::string player_id = prefix;
        for (std::size_t point = prefix.size(); point < 128; ++point) {
            player_id += supplementary;
        }
        unicode_size_event["participants"].push_back({
            {"playerId", std::move(player_id)}, {"metrics", Json::object()}});
    }
    expect_invalid(
        [&] {
            static_cast<void>(
                randseed::parse_game_result_event(unicode_size_event));
        },
        "event UTF-8 byte limit was not enforced");
}

void test_transition_outputs(const Json& fixture) {
    const auto definition =
        randseed::parse_game_result_definition(fixture.at("definition"));
    const auto event =
        randseed::parse_game_result_event(fixture.at("event"), &definition);
    const auto result_units = std::vector<GameResultEvent>{event};
    const Json event_json = fixture.at("event");

    const auto check_result_units = [&event_json](const Json& result) {
        require(
            result.at("resultUnits") == Json::array({event_json}),
            "transition did not serialize resultUnits");
    };
    check_result_units(randseed::InitResult{
        .state = Json::object(),
        .broadcasts = std::nullopt,
        .result_units = result_units});
    check_result_units(randseed::TickResult{
        .state = Json::object(),
        .broadcasts = std::nullopt,
        .persist = false,
        .result_units = result_units});
    check_result_units(randseed::PlayerEventResult{
        .state = Json::object(),
        .broadcasts = std::nullopt,
        .result_units = result_units});
    check_result_units(randseed::CommandResult{
        .state = Json::object(),
        .broadcasts = std::nullopt,
        .result_units = result_units});
    check_result_units(randseed::GameOverResult{
        .over = false,
        .summary = std::nullopt,
        .next_match_policy = std::nullopt,
        .result_units = result_units});

    require(
        !Json(randseed::InitResult{
                 .state = Json::object(),
                 .broadcasts = std::nullopt,
                 .result_units = std::nullopt})
             .contains("resultUnits"),
        "absent optional resultUnits changed existing wire behavior");
    require(
        Json(randseed::InitResult{
                 .state = Json::object(),
                 .broadcasts = std::nullopt,
                 .result_units = std::vector<GameResultEvent>{}})
            .at("resultUnits")
            .empty(),
        "explicitly empty resultUnits was not serialized");
    static_cast<void>(randseed::validate_game_result_units(
        std::optional<std::vector<GameResultEvent>>(result_units), &definition));

    auto invalid_event = event;
    invalid_event.participants[0].metrics["undeclaredMetric"] = 1.0;
    expect_invalid(
        [&] {
            randseed::validate_game_result_units(
                std::optional<std::vector<GameResultEvent>>(
                    std::vector<GameResultEvent>{invalid_event}),
                &definition);
        },
        "transition result metric outside the declaration was accepted");

    auto duplicate_events = result_units;
    duplicate_events.push_back(event);
    expect_invalid(
        [&] {
            randseed::validate_game_result_units(
                std::optional<std::vector<GameResultEvent>>(duplicate_events),
                &definition);
        },
        "duplicate result event and unit in one transition was accepted");

    auto second_event = event;
    second_event.event_id = "result-event-002";
    second_event.unit_id = "round-002";
    randseed::validate_game_result_units(
        std::optional<std::vector<GameResultEvent>>(
            std::vector<GameResultEvent>{event, second_event}),
        &definition);
    second_event.event_id = event.event_id;
    second_event.unit_id = "round-002";
    expect_invalid(
        [&] {
            randseed::validate_game_result_units(
                std::optional<std::vector<GameResultEvent>>(
                    std::vector<GameResultEvent>{event, second_event}),
                &definition);
        },
        "duplicate event IDs in one transition were accepted");
    second_event.event_id = "result-event-002";
    second_event.unit_id = event.unit_id;
    expect_invalid(
        [&] {
            randseed::validate_game_result_units(
                std::optional<std::vector<GameResultEvent>>(
                    std::vector<GameResultEvent>{event, second_event}),
                &definition);
        },
        "duplicate match/unit IDs in one transition were accepted");

    std::vector<GameResultEvent> max_events;
    for (std::size_t index = 0; index < 32; ++index) {
        auto item = event;
        item.event_id = "event-" + std::to_string(index);
        item.unit_id = "unit-" + std::to_string(index);
        max_events.push_back(std::move(item));
    }
    randseed::validate_game_result_units(
        std::optional<std::vector<GameResultEvent>>(max_events), &definition);
    max_events.push_back(event);
    expect_invalid(
        [&] {
            randseed::validate_game_result_units(
                std::optional<std::vector<GameResultEvent>>(max_events),
                &definition);
        },
        "transition with over 32 result events was accepted");
}

} // namespace

int main(int argc, char** argv) {
    try {
        if (argc != 2) {
            throw std::runtime_error("Expected canonical Result Unit fixture path");
        }
        const Json fixture = read_fixture(argv[1]);
        test_fixture_round_trip(fixture);
        test_definition_validation(fixture);
        test_event_validation(fixture);
        test_transition_outputs(fixture);
        std::cout << "RandSeed C++ Result Unit tests passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "Test failure: " << error.what() << '\n';
        return 1;
    }
}
