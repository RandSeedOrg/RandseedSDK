import assert from "node:assert/strict";
import { createGameServer } from "../node/dist/index.js";
import { game } from "../node/examples/authoritative-room.mjs";

async function post(baseUrl, path, body) {
  const response = await fetch(`${baseUrl}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return { status: response.status, body: await response.json() };
}

async function connectStream(port) {
  const socket = new WebSocket(`ws://127.0.0.1:${port}/stream`);
  const inbox = [];
  let wake;
  socket.addEventListener("message", event => {
    inbox.push(JSON.parse(String(event.data)));
    wake?.();
    wake = undefined;
  });
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  return {
    socket,
    async send(message) {
      socket.send(JSON.stringify(message));
      while (inbox.length === 0) await new Promise(resolve => { wake = resolve; });
      return inbox.shift();
    },
    async close() {
      if (socket.readyState === WebSocket.CLOSED) return;
      await new Promise(resolve => {
        socket.addEventListener("close", resolve, { once: true });
        socket.close();
      });
    },
  };
}

const server = createGameServer(game, 0, { gameId: "migration-preflight" });
await new Promise(resolve => server.once("listening", resolve));
const address = server.address();
assert.ok(address && typeof address === "object");
const baseUrl = `http://127.0.0.1:${address.port}`;
const sockets = [];

try {
  const manifestResponse = await fetch(`${baseUrl}/manifest`);
  const manifest = await manifestResponse.json();
  assert.equal(manifestResponse.status, 200);
  assert.equal(manifest.playerViewMode, "private");

  const config = {
    matchId: "preflight-match",
    gameId: "migration-preflight",
    players: ["p1", "p2"],
    matchSeq: 1,
    options: { targetScore: 3 },
  };
  const { status: initStatus, body: init } = await post(baseUrl, "/init", config);
  assert.equal(initStatus, 200);
  assert.deepEqual(Object.keys(init.playerViews).sort(), ["p1", "p2"]);
  assert.equal(JSON.stringify(init.playerViews.p1).includes("p2"), false);

  const invalidOptions = await post(baseUrl, "/init", {
    ...config,
    matchId: "invalid-options",
    options: { targetScore: 0 },
  });
  assert.equal(invalidOptions.status, 400);
  assert.equal(invalidOptions.body.code, "INVALID_OPTIONS");

  const reconnect = await post(baseUrl, "/onPlayerJoin", {
    playerId: "p2",
    state: init.state,
    players: ["p1", "p2"],
  });
  assert.equal(reconnect.status, 200);
  assert.deepEqual(reconnect.body.state.players, ["p1", "p2"]);

  const leave = await post(baseUrl, "/onPlayerLeave", {
    playerId: "p2",
    state: reconnect.body.state,
    players: ["p1"],
  });
  assert.equal(leave.status, 200);
  assert.deepEqual(Object.keys(leave.body.playerViews), ["p1"]);

  const rejoin = await post(baseUrl, "/onPlayerJoin", {
    playerId: "p2",
    state: leave.body.state,
    players: ["p1", "p2"],
  });
  assert.equal(rejoin.status, 200);

  const validTick = await post(baseUrl, "/tick", {
    ctx: {
      tick: 1,
      elapsedSeconds: 0.1,
      inputs: [{
        playerId: "p1",
        data: { delta: 1 },
        actionId: "http-action-1",
        actionSchemaId: "counter.action.v1",
      }],
    },
    state: rejoin.body.state,
    players: ["p1", "p2"],
  });
  assert.equal(validTick.status, 200);
  assert.equal(validTick.body.state.count, 1);
  assert.equal(validTick.body.actionResults[0].status, "applied");

  const invalidRoster = await post(baseUrl, "/tick", {
    ctx: {
      tick: 2,
      elapsedSeconds: 0.2,
      inputs: [{ playerId: "p3", data: { delta: 1 } }],
    },
    state: validTick.body.state,
    players: ["p1", "p2"],
  });
  assert.equal(invalidRoster.status, 400);
  assert.equal(invalidRoster.body.code, "WRONG_ROSTER");

  const rejectedAction = {
    playerId: "p2",
    data: { delta: "not-an-integer" },
    actionId: "http-action-2",
    actionSchemaId: "counter.action.v1",
  };
  const rejectedTick = await post(baseUrl, "/tick", {
    ctx: { tick: 2, elapsedSeconds: 0.2, inputs: [rejectedAction] },
    state: validTick.body.state,
    players: ["p1", "p2"],
  });
  assert.equal(rejectedTick.body.state.count, 1);
  assert.equal(rejectedTick.body.actionResults[0].status, "rejected");

  const gameOverState = await post(baseUrl, "/tick", {
    ctx: {
      tick: 3,
      elapsedSeconds: 0.3,
      inputs: [{
        playerId: "p1",
        data: { delta: 2 },
        actionId: "http-action-3",
        actionSchemaId: "counter.action.v1",
      }],
    },
    state: rejectedTick.body.state,
    players: ["p1", "p2"],
  });
  const terminal = await post(baseUrl, "/isGameOver", { state: gameOverState.body.state });
  assert.equal(terminal.body.over, true);
  assert.equal(terminal.body.nextMatchPolicy.mode, "close_room");

  const initialStream = await connectStream(address.port);
  sockets.push(initialStream);
  const hydrated = await initialStream.send({
    type: "hydrate",
    matchId: "stream-preflight",
    tickRateHz: 10,
    tick: 0,
    state: init.state,
    players: ["p1", "p2"],
    playerViews: init.playerViews,
  });
  assert.equal(hydrated.type, "hydrated");

  const firstStep = await initialStream.send({
    type: "step",
    tick: 1,
    inputs: [{
      playerId: "p1",
      data: { delta: 1 },
      actionId: "stream-action-1",
      actionSchemaId: "counter.action.v1",
    }],
    events: [
      { type: "join", playerId: "p2" },
      { type: "leave", playerId: "p2" },
      { type: "join", playerId: "p2" },
    ],
    wantState: true,
  });
  assert.equal(firstStep.type, "stepResult");
  assert.deepEqual(firstStep.actionResults, [{ actionId: "stream-action-1", status: "applied" }]);
  assert.deepEqual(Object.keys(firstStep.playerViews).sort(), ["p1", "p2"]);

  const sequenceMismatch = await initialStream.send({ type: "step", tick: 3 });
  assert.equal(sequenceMismatch.code, "TICK_SEQUENCE_MISMATCH");

  const replayStep = {
    tick: 2,
    inputs: [rejectedAction],
    events: [],
  };
  const uninterrupted = await initialStream.send({
    type: "step",
    tick: 2,
    inputs: replayStep.inputs,
    wantState: true,
  });
  assert.equal(uninterrupted.actionResults[0].status, "rejected");

  const restarted = await connectStream(address.port);
  sockets.push(restarted);
  const recovered = await restarted.send({
    type: "hydrate",
    matchId: "stream-preflight",
    tickRateHz: 10,
    tick: 1,
    state: firstStep.state,
    players: ["p1", "p2"],
    playerViews: firstStep.playerViews,
    replay: [replayStep],
  });
  assert.equal(recovered.type, "hydrated");
  assert.deepEqual(recovered.state, uninterrupted.state);
  assert.equal(recovered.tick, 2);

  const recoveredOver = await restarted.send({
    type: "step",
    tick: 3,
    inputs: [{
      playerId: "p1",
      data: { delta: 2 },
      actionId: "stream-action-3",
      actionSchemaId: "counter.action.v1",
    }],
  });
  assert.equal(recoveredOver.gameResult.over, true);

  const badReplay = await restarted.send({
    type: "hydrate",
    matchId: "stream-preflight",
    tickRateHz: 10,
    tick: 0,
    state: init.state,
    players: ["p1", "p2"],
    playerViews: init.playerViews,
    replay: [{ tick: 2, inputs: [], events: [] }],
  });
  assert.equal(badReplay.code, "REPLAY_MISMATCH");

  const missingView = await restarted.send({
    type: "hydrate",
    matchId: "stream-preflight",
    tickRateHz: 10,
    tick: 0,
    state: init.state,
    players: ["p1", "p2"],
    playerViews: { p1: init.playerViews.p1 },
  });
  assert.equal(missingView.code, "INVALID_RESULT");

  console.log("Node multiplayer migration preflight passed.");
} finally {
  await Promise.all(sockets.map(socket => socket.close()));
  await new Promise((resolve, reject) => {
    server.close(error => error ? reject(error) : resolve());
  });
}
