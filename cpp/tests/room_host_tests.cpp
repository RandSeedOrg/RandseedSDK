#include "randseed/mulberry32.hpp"
#include "randseed/room_host.hpp"

#include <httplib.h>

#include <cmath>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <utility>

namespace {

using randseed::CommandResult;
using randseed::Game;
using randseed::GameManifest;
using randseed::GameOverResult;
using randseed::GameResultDefinition;
using randseed::GameResultEvent;
using randseed::InitResult;
using randseed::Json;
using randseed::MatchConfig;
using randseed::PlayerEventResult;
using randseed::TickContext;
using randseed::TickResult;

void require(bool condition, const char* message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

class FixtureGame final : public Game {
public:
    explicit FixtureGame(Json fixture) : fixture_(std::move(fixture)) {}

    GameManifest manifest() const override {
        auto result = fixture_.at("manifest").get<GameManifest>();
        if (result_definition_) {
            result.result_definition = result_definition_;
        }
        return result;
    }

    InitResult init(const MatchConfig&) const override {
        if (fail_init_) {
            throw std::runtime_error("private callback failure detail");
        }
        const auto& expected = fixture_.at("initResponse");
        InitResult result{
            .state = expected.at("state"),
            .broadcasts = expected.value("broadcasts", Json::array()),
            .result_units = result_units_};
        return result;
    }

    TickResult tick(const TickContext&, const Json&) const override {
        const auto& expected = fixture_.at("tickResponse");
        TickResult result{
            .state = expected.at("state"),
            .broadcasts = expected.value("broadcasts", Json::array()),
            .persist = expected.at("persist").get<bool>(),
            .result_units = result_units_};
        return result;
    }

    PlayerEventResult on_player_join(
        std::string_view,
        const Json&) const override {
        return player_event_result();
    }

    PlayerEventResult on_player_leave(
        std::string_view,
        const Json&) const override {
        return player_event_result();
    }

    GameOverResult is_game_over(const Json& state) const override {
        const auto& expected = fixture_.at("gameOverResponse");
        return GameOverResult{
            .over = expected.at("over").get<bool>(),
            .summary = state.is_object() && state.contains("testSummary")
                ? state.at("testSummary") : expected.at("summary"),
            .next_match_policy = expected.at("nextMatchPolicy"),
            .result_units = result_units_};
    }

    CommandResult on_command(const Json&, const Json&) const override {
        const auto& expected = fixture_.at("commandResponse");
        return CommandResult{
            .state = expected.at("state"),
            .broadcasts = expected.value("broadcasts", Json::array()),
            .result_units = result_units_};
    }

    void fail_init(bool value) {
        fail_init_ = value;
    }

    void set_result_definition(GameResultDefinition definition) {
        result_definition_ = std::move(definition);
    }

    void set_result_units(std::vector<GameResultEvent> result_units) {
        result_units_ = std::move(result_units);
    }

    const Json& fixture() const {
        return fixture_;
    }

private:
    PlayerEventResult player_event_result() const {
        const auto& expected = fixture_.at("playerEventResponse");
        return PlayerEventResult{
            .state = expected.at("state"),
            .broadcasts = expected.value("broadcasts", Json::array()),
            .result_units = result_units_};
    }

    Json fixture_;
    bool fail_init_{false};
    std::optional<GameResultDefinition> result_definition_;
    std::optional<std::vector<GameResultEvent>> result_units_;
};

Json read_fixture(const char* path) {
    std::ifstream input(path);
    if (!input) {
        throw std::runtime_error("Unable to read shared protocol fixture");
    }
    Json fixture;
    input >> fixture;
    return fixture;
}

void test_fixture_serialization(const Json& fixture) {
    auto optional_options = fixture.at("config");
    optional_options.erase("options");
    require(optional_options.get<MatchConfig>().options == Json::object(), "omitted options must be accepted");
    optional_options["options"] = Json::object();
    require(optional_options.get<MatchConfig>().options == Json::object(), "empty options must be accepted");
    optional_options["options"] = nullptr;
    bool invalid_options_rejected = false;
    try {
        optional_options.get<MatchConfig>();
    } catch (const std::exception&) {
        invalid_options_rejected = true;
    }
    require(invalid_options_rejected, "null options must be rejected");
    require(
        Json(fixture.at("manifest").get<GameManifest>()) == fixture.at("manifest"),
        "manifest serialization differs from fixture");
    require(
        Json(fixture.at("config").get<MatchConfig>()) == fixture.at("config"),
        "match configuration serialization differs from fixture");
    require(
        Json(fixture.at("tickRequest").get<randseed::TickRequest>()) ==
            fixture.at("tickRequest"),
        "tick request serialization differs from fixture");
    require(
        Json(fixture.at("playerEventRequest")
                 .get<randseed::PlayerEventRequest>()) ==
            fixture.at("playerEventRequest"),
        "player-event serialization differs from fixture");
    require(
        Json(fixture.at("isGameOverRequest")
                 .get<randseed::IsGameOverRequest>()) ==
            fixture.at("isGameOverRequest"),
        "game-over request serialization differs from fixture");
    require(
        Json(fixture.at("commandRequest").get<randseed::CommandRequest>()) ==
            fixture.at("commandRequest"),
        "command request serialization differs from fixture");

    const auto& init = fixture.at("initResponse");
    require(
        Json(InitResult{
            .state = init.at("state"),
            .broadcasts = init.at("broadcasts")}) == init,
        "init response serialization differs from fixture");
    const auto& tick = fixture.at("tickResponse");
    require(
        Json(TickResult{
            .state = tick.at("state"),
            .broadcasts = tick.at("broadcasts"),
            .persist = tick.at("persist").get<bool>()}) == tick,
        "tick response serialization differs from fixture");
    const auto& player_event = fixture.at("playerEventResponse");
    require(
        Json(PlayerEventResult{
            .state = player_event.at("state"),
            .broadcasts = player_event.at("broadcasts")}) == player_event,
        "player-event response serialization differs from fixture");
    const auto& game_over = fixture.at("gameOverResponse");
    require(
        Json(GameOverResult{
            .over = game_over.at("over").get<bool>(),
            .summary = game_over.at("summary"),
            .next_match_policy = game_over.at("nextMatchPolicy")}) ==
            game_over,
        "game-over response serialization differs from fixture");
    const auto& command = fixture.at("commandResponse");
    require(
        Json(CommandResult{
            .state = command.at("state"),
            .broadcasts = command.at("broadcasts")}) == command,
        "command response serialization differs from fixture");
}

void test_prng_vectors(const Json& fixture) {
    for (const auto& vector : fixture.at("prngVectors")) {
        randseed::Mulberry32 generator(vector.at("seed").get<double>());
        for (const auto& expected : vector.at("u32")) {
            require(
                generator.next_u32() == expected.get<std::uint32_t>(),
                "Mulberry32 output differs from fixture");
        }
    }

    require(
        randseed::Mulberry32::to_int32(4294967295.0) == -1,
        "seed conversion did not use JavaScript signed 32-bit semantics");
    require(
        randseed::Mulberry32::to_int32(4294967297.0) == 1,
        "seed conversion did not wrap at 32 bits");
    require(
        randseed::Mulberry32::to_int32(-1.9) == -1,
        "seed conversion did not truncate toward zero");
    require(
        randseed::Mulberry32::to_int32(
            std::numeric_limits<double>::infinity()) == 0,
        "non-finite seed conversion did not return zero");

    randseed::Mulberry32 float_generator(1.0);
    const double next_float = float_generator.next_float();
    require(next_float >= 0.0 && next_float < 1.0, "next_float is out of range");
}

httplib::Result post_json(
    httplib::Client& client,
    const char* path,
    const Json& body) {
    return client.Post(path, body.dump(), "application/json");
}

Json require_response(const httplib::Result& response, int status) {
    require(static_cast<bool>(response), "HTTP request failed");
    require(response->status == status, "unexpected HTTP response status");
    return Json::parse(response->body);
}

void test_http_lifecycle(const Json& fixture) {
    FixtureGame game(fixture);
    randseed::HostOptions options;
    options.game_id = "room-counter";
    randseed::RoomHost host(game, std::move(options));

    const auto port = host.start_on_any_port();
    require(port.has_value(), "HTTP host failed to bind to an available port");
    httplib::Client client("127.0.0.1", *port);

    require(
        require_response(client.Get("/manifest"), 200) == fixture.at("manifest"),
        "GET /manifest response differs from fixture");
    const Json health = require_response(client.Get("/health"), 200);
    require(health.at("status") == "ok", "health status is invalid");
    require(health.at("gameId") == "room-counter", "health gameId is invalid");
    require(
        health.at("uptime").is_number() &&
            health.at("uptime").get<double>() >= 0.0,
        "health uptime is invalid");

    require(
        require_response(
            post_json(client, "/init", fixture.at("config")), 200) ==
            fixture.at("initResponse"),
        "POST /init response differs from fixture");
    require(
        require_response(
            post_json(client, "/tick", fixture.at("tickRequest")), 200) ==
            fixture.at("tickResponse"),
        "POST /tick response differs from fixture");
    require(
        require_response(
            post_json(
                client,
                "/onPlayerJoin",
                fixture.at("playerEventRequest")),
            200) == fixture.at("playerEventResponse"),
        "POST /onPlayerJoin response differs from fixture");
    require(
        require_response(
            post_json(
                client,
                "/onPlayerLeave",
                fixture.at("playerEventRequest")),
            200) == fixture.at("playerEventResponse"),
        "POST /onPlayerLeave response differs from fixture");
    require(
        require_response(
            post_json(
                client,
                "/isGameOver",
                fixture.at("isGameOverRequest")),
            200) == fixture.at("gameOverResponse"),
        "POST /isGameOver response differs from fixture");
    for (const Json& summary : {
             Json(nullptr), Json::array(), Json::object(),
             Json{{"durationSeconds", nullptr}}, Json{{"durationSeconds", "1"}},
             Json{{"durationSeconds", -1}}}) {
        const auto response = post_json(
            client, "/isGameOver", Json{{"state", {{"testSummary", summary}}}});
        const Json error = require_response(response, 500);
        require(error.at("error") == "Internal server error", "invalid summary was not sanitized");
    }
    for (const double duration : {0.0, 2.5}) {
        const auto response = post_json(client, "/isGameOver",
            Json{{"state", {{"testSummary", {{"durationSeconds", duration}}}}}});
        require(require_response(response, 200).at("summary").at("durationSeconds") == duration,
            "valid game-over duration was rejected");
    }
    for (const double duration : {std::numeric_limits<double>::infinity(),
                                  std::numeric_limits<double>::quiet_NaN()}) {
        bool rejected = false;
        try {
            static_cast<void>(Json(GameOverResult{
                .over = true,
                .summary = Json{{"durationSeconds", duration}},
                .next_match_policy = Json{{"mode", "close_room"}, {"timeoutMs", 0}}}));
        } catch (const std::invalid_argument&) {
            rejected = true;
        }
        require(rejected, "non-finite game-over duration was accepted");
    }
    require(
        require_response(
            post_json(client, "/onCommand", fixture.at("commandRequest")), 200) ==
            fixture.at("commandResponse"),
        "POST /onCommand response differs from fixture");

    const auto malformed = client.Post(
        "/init", "{", "application/json");
    const Json malformed_body = require_response(malformed, 400);
    require(
        malformed_body.at("error") == "Invalid JSON request body",
        "malformed JSON error was not sanitized");
    const auto invalid = client.Post(
        "/init", "{}", "application/json");
    require_response(invalid, 400);

    constexpr std::uint64_t max_safe_integer = 9007199254740991ULL;
    Json bounded_config = fixture.at("config");
    bounded_config["matchSeq"] = max_safe_integer;
    require(
        require_response(post_json(client, "/init", bounded_config), 200) ==
            fixture.at("initResponse"),
        "maximum safe positive matchSeq was rejected");

    const auto expect_bad_request = [&client](const char* path, const Json& body) {
        const auto response = post_json(client, path, body);
        require(static_cast<bool>(response), "invalid request did not receive a response");
        require(response->status == 400, "invalid request was not rejected with 400");
    };

    Json invalid_match_seq = fixture.at("config");
    invalid_match_seq["matchSeq"] = 0;
    expect_bad_request("/init", invalid_match_seq);
    invalid_match_seq["matchSeq"] = max_safe_integer + 1;
    expect_bad_request("/init", invalid_match_seq);

    Json bounded_tick = fixture.at("tickRequest");
    bounded_tick["ctx"]["tick"] = max_safe_integer;
    bounded_tick["ctx"]["inputs"][0]["timestamp"] = max_safe_integer;
    require(
        require_response(post_json(client, "/tick", bounded_tick), 200) ==
            fixture.at("tickResponse"),
        "maximum safe tick or timestamp was rejected");

    Json invalid_tick = fixture.at("tickRequest");
    invalid_tick["ctx"]["tick"] = -1;
    expect_bad_request("/tick", invalid_tick);
    invalid_tick = fixture.at("tickRequest");
    invalid_tick["ctx"]["tick"] = max_safe_integer + 1;
    expect_bad_request("/tick", invalid_tick);
    invalid_tick = fixture.at("tickRequest");
    invalid_tick["ctx"]["inputs"][0]["timestamp"] = -1;
    expect_bad_request("/tick", invalid_tick);
    invalid_tick = fixture.at("tickRequest");
    invalid_tick["ctx"]["inputs"][0]["timestamp"] = max_safe_integer + 1;
    expect_bad_request("/tick", invalid_tick);

    game.fail_init(true);
    const auto failed_callback =
        post_json(client, "/init", fixture.at("config"));
    require(
        static_cast<bool>(failed_callback),
        "callback failure request failed");
    require(
        failed_callback->status == 500,
        "callback failure did not produce HTTP 500");
    require(
        failed_callback->body.find("private callback failure detail") ==
            std::string::npos,
        "callback exception text leaked into response");
    game.fail_init(false);

    std::string exact_limit_body = fixture.at("config").dump();
    exact_limit_body.resize(
        randseed::max_request_body_bytes, ' ');
    const auto exact_limit = client.Post(
        "/init", exact_limit_body, "application/json");
    require(
        require_response(exact_limit, 200) == fixture.at("initResponse"),
        "a request exactly at the 1 MiB limit was rejected");

    const auto too_large = client.Post(
        "/init",
        std::string(randseed::max_request_body_bytes + 1, 'x'),
        "application/json");
    require(
        static_cast<bool>(too_large),
        "oversized request did not receive a response");
    require(
        too_large->status == 413,
        "oversized request was not rejected by the body-size limit");
    require(
        too_large->body.find("private callback failure detail") ==
            std::string::npos,
        "oversized request response contains internal details");

    const auto unknown = client.Get("/unknown");
    const Json unknown_body = require_response(unknown, 404);
    require(unknown_body.at("error") == "Not found", "404 response is not JSON");
    host.stop();
}

void test_http_result_unit_outputs(
    const Json& room_fixture,
    const Json& result_fixture) {
    const auto definition =
        randseed::parse_game_result_definition(result_fixture.at("definition"));
    const auto event =
        randseed::parse_game_result_event(result_fixture.at("event"), &definition);
    FixtureGame game(room_fixture);
    game.set_result_definition(definition);
    game.set_result_units({event});
    randseed::HostOptions options;
    options.game_id = "room-counter";
    randseed::RoomHost host(game, std::move(options));
    const auto port = host.start_on_any_port();
    require(port.has_value(), "HTTP host failed to bind for result-unit test");
    httplib::Client client("127.0.0.1", *port);

    Json expected_manifest = room_fixture.at("manifest");
    expected_manifest["resultDefinition"] = result_fixture.at("definition");
    require(
        require_response(client.Get("/manifest"), 200) == expected_manifest,
        "GET /manifest omitted resultDefinition");

    const auto check_result = [&client, &result_fixture, &room_fixture](
                                  const char* path,
                                  const Json& request,
                                  const char* fixture_response) {
        Json expected = room_fixture.at(fixture_response);
        expected["resultUnits"] = Json::array({result_fixture.at("event")});
        require(
            require_response(post_json(client, path, request), 200) == expected,
            "transition response omitted or changed resultUnits");
    };

    check_result("/init", room_fixture.at("config"), "initResponse");
    check_result("/tick", room_fixture.at("tickRequest"), "tickResponse");
    check_result(
        "/onPlayerJoin",
        room_fixture.at("playerEventRequest"),
        "playerEventResponse");
    check_result(
        "/onPlayerLeave",
        room_fixture.at("playerEventRequest"),
        "playerEventResponse");
    check_result(
        "/isGameOver",
        room_fixture.at("isGameOverRequest"),
        "gameOverResponse");
    check_result(
        "/onCommand",
        room_fixture.at("commandRequest"),
        "commandResponse");
    host.stop();

    auto invalid_event = event;
    invalid_event.participants[0].metrics["unpublishedMetric"] = 1.0;
    FixtureGame invalid_game(room_fixture);
    invalid_game.set_result_definition(definition);
    invalid_game.set_result_units({std::move(invalid_event)});
    randseed::HostOptions invalid_options;
    invalid_options.game_id = "room-counter";
    randseed::RoomHost invalid_host(invalid_game, std::move(invalid_options));
    const auto invalid_port = invalid_host.start_on_any_port();
    require(invalid_port.has_value(), "HTTP host failed to bind for invalid results");
    httplib::Client invalid_client("127.0.0.1", *invalid_port);
    const Json invalid_response = require_response(
        post_json(invalid_client, "/tick", room_fixture.at("tickRequest")), 500);
    require(
        invalid_response.at("error") == "Internal server error",
        "invalid result declaration error was not sanitized");
    invalid_host.stop();
}

} // namespace

int main(int argc, char** argv) {
    try {
        if (argc != 3) {
            throw std::runtime_error("Expected room-host and Result Unit fixture paths");
        }
        const Json fixture = read_fixture(argv[1]);
        const Json result_fixture = read_fixture(argv[2]);
        test_fixture_serialization(fixture);
        test_prng_vectors(fixture);
        test_http_lifecycle(fixture);
        test_http_result_unit_outputs(fixture, result_fixture);
        std::cout << "RandSeed C++ room-host tests passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "Test failure: " << error.what() << '\n';
        return 1;
    }
}
