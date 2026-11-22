# Room Host Protocol v1

This protocol is the language-independent contract between a RandSeed room
host and a game-server container. It uses UTF-8 JSON over HTTP. Object keys in
the wire format are camelCase. A host invokes the container; browsers must use
the browser `RoomClient`, not call these endpoints directly.

## Endpoints

| Method | Path | Request | Response |
| --- | --- | --- | --- |
| `GET` | `/manifest` | None | `GameManifest` |
| `GET` | `/health` | None | `{ "status": "ok", "gameId": "...", "uptime": 1.0 }` |
| `POST` | `/init` | `MatchConfig` | `InitResult` |
| `POST` | `/tick` | `TickRequest` | `TickResult` |
| `POST` | `/onPlayerJoin` | `PlayerEventRequest` | `PlayerEventResult` |
| `POST` | `/onPlayerLeave` | `PlayerEventRequest` | `PlayerEventResult` |
| `POST` | `/isGameOver` | `IsGameOverRequest` | `GameResult` |
| `POST` | `/onCommand` | `CommandRequest` | `CommandResult` |
| `GET` | `/agent/player-contract` | None | Optional `AgentPlayerContract` |
| `POST` | `/agent/player-view` | Trusted host `AgentPlayerViewRequest` | Recipient-only `AgentPlayerView` |

The server listens on port `8080` by default. Request validation errors return
HTTP `400` with `{ "error": "..." }`. Unhandled game or server errors return
HTTP `500` with a sanitized `{ "error": "..." }` response; never return stack
traces, credentials, or internal platform data. Each request body is limited
to 1 MiB; oversized bodies return HTTP `413`.

The Agent player endpoints are host-to-container protocol operations. A browser
must use the Player/Room gateway and must never submit canonical state or choose
the recipient seat directly.

In deployed containers, `gameId` is the platform-assigned RandSeed Game ID, not
an SDK package name, repository name, runtime release, or a value guessed by
the game. Valid IDs match `^[A-Za-z0-9_-]{1,64}$`. Node reads
`RANDSEED_GAME_ID`; Rust runner examples read the same environment variable (or callers pass the
platform value to the router); Go reads `HandlerOptions.GameID` or
`GameIDProvider`. `/health` checks that this identity is present and valid.
`/manifest` independently checks and returns the game configuration. Check both:
a healthy process does not prove that its manifest is usable, and a valid
manifest does not prove that the target platform rollout is enabled.

## Data contracts

### Game manifest

```json
{
  "title": "Room Counter",
  "version": "1.0.0",
  "minPlayers": 2,
  "maxPlayers": 4,
  "tickRateHz": 5,
  "persistentTable": false,
  "resultDefinition": {
    "version": "results-v1",
    "unitTypes": ["round", "match"],
    "metrics": [
      {
        "id": "handsPlayed",
        "scope": "participant",
        "unit": "hands",
        "unitTypes": ["round"],
        "aggregation": "sum"
      },
      {
        "id": "handsWon",
        "scope": "participant",
        "unit": "hands",
        "unitTypes": ["round"],
        "aggregation": "sum"
      },
      {
        "id": "winRate",
        "scope": "participant",
        "unit": "ratio",
        "unitTypes": ["round"],
        "aggregation": {
          "type": "ratio",
          "numeratorMetricId": "handsWon",
          "denominatorMetricId": "handsPlayed"
        }
      }
    ]
  }
}
```

`title` and `version` are non-empty strings. `minPlayers` and `maxPlayers` are
integers satisfying `1 <= minPlayers <= maxPlayers <= 100`; zero-player starts
are unsupported. The current room platform accepts tick rates from 1 through
60 Hz, including fractional rates.
`persistentTable` is a boolean. `optionsSchema` may optionally declare
a JSON Schema for game options; its wire value must be an object or boolean
(`null`, arrays, strings, and numbers are invalid). The SDKs validate this wire
shape but do not evaluate the schema against each game's options payload.
`resultDefinition` is optional and declares the result-unit types and metric
aggregation contract described below. Keep its version stable only while its
meaning is unchanged.

For example, this is a valid object schema (a boolean schema is also valid):

```json
{
  "optionsSchema": {
    "type": "object",
    "properties": { "mode": { "type": "string", "enum": ["casual", "ranked"] } },
    "additionalProperties": false
  }
}
```

On the RandSeed platform, `minPlayers` is the minimum number of assigned
players required before a match may start (at least one; zero-player starts
are unsupported). It counts assigned players, not connected WebSockets.
A match can begin at this threshold while some seats remain
empty. `maxPlayers` is the maximum number of player seats, not a target the
lobby must fill. For example, `minPlayers: 2` and `maxPlayers: 4` allows the
platform to start with two assigned players and leaves capacity for up to two
more. A solo game uses `minPlayers: 1`. If a game requires an exact roster, set
both values to that roster size. After startup, a new player identity can be
assigned only while the game reports `joinable: true` and an unassigned seat
remains. `joinable` does not revoke a seat already assigned to an identity: that
player may reconnect while `joinable` is false and does not consume another
seat. A leave event represents a connection leaving, not permanent seat
release; v1 has no separate permanent-departure event.

The host/platform MUST track assigned seats separately from the active `players`
roster and preserve that admission set across checkpoints, replay, restart, and
restore. The active roster alone drives callbacks, input authorization, and
recipient projections; retained seats MUST NOT be added to callback rosters or
private views. A reconnect may produce `onPlayerJoin` with the resulting active
roster, without allocating a duplicate seat. Node.js, Rust, Go, and .NET SDKs
enforce manifest roster bounds at `/init`; C++ currently validates only
generic roster constraints. Gamecreator also enforces manifest bounds
platform-side. Matchmaking and startup timing belong to the host/platform.
`wait_for_ready` is a separate post-match policy and uses the completed match's
full assigned roster to decide when to start the next match.

### Private room-host player views

`playerViewMode` opts a game into the general room-host v1 recipient-specific
projection contract. The mode value is either omitted (ordinary public game) or
`"private"`; do not send `"public"` or `null`. The schema version defaults to
v1 when omitted for private mode. An explicit version is optional and may only
be the JSON number `1` when `playerViewMode` is `"private"`. An explicit
`null`, unsupported value, or schema version without private mode is invalid.
The canonical vectors are
[`room-host-private-v1.json`](./fixtures/room-host-private-v1.json).

Public wire manifest (both private fields omitted):

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

Private mode with the v1 default (no schema version required):

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

Private mode with the optional explicit v1 declaration:

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

`POST /init` derives its required projection roster from `MatchConfig.players`.
Private `/tick` and `/onCommand` requests must include the host-supplied active
`players` roster. A join request includes the resulting active roster with the
joining player; a leave request includes the resulting active roster without
the leaving player. The callback receives that roster in the SDK-specific
request context. For every private state transition (`/init`, `/tick`,
`/onPlayerJoin`, `/onPlayerLeave`, and `/onCommand`), return exactly one JSON
`playerViews` entry for each active roster member. An empty active roster is
valid after a final leave and requires the explicit `players: []` roster and
`playerViews: {}`. A missing or `null` roster is not an empty roster; it is
invalid wherever the active roster is required. Invalid, missing, or extra
view entries fail closed. For public HTTP transitions, an explicitly supplied
empty `players` array likewise means an empty active roster; `null` is invalid.
The stream hydration roster is required and may be empty when hydrating a
checkpoint after the final leave.
Each serialized view is limited to 64 KiB; the complete UTF-8 JSON serialization of the view map, including player ID keys and JSON
syntax, is limited to 1 MiB. No view map is required for `/isGameOver`, which
does not return a state transition.

Example transition response (the identifiers are fixture placeholders, not
platform or user identity values):

```json
{
  "state": {
    "hands": {
      "seat-a": ["ACE_OF_SPADES"],
      "seat-b": ["KING_OF_CLUBS"]
    }
  },
  "playerViews": {
    "seat-a": { "hand": ["ACE_OF_SPADES"], "viewer": "seat-a" },
    "seat-b": { "hand": ["KING_OF_CLUBS"], "viewer": "seat-b" }
  }
}
```

`state` remains canonical authoritative simulation state and is delivered only
to the trusted room host. The game callback must construct each view from the
recipient perspective and exclude other seats private data, hidden random
state, and secrets. The host must select the view keyed by the authenticated
recipient; it must never deliver canonical `state`, another player view, or an
empty fallback when a view is missing. The SDK validates JSON, exact roster
coverage and resource limits; it cannot judge whether a game author classified
the data correctly. Do not expose `/manifest` or lifecycle endpoints as a
browser player API.

Node.js, Rust, and Go implement these HTTP lifecycle checks. Go support here is
limited to the HTTP room lifecycle; this does not add Go `/stream`, Agent, or
other capabilities. Rust retains existing public `GameManifest` struct literal
compatibility and emits the optional explicit `playerViewSchemaVersion: 1`
when a private manifest is serialized.

Rollout status recorded 2026-11-17: Gamecreator PR #340 is open and not
deployed, and this cross-SDK source change is pending independent review; no
released SDK or platform rollout is implied. Confirm both the installed SDK
release and target host environment before enabling private player routing.

### Match configuration

The JSON below is illustrative: replace its `gameId` and `players` with the
platform-assigned RandSeed Game ID and the host-supplied player IDs. Those
identity values must not be guessed or copied from the example.

```json
{
  "matchId": "match-1",
  "gameId": "<platform-assigned-game-id>",
  "players": ["<host-player-id-a>", "<host-player-id-b>"],
  "matchSeq": 1,
  "options": {
    "randomSeed": "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    "randomSeedIndex": 42,
    "seed": 50462976,
    "mode": "ranked"
  },
  "carryover": null,
  "tickRateHz": 5
}
```

`players` is a non-empty array of unique player IDs supplied by the host.
`matchSeq` is a positive safe integer. `tickRateHz`, when present, is a finite
number between 1 and 60; fractional rates are valid and `null` is not valid.
`options` may be omitted; when present it must be an object, not `null`.
`options.randomSeed`, when present, must be a 64-character lowercase ASCII hex
string (`^[0-9a-f]{64}$`) encoding the host-supplied 32-byte (`[u8; 32]`) VRF
match root seed; `null` and non-canonical strings are rejected with
`INVALID_OPTIONS`. `options.randomSeedIndex`, when present, must be a
non-negative safe integer (`0 <= n <= 9007199254740991`) recording the on-chain
`random_oracle` record index; `null`, fractional, negative, or unsafe integers
are rejected with `INVALID_OPTIONS`. `options.seed`, when present, is a finite
JSON number (`u32_le(randomSeed[0..4])` when derived from `randomSeed`, retained
for legacy 32-bit compatibility). Clients may never supply `seed`,
`randomSeed`, or `randomSeedIndex`; the trusted room host injects them. Other
`options` entries are game-owned JSON configuration. `carryover` is opaque JSON
and may be any JSON value.

### Agent player protocol v1

The canonical cross-language vectors are in
[`agent-player-v1.json`](./fixtures/agent-player-v1.json). SDKs must preserve
the wire casing, reject an invalid roster or recipient, and fail closed when a
view is missing or larger than 6,000 UTF-8 bytes.

`GET /agent/player-contract` returns `404` when authoritative per-seat views
are not implemented. A valid contract declares:

```json
{
  "playerViewProtocolVersion": 1,
  "actionProtocolVersion": 1,
  "playerViewSchemaId": "hidden-hand.view.v1",
  "actionSchemaId": "hidden-hand.action.v1",
  "modes": ["follow", "autoplay"],
  "maxDecisionHz": 5,
  "independentSeats": true,
  "backgroundAutoplay": false
}
```

Decision frequency is finite, positive, and capped at 10 Hz. Background
Autoplay requires both `autoplay` mode and independent-seat support. A contract
declaration is not a privacy proof, authorization, or release approval.

The Room host constructs `AgentPlayerViewRequest` from the exact committed
state, scope, roster, seat identity, tick, sequence, and committed events. It
calls `/agent/player-view`; the game server projects that canonical input into:

```json
{
  "protocolVersion": 1,
  "scope": {
    "gameId": "hidden-hand",
    "environment": "dev",
    "releaseVersion": "release-1"
  },
  "roomId": "room-1",
  "matchId": "match-1",
  "matchSeq": 1,
  "playerId": "seat-a",
  "sequence": 7,
  "tick": 12,
  "schemaId": "hidden-hand.view.v1",
  "publicState": { "round": 2, "turn": "seat-a" },
  "self": { "playerId": "seat-a", "privateState": { "hand": ["A_SECRET"] } },
  "visibleEvents": [{ "type": "turn_started", "playerId": "seat-a" }],
  "decision": { "canAct": true, "deadlineAt": 1793810400000 }
}
```

The game callback is responsible for filtering opponent-private state, events,
random state, initialization secrets, and any other non-recipient data. The SDK
copies and bounds the result but cannot infer whether a publisher classified
game data correctly. The platform must send each view only to the authenticated
recipient selected by the Room host; it must not fall back to canonical state.

An Agent action binds an action ID to scope, room, match, player seat, controller
lease epoch, source observation sequence, expected tick, required action schema,
and JSON payload. Retries with the same ID and identical payload are idempotent;
reuse with a different payload is a conflict. Receipts distinguish `queued`,
`applied`, `rejected`, and `unresolved`. A newer tick is not proof that a
specific action was applied. A Skill decision is either `{ "kind": "act",
"action": ..., "memory": ... }` or `{ "kind": "wait", "resumeAt": null,
"memory": ... }`; `wait` does not submit a game action and must use bounded
scheduling.

### Initialization

Request: `MatchConfig`.

Response:

```json
{
  "state": { "count": 0 },
  "broadcasts": [],
  "resultUnits": [{
    "schemaVersion": 1,
    "eventId": "match-1-round-1-result",
    "matchId": "match-1",
    "unitId": "round-1",
    "unitType": "round",
    "rulesVersion": "rules-v1",
    "definitionVersion": "results-v1",
    "participants": [{
      "playerId": "pairwise-a",
      "outcome": "win",
      "rank": 1,
      "metrics": { "handsPlayed": 1, "handsWon": 1 }
    }]
  }]
}
```

The `state` property is required. `playerViews`, `broadcasts`, `joinable`, and
`resultUnits` are optional. When `playerViews` is provided as an object keyed by
`playerId`, its keys must exactly match the request's active roster. The room
host retains `state` as authoritative simulation state and delivers only the
matching per-player view. It must never fall back to canonical state for a
missing key. Private-view manifests require `playerViews` on every state
transition. Any transition may emit zero or more completed units: `/init`,
`/tick`, `/onPlayerJoin`, `/onPlayerLeave`, `/isGameOver`, and `/onCommand`.

### Tick

Request:

```json
{
  "ctx": {
    "tick": 1,
    "inputs": [{ "playerId": "pairwise-a", "data": { "delta": 1 } }],
    "commands": [],
    "elapsedSeconds": 0.2
  },
  "state": { "count": 0 },
  "players": ["pairwise-a", "pairwise-b"]
}
```

Each input has a `playerId` and a required JSON `data` value; `timestamp` is
optional, is a non-negative safe integer, and is measured in epoch milliseconds.
`actionId` and `actionSchemaId` are also optional tick-input metadata. Both
fields may be absent; a valid `actionId` may appear without a schema; and a
valid pair is accepted. `actionSchemaId` without `actionId`, malformed
identifiers, and explicit `null` values are invalid. An omitted schema does not
identify a controller or establish human identity. The separate
`AgentActionRequest` continues to require `actionSchemaId`.

`commands` is optional and defaults to an empty array. `tick` is a non-negative
safe integer. `elapsedSeconds` is finite, non-negative cumulative match time
derived from the host tick, not wall-clock time. Do not add successive
`elapsedSeconds` values to calculate total duration.

The game may return `actionResults` for submitted action IDs. Each result must
refer to a unique `actionId` submitted on that tick and reports the game
callback's outcome; the SDK does not manufacture an outcome for an input.

Response:

```json
{ "state": { "count": 1 }, "broadcasts": [], "persist": false }
```

`state` and boolean `persist` are required. `broadcasts` and `resultUnits` are
optional arrays. When player projections are returned, include `players` in the
request so the SDK can check exact roster coverage. `persist` requests a
snapshot; it does not determine whether the host keeps the authoritative state
or journals and replays committed ticks.

### Player events

`POST /onPlayerJoin` and `POST /onPlayerLeave` both accept:

```json
{ "playerId": "pairwise-a", "state": { "count": 1 } }
```

They return `{ "state": ..., "broadcasts": [] }`. `state` is required;
`broadcasts` and `resultUnits` are optional. Include `players` when returning
per-player views; it describes the resulting roster after the event.

### Game over and trusted commands

`POST /isGameOver` accepts `{ "state": ... }` and returns:

```json
{
  "over": true,
  "summary": { "durationSeconds": 0.2 },
  "nextMatchPolicy": { "mode": "close_room", "timeoutMs": 0 },
  "resultUnits": []
}
```

When `over` is true, `summary` and `nextMatchPolicy` are required. The summary
must be an object containing a finite, non-negative `durationSeconds`. The policy must contain
a supported `mode` and a non-negative integer `timeoutMs` (include it even
when the timeout is zero). Supported policy modes are `auto_next_match`,
`wait_for_ready`, and `close_room`.
Browser-ready signaling for `wait_for_ready` is outside this host-to-container
protocol. On the RandSeed platform, it is a separate browser operation; it
does not change the initial `minPlayers` threshold.

Trusted host commands use `POST /onCommand` with
`{ "command": { "type": "reset", "data": {} }, "state": ... }` and return
`{ "state": ..., "broadcasts": [], "resultUnits": [] }`. This endpoint is not
a player input API. Include `players` when returning `playerViews`.

The stateful WebSocket contract, roster lifecycle ordering, recovery, action
receipts, and structured stream errors are specified in
[`session-stream-v1.md`](./session-stream-v1.md).

## Room lifecycle and recovery

The room host owns matching, authoritative player identity, scheduling,
persistence, and recovery. The game server owns game rules and must calculate
transitions from the supplied state and context. These responsibilities do not
change the host-to-container request and response shapes above.

### Startup and browser migration

- The platform may start when the assigned roster reaches `minPlayers`;
  assignment does not prove that every player is online. Late joins require
  `joinable: true` and remaining capacity.
- Browser `RoomClient.warm()` prepares a container; a ready warm receipt is
  not proof that a match is initialized or that a player is connected.
- The startup and first-observation rollout requires browser
  `@randseed/game-sdk` 0.4.0 or a compatible later release. Once published,
  update the browser dependency and lockfile, rebuild, and redeploy every
  affected room game. An existing deployed bundle remains unchanged until
  rebuilt. Coordinate the game release with the target platform environment.
  Static games without rooms do not need this migration.
- The browser SDK and platform handle first-snapshot acknowledgement. Do not
  implement it in a game-server callback or imitate a protocol header in an
  older browser SDK. This rollout does not require a Server SDK upgrade.
  See the
  [browser room SDK guide](https://github.com/RandSeedOrg/Gamecreator/blob/dev/docs/game-room-sdk.md)
  for the platform rollout and client behavior.

After confirming that version 0.4.0 is published, a game using npm can update
its browser dependency with:

```sh
npm install --save-exact @randseed/game-sdk@0.4.0
```

Commit the dependency and lockfile, then run the game's normal build and
deployment workflow. A package update alone does not replace an already
deployed or immutable game release.

### Disconnect and reconnect

`onPlayerLeave` can be delivered when a player's connection closes, including
a temporary disconnect. The player-event request contains `playerId` and
`state`, but no reason that distinguishes a network failure from a deliberate
departure. Do not assume one leave event means a player forfeited. An
`onPlayerJoin` can represent a reconnect; handle an existing player
idempotently instead of creating a second seat or resetting the hand.

Decide and implement the game's own offline behavior in its authoritative
state: for example, retain a player's logical seat and hand for a bounded wait,
or apply a game-defined timeout, automatic action, or forfeiture. There is no
general server SDK setting that makes the platform wait or pause a match for
one disconnected player. Simulation and game timers can advance while a
player is offline. A reconnect receives the current authoritative state, not
necessarily the pre-disconnect state. Use the supplied tick and
`elapsedSeconds` for deterministic gameplay timing; a platform idle pause
when all players are offline is not a per-player wait guarantee.

Store all information needed to continue the match, such as seats, hands,
turns, deadlines, and random state, in the game state passed through the
callbacks. Do not rely on browser state or process-local memory for recovery.
Reconnect can fail when authorization expires or is revoked, the release
changes, or the room is no longer available; it is not an unlimited
reservation.

### Private views and next matches

For private-view games, return a view for every member of the resulting active
roster on every transition as specified in the manifest contract above. A
game may retain a disconnected player's game data in canonical state when its
rules require it, but must never expose another player's hidden data in a
recipient's view or broadcast. The platform handles eligible terminal-view
retention; it does not replace the game's projection logic.

`auto_next_match`, `wait_for_ready`, and `close_room` are distinct game-over
policies, not disconnect policies. With `auto_next_match`, a reconnect after
settlement may see a later match. Preserve required between-match data through
the game-defined carryover. With `wait_for_ready`, platform readiness and its
timeout determine when the next match may start. With `close_room`, a new
allocation is needed for another room.

### Online-player roster and display profiles

The platform's online-player endpoint returns an object with a `players` array
of currently connected players. Each member MUST contain a non-empty,
game-scoped opaque string `playerId` and a non-negative safe-integer `joinedAt`
timestamp. It MAY contain these additional display-only values:

| Field | Accepted JSON values |
| --- | --- |
| `displayName` | string or `null` |
| `avatarUrl` | string or `null` |
| `isGameCreator` | boolean or `null` |

Each optional field may also be absent. Clients MUST accept both absent and
`null` optional fields for compatibility with older hosts. Node preserves an
explicit `null` as `null` and leaves an absent field omitted. Rust `Option` and
Go `nil` representations normalize both absent and `null` to no value; their
serialization may omit the field. Callers MUST NOT rely on distinguishing
absent from `null` across SDK languages. A present non-null value of the wrong
JSON type makes the roster invalid. Clients MUST return only the listed fields
and MUST discard every other field, including principal, account, or ownership
metadata, even if a host includes it in a response.

`isGameCreator` is a display hint, not an authorization claim. Games MUST NOT
use it to grant privileges or authorize actions. Profile information is for
in-room display; it does not change player identity or roster membership.

### Cross-release data continuity

Room state, session checkpoints, and game-defined carryover are recovery
mechanisms for a room bound to its current game, environment, and runtime
version. On RandSeed, activating a new runtime version does not automatically
hydrate the new version from an older room. The old room may remain associated
with its original version, but the SDK does not define a cross-release state
migration or guarantee that the old room history remains queryable. A new
release is not a storage migration.

Do not treat `persistentTable`, `nextMatchPolicy`, `manifest.version`, or
`resultDefinition.version` as a promise of cross-release persistence:

- `persistentTable` and carryover do not replace a durable game-level data
  contract. A history kept only in room state (especially a bounded recent
  history) can appear empty when players enter a room on a new runtime version.
- `manifest.version` is publisher metadata. It is distinct from the host's
  runtime-version binding and does not migrate stored state.
- `resultDefinition.version` identifies the result metric contract. Keep it
  stable while metric meanings stay the same; changing it creates a separately
  versioned result series for consumers. The Server SDK validates result
  declarations and events but does not persist them, deliver them to Archive,
  merge versions, or expose leaderboard/statistics queries.

For all-time rankings, player progression, inventory, or other data that must
survive bug-fix releases, use a durable game-level store whose namespace and
retention are explicitly stable across runtime versions. Key records by stable
game and player identities, not an ephemeral room or runtime instance. Define
schema versioning, idempotent writes, retention/deletion, and a migration or
backfill plan. If a metric's meaning changes, decide whether to start a new
ranking, provide a combined historical view, or migrate old results; do not
silently assume the new release will see old room state.

Game data schemas are publisher-defined, and game code can read back its writes
only through a storage or query API that the host provides. On the current
Gamecreator host, Node.js and Rust `PlatformClient.sql` reads/writes the active
GameRoom Durable Object's SQLite database. It is durable for that Room DO, but
room-scoped rather than shared across all rooms for a game. `PlatformClient`
`kvGet`/`kvPut` use an in-memory map local to the Room DO and are not durable
across object restarts. Neither is the correct store for an all-time leaderboard
that must be visible from new rooms or releases.

`persist: true` requests room-state snapshotting; it does not write an
independent game-level record. Result Units are the game-to-host result-event
contract; the host must accept and archive them, and a query API is separately
required to display historical rankings. Gamecreator exposes Result Unit
queries only to trusted internal services, not as a general Server SDK API.
The current Server SDK source separately defines a versioned, explicitly
game-authored `platform.data` document protocol. The Gamecreator implementation
remains gated off by default; it does not make Result Units queryable, support
arbitrary queries, or backfill earlier Room/Archive data. See
[Game Data Protocol v1](./game-data-v1.md) for its wire contract and limitations.
Do not document `PlatformClient.sql`, room KV, or emitted Result Units as a
game-level shared store.

Any game-level service must document stable game/environment scope,
read/query authorization, schema migration, deletion, retention, and backfill.
The current `platform.data` foundation explicitly has no historical backfill;
separately enabled capabilities must not silently imply one.

Before deploying an upgrade, test with existing records: publish a new runtime
version, create a room under it, and verify that the intended historical
rankings/progression remain visible. Confirm persisted result/archive receipts
and the exact query path; a `resultUnits` response alone is not proof that the
event was stored or can be queried.

### Verification checklist

- Start with the configured minimum assigned roster, including an assigned
  player who has not connected; verify late joining and `joinable` behavior.
- Disconnect during a live turn and reconnect before and after the
  game-defined deadline. Verify the player, room, match, seat, and hand before
  the deadline, and the expected timeout or settlement afterward.
- Verify separate player views with private-data canaries and exercise
  reconnect after checkpoint recovery. Test each supported next-match policy.
- In the target environment, rebuild with the published browser SDK, then
  verify first connection, background/foreground renewal, reconnect, and a
  second match with the actual deployed release.

## Result Units and metrics

A Result Unit is a game-defined settlement boundary within a match: a round,
hand, lap, stage, or the complete match. One match can emit multiple units, and
the contract does not require one winner, a numeric score, or even an individual
outcome. Games may report participant results, team results, or both.

`resultDefinition` has a version, unique `unitTypes`, and up to 128 metric
declarations. Each metric specifies a stable `id`, `scope` (`participant` or
`team`), semantic `unit`, applicable unit types, and an aggregation:
`sum`, `min`, `max`, or a derived ratio. A ratio references two `sum` metrics
with the same scope and applicable unit types. Submit only direct metrics;
ratios are derived from cumulative numerator and denominator totals, not from
an average of per-room or per-match ratios. A zero denominator has no numeric
ratio result.

The complete cross-language event example is in
[`game-result-event-v1.json`](./fixtures/game-result-event-v1.json):

```json
{
  "schemaVersion": 1,
  "eventId": "match-1-round-1-result",
  "matchId": "match-1",
  "unitId": "round-1",
  "unitType": "round",
  "rulesVersion": "rules-v1",
  "definitionVersion": "results-v1",
  "participants": [{
    "playerId": "pairwise-a",
    "outcome": "win",
    "rank": 1,
    "metrics": { "handsPlayed": 1, "handsWon": 1 }
  }],
  "teams": [{
    "teamId": "team-red",
    "outcome": "win",
    "rank": 1,
    "metrics": {}
  }]
}
```

`participants` is required; `teams` is optional. Participant/team outcomes are
optional `win`, `loss`, `draw`, or `push`; ranks are optional positive
one-based integers and equal ranks represent a tie. Use stable IDs and exact
metric names. The trusted room host binds `gameId`, room, environment, source,
match, and roster identity; never add those identities to the event or accept
them from a client. `matchId` must match the host's active match, and each
participant must belong to its authenticated roster. `occurredAt`, when
present, is a non-negative safe-integer Unix millisecond timestamp obtained
from deterministic game context, never a wall clock.

An event ID is stable across retries of the same logical event. A correction
is a new event with a new ID and
`correction: { "action": "replace" | "retract", "supersedesEventId": "..." }`.
Replacement carries the full corrected event; retraction carries empty
`participants` and `teams`. The archive, not the SDK, verifies that the target
is the currently effective event for the same match and unit. Corrections are
append-only; a later event must not overwrite earlier history.

Limits: 256 KiB serialized UTF-8 per event, 1000 participants, 256 teams, 128
metrics per participant/team and per definition, 128 Unicode code points per
identifier/version, and at most 32 events in one lifecycle transition. A host
checkpoint or HTTP request limit may impose a lower effective payload size.
Metric values must be finite JSON numbers; financial amounts are not metrics
and must use the separate integer-minor-unit payment and ledger receipt
contract.

SDK validators can check event shape and declarations. Only the trusted host
can validate published definition versions, roster membership, event identity,
delivery, persistence, deduplication, and authorization. A result event in an
SDK response does not itself prove that it has been committed or archived.

## Determinism

The host may replay a tick after container recovery. Lifecycle implementations
must use the supplied state and context and produce the same result for the
same inputs. Do not use wall-clock time, process-global authoritative match
state, operating-system randomness, or unrecorded network responses in
authoritative simulation.

Each match receives a single authoritative 32-byte VRF root seed in
`options.randomSeed` (64 lowercase hex characters), kept confidential during an
active match and publicized on-chain after match settlement. Do not expose
`options.randomSeed`, raw PRNG keys, or `options.seed` in live `playerViews` or
mid-match broadcasts.

Authoritative simulation randomness should use `ChaCha20Rng` (20-round ChaCha20
matching `rand_chacha::ChaCha20Rng`: `"expand 32-byte k"` constants, 8
little-endian `u32` key words in `state[4..12]`, 64-bit little-endian block
counter in `state[12..14]`, 64-bit little-endian stream ID in `state[14..16]`,
and sequential little-endian `u32` word consumption). Either persist the
serializable cursor `{ seedHex, stream, wordPos }` in canonical game state or
derive independent sub-seeds per phase/tick using `deriveSeed`:

`SHA-256("randseed-chacha20-derive-v1\0" || rootSeed[0..32] || u32_le(domains.length) || concat(u32_le(domain.byteLength) || domain))`

Bounded integers (`nextBounded(bound)`) use unbiased rejection sampling over
`u32` (`limit = 2^32 - (2^32 % bound)`, returning `candidate % bound` for the
first `candidate < limit`), and `nextFloat` returns `nextU32 / 2^32` in
`[0, 1)`. Cross-language golden vectors are defined in `chacha20Vectors` and
`deriveSeedVectors` in [`fixtures/room-host-v1.json`](./fixtures/room-host-v1.json).

`options.seed` and the 32-bit `Mulberry32` generator (`ToInt32` seed conversion,
`prngVectors` in the shared fixture) remain supported for backward compatibility
with legacy releases; new games should use `options.randomSeed` and
`ChaCha20Rng`.

## Shared compatibility fixture

The fixture at [`fixtures/room-host-v1.json`](./fixtures/room-host-v1.json)
contains requests, expected responses, and PRNG vectors.
[`fixtures/game-result-event-v1.json`](./fixtures/game-result-event-v1.json)
contains a cross-language result definition and event. SDK changes must keep
all implementations compatible with both fixtures.
