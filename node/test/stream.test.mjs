import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { createGameServer, STREAM_PROTOCOL_V1 } from "../dist/index.js";

const protocolFixture = JSON.parse(
  await readFile(new URL("../../protocol/fixtures/session-stream-v1.json", import.meta.url), "utf8"),
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
      const add = ctx.inputs.reduce((acc, inp) => acc + (inp.data?.add ?? 0), 0);
      const next = {
        tick: ctx.tick,
        sum: state.sum + add + 1,
        players: state.players,
      };
      return {
        state: next,
        playerViews: Object.fromEntries(
          next.players.map((p) => [p, { tick: next.tick, sum: next.sum }]),
        ),
        broadcasts: [],
        persist: false,
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

  await (async () => {
    const game = {
      manifest: () => ({
        title: "Private Counter",
        version: "1.0.0",
        minPlayers: 2,
        maxPlayers: 2,
        tickRateHz: 10,
        persistentTable: false,
        playerViewMode: "private",
      }),
      init: () => ({ state: {} }),
      tick: (ctx, state) => ({
        state: { ...state, tick: ctx.tick },
        persist: false,
        playerViews: {
          p1: { ownSecret: "P1_CANARY", tick: ctx.tick },
          p2: { ownSecret: "P2_CANARY", tick: ctx.tick },
        },
        actionResults: ctx.inputs
          .filter(input => input.actionId)
          .map(input => ({ actionId: input.actionId, status: "applied" })),
      }),
      onPlayerJoin: (_playerId, state) => ({ state }),
      onPlayerLeave: (_playerId, state) => ({ state }),
      isGameOver: () => ({ over: false }),
    };

    const server = createGameServer(game, 0);
    await new Promise(resolve => server.once("listening", resolve));
    t.after(() => new Promise((resolve, reject) => {
      server.close(error => error ? reject(error) : resolve());
    }));
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
    const next = async () => {
      while (inbox.length === 0) await new Promise(resolve => { notify = resolve; });
      return inbox.shift();
    };
    const send = async payload => {
      ws.send(JSON.stringify(payload));
      return next();
    };

    const hydrated = await send({
      type: "hydrate",
      matchId: "private-1",
      tickRateHz: 10,
      tick: 0,
      state: { secrets: { p1: "P1_CANARY", p2: "P2_CANARY" } },
      players: ["p1", "p2"],
      playerViews: {
        p1: { ownSecret: "P1_CANARY" },
        p2: { ownSecret: "P2_CANARY" },
      },
    });
    assert.equal(hydrated.type, "hydrated");
    assert.equal(hydrated.state, undefined);
    assert.equal(hydrated.playerViews.p1.ownSecret, "P1_CANARY");
    assert.equal(JSON.stringify(hydrated.playerViews.p1).includes("P2_CANARY"), false);

    const missingInputData = await send({
      type: "step",
      tick: 1,
      inputs: [{ playerId: "p1" }],
    });
    assert.equal(missingInputData.code, "INVALID_ACTION");

    const wrongRoster = await send({
      type: "step",
      tick: 1,
      inputs: [{ playerId: "p3", data: {} }],
    });
    assert.equal(wrongRoster.code, "WRONG_ROSTER");
    assert.equal(wrongRoster.retryable, false);
    const sequenceMismatch = await send({ type: "step", tick: 2 });
    assert.equal(sequenceMismatch.code, protocolFixture.sequenceMismatchCode);

    const stepped = await send({
      type: "step",
      tick: 1,
      inputs: [{
        playerId: "p1",
        data: {},
        actionId: "act-1",
        actionSchemaId: "counter.action.v1",
      }],
    });
    assert.equal(stepped.type, "stepResult");
    assert.deepEqual(stepped.actionResults, [{ actionId: "act-1", status: "applied" }]);
    assert.equal(JSON.stringify(stepped.playerViews.p1).includes("P2_CANARY"), false);

    const incompleteViews = await send({
      type: "hydrate",
      matchId: "private-2",
      tickRateHz: 10,
      tick: 0,
      state: {},
      players: ["p1", "p2"],
      playerViews: { p1: { ownSecret: "P1_CANARY" } },
    });
    assert.equal(incompleteViews.code, "INVALID_RESULT");
    assert.equal(incompleteViews.retryable, false);
    const notHydrated = await send({ type: "step", tick: 1 });
    assert.equal(notHydrated.code, "INVALID_STREAM_MESSAGE");
    await new Promise(resolve => {
      if (ws.readyState === WebSocket.CLOSED) return resolve();
      ws.addEventListener("close", resolve, { once: true });
      ws.close();
    });
  })();
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
      players: ["p1"],
      replay: [
        { tick: 1, inputs: [{ playerId: "p1", data: { add: 2 } }] },
        { tick: 2, events: [{ type: "join", playerId: "p2" }] },
      ],
    }),
  );

  const hydrated = await nextMsg();
  assert.equal(hydrated.type, "hydrated");
  assert.equal(hydrated.tick, 2);
  assert.equal(hydrated.state.sum, 4);

  // Send 4 pipelined steps across ticks 3..6 (tick 5 triggers gameOver, tick 6 is ignored).
  ws.send(JSON.stringify({ type: "step", tick: 3, inputs: [{ playerId: "p1", data: { add: 3 } }] }));
  ws.send(JSON.stringify({ type: "step", tick: 4, wantState: true }));
  ws.send(JSON.stringify({ type: "step", tick: 5 }));
  ws.send(JSON.stringify({ type: "step", tick: 6 }));

  const r3 = await nextMsg();
  assert.equal(r3.type, "stepResult");
  assert.equal(r3.tick, 3);
  assert.equal(r3.state, undefined);
  assert.equal(r3.playerViews.p1.sum, 8);
  assert.equal(r3.playerViews.p2.sum, 8);

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
