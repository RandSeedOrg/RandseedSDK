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
Pin a reviewed tag or commit instead of `main` for reproducible builds. The
private-view changes described below are pending review and are not present in
the current `main` until merged; this command alone does not install that
unreleased implementation.
The package requires Node.js 20 or later and supports ESM imports. JavaScript
and TypeScript use the same SDK; TypeScript declarations are included.

For the room startup and first-observation rollout, use the browser
`@randseed/game-sdk` 0.4.0 or a compatible later release once published, and
rebuild and redeploy existing room games. The browser SDK handles the
acknowledgement; no Server SDK upgrade or new game-server callback is required
for this rollout. See
[Room lifecycle and recovery](../protocol/room-host-v1.md#room-lifecycle-and-recovery)
for startup, disconnect, state, and reconnect requirements.

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

The platform supplies the canonical `RANDSEED_GAME_ID` to the server process.
The SDK uses it for health checks and Agent player-view scope checks. A missing
ID makes these endpoints unavailable; invalid or conflicting configured IDs
prevent startup. The optional `gameId` argument only checks for a conflict;
it cannot replace a missing platform ID. Local fixtures must explicitly set
`RANDSEED_GAME_ID` to their test ID. Never substitute a repository slug,
deployment ID, or guessed value in a deployed game.

For matchmaking, `minPlayers` is the minimum number of assigned players the
RandSeed platform needs before starting a match. It must be at least `1`; a
zero-player start is not supported. A single-player game uses
`minPlayers: 1`. `maxPlayers` must be at least `minPlayers` and is the room's
seat cap, not a required fill count. Assigned players are not the same as
connected WebSockets. For example, `minPlayers: 2, maxPlayers: 4` may start
with two assigned players and leave two seats open. To require an exact roster size,
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

## Private room-host player views

Public manifest wire format (omit both private fields):

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

Omit `playerViewMode` and `playerViewSchemaVersion` for an ordinary public
game. For per-seat private projections set `playerViewMode: "private"`.
Room-host v1 is the default when `playerViewSchemaVersion` is omitted; an
explicit `1` is optional and accepted only with private mode. `null`, an
unsupported version, a schema version without private mode, a null mode, or
`playerViewMode: "public"` is invalid.

Private default-v1 manifest:

```json
{
  "title": "Hidden Hand",
  "version": "1.0.0",
  "minPlayers": 2,
  "maxPlayers": 4,
  "tickRateHz": 5,
  "persistentTable": false,
  "playerViewMode": "private"
}
```

A valid `optionsSchema` is a JSON Schema object or boolean (not `null`, an
array, string, or number). For example:

```js
optionsSchema: {
  type: "object",
  properties: { mode: { type: "string", enum: ["casual", "ranked"] } },
  additionalProperties: false,
}
```

The SDK checks that the manifest value has this object-or-boolean shape; it
does not execute the schema against the supplied match options.

Optional explicit-v1 form:

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

The `init` callback uses `config.players`. Private `/tick` and `/onCommand`
requests require the active `players` roster. Join and leave requests include
the resulting roster; it includes the joining player and excludes the leaving
player. The SDK supplies this optional third callback argument to `tick`,
`onCommand`, `onPlayerJoin`, and `onPlayerLeave` so existing two-argument
callbacks remain source-compatible. Return one `playerViews[playerId]` JSON
value for every active seat from `init`, `tick`, `onPlayerJoin`, `onPlayerLeave`,
and `onCommand`. The SDK rejects missing, malformed, missing-key, extra-key,
or oversized views; each serialized view is at most 64 KiB and the entire JSON
map is at most 1 MiB.

The returned `state` remains canonical and is for the trusted room host only.
Each per-player view must exclude opponent-private data, hidden random state,
and all other non-recipient secrets. The host must deliver only the map entry
for the authenticated recipient; missing views never fall back to canonical
state. These SDK checks cannot determine whether a game callback classified
every field correctly. Do not have a browser call the lifecycle endpoints.

For deployment, `RANDSEED_GAME_ID` must be the actual platform-assigned RandSeed
Game ID matching `^[A-Za-z0-9_-]{1,64}$`; do not use a repository slug, guessed
ID, or runtime label. The SDK listens on the supplied port (the examples use
`PORT` with `8080` as the local default). Check both endpoints on the actual
bound port:

```sh
curl --fail http://127.0.0.1:8080/health
curl --fail http://127.0.0.1:8080/manifest
```

`/health` checks process availability and the configured platform Game ID;
`/manifest` independently checks and returns the game configuration. A healthy
process does not by itself prove the manifest is valid or that platform private
routing is enabled.

As of 2026-11-17, this cross-SDK alignment is pending independent review and is
not a published Node package release. Gamecreator PR #340 remains open and is
not deployed. Verify the SDK version and target host rollout before enabling
private routing.

## Data continuity across releases

The callback `state`, session checkpoints, and between-match carryover support
recovery of the room and runtime version that produced them; they are not an
all-time game database. On RandSeed, rooms are pinned to a game, environment,
and runtime version. Activating a new runtime version does not automatically
copy the old room state into the new version. In particular, do not implement
all-time rankings, progression, or inventory only in room state, a bounded
`roundHistory`, browser memory, or `persistentTable` carryover.

Use an explicitly durable game-level store with stable game/player keys and
documented cross-release retention. Version its stored schema separately from
the runtime release, and implement/test migrations or backfills when its
meaning or shape changes. A bug-fix deployment should preserve historical
rankings when their metric meaning is unchanged; if the ranking is intentionally
per-version or per-season, make that scope explicit in its key and UI.

The game owns its data schema and can read back records through a storage API,
but the API must have the right scope. On the current Gamecreator host,
`PlatformClient.sql` reads and writes the active GameRoom Durable Object's SQL
database; it is room-scoped, not a database shared by every room for the game.
`kvGet`/`kvPut` use an in-memory map belonging to that room and are not durable
across room-object restarts. These methods are suitable only for data whose
lifetime is intentionally limited to that room.

`persist: true` requests room-state snapshotting; it does not write an
independent game-level record. Do not put an all-time leaderboard in
`PlatformClient.sql`, `PlatformClient` KV, room state, or carryover and assume
it will be visible from a newly allocated room or runtime version.

For match results, emit Result Units and verify the host's archive acceptance.
The Archive result-query API remains reserved for trusted platform services;
Result Units are not directly queryable by game code. The current Server SDK
source separately includes the opt-in `platform.data` document API described
below. Its Gamecreator host gate is off by default, and this API does not
provide general Result Unit queries or arbitrary analytics.

## Explicit game-owned data (Game Data protocol v1)

The Server SDK source exposes `PlatformClient.data` for explicitly authored
JSON records shared across rooms and runtime releases of the same game and full
environment. The Gamecreator host gate `GAME_DATA_STORE_ENABLED` is off by
default; an SDK method existing is not a production availability promise.
`DataPlugin` is separate and remains disabled. The host—not the game request,
browser, room ID, or runtime version—derives the game/environment scope from
verified room admission. Public, preview, and sandbox environments, as well as
dev, test, and production deployments, are isolated.

This is forward-only. Only values that game code explicitly writes are stored;
there is no automatic copy of Room SQLite, snapshots, KV, old Result Units,
Blackjack results, legacy client-reported boards, profiles, principals, or R2
replays. A `platform.data` call and a Room transition are separate Durable
Object operations: they are not atomic and the callback is not a durable
outbox. Use deterministic idempotency keys and retry only the identical
payload after transient failures. Exact committed-transition mirroring is not
available in this stage.

```ts
import { PlatformClient } from "@randseed/server-sdk";

const data = new PlatformClient().data;
await data.registerSchema("seasonScores", {
  version: 1,
  fields: { season: "number", score: "number", label: "string" },
  indexes: ["score"],
  compoundIndexes: [["season", "score"]],
});

const prior = await data.get("seasonScores", "player-summary");
await data.put({
  idempotencyKey: "match-42/player-summary",
  collection: "seasonScores",
  id: "player-summary",
  expectedRevision: prior?.revision ?? 0,
  value: { season: 2026, score: 1200, label: "final" },
});
const page = await data.query({
  collection: "seasonScores",
  where: [{ field: "season", op: "eq", value: 2026 }],
  orderBy: { field: "score", direction: "desc" },
  limit: 50,
});
```

The compound query above requires the declared ordered pair
`["season", "score"]`; it supports one equality filter on `season`, then
ascending or descending ordering on `score`. A document ID is the stable tie
breaker. Same-field queries use a declared single-field index; an unfiltered
query can list by document ID. Other cross-field predicates/orderings, multiple
filters, joins, offsets, arbitrary SQL, and unbounded scans are explicitly
unsupported. Aggregates are limited to `count`, `sum`, `min`, `max`, and `avg`
over a declared numeric field, with at most one single-index filter and an
explicit scan limit of at most 1,000. Unsupported plans and exceeded bounds
return errors rather than falling back to a scan. Aggregates exactly accumulate
stored binary64 values and round the final result to nearest-even; unsafe integer
results return an error. JSON integers outside the JavaScript safe-integer range
are rejected; Node also rejects non-finite numbers and negative zero before
sending.

Writes use revision-based compare-and-swap and tombstones; recreating a deleted
record must use its tombstone revision. `mutate` applies at most 25 unique
records atomically within one request. Repeating an idempotency key with the
same payload returns its original receipt; changing that payload conflicts.
Schema upgrades are additive, and old releases may re-register their unchanged
schema; new sparse fields must not break old writers. Do not newly index a
previously unindexed string field, because the index length bound could reject
old values. Consult the [Game Data v1 protocol](../protocol/game-data-v1.md)
for exact wire fields, errors, compatibility rules, and quotas.

The service caps each document at 64 KiB, a write request at 512 KiB, query
pages at 100, collections at 32 per game/environment scope, fields at 32 per
collection, single and compound indexes at 16 each per collection, live records
at 8,192 per scope, stored records including tombstones at 16,384 per scope,
and live serialized data at 128 MiB per scope. Archive-wide across environments,
it caps declared indexes at 128 and normalized query-index entries at 16,384.
It retains at most 4,096 idempotency receipts per scope for 30 days and 4,096
pagination cursors per scope for 15 minutes. Rate is 120 requests per room per
minute, with at most 10,000 tracked rooms per scope. These caps are part of the
contract, not target capacity guarantees.

TypeScript throws `PlatformDataError` with HTTP `status`, sanitized stable
`code`, and `retryable`; branch on the code rather than message text. Do not
retry conflicts, invalid inputs, or unsupported query shapes.

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
`resultDefinition.version` versions the meaning/schema of result metrics; it is
not the game release or runtime version. Keep it stable for bug fixes that do
not change metric meaning. When its meaning changes, plan how readers will
combine or migrate the old and new result series. Never assume that emitting a
Result Unit creates a cross-version leaderboard. The game can query archived
results only if the target platform exposes an authorized Archive query API;
otherwise, write a durable game-level projection through an available storage
service and read it back there.

For TypeScript, import `type GameServer` from the same package and declare
`const game: GameServer<State> = ...`. Set `"type": "module"` in a JavaScript
project's `package.json`, or use an `.mjs` entry point.

## Additional APIs

- `PlatformClient` calls the documented room platform virtual hosts.
  `getPlayers()` returns connected players’ game-scoped opaque `playerId` and
  `joinedAt`, plus the optional display-only `displayName`, `avatarUrl`, and
  `isGameCreator` fields. Missing or `null` optional fields are accepted;
  present `null` values remain `null`, while absent fields remain omitted.
  Principal, account, ownership, and all other unlisted metadata are stripped.
  `isGameCreator` is a display hint only and must never be used for authorization
  or privileged actions. See the [online-player profile contract](../protocol/room-host-v1.md#online-player-roster-and-display-profiles).
- `ChaCha20Rng`, `deriveSeed`, and `deriveSeedHex` provide deterministic 20-round
  ChaCha20 simulation randomness (matching `rand_chacha::ChaCha20Rng`) seeded
  from the host-injected 256-bit VRF root seed (`options.randomSeed`), with
  JSON-safe `{ seedHex, stream, wordPos }` cursor snapshots via `getState()` /
  `ChaCha20Rng.fromState()`. `mulberry32` and `mulberry32U32` remain available
  for legacy `options.seed` compatibility.
- Replay helpers validate, execute, and prepare replay data for storage tiers.
- Agent capability and player-specific observation helpers are opt-in.
- Authoritative Agent player-view contracts and host-only projection endpoints
  are available in the SDK source. The Gamecreator Worker connects them to
  per-seat HTTP/WebSocket snapshots in source; action receipts and Skill
  dispatch remain incomplete platform work.

See the [protocol contract](../protocol/room-host-v1.md) and shared
[compatibility fixture](../protocol/fixtures/room-host-v1.json) for the exact
wire format. The SDK does not provide payment APIs or a direct VRF canister
client (the trusted room host injects `options.randomSeed` at match init).

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
