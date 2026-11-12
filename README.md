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

## Room-host protocol

All server SDKs implement the same HTTP/JSON lifecycle contract:

- `GET /manifest` and `GET /health`
- `POST /init`, `/tick`, `/onPlayerJoin`, `/onPlayerLeave`, `/isGameOver`
- `POST /onCommand` for trusted host commands

Read the [version 1 protocol](./protocol/room-host-v1.md). Every language
implementation in this repository is tested against the same
[wire fixture](./protocol/fixtures/room-host-v1.json), including deterministic
Mulberry32 test vectors. Result Unit declarations and events are also validated
against the shared [Result Unit fixture](./protocol/fixtures/game-result-event-v1.json).
All server languages expose optional `resultDefinition` and `resultUnits`
fields; emitting an event does not itself persist or archive it.

Node.js and Rust also implement the internal
[Session Stream v1](./protocol/session-stream-v1.md) WebSocket contract used
between the trusted room host and the container; browsers must use `RoomClient`
instead. The bilingual [AI-assisted frontend-to-multiplayer migration
guide](./protocol/ai-multiplayer-migration.md) explains the trust boundary,
lifecycle mapping, deterministic recovery, and release-blocking checks.

The room host owns scheduling, authoritative persistence, player identity,
and recovery. Game code must calculate from the supplied state and context;
it must not run a separate authoritative timer or rely on process-global match
state. The current server SDKs and room host do not include a GameVRF client
or call the GameVRF canister; VRF is not currently available through this room
runtime.
