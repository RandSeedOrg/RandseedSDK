# RandSeed C++ Room Host SDK

This C++20 SDK implements the game-container side of the
[Room Host v1 protocol](../protocol/room-host-v1.md). It provides a game
callback interface, arbitrary JSON state and payloads, a bounded HTTP host for
all v1 lifecycle routes, and the protocol's deterministic Mulberry32 generator.

## Requirements and dependencies

- CMake 3.20 or newer
- A C++20 compiler
- Network access on the first CMake configure

CMake FetchContent downloads the pinned dependencies:

- [nlohmann/json](https://github.com/nlohmann/json), `v3.11.3`
- [cpp-httplib](https://github.com/yhirose/cpp-httplib), `v0.18.6`

Build the SDK, counter example, and tests from this directory:

```sh
cmake -S . -B build
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

The contract test reads the shared fixture at
`../protocol/fixtures/room-host-v1.json`.

## Run the counter host

```sh
./build/room_counter
```

The example listens on `0.0.0.0:8080` by default. An optional first argument
selects another port. The host exposes `GET /manifest`, `GET /health`, and
`POST /init`, `/tick`, `/onPlayerJoin`, `/onPlayerLeave`, `/isGameOver`, and
`/onCommand`.

## Implement a game

Derive from `randseed::Game` and implement its manifest and lifecycle
callbacks. `randseed::Json` is `nlohmann::json`, so state, carryover, options,
input data, commands, summaries, and broadcasts can contain any JSON value.
The host validates protocol fields before callbacks run. Callback failures
return a generic sanitized HTTP 500 response; request validation failures
return HTTP 400. The request-body limit is fixed at 1 MiB; larger requests
receive HTTP 413. `matchSeq` must be a positive safe integer, and `tick` and
input `timestamp` values must be non-negative safe integers.

```cpp
#include <randseed/room_host.hpp>

class MyGame final : public randseed::Game {
public:
    randseed::GameManifest manifest() const override;
    randseed::InitResult init(const randseed::MatchConfig& config) const override;
    randseed::TickResult tick(
        const randseed::TickContext& context,
        const randseed::Json& state) const override;
    randseed::PlayerEventResult on_player_join(
        std::string_view player_id,
        const randseed::Json& state) const override;
    randseed::PlayerEventResult on_player_leave(
        std::string_view player_id,
        const randseed::Json& state) const override;
    randseed::GameOverResult is_game_over(
        const randseed::Json& state) const override;
    randseed::CommandResult on_command(
        const randseed::Json& command,
        const randseed::Json& state) const override;
};
```

Keep authoritative logic deterministic: derive each result from the supplied
state and request context, and do not rely on wall-clock time, process-global
match state, operating-system randomness, or unrecorded network calls.
Callbacks may be invoked concurrently, so game implementations should be
thread-safe and should not mutate shared authoritative state. The `Game`
instance is non-owning and must outlive its `RoomHost`.

`randseed::Mulberry32` accepts a numeric seed, applies JavaScript-compatible
`ToInt32` conversion, and exposes `next_u32()` and `next_float()`. It is a
deterministic simulation PRNG, not a cryptographic generator or GameVRF.
