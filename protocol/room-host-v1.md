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
a JSON Schema for game options. `resultDefinition` is optional and declares
the result-unit types and metric aggregation contract described below. Keep its
version stable only while its meaning is unchanged.

On the RandSeed platform, `minPlayers` is the minimum number of assigned
players required before a match may start (at least one; zero-player starts
are unsupported). It counts assigned players, not connected WebSockets.
A match can begin at this threshold while some seats remain
empty. `maxPlayers` is the maximum number of player seats, not a target the
lobby must fill. For example, `minPlayers: 2` and `maxPlayers: 4` allows the
platform to start with two assigned players and leaves capacity for up to two
more. A solo game uses `minPlayers: 1`. If a game requires an exact roster, set
both values to that roster size. After startup, additional seats can be
admitted only while the game reports `joinable: true` and the roster remains
below `maxPlayers`. Node.js, Rust, and .NET SDKs enforce manifest roster bounds
at `/init`; Go and C++ currently validate only generic roster constraints.
Gamecreator also enforces manifest bounds platform-side. Matchmaking and startup
timing belong to the host/platform. `wait_for_ready` is a separate
post-match policy and uses the completed match's full roster to decide when to
start the next match.

`playerViewMode` may be `"private"` when every active seat must receive a
recipient-specific projection. In this mode, `POST /init` must return exactly
one `playerViews` entry for every configured player. `POST /tick` and
`POST /onCommand` requests must include the active `players` roster whenever
the game returns per-player views; private-view games must always include it.
Player-event requests may include `players` for the resulting roster (after a
join or leave). Whenever `playerViews` is returned, the request must provide
that resulting roster so the SDK can verify an exact key match. Private mode
rejects missing views; an absent or extra key is an error, never a fallback to
canonical `state`. Each view is capped at 64 KiB and the complete UTF-8 JSON
serialization of the map (including player ID keys and JSON syntax overhead)
at 1 MiB.

### Match configuration

```json
{
  "matchId": "match-1",
  "gameId": "room-counter",
  "players": ["pairwise-a", "pairwise-b"],
  "matchSeq": 1,
  "options": { "seed": 4294967295, "mode": "ranked" },
  "carryover": null,
  "tickRateHz": 5
}
```

`players` is a non-empty array of unique player IDs supplied by the host.
`matchSeq` is a positive safe integer. `tickRateHz`, when present, is a finite
number between 1 and 60; fractional rates are valid and `null` is not valid.
`options` may be omitted; when present it must be an object, not `null`.
`options.seed`, when present, is a finite JSON number. Other `options` entries are game-owned
JSON configuration. `carryover` is opaque JSON and may be any JSON value.

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
lease epoch, source observation sequence, expected tick, action schema, and JSON
payload. Retries with the same ID and identical payload are idempotent; reuse
with a different payload is a conflict. Receipts distinguish `queued`,
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
`commands` is optional and defaults to an empty array. `tick` is a non-negative
safe integer. `elapsedSeconds` is finite, non-negative cumulative match time
derived from the host tick, not wall-clock time. Do not add successive
`elapsedSeconds` values to calculate total duration.

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
state: for example, retain a seat and hand for a bounded wait, or apply a
game-defined timeout, automatic action, or forfeiture. There is currently no
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

`options.seed` is consumed by the deterministic Mulberry32 generator. Seed
conversion follows JavaScript signed 32-bit conversion (`ToInt32`) before the
Mulberry32 steps. `nextU32` returns the unsigned 32-bit output; `nextFloat`
returns `nextU32 / 2^32` in `[0, 1)`. The shared fixture defines cross-language
test vectors. This PRNG is deterministic, not cryptographic, and is not GameVRF.

## Shared compatibility fixture

The fixture at [`fixtures/room-host-v1.json`](./fixtures/room-host-v1.json)
contains requests, expected responses, and PRNG vectors.
[`fixtures/game-result-event-v1.json`](./fixtures/game-result-event-v1.json)
contains a cross-language result definition and event. SDK changes must keep
all implementations compatible with both fixtures.
