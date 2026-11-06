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

test("action requests, receipts, leases, and no-action Skill decisions match shared vectors", () => {
  assert.deepEqual(validateAgentControllerLease(fixture.lease), fixture.lease);
  assert.deepEqual(validateAgentActionRequest(fixture.actionRequest), fixture.actionRequest);
  assert.deepEqual(validateAgentActionReceipt(fixture.queuedReceipt), fixture.queuedReceipt);
  assert.deepEqual(validateAgentActionReceipt(fixture.appliedReceipt), fixture.appliedReceipt);
  assert.deepEqual(validateAgentActionReceipt(fixture.rejectedReceipt), fixture.rejectedReceipt);
  assert.deepEqual(validateAgentSkillDecision(fixture.waitDecision), fixture.waitDecision);
});

test("host projection endpoint returns only the game-authored recipient view", async t => {
  const server = createGameServer({
    agentPlayerContract: () => fixture.contract,
    projectAgentPlayerView: () => fixture.projection,
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
