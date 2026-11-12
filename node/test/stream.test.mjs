import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { createGameServer, STREAM_PROTOCOL_V1 } from "../dist/index.js";

const streamFixture = JSON.parse(
  readFileSync(new URL("../../protocol/fixtures/session-stream-v1.json", import.meta.url), "utf8"),
);

test("Node SDK advertises session-v1 headers and drives pipelined /stream sessions", async (t) => {
  const game = {
    durability: "checkpoint",
    manifest: () => ({
      title: "Stream Counter",
      version: "1.0.0",
      minPlayers: 1,
      maxPlayers: 4,
      tickRateHz: 20,
      persistentTable: true,
    }),
    init: () => ({
      state: { tick: 0, sum: 0, players: ["p1"] },
    }),
    tick: (ctx, state) => {
      const add = ctx.inputs.reduce((acc, inp) => acc + (inp.data?.delta ?? 0), 0);
      const next = {
        tick: ctx.tick,
        sum: state.sum + add + 1,
        players: state.players,
      };
      const actionResults = ctx.inputs.flatMap(input => {
        if (!input.actionId) return [];
        return Number.isSafeInteger(input.data?.delta) && input.data.delta > 0
          ? [{ actionId: input.actionId, status: "applied" }]
          : [{ actionId: input.actionId, status: "rejected", code: "missing_delta" }];
      });
      return {
        state: next,
        playerViews: Object.fromEntries(
          next.players.map((p) => [p, { tick: next.tick, sum: next.sum }]),
        ),
        broadcasts: [],
        persist: false,
        actionResults,
      };
    },
    onPlayerJoin: (playerId, state) => ({
      state: {
        ...state,
        players: state.players.includes(playerId) ? state.players : [...state.players, playerId],
      },
    }),
    onPlayerLeave: (playerId, state) => ({
      state: {
        ...state,
        players: state.players.filter((p) => p !== playerId),
      },
    }),
    isGameOver: (state) => {
      if (state.tick >= 5) {
        return {
          over: true,
          summary: { durationSeconds: state.tick / 20 },
          nextMatchPolicy: { mode: "close_room", timeoutMs: 0 },
        };
      }
      return { over: false };
    },
  };

  const server = createGameServer(game, 0);
  await new Promise((resolve) => server.once("listening", resolve));
  t.after(
    () =>
      new Promise((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      ),
  );

  const port = server.address().port;
  const manifestRes = await fetch(`http://127.0.0.1:${port}/manifest`);
  assert.equal(manifestRes.status, 200);
  assert.equal(manifestRes.headers.get("x-randseed-stream-protocol"), STREAM_PROTOCOL_V1);
  assert.equal(manifestRes.headers.get("x-randseed-stream-durability"), "checkpoint");

  const ws = new WebSocket(`ws://127.0.0.1:${port}/stream`);
  const inbox = [];
  let notify = null;
  ws.addEventListener("message", (ev) => {
    inbox.push(JSON.parse(String(ev.data)));
    if (notify) {
      const fn = notify;
      notify = null;
      fn();
    }
  });
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });
  t.after(() => ws.close());

  const nextMsg = async () => {
    while (inbox.length === 0) {
      await new Promise((resolve) => {
        notify = resolve;
      });
    }
    return inbox.shift();
  };

  ws.send(
    JSON.stringify({
      type: "hydrate",
      matchId: "m-1",
      tickRateHz: 20,
      tick: 0,
      state: { tick: 0, sum: 0, players: ["p1"] },
      replay: [
        { tick: 1, inputs: [streamFixture.actionInput] },
        { tick: 2, events: [{ type: "join", playerId: "p2" }] },
      ],
    }),
  );

  const hydrated = await nextMsg();
  assert.equal(hydrated.type, "hydrated");
  assert.equal(hydrated.tick, 2);
  assert.equal(hydrated.state.sum, 4);
  assert.deepEqual(hydrated.actionResults, [streamFixture.appliedActionResult]);

  // Send 4 pipelined steps across ticks 3..6 (tick 5 triggers gameOver, tick 6 is ignored).
  ws.send(JSON.stringify({ type: "step", tick: 4, wantState: true }));
  const skippedTick = await nextMsg();
  assert.equal(skippedTick.type, "error");
  assert.equal(skippedTick.code, "VALIDATION_ERROR");
  ws.send(JSON.stringify({ type: "step", tick: 3, inputs: "invalid" }));
  const invalidInputs = await nextMsg();
  assert.equal(invalidInputs.type, "error");
  assert.equal(invalidInputs.code, "VALIDATION_ERROR");
  ws.send(JSON.stringify({ type: "step", tick: 3, events: [{ type: "unsupported" }] }));
  const invalidEvent = await nextMsg();
  assert.equal(invalidEvent.type, "error");
  assert.equal(invalidEvent.code, "VALIDATION_ERROR");
  ws.send(JSON.stringify({
    type: "step",
    tick: 3,
    inputs: [
      {
        playerId: "p1",
        data: { delta: 3 },
        actionId: "act-3",
        actionSchemaId: "counter.action.v1",
      },
      streamFixture.rejectedActionInput,
    ],
  }));
  ws.send(JSON.stringify({ type: "step", tick: 4, wantState: true }));
  ws.send(JSON.stringify({ type: "step", tick: 5 }));
  ws.send(JSON.stringify({ type: "step", tick: 6 }));

  const r3 = await nextMsg();
  assert.equal(r3.type, "stepResult");
  assert.equal(r3.tick, 3);
  assert.equal(r3.state, undefined);
  assert.equal(r3.playerViews.p1.sum, 8);
  assert.equal(r3.playerViews.p2.sum, 8);
  assert.deepEqual(r3.actionResults, [
    { actionId: "act-3", status: "applied" },
    streamFixture.rejectedActionResult,
  ]);

  const r4 = await nextMsg();
  assert.equal(r4.type, "stepResult");
  assert.equal(r4.tick, 4);
  assert.equal(r4.state.sum, 9);

  const r5 = await nextMsg();
  assert.equal(r5.type, "stepResult");
  assert.equal(r5.tick, 5);
  assert.equal(r5.gameResult.over, true);
  assert.equal(r5.state.sum, 10);

  const r6 = await nextMsg();
  assert.equal(r6.type, "ignoredAfterGameOver");
  assert.equal(r6.tick, 6);

  await new Promise((resolve) => {
    if (ws.readyState === WebSocket.CLOSED) return resolve();
    ws.addEventListener("close", resolve, { once: true });
    ws.close();
  });
});

test("Node Session Stream rejects invalid callback results instead of coercing or forwarding them", async (t) => {
  const game = {
    manifest: () => ({
      title: "Validation Counter",
      version: "1.0.0",
      minPlayers: 1,
      maxPlayers: 2,
      tickRateHz: 20,
      persistentTable: false,
    }),
    init: () => ({ state: { value: 0 } }),
    tick: (ctx, state) => {
      if (ctx.inputs.some(input => input.data?.invalidPersist)) {
        return { state, persist: "false" };
      }
      if (ctx.inputs.some(input => input.data?.invalidGameOver)) {
        return { state: { ...state, value: 1 }, persist: false };
      }
      return { state, persist: false };
    },
    onPlayerJoin: (_playerId, state) => ({ state }),
    onPlayerLeave: (_playerId, state) => ({ state }),
    isGameOver: state => state.value === 1 ? { over: true } : { over: false },
  };
  const server = createGameServer(game, 0);
  await new Promise(resolve => server.once("listening", resolve));
  t.after(() => new Promise((resolve, reject) =>
    server.close(error => error ? reject(error) : resolve()),
  ));

  const ws = new WebSocket(`ws://127.0.0.1:${server.address().port}/stream`);
  const inbox = [];
  let notify;
  ws.addEventListener("message", event => {
    inbox.push(JSON.parse(String(event.data)));
    notify?.();
    notify = undefined;
  });
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });
  t.after(() => ws.close());
  const nextMessage = async () => {
    while (inbox.length === 0) {
      await new Promise(resolve => { notify = resolve; });
    }
    return inbox.shift();
  };
  const send = async message => {
    ws.send(JSON.stringify(message));
    return nextMessage();
  };
  const hydrate = () => send({
    type: "hydrate",
    matchId: "validation-match",
    tickRateHz: 20,
    tick: 0,
    state: { value: 0 },
  });

  assert.equal((await hydrate()).type, "hydrated");
  const invalidPersist = await send({
    type: "step",
    tick: 1,
    inputs: [{ playerId: "p1", data: { invalidPersist: true } }],
  });
  assert.equal(invalidPersist.code, "INTERNAL_ERROR");

  assert.equal((await hydrate()).type, "hydrated");
  const invalidGameOver = await send({
    type: "step",
    tick: 1,
    inputs: [{ playerId: "p1", data: { invalidGameOver: true } }],
  });
  assert.equal(invalidGameOver.code, "INTERNAL_ERROR");

  await new Promise(resolve => {
    ws.addEventListener("close", resolve, { once: true });
    ws.close();
  });
});
