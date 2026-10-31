# RandSeed SDKs

This repository contains the public game-server SDKs, their shared room-host
protocol, examples, and cross-language compatibility fixtures. It does not
contain RandSeed's private platform services, Durable Objects, authentication,
or deployment implementation.

## Supported server languages

| Language | SDK location | Install and verify |
| --- | --- | --- |
| Node.js / TypeScript | [`node/`](./node/) | Consumer install: `npm install github:RandSeedOrg/RandseedSDK#main`; repository checks: `npm ci && npm test` |
| Go | [`go/`](./go/) | `go get github.com/randseedorg/randseedsdk/go/roomhost@v0.1.0`; `go test ./...` |
| Rust | [`rust/`](./rust/) | Use the path dependency and checks in the [crate README](./rust/randseed-game-sdk/README.md). |
| C# | [`dotnet/`](./dotnet/) | `dotnet run --project dotnet/verification` |
| C++ | [`cpp/`](./cpp/) | See the CMake build instructions in [`cpp/README.md`](./cpp/README.md). |

The Node.js server SDK source and other backend SDKs are maintained in this
repository. The Node package is installed from this GitHub repository, not
from the npm registry. The browser-only SDK remains published on npm as
[`@randseed/game-sdk`](https://www.npmjs.com/package/@randseed/game-sdk).

## Room-host protocol

All server SDKs implement the same HTTP/JSON lifecycle contract:

- `GET /manifest` and `GET /health`
- `POST /init`, `/tick`, `/onPlayerJoin`, `/onPlayerLeave`, `/isGameOver`
- `POST /onCommand` for trusted host commands

Read the [version 1 protocol](./protocol/room-host-v1.md). Every language
implementation in this repository is tested against the same
[wire fixture](./protocol/fixtures/room-host-v1.json), including deterministic
Mulberry32 test vectors.

The room host owns scheduling, authoritative persistence, player identity,
and recovery. Game code must calculate from the supplied state and context;
it must not run a separate authoritative timer or rely on process-global match
state. The current server SDKs and room host do not include a GameVRF client
or call the GameVRF canister; VRF is not currently available through this room
runtime.
