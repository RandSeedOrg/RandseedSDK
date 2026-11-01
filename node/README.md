# RandSeed Node.js Server SDK

The Node.js SDK implements the shared [Room Host Protocol v1](../protocol/room-host-v1.md)
and is maintained in the public [RandseedSDK repository](https://github.com/RandSeedOrg/RandseedSDK).

## Install from GitHub

From the Node.js game-server project, install the package directly from this repository:

```sh
npm install github:RandSeedOrg/RandseedSDK#main
```

This installs the backend SDK from GitHub; it is not an npm-registry release.
The browser SDK is separate and is installed from npm as
`@randseed/game-sdk`.
Pin a reviewed tag or commit instead of `main` for reproducible builds.
The package requires Node.js 20 or later and supports ESM imports. JavaScript
and TypeScript use the same SDK; TypeScript declarations are included.

## Minimal server (JavaScript)

```js
import { createGameServer } from "@randseed/server-sdk";

const game = {
  manifest: () => ({
    title: "My Game",
    version: "1.0.0",
    minPlayers: 1,
    maxPlayers: 4,
    tickRateHz: 20,
    persistentTable: false,
  }),
  init: () => ({ state: { score: 0 } }),
  tick: (_context, state) => ({ state, broadcasts: [], persist: false }),
  onPlayerJoin: (_playerId, state) => ({ state }),
  onPlayerLeave: (_playerId, state) => ({ state }),
  isGameOver: () => ({ over: false }),
};

createGameServer(game, Number(process.env.PORT ?? 8080));
```

`createGameServer` starts the HTTP server and returns its Node.js
`http.Server`. It exposes the lifecycle routes listed in the protocol, validates
requests and game responses, limits request bodies to 1 MiB, and returns
sanitized errors for failed game callbacks.

For TypeScript, import `type GameServer` from the same package and declare
`const game: GameServer<State> = ...`. Set `"type": "module"` in a JavaScript
project's `package.json`, or use an `.mjs` entry point.

## Additional APIs

- `PlatformClient` calls the documented room platform virtual hosts.
- `mulberry32` and `mulberry32U32` provide deterministic, non-cryptographic
  random number generation.
- Replay helpers validate, execute, and prepare replay data for storage tiers.
- Agent capability and player-specific observation helpers are opt-in.

See the [protocol contract](../protocol/room-host-v1.md) and shared
[compatibility fixture](../protocol/fixtures/room-host-v1.json) for the exact
wire format. The SDK does not provide payment APIs or a GameVRF client.

The Tier B helpers require storage of `match_config_json` and `commands_json`
alongside the replay row, plus a zero-based `input_index` on every input row.
Persist the index from `splitReplayForTierB` and return all indexed rows to
`assembleReplayFromTierB`; it preserves the original order of inputs sharing a
tick and rejects missing or duplicate indexes. The existing GameArchive Tier B
schema does not yet store these fields; do not write these helper rows to that
schema or assume they can be reconstructed from older archives. Missing fields
are rejected rather than silently dropping data. Replay execution covers ticks
through the last recorded input or command; applications must record their
final tick separately if simulation continues after the last event.
