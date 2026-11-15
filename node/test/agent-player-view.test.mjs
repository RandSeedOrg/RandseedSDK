import { once } from "node:events";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import {
  createAgentPlayerView,
  createGameServer,
  validateAgentActionReceipt,
  validateAgentActionRequest,
  validateAgentControllerLease,
  validateAgentPlayerContract,
  validateAgentPlayerViewRequest,
  validateAgentSkillDecision,
} from "../dist/index.js";

const fixture = JSON.parse(await readFile(
  new URL("../../protocol/fixtures/agent-player-v1.json", import.meta.url),
  "utf8",
));

test("Agent player view matches the shared fixture and excludes other-seat secrets", () => {
  const contract = validateAgentPlayerContract(fixture.contract);
  const request = validateAgentPlayerViewRequest(fixture.request);
  const view = createAgentPlayerView(contract, request, fixture.projection);
  assert.deepEqual(view, fixture.view);
  assert.equal(JSON.stringify(view).includes("B_SECRET"), false);
  assert.equal(JSON.stringify(view).includes("randomSeed"), false);
});

test("Agent player contract and recipient roster validation fail closed", () => {
  assert.throws(() => validateAgentPlayerContract({ ...fixture.contract, maxDecisionHz: 11 }));
  assert.throws(() => validateAgentPlayerViewRequest({ ...fixture.request, players: ["seat-b"] }));
  assert.throws(() => validateAgentPlayerViewRequest({
    ...fixture.request,
    players: ["seat-a", "seat-a"],
  }));
});

test("Agent player identifiers use UTF-8 bounds and reject C1 controls", () => {
  assert.throws(() => validateAgentPlayerViewRequest({
    ...fixture.request,
    context: { ...fixture.request.context, roomId: "界".repeat(86) },
  }));
  assert.throws(() => validateAgentPlayerViewRequest({
    ...fixture.request,
    context: { ...fixture.request.context, roomId: "room-\u0085" },
  }));
});

test("action requests, receipts, leases, and no-action Skill decisions match shared vectors", () => {
  assert.deepEqual(validateAgentControllerLease(fixture.lease), fixture.lease);
  assert.deepEqual(validateAgentActionRequest(fixture.actionRequest), fixture.actionRequest);
  assert.deepEqual(validateAgentActionReceipt(fixture.queuedReceipt), fixture.queuedReceipt);
  assert.deepEqual(validateAgentActionReceipt(fixture.appliedReceipt), fixture.appliedReceipt);
  assert.deepEqual(validateAgentActionReceipt(fixture.rejectedReceipt), fixture.rejectedReceipt);
  assert.deepEqual(validateAgentSkillDecision(fixture.waitDecision), fixture.waitDecision);
});

test("host projection endpoint returns only the game-authored recipient view", async t => {
  const previous = process.env.RANDSEED_GAME_ID;
  process.env.RANDSEED_GAME_ID = "hidden-hand";
  t.after(() => {
    if (previous === undefined) delete process.env.RANDSEED_GAME_ID;
    else process.env.RANDSEED_GAME_ID = previous;
  });
  const server = createGameServer({
    agentPlayerContract: () => fixture.contract,
    projectAgentPlayerView: request => {
      request.context.playerId = "seat-b";
      request.context.scope.gameId = "another-game";
      request.context.sequence = 100;
      request.context.tick = 100;
      return fixture.projection;
    },
  }, 0, { gameId: "hidden-hand" });
  t.after(() => new Promise(resolve => server.close(resolve)));
  await once(server, "listening");
  const address = server.address();
  assert.ok(address && typeof address === "object");

  const response = await fetch(`http://127.0.0.1:${address.port}/agent/player-view`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(fixture.request),
  });
  assert.equal(response.status, 200);
  const view = await response.json();
  assert.deepEqual(view, fixture.view);
  assert.equal(JSON.stringify(view).includes("B_SECRET"), false);
});

test("nullable Agent player contracts return 404 without calling the projector", async t => {
  const previous = process.env.RANDSEED_GAME_ID;
  process.env.RANDSEED_GAME_ID = "hidden-hand";
  t.after(() => {
    if (previous === undefined) delete process.env.RANDSEED_GAME_ID;
    else process.env.RANDSEED_GAME_ID = previous;
  });
  let projectCalls = 0;
  const server = createGameServer({
    agentPlayerContract: () => null,
    projectAgentPlayerView: () => {
      projectCalls += 1;
      return fixture.projection;
    },
  }, 0, { gameId: "hidden-hand" });
  t.after(() => new Promise(resolve => server.close(resolve)));
  await once(server, "listening");
  const address = server.address();
  assert.ok(address && typeof address === "object");
  const baseUrl = `http://127.0.0.1:${address.port}`;

  const contractResponse = await fetch(`${baseUrl}/agent/player-contract`);
  assert.equal(contractResponse.status, 404);
  const viewResponse = await fetch(`${baseUrl}/agent/player-view`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(fixture.request),
  });
  assert.equal(viewResponse.status, 404);
  assert.equal(projectCalls, 0);
});

test("Agent player views require and validate the platform game scope", async t => {
  const previous = process.env.RANDSEED_GAME_ID;
  t.after(() => {
    if (previous === undefined) delete process.env.RANDSEED_GAME_ID;
    else process.env.RANDSEED_GAME_ID = previous;
  });
  const game = {
    agentPlayerContract: () => fixture.contract,
    projectAgentPlayerView: () => fixture.projection,
  };
  delete process.env.RANDSEED_GAME_ID;
  const unconfigured = createGameServer(game, 0, { gameId: "hidden-hand" });
  t.after(() => new Promise(resolve => unconfigured.close(resolve)));
  await once(unconfigured, "listening");
  process.env.RANDSEED_GAME_ID = "hidden-hand";
  const configured = createGameServer(game, 0);
  t.after(() => new Promise(resolve => configured.close(resolve)));
  await once(configured, "listening");
  const request = async (server, body) => fetch(`http://127.0.0.1:${server.address().port}/agent/player-view`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const unavailable = await request(unconfigured, fixture.request);
  assert.equal(unavailable.status, 503);
  assert.equal((await unavailable.json()).error, "The platform RANDSEED_GAME_ID is unavailable.");
  const wrongScope = structuredClone(fixture.request);
  wrongScope.context.scope.gameId = "other-game";
  assert.equal((await request(configured, wrongScope)).status, 400);
  const accepted = await request(configured, fixture.request);
  assert.equal(accepted.status, 200);
  assert.deepEqual(await accepted.json(), fixture.view);
});
