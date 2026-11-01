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

The server listens on port `8080` by default. Request validation errors return
HTTP `400` with `{ "error": "..." }`. Unhandled game or server errors return
HTTP `500` with a sanitized `{ "error": "..." }` response; never return stack
traces, credentials, or internal platform data. Each request body is limited
to 1 MiB; oversized bodies return HTTP `413`.

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

`title` and `version` are non-empty strings. `minPlayers` is at least one and
`maxPlayers` is no smaller than `minPlayers`. The current room platform accepts
at most 100 players and rates from 1 through 60 Hz, including fractional
rates. `persistentTable` is a boolean. `optionsSchema` may optionally declare
a JSON Schema for game options. `resultDefinition` is optional and declares
the result-unit types and metric aggregation contract described below. Keep its
version stable only while its meaning is unchanged.

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

The `state` property is required. `broadcasts` and `resultUnits` are optional.
Any transition may emit zero or more completed units: `/init`, `/tick`,
`/onPlayerJoin`, `/onPlayerLeave`, `/isGameOver`, and `/onCommand`.

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
  "state": { "count": 0 }
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
optional arrays. `persist` requests a snapshot; it does not determine whether
the host keeps the authoritative state or journals and replays committed ticks.

### Player events

`POST /onPlayerJoin` and `POST /onPlayerLeave` both accept:

```json
{ "playerId": "pairwise-a", "state": { "count": 1 } }
```

They return `{ "state": ..., "broadcasts": [] }`. `state` is required;
`broadcasts` and `resultUnits` are optional.

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
Browser-ready signaling for `wait_for_ready` is not part of this protocol yet.

Trusted host commands use `POST /onCommand` with
`{ "command": { "type": "reset", "data": {} }, "state": ... }` and return
`{ "state": ..., "broadcasts": [], "resultUnits": [] }`. This endpoint is not
a player input API.

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
