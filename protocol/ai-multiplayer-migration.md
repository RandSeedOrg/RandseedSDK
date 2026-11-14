# AI-assisted multiplayer migration

This guide helps an AI coding assistant or game developer move a browser game
from client-owned simulation to an authoritative RandSeed room server. Keep
the migration incremental: preserve rendering and input UX in the browser,
move gameplay decisions and mutable match state to the server, and use the
browser `RoomClient` as the only client-to-room boundary.

The paired executable examples are
[`authoritative-room.mjs`](../node/examples/authoritative-room.mjs) and
[`authoritative_room.rs`](../rust/randseed-game-sdk/examples/authoritative_room.rs).
Both implement the same two-player score counter, options validation,
idempotent reconnect, action receipts, terminal policy, and private player
views.

## Migration checklist

1. **Inventory the current game.** List mutable state, actions, client-side
   validation, timers, randomness, save/load behavior, player-specific data,
   round boundaries, and game-over conditions. Mark every field as public,
   private-to-one-seat, or server-only.
2. **Define versioned JSON state.** Add an explicit `version` field and store
   everything needed to continue the match: board, turn, scores, roster,
   random seed/state, and private per-player values. Do not rely on module
   globals or browser storage as authoritative state.
3. **Move decisions to callbacks.** Map initialization to `init`, one
   deterministic simulation turn to `tick`, roster changes to
   `onPlayerJoin`/`onPlayerLeave`, and terminal checks to `isGameOver`.
   Commands are for trusted host operations, not player actions.
4. **Keep the browser as a renderer and input surface.** It may animate,
   predict cosmetic effects, and collect input. It must not choose the
   authenticated player ID, authoritative tick, roster, result, or private
   recipient. Submit actions through `@randseed/game-sdk`'s browser
   `RoomClient`; never call container `/tick`, `/init`, or `/stream` from the
   browser.
5. **Add privacy and recovery cases before switching traffic.** Use two
   different canary secrets, test both recipients, test reconnect and leave,
   restart from a checkpoint, and replay the exact ordered event log.
6. **Run the migration preflight.** Run `npm run sdk:preflight` and
   `cargo test --manifest-path Cargo.toml -p randseed-game-sdk --test stream_test`.

## State and action mapping

Create a mapping table before editing:

| Existing browser game concept | Authoritative contract |
| --- | --- |
| New game / room setup | `init(config)`; validate options and initialize versioned JSON state |
| Browser state reducer / turn handler | `tick(ctx, state)`; consume only the host-sealed inputs for this tick |
| UI button or key binding | `TickInput.data`; validate its schema and return `actionResults` for supplied action IDs |
| Player joins or reconnects | `onPlayerJoin(playerId, state)`; treat an already-present ID as reconnect, not a second seat |
| Player disconnects or leaves | `onPlayerLeave(playerId, state)`; apply the game's offline rules and retain or remove the player's logical seat and private data in game state as appropriate |
| End-screen condition | `isGameOver(state)`; return a valid summary and `nextMatchPolicy` |
| Browser-specific hidden fields | Keep canonical data in server state, but project exactly one recipient-safe view per active seat |
| Save game / periodic checkpoint | Return boolean `persist`; the trusted room host owns durable snapshots and the replay journal |

Client-submitted data remains untrusted even when it has a valid action ID.
Action IDs provide correlation/idempotency metadata; they do not prove that the
action is legal or applied. A game should return `applied` or `rejected` for
each submitted action ID. Use a sanitized machine-readable code for a rejection
and avoid throwing for an ordinary illegal move.

## Event ordering, reconnects, and replay

The room host owns tick scheduling. In a Session Stream step it validates the
message and projected roster, applies lifecycle events in their array order,
calls `tick` once, then evaluates `isGameOver`. A join event may represent a
reconnect. Make the join callback idempotent: update a seat only if it is not
already present, and refresh its view baseline either way. A leave event must
refer to an active seat. Inputs in the same step are checked against the roster
after those events.

The event does not distinguish a transient disconnect from a deliberate
departure. Retain the seat and game state when the rules allow reconnect;
otherwise apply a game-defined timeout or removal. Timers may continue while
the player is offline, and there is no general SDK disconnect-wait setting.
See [Room lifecycle and recovery](./room-host-v1.md#room-lifecycle-and-recovery).

Do not use `Date.now()`, `setInterval`, process-local counters, unseeded
randomness, or unrecorded network responses to determine gameplay. Use the
supplied tick and elapsed time, `mulberry32` for deterministic non-cryptographic
randomness, and include random state in the checkpointed game state. `persist`
requests a snapshot but does not choose the host's durability tier or persist
data by itself.

On restart, the host hydrates the game with the latest checkpoint and replays
each later committed tick exactly once in order. A replay gap, duplicate tick,
or replay after game over is a fatal `REPLAY_MISMATCH`. Do not truncate a
replay or invent missing inputs. Test that replay reaches the same state and
action results as uninterrupted execution.

## State versions and match options

Put a monotonically increasing state schema version in state. During rolling
deployments, support every persisted version that can still be restored, and
convert it with a deterministic migration before the next transition. If a
version is not supported, return a clear `INVALID_STATE` error and coordinate
a host-side reset or migration; never silently replace it with an empty state.

Declare a JSON Schema in `manifest.optionsSchema` for Creator tooling, then
validate actual option values in `init`. The SDK verifies that options are a
JSON object and that the optional seed is finite, but it does not execute a
game-specific JSON Schema validator. Return `INVALID_OPTIONS` with a
creator-facing correction when options cannot initialize a match.

## Per-seat privacy

Declare `playerViewMode: "private"` when each active seat must receive a
recipient-specific projection. Every state transition must return
`playerViews` with exactly one key for each resulting active roster seat. The
host sends each map entry only to that authenticated seat. A missing or extra
key is an error; canonical state is never a fallback.

Treat view generation as an allow-list, not a copy-and-delete filter. Build
each player's object from public fields plus only that player's private fields.
Keep secrets, other players' hands, unrevealed random state, and hidden events
out of broadcasts and recipient views. Test canaries for every active seat.
The SDK can validate roster coverage and byte limits, but cannot infer whether
the game classified each field correctly.

The `/agent/player-view` endpoint is a separate host-to-container contract.
It does not replace live `playerViews`, authorize the browser, or prove that
the projection is semantically private.

## Terminal policy and errors

When `over` is true, return a summary with finite non-negative
`durationSeconds` and a supported `nextMatchPolicy` with a non-negative integer
`timeoutMs`. Supported modes are `auto_next_match`, `wait_for_ready`, and
`close_room`; browser-ready signaling for `wait_for_ready` is not part of this
protocol yet.

Use the shared
[`sdk-error-codes-v1.json`](./fixtures/sdk-error-codes-v1.json) taxonomy.
Invalid requests, options, state, roster, actions, callback failures, invalid
results, game-over policies, resource limits, tick gaps, and replay mismatches
have stable codes and retryability. Callback exceptions and panics are
sanitized; never put credentials, private state, or stack traces in an error.

## Example and preflight commands

```sh
npm ci
npm run sdk:preflight
cargo fmt --all -- --check
cargo test --manifest-path Cargo.toml -p randseed-game-sdk
```

The examples are illustrative implementations of the shared contract. They do
not implement room matchmaking, authentication, browser transport, database
persistence, archive delivery, payments, GameVRF, or spectator authorization;
those remain host/platform responsibilities.
