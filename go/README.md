# RandSeed Game Server SDK for Go

The `roomhost` package implements the RandSeed room-host v1 HTTP/JSON
protocol. It provides typed lifecycle requests and responses, an HTTP handler,
a default-port server runner, and deterministic `ChaCha20Rng` / `DeriveSeed`
(plus legacy `Mulberry32`) generators.

## Install

From a Go module, install the package with:

```sh
go get github.com/randseedorg/randseedsdk/go/roomhost@v0.1.0
```

The package import path is
`github.com/randseedorg/randseedsdk/go/roomhost`. This released `v0.1.0` does
not include the private-view additions described below. They remain pending
review and are not usable from this release until a later module version is
published; the checkout tests validate the unreleased source only.

## Run the counter example

From the repository root:

```sh
go run ./go/examples/counter
```

The example listens on port `8080`. It demonstrates a complete `GameServer`
implementation, including deterministic tick processing, player lifecycle
events, a trusted reset command, and match completion.

## Implement a game

Implement `roomhost.GameServer`, then start it with
`roomhost.ListenAndServe(yourGame)`. Use `roomhost.NewHandler` if the game
needs to be hosted by an existing `net/http` server. See
[`examples/counter/main.go`](./examples/counter/main.go) for a complete
implementation.

The room host owns scheduling, persistence, player identity, and recovery.
Derive authoritative results only from the supplied state and request context;
do not use wall-clock time, process-global match state, or unrecorded external
inputs. Request bodies are limited to 1 MiB, and protocol sequence/tick
numbers use safe-integer bounds.

In `GameManifest`, configure integer limits satisfying
`1 <= MinPlayers <= MaxPlayers <= 100`. `MinPlayers` is the minimum number of
assigned players required before the RandSeed platform can start a match.
Zero-player starts are unsupported; a solo game uses `MinPlayers: 1`.
`MaxPlayers` must be at least `MinPlayers` and is the room's seat-capacity
limit, not a lobby fill target. For
example, `MinPlayers: 2, MaxPlayers: 4`
may start with two assigned players and leave two seats open. Set both values
to the same number when the game requires an exact roster size. After startup,
additional seats are admitted only while the game reports `joinable: true` and
the roster remains below `MaxPlayers`.

## Private player views (room-host v1)

Go implements the general HTTP private-view contract without changing the existing `GameServer` interface or public struct layouts. It does not add Go `/stream` or Agent endpoints. For a private game, implement the opt-in `GameServerV1` interface, return `GameManifestV1` from `ManifestV1`, and serve it with `NewHandlerV1` or `ListenAndServeV1`. Embed the unchanged `GameManifest` and set `PlayerViewMode: roomhost.PlayerViewPrivate`; this is v1 even when `PlayerViewSchemaVersion` is omitted. Explicit numeric `1`, `1.0`, and `1e0` are accepted only with private mode and normalized to integer 1. `null`, an unsupported version, a version without private mode, `playerViewMode: null`, and a `public` mode value are rejected. V1 roster and projection fields are available through separate `TickRequestV1`, `PlayerEventRequestV1`, `CommandRequestV1`, and V1 result types; old callers keep their original field layouts.

Public wire manifest:

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

A valid `OptionsSchema` is a JSON Schema object or boolean; `null`, arrays,
strings, and numbers are rejected. Set an object schema as raw JSON, for example:

```go
OptionsSchema: json.RawMessage(`{"type":"object","properties":{"mode":{"type":"string","enum":["casual","ranked"]}},"additionalProperties":false}`),
```

The SDK checks the wire shape but does not execute the schema against match
options.

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

In private mode, init uses `MatchConfig.Players`; tick and command requests
must include `Players`; join and leave requests include `Players` for the
resulting roster. `GameServerV1` callbacks receive these fields through their V1 request wrappers. Return `PlayerViews map[string]json.RawMessage` from every V1 state transition. The handler requires exact roster-key coverage,
valid JSON, at most 64 KiB per serialized view and at most 1 MiB for the full
serialized map. Missing, malformed, oversized, or extra views fail closed with
an error response. There is no fallback to canonical `State`.

The `State` response remains private authoritative simulation state for the
trusted room host. Each `PlayerViews[playerID]` value must exclude secrets belonging to all other
seats and hidden random state. The room host, not a browser, chooses the
entry for the authenticated recipient; never expose canonical state or another
seat view as a fallback. SDK checks cannot infer whether a game-authored view
contains hidden data.

For deployment identity, `GameIDProvider.GameID()` or
`HandlerOptions.GameID` must use the platform-assigned RandSeed Game ID, matching
`^[A-Za-z0-9_-]{1,64}$`. The counter example implements `GameIDProvider` by
reading `RANDSEED_GAME_ID`; set that variable to the actual platform ID when
running it. The SDK invents no default ID, and `/health` returns `503` if the
ID is missing or invalid. Do not substitute a repository, package, or runtime
name. `ListenAndServe` binds port `8080`; `NewHandler` can be mounted on an
existing `http.Server` when the runtime supplies another port. Check both
endpoints on the bound address:

```sh
curl --fail http://127.0.0.1:8080/health
curl --fail http://127.0.0.1:8080/manifest
```

`/health` proves the process is serving with a valid platform Game ID;
`/manifest` proves the returned configuration passes SDK validation. These are
different checks.

As of 2026-11-17, this cross-SDK private-view source change is pending
independent review and is not a Go module release. Gamecreator PR #340 remains
open and is not deployed. Verify the exact installed SDK release and target
platform rollout before enabling private routing.

## Result Units

Declare `GameResultDefinition` in `GameManifest` (or its `GameManifestV1` wrapper), then return `ResultUnits`
from init, tick, player lifecycle, command, or game-over callbacks. A match may
emit multiple rounds, laps, stages, or a match result without requiring one
winner or a generic score. Use `ValidateGameResultUnits` for a transition and
`GameResultEvent.Validate` with the published definition to validate declared
metric IDs. The shared event and definition are in
[`game-result-event-v1.json`](../protocol/fixtures/game-result-event-v1.json).

The Go host checks event shape and limits, not the room's authenticated match,
roster, durable delivery, or archive idempotency. Keep event IDs stable across
retries and use append-only correction events.

`roomhost.NewChaCha20RngFromHex`, `roomhost.NewChaCha20RngDerived`, and
`roomhost.DeriveSeed` provide deterministic 20-round ChaCha20 simulation
randomness seeded from the host-injected 256-bit VRF root seed
(`options.randomSeed`), with validated JSON-safe `ChaCha20State` cursor
snapshots (`State()` / `NewChaCha20RngFromState`). `roomhost.NewMulberry32`
remains available for legacy `options.seed` compatibility. See the
[room-host v1 protocol](../protocol/room-host-v1.md).

## Verify

From the repository root:

```sh
go test ./...
```
