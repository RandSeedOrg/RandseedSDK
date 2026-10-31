#pragma once

#include "randseed/protocol.hpp"

#include <cstddef>
#include <memory>
#include <optional>
#include <string>

namespace randseed {

inline constexpr std::size_t max_request_body_bytes = 1024U * 1024U;

struct HostOptions {
    std::string game_id;
    std::string bind_address{"0.0.0.0"};
    int port{8080};
};

class RoomHost final {
public:
    explicit RoomHost(const Game& game, HostOptions options);
    ~RoomHost();

    RoomHost(const RoomHost&) = delete;
    RoomHost& operator=(const RoomHost&) = delete;
    RoomHost(RoomHost&&) = delete;
    RoomHost& operator=(RoomHost&&) = delete;

    [[nodiscard]] bool listen();
    [[nodiscard]] bool listen(const std::string& bind_address, int port);
    [[nodiscard]] bool start();
    [[nodiscard]] bool start(const std::string& bind_address, int port);
    [[nodiscard]] std::optional<int> start_on_any_port(
        const std::string& bind_address = "127.0.0.1");
    void stop() noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace randseed
