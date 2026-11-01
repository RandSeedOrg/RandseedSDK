# RandSeed Game Server SDK for Go

The `roomhost` package implements the RandSeed room-host v1 HTTP/JSON
protocol. It provides typed lifecycle requests and responses, an HTTP handler,
a default-port server runner, and the deterministic Mulberry32 generator.

## Install

From a Go module, install the package with:

```sh
go get github.com/randseedorg/randseedsdk/go/roomhost@v0.1.0
```

The package import path is
`github.com/randseedorg/randseedsdk/go/roomhost`.

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

## Result Units

Declare `GameResultDefinition` in `GameManifest`, then return `ResultUnits`
from init, tick, player lifecycle, command, or game-over callbacks. A match may
emit multiple rounds, laps, stages, or a match result without requiring one
winner or a generic score. Use `ValidateGameResultUnits` for a transition and
`GameResultEvent.Validate` with the published definition to validate declared
metric IDs. The shared event and definition are in
[`game-result-event-v1.json`](../protocol/fixtures/game-result-event-v1.json).

The Go host checks event shape and limits, not the room's authenticated match,
roster, durable delivery, or archive idempotency. Keep event IDs stable across
retries and use append-only correction events.

`roomhost.NewMulberry32` provides the protocol's deterministic simulation
generator. It is not cryptographic and is not GameVRF. See the
[room-host v1 protocol](../protocol/room-host-v1.md).

## Verify

From the repository root:

```sh
go test ./...
```
