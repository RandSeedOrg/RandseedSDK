# RandSeed Game Server SDK for Rust

The `randseed-game-sdk` crate helps Rust games implement the RandSeed room-host
HTTP/JSON protocol. It provides typed lifecycle contracts, an Axum router and
server runner, deterministic `ChaCha20Rng` and `derive_seed` (plus legacy
`Mulberry32`), replay data helpers, and a client for host-provided platform
services.

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
as part of this source preparation. The private-view changes below are pending
review; a checkout of the current default branch will not contain them until
they are merged.

## Private room-host player views

Omit both `playerViewMode` and `playerViewSchemaVersion` for an ordinary
public game. Set `player_view_mode: Some(PlayerViewMode::Private)` for the
general room-host v1 private projection contract. The schema version defaults
to v1 when omitted; explicit `1` is accepted only with private mode. `null`, an
unsupported version, a version without private mode, a null mode, and
`playerViewMode: "public"` are rejected. The Rust SDK serializes private
manifests with explicit `playerViewSchemaVersion: 1`; it does this without
adding a field to `GameManifest`, preserving existing downstream struct
literals. A public manifest serializes with both private fields omitted.

Public wire manifest:

```json
{
  "title": "Room Counter",
  "version": "1.0.0",
  "minPlayers": 2,
  "maxPlayers": 4,
  "tickRateHz": 5,
  "persistentTable": false
}
```

A valid `options_schema` is a JSON Schema object or boolean; the SDK checks
its wire shape but does not evaluate it against match options. For example,
add this to `GameManifest`:

```rust
options_schema: Some(serde_json::json!({
    "type": "object",
    "properties": { "mode": { "type": "string", "enum": ["casual", "ranked"] } },
    "additionalProperties": false
})),
```

Private wire manifest emitted by this SDK:

```json
{
  "title": "Hidden Hand",
  "version": "1.0.0",
  "minPlayers": 2,
  "maxPlayers": 4,
  "tickRateHz": 5,
  "persistentTable": false,
  "playerViewMode": "private",
  "playerViewSchemaVersion": 1
}
```

`/init` uses `MatchConfig.players`. Private `/tick` and `/onCommand` requests
must include `players`; join and leave requests include the resulting roster,
with the joining seat included and the leaving seat excluded. Request-context
methods (`tick_ext`, `on_player_join_ext`, `on_player_leave_ext`, and
`on_command_ext`) retain this validated roster. Return an exact
`player_views` map on each state transition. The server validates exact roster
coverage, JSON values, a 64 KiB limit per serialized view and a 1 MiB limit for
the entire serialized map. Missing, malformed, missing-key, extra-key, or
oversized projections fail closed.

The returned `state` remains canonical and is visible only to the trusted Room
host. Build each view for its recipient and exclude every opponent-private
field, hidden random state, and other non-recipient secret. The host must route
only the entry matching the authenticated recipient; never fall back to
canonical state or another seat view. The SDK cannot infer whether a game
callback classified every field correctly. Do not expose lifecycle endpoints
to browsers.

For deployment, `run_game_server` and `run_typed_game_server` read the actual
platform-assigned RandSeed Game ID from `RANDSEED_GAME_ID`. Direct router
callers pass that platform value to `create_game_router`; there is no invented
fallback ID. IDs must match `^[A-Za-z0-9_-]{1,64}$`. The examples listen on
`PORT` or `8080`, and bind `0.0.0.0`. Check both routes on the actual port:

```sh
curl --fail http://127.0.0.1:8080/health
curl --fail http://127.0.0.1:8080/manifest
```

`/health` checks the process and valid platform Game ID; `/manifest`
independently validates game configuration. A healthy process does not prove
that the manifest is valid or that platform routing is enabled.

As of 2026-11-17, this cross-SDK alignment is pending independent review and
is not a published crate release. Gamecreator PR #340 remains open and is not
deployed. Verify the SDK version and target host rollout before enabling
private routing.

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

## Explicit game-owned data (Game Data protocol v1)

`PlatformClient::data()` exposes the typed game-owned document API. The current
Gamecreator host gate `GAME_DATA_STORE_ENABLED` remains off by default, so this
SDK source API does not promise that any deployment has enabled the service.
`DataPlugin` is separate and remains disabled. The host derives game ID and the
full environment from verified room admission; a game cannot choose scope.
Records are shared across rooms and runtime releases of the same game and
environment, while games and public/preview/sandbox plus dev/test/production
environments remain isolated.

The feature is forward-only: game code must explicitly write new records. It
does not import Room SQLite/snapshots/KV, old Result Units, Blackjack archives,
legacy client boards, profiles, principals, or R2 replay data. A store call and
a Room transition are separate Durable Object operations, not one transaction;
a callback is not a durable outbox. Use deterministic idempotency keys and
retry the same payload after transient failures. Exact committed-transition
mirroring is not available in this stage.

```rust
use randseed_game_sdk::{
    GameDataFieldType, GameDataFilter, GameDataFilterOperator, GameDataQuery,
    GameDataSchema, GameDataSort, GameDataSortDirection, PlatformClient, SdkError,
};
use serde_json::json;
use std::collections::BTreeMap;

async fn save_and_read(platform: &PlatformClient) -> Result<(), SdkError> {
    let data = platform.data();
    let schema = GameDataSchema {
        version: 1,
        fields: BTreeMap::from([
            ("season".into(), GameDataFieldType::Number),
            ("score".into(), GameDataFieldType::Number),
            ("label".into(), GameDataFieldType::String),
        ]),
        indexes: vec!["score".into()],
        compound_indexes: vec![["season".into(), "score".into()]],
    };
    data.register_schema("seasonScores", &schema).await?;
    let prior = data.get("seasonScores", "player-summary").await?;
    data.put(
        "match-42/player-summary",
        "seasonScores",
        "player-summary",
        prior.map(|record| record.revision).unwrap_or(0),
        json!({"season": 2026, "score": 1200, "label": "final"}),
    ).await?;
    let page = data.query(&GameDataQuery {
        collection: "seasonScores".into(),
        filters: vec![GameDataFilter {
            field: "season".into(),
            op: GameDataFilterOperator::Eq,
            value: json!(2026),
        }],
        order_by: Some(GameDataSort {
            field: "score".into(),
            direction: GameDataSortDirection::Desc,
        }),
        limit: 50,
        cursor: None,
    }).await?;
    let _next_cursor = page.next_cursor;
    Ok(())
}
```

This compound query requires the ordered pair `["season", "score"]`: one
`season == X` equality filter followed by ascending or descending `score`, with
document ID as stable tie-breaker. Same-field indexed queries and unfiltered
ID listing are supported; arbitrary cross-field filters, multiple predicates,
joins, offset paging, and arbitrary SQL are not. Aggregates support `count`,
`sum`, `min`, `max`, and `avg` with at most one single-index filter and an
explicit scan limit no greater than 1,000. Unsupported plans and limit
exceedance are errors, never unbounded scans. Numeric sums and averages use
exact binary64 accumulation and final nearest-even rounding; unsafe integer
results return an error. JSON integers outside JavaScript’s safe-integer range
are rejected. Rust preserves negative-zero sign in documents; numeric indexes
treat negative and positive zero as equal.

Writes use CAS revisions and deletion tombstones. Recreate with the tombstone
revision. An atomic `mutate` batch is limited to 25 unique records; retrying the
same idempotency key and payload returns the prior receipt, while changing the
payload conflicts. Schema upgrades are additive; older releases can re-register
unchanged declarations and omit later sparse fields. Do not newly index an
existing unindexed string field, since older long values could violate the
index bound. The [Game Data v1 protocol](../../protocol/game-data-v1.md)
documents exact errors and quotas.

Game-owned caps include 64 KiB per document, 512 KiB per write request, 100
records per query page, 32 collections and 8,192 live documents per scope,
16,384 stored records including tombstones per scope, and 128 MiB live serialized
data per scope. Each collection supports at most 32 fields, 16 single-field and
16 compound indexes. Across environments, an Archive supports 128 game-declared
indexes and 16,384 normalized index entries. Each scope retains at most 4,096
receipts for 30 days and 4,096 cursors for 15 minutes; room rate is 120 requests
per minute and tracked rooms are capped at 10,000 per scope.

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

## Developer-initiated callback benchmark

`run_serial_benchmark` measures an async callback serially in the current Rust
process. It accepts 1 through 100 samples, uses `std::time::Instant`, and
returns only the sample count and nearest-rank p50/p95 durations in
milliseconds. Successful callback values are discarded; callback errors are
returned unchanged inside `SerialBenchmarkError::Callback`. The helper does not
log or send telemetry, contact a room or network, or measure room RTT/action
latency. Use only deterministic callbacks with isolated fixtures and no
external side effects; never repeat real room mutations. See the
[room sync performance guide](../../protocol/room-sync-performance.md) for the
Node/Rust comparison and measurement-layer distinctions.

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
- `ChaCha20Rng` and `derive_seed` / `derive_seed_strs` / `derive_seed_hex`
  provide cross-language 20-round ChaCha20 randomness matching
  `rand_chacha::ChaCha20Rng` from the host-injected 256-bit VRF root seed
  (`options.random_seed`), with validated JSON-safe `{ seedHex, stream, wordPos }`
  cursor snapshots (`ChaCha20State`, `MAX_SAFE_CURSOR`). `Mulberry32` (using
  JavaScript `ToInt32` seed conversion) remains available for legacy
  `options.seed` compatibility. Cross-language golden vectors are in
  [`tests/fixtures/room-host-v1.json`](tests/fixtures/room-host-v1.json).
- `PlatformClient` calls host services; do not make those calls a source of
  unrecorded authoritative simulation inputs.

`PlatformClient::get_players()` retains the original `PlayerInfo` contract with
only `player_id` and `joined_at`. For game-scoped display hints, call
`get_players_with_profiles()`, which returns `PlayerProfileInfo` with optional
`display_name`, `avatar_url`, and `is_game_creator` fields. Missing and `null`
optional fields both decode as `None`; unknown fields are ignored. Principal,
account, and other private metadata are not exposed by this typed API.

`is_game_creator` is a display hint only, not an authorization signal. Keep
authorization tied to trusted host-side identity and policy.

```rust
use randseed_game_sdk::{PlatformClient, SdkError};

async fn show_room_players(platform: &PlatformClient) -> Result<(), SdkError> {
    for player in platform.get_players_with_profiles().await? {
        let label = player.display_name.as_deref().unwrap_or(&player.player_id);
        let is_creator_hint = player.is_game_creator.unwrap_or(false);
        println!("{label} (creator hint: {is_creator_hint})");
    }
    Ok(())
}
```

Run the crate checks from this directory:

```sh
cargo fmt --check
cargo test
cargo package --list
cargo package
```
