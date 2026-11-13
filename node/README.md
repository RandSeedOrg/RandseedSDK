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

For matchmaking, `minPlayers` is the minimum number of assigned players the
RandSeed platform needs before starting a match. It must be at least `1`; a
zero-player start is not supported. A single-player game uses
`minPlayers: 1`. `maxPlayers` must be at least `minPlayers` and is the room's
seat cap, not a required fill count. For example, `minPlayers: 2, maxPlayers: 4` may start with
two assigned players and leave two seats open. To require an exact roster size,
set both values to that size. More players can join after startup only while
the game reports `joinable: true` and seats remain below `maxPlayers`.

`createGameServer` starts the HTTP server and returns its Node.js
`http.Server`. It exposes the lifecycle routes listed in the protocol, validates
requests and game responses, limits request bodies to 1 MiB, and returns
sanitized errors for failed game callbacks. It also exposes the stateful
`/stream` WebSocket contract from
[`Session Stream v1`](../protocol/session-stream-v1.md). Stream hydration
requires the active roster, each step must advance one contiguous tick, and
per-player views are matched exactly to the active seats with no canonical-state
fallback. `actionResults` are returned in `stepResult` and in replay hydration.

## Result Units

Declare versioned result-unit types and metric aggregations in the manifest,
then return completed units from any lifecycle transition. A match can emit
multiple round, lap, stage, or match units; this contract does not require one
winner or a generic score. See the [Result Unit protocol](../protocol/room-host-v1.md#result-units-and-metrics)
and [shared event fixture](../protocol/fixtures/game-result-event-v1.json).

```ts
const resultDefinition = {
  version: "results-v1",
  unitTypes: ["round"],
  metrics: [
    { id: "roundsPlayed", scope: "participant", unit: "rounds", unitTypes: ["round"], aggregation: "sum" },
    { id: "roundsWon", scope: "participant", unit: "rounds", unitTypes: ["round"], aggregation: "sum" },
    {
      id: "winRate",
      scope: "participant",
      unit: "ratio",
      unitTypes: ["round"],
      aggregation: { type: "ratio", numeratorMetricId: "roundsWon", denominatorMetricId: "roundsPlayed" },
    },
  ],
};

const game = {
  manifest: () => ({
    title: "My Game", version: "1.0.0", minPlayers: 1, maxPlayers: 4,
    tickRateHz: 20, persistentTable: false, resultDefinition,
  }),
  tick: (_ctx, state) => ({
    state,
    persist: true,
    resultUnits: [{
      schemaVersion: 1,
      eventId: "match-42-round-3",
      matchId: "match-42",
      unitId: "round-3",
      unitType: "round",
      rulesVersion: "rules-v1",
      definitionVersion: "results-v1",
      participants: [{ playerId: "player-a", outcome: "win", metrics: { roundsPlayed: 1, roundsWon: 1 } }],
    }],
  }),
};
```

`validateGameResultDefinition`, `validateGameResultEvent`, and
`validateGameResultUnits` are exported for direct validation. Supply a
definition to validate declared metric IDs and scopes. The HTTP host checks
shape and size; the trusted room host must additionally validate the published
definition version, active match, authenticated roster, durable delivery, and
Archive idempotency. Use stable event IDs across retries. The SDK does not
persist or deliver results and does not provide statistics queries.

For TypeScript, import `type GameServer` from the same package and declare
`const game: GameServer<State> = ...`. Set `"type": "module"` in a JavaScript
project's `package.json`, or use an `.mjs` entry point.

## Additional APIs

- `PlatformClient` calls the documented room platform virtual hosts.
  `getPlayers()` returns online room members with optional public `displayName`
  and platform-hosted `avatarUrl` fields, and may include `isGameCreator` to
  indicate whether a player owns the game for the current room. Missing profile
  fields and unavailable ownership information remain absent. These values are
  platform-supplied metadata, not authorization evidence; validate ownership
  and permissions independently before protected operations. Game servers must
  not use client-submitted names or avatars as authenticated data.
- `mulberry32` and `mulberry32U32` provide deterministic, non-cryptographic
  random number generation.
- Replay helpers validate, execute, and prepare replay data for storage tiers.
- Agent capability and player-specific observation helpers are opt-in.
- Authoritative Agent player-view contracts and host-only projection endpoints
  are available in the SDK source. The Gamecreator Worker connects them to
  per-seat HTTP/WebSocket snapshots in source; action receipts and Skill
  dispatch remain incomplete platform work.

See the [protocol contract](../protocol/room-host-v1.md) and shared
[compatibility fixture](../protocol/fixtures/room-host-v1.json) for the exact
wire format. The SDK does not provide payment APIs or a GameVRF client.

For an existing browser game migration, see the paired
[Node/Rust migration guide](../protocol/ai-multiplayer-migration.md) and
[Node example](./examples/authoritative-room.mjs).

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
