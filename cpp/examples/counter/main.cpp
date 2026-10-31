#include "randseed/room_host.hpp"

#include <algorithm>
#include <charconv>
#include <cstdint>
#include <iostream>
#include <limits>
#include <optional>
#include <string_view>
#include <utility>

namespace {

using randseed::CommandResult;
using randseed::Game;
using randseed::GameManifest;
using randseed::GameOverResult;
using randseed::InitResult;
using randseed::Json;
using randseed::MatchConfig;
using randseed::PlayerEventResult;
using randseed::TickContext;
using randseed::TickResult;

bool read_int64(const Json& value, std::int64_t& result) {
    if (value.is_number_integer() && !value.is_number_unsigned()) {
        result = value.get<std::int64_t>();
        return true;
    }
    if (value.is_number_unsigned()) {
        const auto unsigned_value = value.get<std::uint64_t>();
        if (unsigned_value <=
            static_cast<std::uint64_t>(std::numeric_limits<std::int64_t>::max())) {
            result = static_cast<std::int64_t>(unsigned_value);
            return true;
        }
    }
    return false;
}

std::int64_t state_count(const Json& state) {
    if (!state.is_object() || !state.contains("count")) {
        return 0;
    }
    std::int64_t count = 0;
    return read_int64(state.at("count"), count) ? count : 0;
}

std::int64_t saturating_add(std::int64_t left, std::int64_t right) {
    if (right > 0 &&
        left > std::numeric_limits<std::int64_t>::max() - right) {
        return std::numeric_limits<std::int64_t>::max();
    }
    if (right < 0 &&
        left < std::numeric_limits<std::int64_t>::min() - right) {
        return std::numeric_limits<std::int64_t>::min();
    }
    return left + right;
}

class CounterGame final : public Game {
public:
    GameManifest manifest() const override {
        return GameManifest{
            .title = "Room Counter",
            .version = "1.0.0",
            .min_players = 1,
            .max_players = 4,
            .tick_rate_hz = 5,
            .persistent_table = false,
            .options_schema = std::nullopt};
    }

    InitResult init(const MatchConfig& config) const override {
        std::int64_t initial_count = 0;
        static_cast<void>(read_int64(config.carryover, initial_count));
        return InitResult{
            .state = Json{{"count", initial_count}, {"players", config.players}},
            .broadcasts = Json::array({Json{{"type", "started"}}})};
    }

    TickResult tick(
        const TickContext& context,
        const Json& state) const override {
        std::int64_t count = state_count(state);
        bool changed = false;
        for (const auto& input : context.inputs) {
            if (!input.data.is_object() || !input.data.contains("delta")) {
                continue;
            }
            std::int64_t delta = 0;
            if (read_int64(input.data.at("delta"), delta)) {
                count = saturating_add(count, delta);
                changed = changed || delta != 0;
            }
        }

        Json broadcasts = Json::array();
        if (changed) {
            broadcasts.push_back(
                Json{{"type", "counter.changed"}, {"count", count}});
        }
        return TickResult{
            .state = Json{{"count", count}},
            .broadcasts = std::move(broadcasts),
            .persist = changed};
    }

    PlayerEventResult on_player_join(
        std::string_view player_id,
        const Json& state) const override {
        Json next_state = state.is_object() ? state : Json::object();
        Json players = next_state.value("players", Json::array());
        if (!players.is_array()) {
            players = Json::array();
        }
        const std::string player(player_id);
        const bool present = std::any_of(
            players.begin(), players.end(), [&player](const Json& entry) {
                return entry.is_string() && entry.get<std::string>() == player;
            });
        if (!present) {
            players.push_back(player);
        }
        next_state["players"] = std::move(players);
        return PlayerEventResult{
            .state = std::move(next_state),
            .broadcasts =
                Json::array({Json{{"type", "player_joined"},
                                  {"playerId", player}}})};
    }

    PlayerEventResult on_player_leave(
        std::string_view player_id,
        const Json& state) const override {
        Json next_state = state.is_object() ? state : Json::object();
        Json players = next_state.value("players", Json::array());
        if (!players.is_array()) {
            players = Json::array();
        }
        const std::string player(player_id);
        players.erase(
            std::remove_if(
                players.begin(),
                players.end(),
                [&player](const Json& entry) {
                    return entry.is_string() && entry.get<std::string>() == player;
                }),
            players.end());
        next_state["players"] = std::move(players);
        return PlayerEventResult{
            .state = std::move(next_state),
            .broadcasts =
                Json::array({Json{{"type", "player_left"},
                                  {"playerId", player}}})};
    }

    GameOverResult is_game_over(const Json& state) const override {
        const bool over = state_count(state) >= 20;
        if (!over) {
            return GameOverResult{
                .over = false,
                .summary = std::nullopt,
                .next_match_policy = std::nullopt};
        }
        return GameOverResult{
            .over = true,
            .summary = Json{{"count", state_count(state)}},
            .next_match_policy =
                Json{{"mode", "close_room"}, {"timeoutMs", 0}}};
    }

    CommandResult on_command(
        const Json& command,
        const Json& state) const override {
        if (command.is_object() && command.value("type", std::string{}) == "reset") {
            std::int64_t count = 0;
            const auto data = command.value("data", Json::object());
            if (data.is_object() && data.contains("count")) {
                static_cast<void>(read_int64(data.at("count"), count));
            }
            return CommandResult{
                .state = Json{{"count", count}},
                .broadcasts =
                    Json::array({Json{{"type", "counter.reset"}}})};
        }
        return CommandResult{
            .state = state,
            .broadcasts = Json::array()};
    }
};

bool parse_port(std::string_view text, int& port) {
    const auto result =
        std::from_chars(text.data(), text.data() + text.size(), port);
    return result.ec == std::errc{} && result.ptr == text.data() + text.size() &&
           port >= 1 && port <= 65535;
}

} // namespace

int main(int argc, char** argv) {
    int port = 8080;
    if (argc > 2 ||
        (argc == 2 && !parse_port(argv[1], port))) {
        std::cerr << "Usage: room_counter [port 1-65535]\n";
        return 2;
    }

    CounterGame game;
    randseed::HostOptions options;
    options.game_id = "room-counter";
    options.port = port;
    randseed::RoomHost host(game, std::move(options));
    std::cout << "Room Counter host listening on 0.0.0.0:" << port << '\n';
    if (!host.listen()) {
        std::cerr << "Unable to start the room host\n";
        return 1;
    }
    return 0;
}
