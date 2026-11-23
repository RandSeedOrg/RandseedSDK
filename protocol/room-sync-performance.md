# Room connection and action timing diagnostics

This guide describes developer-initiated measurements for investigating slow
room connection, sit/join, and deal/start experiences. Measurements are
diagnostic observations, not an availability or performance guarantee.

## Keep the timing layers separate

Use separate measurements and label exactly what each one includes:

1. **Room connection/startup observation:** elapsed time from the developer
   starting the normal browser connection flow until the existing UI reports
   the room as connected/ready. This may include browser, network, matchmaking,
   host startup, and game initialization; it does not isolate any one layer.
2. **Developer action to authoritative confirmation:** elapsed time from a
   developer submitting an action through the game's normal UI until the
   existing client observes an authoritative game result for it. This is an
   end-to-end observation only when the game/client already exposes a reliable
   action-specific confirmation. A click, local animation, WebSocket send, or
   generic transport acknowledgement is not proof that the action was accepted
   or applied.
3. **UI update/render observation:** measure the client-side interval from
   receipt/handling of the relevant update to the application's render or
   next-frame observation. A browser frame callback is not proof that pixels
   have been presented to the display.
4. **In-process game callback time:** the Node helper below times only a
   callback invoked in the local process. It is not a network request, room
   host timing, or end-to-end action timing.
5. **Room RTT:** requires explicit opt-in support in the deployed browser SDK
   and game release. This Server SDK helper does not provide RTT. Do not infer
   that a new RTT probe is available merely because this document or a
   development branch mentions it; verify the browser SDK version, published
   package, built game bundle, and target environment first.

If authoritative confirmation is pending or unavailable, report that timing as
**pending** or **unknown**. Do not substitute click-to-send, a generic
acknowledgement, or a guessed timeout as action latency.

## Local Node callback benchmark

The SDK API is explicitly invoked by the developer; it does not automatically
run in room callbacks, log samples, or send metrics. Each callback invocation is
awaited before the next begins. Its return value is discarded, and the result
contains only the sample count and nearest-rank p50/p95 durations in
milliseconds (`rank = ceil(percentile * sampleCount)`, using one-based rank).
Sample counts must be integers from 1 through 100. Callback errors propagate
unchanged and stop the benchmark.

Use the actual lifecycle callback only with fixed, isolated fixtures and only
when it is deterministic and has no external side effects. For example:

```ts
import { runSerialBenchmark, type GameServer } from "@randseed/server-sdk";

type FixtureState = Readonly<{ score: number }>;
const game: Pick<GameServer<FixtureState>, "tick"> = {
  tick: (_context, state) => ({
    state: { score: state.score + 1 },
    persist: false,
  }),
};

const fixtureContext = {
  tick: 7,
  elapsedSeconds: 0.05,
  inputs: [],
};
Object.freeze(fixtureContext.inputs);
Object.freeze(fixtureContext);
const fixtureState = Object.freeze({ score: 41 });

const baseline = await runSerialBenchmark(
  () => game.tick(fixtureContext, fixtureState),
  { sampleCount: 30 },
);
// baseline is { sampleCount, p50Ms, p95Ms }; decide locally how to retain it.
```

This example repeatedly evaluates the same deterministic fixture transition;
it does not mutate an actual room or advance authoritative state. Do not use
callbacks that submit actions, write storage, emit results, advance a shared
random generator, access the network, mutate shared fixtures, or otherwise
produce side effects. Do not feed benchmark results into game logic, replay, or
authoritative decisions. If a callback depends on mutable setup, construct an
isolated fixture per invocation and account for that setup in the measured
scope. The helper has no direct access to the platform host or container.

## Local Rust callback benchmark

The Rust SDK exposes `run_serial_benchmark` for the same developer-initiated,
in-process use. It accepts an async callback and a sample count from 1 through
100, awaits each invocation before starting the next, discards successful
callback values, and returns `{ sample_count, p50_ms, p95_ms }` in milliseconds.
It uses `std::time::Instant` and the same one-based nearest-rank calculation as
the Node helper. A callback failure stops sampling and is retained unchanged in
`SerialBenchmarkError::Callback`; an invalid count is distinguished by
`SerialBenchmarkError::InvalidSampleCount`.

For example, benchmark a deterministic `GameServer::tick` against repeated,
immutable fixture inputs:

```rust
use randseed_game_sdk::{
    run_serial_benchmark, GameServer, SerialBenchmarkError, SerialBenchmarkResult, SdkError,
    TickContext,
};
use serde_json::json;

async fn benchmark_fixture_tick<G: GameServer>(
    game: &G,
) -> Result<SerialBenchmarkResult, SerialBenchmarkError<SdkError>> {
    let context = TickContext {
        tick: 7,
        inputs: Vec::new(),
        commands: Vec::new(),
        elapsed_seconds: 0.05,
    };
    let fixture_state = json!({ "score": 41 });

    run_serial_benchmark(
        || game.tick(context.clone(), fixture_state.clone()),
        30,
    )
    .await
}
```

Only benchmark a deterministic game instance with isolated local fixtures and
no external side effects. This times local callback execution; it does not
mutate a real room, advance authoritative state, access the network, or measure
room RTT/action latency. The Rust API is a local SDK-source addition; verify
that the Rust package version used by a game contains it. It does not imply
rollout of a browser SDK, Sandbox panel, or host feature. Neither language
helper automatically runs callbacks, logs samples, or sends telemetry.

## Manual browser observations

Use the application's ordinary, authorized room flow; do not call game-server
lifecycle endpoints from a browser or create a separate transport path. The
examples below use only the browser's standard Performance API and the game's
existing UI observations. They intentionally do not name a `RoomClient`
connect/action method: use only the API and confirmation signal documented for
the browser SDK version actually installed by your game.

In the browser developer console, mark the moments around the normal connect
flow and use the UI's existing connected/ready observation for the end mark:

```js
performance.mark("room-connect-start");
// Start the room using the game's existing Connect control.
// When the existing UI reports connected/ready:
performance.mark("room-connect-observed");
performance.measure(
  "room-connect-observed",
  "room-connect-start",
  "room-connect-observed",
);
```

The interval is a manual client-observed duration, not a pure network
measurement; console interaction can add operator delay. For better precision,
place the marks in existing app callbacks rather than adding persistent
instrumentation. If a version exposes an opt-in room RTT facility, record it
as a separate value and verify its timeout, late-pong, disconnect, and overlap
semantics; RTT is not action latency.

For action timing, place the start mark immediately before using the existing
game action control, then place the end mark only when the existing client
observes the authoritative action-specific result (if supported):

```js
performance.mark("action-submit-observed");
// Submit one developer test action through the normal game UI.
// If/when an existing authoritative confirmation is observed:
performance.mark("action-confirmed-observed");
performance.measure(
  "action-to-confirmation-observed",
  "action-submit-observed",
  "action-confirmed-observed",
);
```

These are manual timing marks, not SDK calls. If the deployed client has no
reliable action-specific confirmation, leave the action timing pending/unknown.
Never repeat live room mutations merely to create a sample set.

For a rough UI scheduling measurement, mark the existing update handling point
and measure to the next animation frame:

```js
performance.mark("ui-update-handled");
requestAnimationFrame(() => {
  performance.mark("ui-next-frame");
  performance.measure(
    "ui-update-to-next-frame",
    "ui-update-handled",
    "ui-next-frame",
  );
});
```

This is only an update-to-next-frame observation, not a guarantee of completed
render or displayed pixels. Prefer an existing framework profiler when it
provides a suitable measurement. Avoid adding persistent instrumentation just
for an exploratory comparison.

## Sandbox and Private Preview comparison

1. Begin in **Sandbox** with an isolated test game/fixture and a small number
   of manually initiated observations. Use local callback benchmarks only for
   pure deterministic work; use the browser flow for the room experience.
2. If a live-environment comparison is needed, use **Private Preview** and the
   same immutable game release/version as the Sandbox comparison where the
   platform supports that pairing. Confirm the installed browser SDK package
   and the built game bundle; a source change or dependency update does not
   upgrade an already deployed game release.
3. Record only non-sensitive comparison context: environment name, game/runtime
   release and browser SDK version, browser/device class, approximate time,
   connection conditions, measurement layer, and sample count. Compare like
   with like and report p50/p95 separately by layer. Differences in host load,
   network, region, browser, release, or environment configuration can explain
   differences; a small sample does not establish a cause.
4. Production measurements require separate explicit authorization. If
   authorized, keep them lightweight and manually initiated; do not run
   repeated callback benchmarks against live mutations or add automatic
   telemetry.

Do not capture or include user/player IDs, room identifiers, room URLs or
tokens, game state, hands, action payloads, credentials, or sensitive logs.
Keep only aggregate timings and the minimum non-sensitive context needed to
compare measurements. Never use private game data as a benchmark fixture.

## Reporting a baseline

Report the measurement layer, environment, exact game/runtime and browser SDK
versions, sample count, p50, p95, and any pending/unknown outcome. Include the
nearest-rank definition for percentile results and state whether timings are
manual client observations, local callback durations, or an explicitly
supported browser RTT measurement. Do not combine these into a single
"latency" number. A sandbox/preview baseline is not a production performance
claim; rerun after changes in the deployed browser bundle, runtime, or host
environment that affect the measured path.
