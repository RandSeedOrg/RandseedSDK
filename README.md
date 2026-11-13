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

The Creator Portal's server SDK summary is sourced from
[`metadata/gamecreator-server-sdk-guide.json`](./metadata/gamecreator-server-sdk-guide.json).
On each push to `main`, this repository uses short-lived GitHub Actions OIDC to
notify the matching dev, test, and production Creator Workers. Each Worker
verifies its environment-specific audience and source workflow, then uses its
own GitHub App to notify Gamecreator. Gamecreator fetches the guide data and
opens a pull request only when the displayed data changes. No PAT is stored in
this repository.

## Server SDK capabilities

- **Authoritative multiplayer lifecycle** across Node.js/TypeScript, Go, Rust,
  C#, and C++.
- **Stateful low-latency sessions** with checkpoint hydration and deterministic
  replay in the Node.js and Rust SDKs.
- **Per-seat private views and action receipts** in Node.js and Rust, with
  fail-closed roster coverage and no fallback to canonical state.
- **Versioned Result Units and metrics** for round, stage, and match outcomes.
- **Structured failures** in Node.js and Rust with stable error codes,
  retryability, and sanitized callback errors.
- **Optional Agent contracts** for capability discovery and recipient-specific
  player views in Node.js and Rust.
- **Host-mediated room services** in Node.js and Rust through `PlatformClient`:
  SQL, key-value storage, broadcasts, direct messages, and online player data.

## Room-host protocol

All server SDKs implement the shared HTTP/JSON room lifecycle:

| Interface | Capability |
| --- | --- |
| `GET /manifest` | Game metadata, options schema, player-view mode, and Result Unit definitions. |
| `GET /health` | Server health and runtime identity. |
| `POST /init` | Initialize a match from host-authenticated players and options. |
| `POST /tick` | Apply one authoritative tick of player inputs and trusted commands. |
| `POST /onPlayerJoin` / `POST /onPlayerLeave` | Update game state for joins, reconnects, and departures. |
| `POST /isGameOver` | Evaluate terminal state, summary, and next-match policy. |
| `POST /onCommand` | Apply a trusted room-host command. |
| `WebSocket /stream` (Node.js, Rust) | Stateful `hydrate` and `step` lifecycle with recovery, replay, private views, and action results. |
| `GET /agent/capabilities` (optional, Node.js/Rust) | Declare supported Agent protocols and modes. |
| `GET /agent/player-contract` (optional, Node.js/Rust) | Declare per-seat Agent view and action schemas. |
| `POST /agent/player-view` (optional, Node.js/Rust) | Produce a recipient-specific view for the trusted host. |

Configure integer limits satisfying `1 <= minPlayers <= maxPlayers <= 100`.
`minPlayers` is the assigned-player threshold at which the RandSeed platform
may start a match; `maxPlayers` is the room's seat capacity, not a required
fill target. If the roster must be a fixed size, set both values to that size.
A zero-player start is unsupported; a solo game uses `minPlayers: 1`.
Additional players can join after startup only while the game reports
`joinable: true` and seats remain below `maxPlayers`. The SDK validates the
roster passed to `/init`; the platform owns matchmaking and startup timing.
This start threshold is separate from `wait_for_ready`, which coordinates the
existing roster between completed and next matches.

Read the [version 1 protocol](./protocol/room-host-v1.md). Every language
implementation in this repository is tested against the same
[wire fixture](./protocol/fixtures/room-host-v1.json), including deterministic
Mulberry32 test vectors. Result Unit declarations and events are also validated
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
state. The current server SDKs and room host do not include a GameVRF client
or call the GameVRF canister; VRF is not currently available through this room
runtime.
