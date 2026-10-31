# RandSeed Game Server SDK for Rust

The `randseed-game-sdk` crate helps Rust games implement the RandSeed room-host
HTTP/JSON protocol. It provides typed lifecycle contracts, an Axum router and
server runner, deterministic Mulberry32 random numbers, replay data helpers,
and a client for host-provided platform services.

The room host owns scheduling, persistence, player identity, and recovery.
Implement authoritative game logic as calculations from the supplied state
and tick context; avoid process-global match state, wall-clock time, operating
system randomness, and unrecorded network responses.

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

## Protocol and determinism

- Each request body is limited to 1 MiB. Match sequence numbers, ticks, and
  optional tick timestamps follow the protocol's safe-integer limits.
- Lifecycle methods must calculate outcomes from the state and context they
  receive so a room host can replay them after recovery.
- `Mulberry32` follows JavaScript `ToInt32` seed conversion. Its cross-language
  vectors are in
  [`tests/fixtures/room-host-v1.json`](tests/fixtures/room-host-v1.json).
  It is deterministic, not cryptographic, and is not GameVRF.
- `PlatformClient` calls host services; do not make those calls a source of
  unrecorded authoritative simulation inputs.

Run the crate checks from this directory:

```sh
cargo fmt --check
cargo test
cargo package --list
cargo package
```
