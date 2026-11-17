# RandSeed SDKs

This repository contains the public game-server SDKs, their shared room-host
protocol, examples, and cross-language compatibility fixtures. It does not
contain RandSeed's private platform services, Durable Objects, authentication,
or deployment implementation.

## Supported server languages & Fixed Runtimes

| Language | SDK location | `backend.runtime` | Install and verify |
| --- | --- | --- | --- |
| Node.js / TypeScript | [`node/`](./node/) | `nodejs-22` / `nodejs-24` | Consumer install: `npm install github:RandSeedOrg/RandseedSDK#main`; repository checks: `npm ci && npm test` |
| Go | [`go/`](./go/) | `go` (shared `native` image) | `go get github.com/randseedorg/randseedsdk/go/roomhost@v0.1.0`; `go test ./...` |
| Rust | [`rust/`](./rust/) | `rust` (shared `native` image) | Use the path dependency and checks in the [crate README](./rust/randseed-game-sdk/README.md). |
| C# | [`dotnet/`](./dotnet/) | `dotnet-8` | `dotnet run --project dotnet/verification` |
| C++ | [`cpp/`](./cpp/) | `cpp` (shared `native` image) | See the CMake build instructions in [`cpp/README.md`](./cpp/README.md). |

Full-stack games declare `backend.runtime` (`rust`, `go`, `cpp`, `nodejs-22`, `nodejs-24`, or `dotnet-8`) and `backend.entrypoint` in `randseed.fullstack.json` and package their compiled Linux `x86_64` binary or single-file ESM bundle into `code.tar.gz` (`r2://runtime-packages/<gameId>/sha256-<sha256>.tar.gz`). The platform Play Worker mounts fixed base runtime images and executes the uploaded package inside an isolated non-root container without per-game Docker image builds or Worker redeployments.

The Node.js server SDK source and other backend SDKs are maintained in this
repository. The Node package is installed from this GitHub repository, not
from the npm registry. The browser-only SDK remains published on npm as
[`@randseed/game-sdk`](https://www.npmjs.com/package/@randseed/game-sdk).

The room startup and first-observation rollout requires the browser
`RoomClient` from `@randseed/game-sdk` 0.4.0 or a compatible later release.
Once that version is published, update the browser dependency and lockfile,
then rebuild and redeploy each room game. Existing deployed bundles do not
update automatically. This rollout does not change the Room Host Protocol v1
or require a server SDK upgrade; game-server callbacks must not implement the
browser's observation acknowledgement. Check the
[room SDK guide](https://github.com/RandSeedOrg/Gamecreator/blob/dev/docs/game-room-sdk.md)
and the target environment's rollout status before deploying.

The Creator Portal's server SDK summary is sourced from
[`metadata/gamecreator-server-sdk-guide.json`](./metadata/gamecreator-server-sdk-guide.json).
On each push to `main`, this repository uses short-lived GitHub Actions OIDC to
notify the matching dev, test, and production Creator Workers. Each Worker
verifies its environment-specific audience and source workflow, then uses its
own GitHub App to notify Gamecreator. Gamecreator fetches the guide data and
opens a pull request only when the displayed data changes. No PAT is stored in
this repository.

As of 2026-11-17, the general private-view alignment in this source tree is
pending independent review and is not a new SDK release. Gamecreator PR #340
remains open and is not deployed; production availability is not implied by
this repository branch. The current install commands and released Go/Rust
artifacts do not provide this pending implementation until the corresponding
changes are merged or released. Verify the SDK artifact and target host rollout
before enabling private routing.

## Server SDK capabilities

- **Authoritative multiplayer lifecycle** across Node.js/TypeScript, Go, Rust,
  C#, and C++.
- **Deterministic VRF root seed and `ChaCha20Rng`** across all five backend
  SDKs (`options.randomSeed` + SHA-256 domain-separated `deriveSeed` + validated
  `{ seedHex, stream, wordPos }` JSON cursor snapshots), with `options.seed` and
  `Mulberry32` retained for legacy compatibility.
- **Stateful low-latency sessions** with checkpoint hydration and deterministic
  replay in the Node.js and Rust SDKs.
- **Room-host private per-seat views** in Node.js, Rust, and Go, with exact
  roster coverage, fail-closed validation, and no canonical-state fallback. Go
  support is HTTP lifecycle only; it does not add `/stream` or Agent APIs.
- **Agent contracts and action receipts** remain Node.js and Rust capabilities.
- **Versioned Result Units and metrics** for round, stage, and match outcomes.
- **Structured failures** in Node.js and Rust with stable error codes,
  retryability, and sanitized callback errors.
- **Optional Agent contracts** for capability discovery and recipient-specific
  player views in Node.js and Rust.
- **Host-mediated room services** in Node.js and Rust through `PlatformClient`:
  SQL, key-value storage, broadcasts, direct messages, and online player data.

### Where to find the concrete interfaces

The route list below describes how the room host calls a game server. The
following table maps capabilities to the interfaces game code implements or
calls:

| Capability | Interface used by game code |
| --- | --- |
| Room lifecycle (all five languages) | Implement `GameServer` in [Node.js](./node/README.md), [`roomhost.GameServer`](./go/README.md), [`GameServer`](./rust/randseed-game-sdk/README.md), [`IGameServer`](./dotnet/README.md), or [`randseed::Game`](./cpp/README.md). These provide the manifest, init, tick, join/leave, game-over, and trusted-command callbacks. |
| Deterministic randomness (all five) | Read the host-injected `options.randomSeed`, then use `ChaCha20Rng` and `deriveSeed` (language naming varies). This is a library API, **not an HTTP VRF endpoint**: the platform obtains and injects the VRF seed; game code does not call the VRF canister through this SDK. |
| Session Stream (Node.js, Rust) | WebSocket `/stream` with `hydrate` and `step`; game tick results may include `actionResults`. See [Session Stream v1](./protocol/session-stream-v1.md). |
| Result Units (all five) | Declare `resultDefinition` in the manifest and return `resultUnits` from lifecycle callbacks. They emit validated result events; they are not a results query API. |
| Agent integration (Node.js, Rust) | Optional `GameServer` callbacks and `/agent/capabilities`, `/agent/player-contract`, and `/agent/player-view`. These are host-facing contracts, not direct player APIs. |
| Room services (Node.js, Rust) | `PlatformClient`: `sql`, `kvGet`/`kvPut`/`kvDelete`, `broadcast`, `send`, and `getPlayers`. These call host-provided room services; their storage scope is described below. |

## Room-host protocol

All server SDKs implement the shared HTTP/JSON room lifecycle:

| Interface | Capability |
| --- | --- |
| `GET /manifest` | Game metadata, options schema, player-view mode, and Result Unit definitions. |
| `GET /health` | Server health and runtime identity. |
| `POST /init` | Initialize a match from host-authenticated players and options. |
| `POST /tick` | Apply one authoritative tick using host-supplied inputs, commands, and elapsed match time. |
| `POST /onPlayerJoin` / `POST /onPlayerLeave` | Update game state for joins, reconnects, and departures. |
| `POST /isGameOver` | Evaluate terminal state, summary, and next-match policy. |
| `POST /onCommand` | Apply a trusted room-host command. |
| `WebSocket /stream` (Node.js, Rust) | Stateful `hydrate` and `step` lifecycle with recovery, replay, private views, and action results. |
| `GET /agent/capabilities` (optional, Node.js/Rust) | Declare supported Agent protocols and modes. |
| `GET /agent/player-contract` (optional, Node.js/Rust) | Declare per-seat Agent view and action schemas. |
| `POST /agent/player-view` (optional, Node.js/Rust) | Produce a recipient-specific view for the trusted host. |

### Tick and command callbacks

The `/tick` callback receives the current state and a host-generated context:
`tick` is the non-negative tick number, `inputs` contains each player's JSON
`data` (and optional epoch-millisecond `timestamp`), `commands` contains
trusted host commands, and `elapsedSeconds` is cumulative match time derived
from host ticks. Use this supplied context rather than wall-clock time or
process-global match state. When returning per-player views, the request
includes the active `players` roster so the SDK can validate exact coverage.

Trusted commands use the separate `/onCommand` callback; they are not player
inputs. Tick and command callbacks return the updated state and may also return
broadcasts, private player views, joinability changes, and completed Result
Units. For the exact request and response shapes, see the
[Room Host Protocol](./protocol/room-host-v1.md#tick).

Configure integer limits satisfying `1 <= minPlayers <= maxPlayers <= 100`.
`minPlayers` is the assigned-player threshold at which the RandSeed platform
may start a match; `maxPlayers` is the room's seat capacity, not a required
fill target. If the roster must be a fixed size, set both values to that size.
A zero-player start is unsupported; a solo game uses `minPlayers: 1`.
Additional players can join after startup only while the game reports
`joinable: true` and seats remain below `maxPlayers`. Node.js, Rust, Go, and .NET
SDKs enforce manifest roster bounds at `/init`; C++ currently validates only
generic roster constraints. Gamecreator also enforces manifest bounds
platform-side; the platform owns matchmaking and startup timing. This start
threshold is separate from `wait_for_ready`, which coordinates the
existing roster between completed and next matches.

For startup, disconnect, reconnect, game-state retention, private views, and
next-match behavior, see
[Room lifecycle and recovery](./protocol/room-host-v1.md#room-lifecycle-and-recovery).
The host provides recovery mechanisms; game rules decide whether an offline
player waits, times out, or is removed. No general disconnect-wait policy is
currently exposed by the server SDK.

### Data continuity across releases

Room state and match carryover are not a cross-release data store. A new
runtime version does not automatically migrate an existing room's state,
history, rankings, or inventory. Store long-lived player/game data in an
explicit durable game-level service and define its identity, retention, schema
migrations, and old-data backfill. Result Units are an event contract, not a
statistics database or query API; verify archive commitment and query support
before relying on them for rankings. See
[Cross-release data continuity](./protocol/room-host-v1.md#cross-release-data-continuity).
The Node.js and Rust `PlatformClient` APIs expose host-mediated SQL and
key-value operations, but they are not automatically game-wide storage. In
the current Gamecreator host, `platform.db/sql` accesses the active GameRoom
Durable Object's database, while `platform.kv` is an ephemeral in-memory
room-local map. Neither is a cross-room leaderboard store. Result Unit archive
queries are platform-internal; the Server SDK does not expose a general
game-facing query API. Use only a game-level service explicitly documented by
the target host for cross-release data.

Read the [version 1 protocol](./protocol/room-host-v1.md). Every language
implementation in this repository is tested against the shared
[room-host fixture](./protocol/fixtures/room-host-v1.json), including
deterministic `ChaCha20Rng`, `deriveSeed`, and legacy `Mulberry32` vectors.
Node.js, Rust, and Go also validate the same
[private-view vectors](./protocol/fixtures/room-host-private-v1.json). Result Unit
declarations and events are also validated
against the shared [Result Unit fixture](./protocol/fixtures/game-result-event-v1.json).
Stateful WebSocket recovery and action receipt behavior follow
[`Session Stream v1`](./protocol/session-stream-v1.md) and its shared
[stream fixture](./protocol/fixtures/session-stream-v1.json). Stable SDK error
codes and retryability are defined in the shared
[error fixture](./protocol/fixtures/sdk-error-codes-v1.json).
All server languages expose optional `resultDefinition` and `resultUnits`
fields; emitting an event does not itself persist or archive it.

Creators migrating an existing browser game can follow the
[AI-assisted multiplayer migration guide](./protocol/ai-multiplayer-migration.md)
and run the Node/Rust migration preflight documented there.

The room host owns scheduling, authoritative persistence, player identity,
and recovery. Game code must calculate from the supplied state and context;
it must not run a separate authoritative timer or rely on process-global match
state. For verifiable randomness, the trusted room host injects one 256-bit
`random_oracle` VRF root seed per match via `options.randomSeed` (kept
confidential during live play and publicized after settlement); game containers
derive simulation streams locally with `ChaCha20Rng` / `deriveSeed` and do not
call VRF canisters directly.
