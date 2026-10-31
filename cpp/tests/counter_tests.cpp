#include "randseed/protocol.hpp"

#include <iostream>
#include <memory>
#include <stdexcept>

std::unique_ptr<randseed::Game> make_counter_game();

int main() {
    try {
        const auto game = make_counter_game();
        const randseed::Json state = {
            {"count", 7}, {"durationSeconds", 3.5}, {"players", {"player-a"}}};
        const randseed::Json command = {{"type", "reset"}, {"data", {{"count", 20}}}};
        const auto reset = game->on_command(command, state);
        if (reset.state.at("count") != 20 ||
            reset.state.at("durationSeconds") != 3.5 ||
            reset.state.at("players") != state.at("players")) {
            throw std::runtime_error("Reset did not preserve the match state");
        }
        const auto result = game->is_game_over(reset.state);
        if (!result.over || !result.summary ||
            result.summary->at("durationSeconds") != 3.5) {
            throw std::runtime_error("Reset lost the match duration");
        }
        std::cout << "Counter regression tests passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "Test failure: " << error.what() << '\n';
        return 1;
    }
}
