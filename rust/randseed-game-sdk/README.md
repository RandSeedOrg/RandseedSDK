# RandSeed Game Server SDK for Rust

The `randseed-game-sdk` crate helps Rust games implement the RandSeed room-host
HTTP/JSON protocol. It provides typed lifecycle contracts, an Axum router and
server runner, deterministic Mulberry32 random numbers, replay data helpers,
and a client for host-provided platform services.

The room host owns scheduling, persistence, player identity, and recovery.
Implement authoritative game logic as calculations from the supplied state
and tick context; avoid process-global match state, wall-clock time, operating
system randomness, and unrecorded network responses.

In `GameManifest`, configure integer limits satisfying
`1 <= minPlayers <= maxPlayers <= 100`. `minPlayers` is the minimum number of
assigned players required before the RandSeed platform can start a match.
Zero-player starts are unsupported; a solo game uses `minPlayers: 1`.
`maxPlayers` must be at least `minPlayers` and is the room's seat-capacity
limit, not a lobby fill target. For
example, `minPlayers: 2, maxPlayers: 4`
may start with two assigned players and leave two seats open. Set both values
to the same number when the game requires an exact roster size. After startup,
additional seats are admitted only while the game reports `joinable: true` and
the roster is below `maxPlayers`.

## Requirements

- Rust 2021 edition
- A RandSeed room host implementing [Room Host Protocol v1](../../protocol/room-host-v1.md)

## Use from a repository checkout

The SDK is maintained in the public
[RandseedSDK repository](https://github.com/RandSeedOrg/RandseedSDK). To use it
before a registry release, add a local path dependency:

```toml
[dependencies]
randseed-game-sdk = { path = "../RandseedSDK/rust/randseed-game-sdk" }
```

Adjust the path to match your checkout. The crate is not published to crates.io
as part of this source preparation.

## Authoritative Agent player views

Implement `GameServer::agent_player_contract` and
`GameServer::project_agent_player_view` to expose a versioned per-seat
projection. The SDK provides host-to-container `GET /agent/player-contract` and
`POST /agent/player-view`, shared action/receipt/lease types, and an explicit
Skill `wait` decision. The projection endpoint is for the trusted Room host;
it does not authorize a browser or prove that game-authored field visibility is
correct. This crate provides the projection contract only; room authorization,
per-seat delivery, action receipts, Skill dispatch, and release admission are
platform responsibilities and are not implemented by the crate.

## Run the cooperative counter example

The crate includes a complete `GameServer` implementation in
[`examples/cooperative_counter.rs`](examples/cooperative_counter.rs). Run it
from this directory:

```sh
cargo run --example cooperative_counter
```

It listens on `0.0.0.0:8080` by default. Set `PORT` to override the port. The
example demonstrates deterministic tick processing, player lifecycle events,
trusted reset commands, and a match-ending policy.

For private per-player projections, strict options validation, action
receipts, and a terminal policy, see
[`examples/authoritative_room.rs`](examples/authoritative_room.rs). The
paired cross-language migration guide is
[`protocol/ai-multiplayer-migration.md`](../../protocol/ai-multiplayer-migration.md).

## Result Units

Declare a versioned `GameResultDefinition` in `GameManifest`, then return
`GameResultEvent` values from any lifecycle transition. A match may emit
multiple rounds, laps, stages, or a match result without requiring one winner
or a generic score. The language-independent contract and event example are
in the [Result Unit protocol](../../protocol/room-host-v1.md#result-units-and-metrics)
and [shared fixture](../../protocol/fixtures/game-result-event-v1.json).
Use `validate_game_result_units` to check a transition; use
`GameResultEvent::validate_against` to check event metrics against a definition.

The server validates event structure, but it does not persist or deliver the
outbox. The trusted room host must validate the published definition, match,
roster, source identity, and archive receipt. Keep event IDs stable across
retries; corrections are append-only events.

## Protocol and determinism

- Each request body is limited to 1 MiB. Match sequence numbers, ticks, and
  optional tick timestamps follow the protocol's safe-integer limits.
- Session Stream hydration requires the active roster. Per-player views must
  exactly cover that roster; private-view manifests fail closed on missing
  views. See the [Session Stream v1 protocol](../../protocol/session-stream-v1.md)
  and its shared fixture.
- Protocol failures expose stable error codes, retryability, and creator
  guidance from the [shared error fixture](../../protocol/fixtures/sdk-error-codes-v1.json).
- Lifecycle methods must calculate outcomes from the state and context they
  receive so a room host can replay them after recovery.
- `Mulberry32` follows JavaScript `ToInt32` seed conversion. Its cross-language
  vectors are in
  [`tests/fixtures/room-host-v1.json`](tests/fixtures/room-host-v1.json).
  It is deterministic, not cryptographic, and is not GameVRF.
- `PlatformClient` calls host services; do not make those calls a source of
  unrecorded authoritative simulation inputs.

`PlatformClient::get_players()` includes optional `display_name` and
`avatar_url` values for online room members when a public profile is available.
It may also include `is_game_creator`, indicating whether a player owns the
game for the current room. Missing profile fields and unavailable ownership
information remain absent. These are platform-supplied metadata fields, not
authorization evidence; independently verify ownership and permissions before
protected operations.

Run the crate checks from this directory:

```sh
cargo fmt --check
cargo test
cargo package --list
cargo package
```
