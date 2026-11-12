# Session Stream v1

Session Stream v1 is the stateful WebSocket contract between the trusted room
host and one game-server container. It is an alternative to repeated stateless
HTTP lifecycle calls; it does not replace the room host's authoritative journal,
authentication, roster, or browser gateway.

The server advertises `X-RandSeed-Stream-Protocol: session-v1` on `/manifest`
and the WebSocket upgrade. Frames are UTF-8 JSON text messages. Messages are
processed in FIFO order. Each frame is limited to 1 MiB. Unknown message and
event fields are rejected.

## Hydration and recovery

The host starts or recovers one in-memory match with:

```json
{
  "type": "hydrate",
  "matchId": "match-42",
  "tickRateHz": 10,
  "tick": 100,
  "state": { "version": 1, "tick": 100, "players": ["p1", "p2"] },
  "players": ["p1", "p2"],
  "playerViews": {
    "p1": { "tick": 100, "privateHand": ["P1_CARD"] },
    "p2": { "tick": 100, "privateHand": ["P2_CARD"] }
  },
  "replay": [
    {
      "tick": 101,
      "inputs": [{ "playerId": "p1", "data": { "move": "draw" } }],
      "events": []
    }
  ],
  "resyncPlayers": []
}
```

`matchId`, `tickRateHz`, `tick`, `state`, and the active `players` roster are
required. The roster must be unique and non-empty. A private-view manifest
requires exactly one checkpoint view for every active seat when replay is
empty. Each view is at most 64 KiB; the view map is at most 1 MiB. Never put
another player's private fields in a recipient's view.

Replay entries are applied in ascending, contiguous tick order, starting at
`tick + 1`. A replay after the checkpoint has already reached game over, a
duplicate tick, or a gap returns `REPLAY_MISMATCH`. Replay is deterministic:
the host must replay the exact committed inputs and lifecycle events in their
original order. A restart is a new WebSocket session; hydrate it from the
latest durable checkpoint and replay later committed steps. Process memory is
not durable storage.

## Step ordering

For each `step`, the server validates the tick, roster, inputs, and events
before invoking game logic. It then invokes lifecycle events in array order
(`join`, `leave`, or trusted `command`), invokes `tick` once, and evaluates
`isGameOver` on the resulting state. Join is also used for reconnect; game
callbacks should be idempotent for a player already in the roster. A leave must
name an active player. Tick inputs may only name players in the resulting
active roster.

```json
{
  "type": "step",
  "tick": 102,
  "inputs": [{
    "playerId": "p2",
    "data": { "move": "play", "card": "P2_CARD" },
    "actionId": "action-102",
    "actionSchemaId": "cards.action.v1"
  }],
  "events": [{ "type": "join", "playerId": "p2" }],
  "resyncPlayers": [],
  "wantState": false
}
```

`tick` must be the current tick plus one and remain a safe integer. `inputs`,
`events`, and `resyncPlayers` default to empty arrays; if supplied, each must
have the documented shape. An invalid action is normally a game-level
`actionResults` entry with status `rejected`, not an exception or an ignored
input. Result IDs must be unique and refer to action IDs submitted on that
tick.

## Responses

Successful hydration returns `type: "hydrated"` with the current tick,
`gameResult`, and the latest projected views. State is included when replay was
applied. Each successful tick returns `type: "stepResult"`:

```json
{
  "type": "stepResult",
  "tick": 102,
  "playerViews": {
    "p1": { "tick": 102, "privateHand": ["P1_CARD"] },
    "p2": { "tick": 102, "privateHand": ["P2_CARD"] }
  },
  "broadcasts": [],
  "persist": false,
  "actionResults": [{ "actionId": "action-102", "status": "rejected", "code": "INVALID_MOVE" }],
  "gameResult": { "over": false }
}
```

The state is included when `wantState` is true, the step contains lifecycle
events, `persist` is true, results were emitted, or the game ends. `persist`
requests that the host checkpoint; it does not itself persist anything.
`actionResults` are forwarded in both `stepResult` and the aggregate hydration
response when replay emits them. A tick after game over may return
`ignoredAfterGameOver`.

Whenever a transition returns `playerViews`, its keys must exactly match the
active roster after lifecycle events. A private-view manifest must return a
complete map on every state transition. The host delivers only
`playerViews[playerId]`; it must never fall back to canonical state for a
missing view. Public-state games may omit the map.

## Errors and compatibility fixture

Errors have a stable code, retryability flag, creator-facing guidance, and a
sanitized message:

```json
{
  "type": "error",
  "tick": 103,
  "code": "TICK_SEQUENCE_MISMATCH",
  "retryable": false,
  "creatorAction": "Send the next contiguous tick after the session's current tick.",
  "error": "Expected tick 102."
}
```

See [`sdk-error-codes-v1.json`](./fixtures/sdk-error-codes-v1.json) for the
cross-language taxonomy and
[`session-stream-v1.json`](./fixtures/session-stream-v1.json) for canonical
Node/Rust request and response vectors. Malformed messages, invalid actions,
wrong-roster actions, callback failures, invalid outputs, game-over policies,
resource-limit violations, tick gaps, and replay mismatches are distinct
failures. Callback details, stack traces, credentials, and internal state are
never included in error messages.
