#include "randseed/room_host.hpp"

#include <httplib.h>

#include <atomic>
#include <chrono>
#include <exception>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <thread>
#include <utility>

namespace randseed {
namespace {

using Clock = std::chrono::steady_clock;
using Request = httplib::Request;
using Response = httplib::Response;

void send_json(Response& response, const Json& value, int status = 200) {
    response.status = status;
    response.set_content(value.dump(), "application/json; charset=utf-8");
}

void send_error(Response& response, int status, std::string_view message) {
    send_json(response, Json{{"error", message}}, status);
}

template <typename Handler>
void handle_game_response(Response& response, Handler&& handler) noexcept {
    try {
        send_json(response, handler());
    } catch (...) {
        send_error(response, 500, "Internal server error");
    }
}

bool valid_port(int port) noexcept {
    return port >= 1 && port <= 65535;
}

} // namespace

struct RoomHost::Impl {
    Impl(const Game& game_ref, HostOptions host_options)
        : game(game_ref),
          options(std::move(host_options)),
          started_at(Clock::now()) {
        if (options.game_id.empty()) {
            throw std::invalid_argument("Host game_id must not be empty");
        }

        server.set_payload_max_length(max_request_body_bytes);
        server.set_error_handler(
            [](const Request&, Response& response) {
                if (!response.body.empty()) {
                    return;
                }
                if (response.status == 413) {
                    send_error(response, 413, "Request body is too large");
                } else if (response.status == 404) {
                    send_error(response, 404, "Not found");
                } else if (response.status >= 400) {
                    send_error(response, response.status, "Request failed");
                }
            });
        server.set_exception_handler(
            [](const Request&, Response& response, std::exception_ptr) {
                send_error(response, 500, "Internal server error");
            });
        register_routes();
    }

    template <typename Input, typename Parser, typename Handler>
    void add_post(const char* path, Parser parser, Handler handler) {
        server.Post(
            path,
            [parser = std::move(parser), handler = std::move(handler)](
                const Request& request,
                Response& response) {
                std::optional<Input> input;
                try {
                    const Json body = Json::parse(request.body);
                    input.emplace(parser(body));
                } catch (const ValidationError& error) {
                    send_error(response, 400, error.what());
                    return;
                } catch (const Json::parse_error&) {
                    send_error(response, 400, "Invalid JSON request body");
                    return;
                } catch (const Json::exception&) {
                    send_error(response, 400, "Invalid request body");
                    return;
                } catch (...) {
                    send_error(response, 500, "Internal server error");
                    return;
                }

                try {
                    send_json(response, handler(*input));
                } catch (...) {
                    send_error(response, 500, "Internal server error");
                }
            });
    }

    void register_routes() {
        server.Get(
            "/manifest",
            [this](const Request&, Response& response) {
                handle_game_response(response, [this]() {
                    const GameManifest game_manifest = game.manifest();
                    validate_game_manifest(game_manifest);
                    return Json(game_manifest);
                });
            });

        server.Get(
            "/health",
            [this](const Request&, Response& response) {
                handle_game_response(response, [this]() {
                    const auto elapsed =
                        std::chrono::duration<double>(Clock::now() - started_at)
                            .count();
                    return Json{
                        {"status", "ok"},
                        {"gameId", options.game_id},
                        {"uptime", elapsed}};
                });
            });

        add_post<MatchConfig>(
            "/init",
            parse_match_config,
            [this](const MatchConfig& config) {
                return game.init(config);
            });
        add_post<TickRequest>(
            "/tick",
            parse_tick_request,
            [this](const TickRequest& request) {
                return game.tick(request.ctx, request.state);
            });
        add_post<PlayerEventRequest>(
            "/onPlayerJoin",
            parse_player_event_request,
            [this](const PlayerEventRequest& request) {
                return game.on_player_join(request.player_id, request.state);
            });
        add_post<PlayerEventRequest>(
            "/onPlayerLeave",
            parse_player_event_request,
            [this](const PlayerEventRequest& request) {
                return game.on_player_leave(request.player_id, request.state);
            });
        add_post<IsGameOverRequest>(
            "/isGameOver",
            parse_is_game_over_request,
            [this](const IsGameOverRequest& request) {
                return game.is_game_over(request.state);
            });
        add_post<CommandRequest>(
            "/onCommand",
            parse_command_request,
            [this](const CommandRequest& request) {
                return game.on_command(request.command, request.state);
            });
    }

    bool bind(const std::string& bind_address, int port) {
        if (!valid_port(port)) {
            throw std::invalid_argument("Port must be between 1 and 65535");
        }
        std::lock_guard lock(lifecycle_mutex);
        if (started_once || !server.bind_to_port(bind_address, port)) {
            return false;
        }
        started_once = true;
        return true;
    }

    bool start_on_port(const std::string& bind_address, int port) {
        std::lock_guard lock(lifecycle_mutex);
        if (started_once || !server.bind_to_port(bind_address, port)) {
            return false;
        }
        started_once = true;
        return launch_listener();
    }

    std::optional<int> start_anywhere(const std::string& bind_address) {
        std::lock_guard lock(lifecycle_mutex);
        if (started_once) {
            return std::nullopt;
        }
        const int port = server.bind_to_any_port(bind_address);
        if (port <= 0) {
            return std::nullopt;
        }
        started_once = true;
        if (!launch_listener()) {
            return std::nullopt;
        }
        return port;
    }

    bool launch_listener() {
        try {
            listener_result.store(false, std::memory_order_relaxed);
            listener = std::thread([this]() {
                try {
                    listener_result.store(
                        server.listen_after_bind(), std::memory_order_release);
                } catch (...) {
                    listener_result.store(false, std::memory_order_release);
                }
            });
        } catch (...) {
            server.stop();
            return false;
        }
        return true;
    }

    void stop() noexcept {
        std::thread thread_to_join;
        {
            std::lock_guard lock(lifecycle_mutex);
            server.stop();
            if (listener.joinable() &&
                listener.get_id() != std::this_thread::get_id()) {
                thread_to_join.swap(listener);
            }
        }
        if (thread_to_join.joinable()) {
            thread_to_join.join();
        }
    }

    const Game& game;
    HostOptions options;
    httplib::Server server;
    Clock::time_point started_at;
    std::mutex lifecycle_mutex;
    std::thread listener;
    std::atomic<bool> listener_result{false};
    bool started_once{false};
};

RoomHost::RoomHost(const Game& game, HostOptions options)
    : impl_(std::make_unique<Impl>(game, std::move(options))) {}

RoomHost::~RoomHost() {
    stop();
}

bool RoomHost::listen() {
    return listen(impl_->options.bind_address, impl_->options.port);
}

bool RoomHost::listen(const std::string& bind_address, int port) {
    if (!impl_->bind(bind_address, port)) {
        return false;
    }
    try {
        return impl_->server.listen_after_bind();
    } catch (...) {
        return false;
    }
}

bool RoomHost::start() {
    return start(impl_->options.bind_address, impl_->options.port);
}

bool RoomHost::start(const std::string& bind_address, int port) {
    if (!valid_port(port)) {
        throw std::invalid_argument("Port must be between 1 and 65535");
    }
    return impl_->start_on_port(bind_address, port);
}

std::optional<int> RoomHost::start_on_any_port(
    const std::string& bind_address) {
    return impl_->start_anywhere(bind_address);
}

void RoomHost::stop() noexcept {
    if (impl_) {
        impl_->stop();
    }
}

} // namespace randseed
