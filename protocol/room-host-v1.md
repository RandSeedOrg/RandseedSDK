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
  "persistentTable": false
}
```

`title` and `version` are non-empty strings. `minPlayers` is at least one and
`maxPlayers` is no smaller than `minPlayers`. The current room platform accepts
at most 100 players and rates from 1 through 60 Hz, including fractional
rates. `persistentTable` is a boolean. `optionsSchema` may optionally declare
a JSON Schema for game options.

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
`matchSeq` is a positive safe integer. `tickRateHz`, when present, is finite
and between 1 and 60; fractional rates are valid.
`options.seed` is a finite JSON number. Other `options` entries are game-owned
JSON configuration. `carryover` is opaque JSON and may be any JSON value.
`tickRateHz` is optional and, when present, is between 1 and 60.

### Initialization

Request: `MatchConfig`.

Response:

```json
{ "state": { "count": 0 }, "broadcasts": [] }
```

The `state` property is required. `broadcasts` is an optional JSON array.

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

`state` and boolean `persist` are required. `broadcasts` is an optional JSON
array.

### Player events

`POST /onPlayerJoin` and `POST /onPlayerLeave` both accept:

```json
{ "playerId": "pairwise-a", "state": { "count": 1 } }
```

They return `{ "state": ..., "broadcasts": [] }`. `state` is required;
`broadcasts` is optional.

### Game over and trusted commands

`POST /isGameOver` accepts `{ "state": ... }` and returns:

```json
{
  "over": true,
  "summary": { "durationSeconds": 0.2 },
  "nextMatchPolicy": { "mode": "close_room", "timeoutMs": 0 }
}
```

When `over` is true, `summary` and `nextMatchPolicy` are required. The summary
must contain a finite, non-negative `durationSeconds`. The policy must contain
a supported `mode` and a non-negative integer `timeoutMs` (include it even
when the timeout is zero). Supported policy modes are `auto_next_match`,
`wait_for_ready`, and `close_room`.
Browser-ready signaling for `wait_for_ready` is not part of this protocol yet.

Trusted host commands use `POST /onCommand` with
`{ "command": { "type": "reset", "data": {} }, "state": ... }` and return
`{ "state": ..., "broadcasts": [] }`. This endpoint is not a player input API.

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
contains requests, expected responses, and PRNG vectors. SDK changes must keep
all implementations compatible with this fixture.
