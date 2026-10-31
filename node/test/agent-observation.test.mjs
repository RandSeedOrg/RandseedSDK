import { test } from "node:test";
import assert from "node:assert/strict";
import { createAgentObservation } from "../dist/index.js";

test("Agent observations include only the selected player's private partition", () => {
  const publicState = { turn: "alice", positions: { alice: [1, 2], bob: [3, 4] } };
  const privateStates = new Map([
    ["alice", { hand: ["ALICE_SECRET"] }],
    ["bob", { hand: ["BOB_SECRET"] }],
  ]);
  const context = { matchId: "match-1", sequence: 3, tick: 2, playerId: "alice" };
  const alice = createAgentObservation(context, publicState, privateStates);
  const bob = createAgentObservation({ ...context, playerId: "bob" }, publicState, privateStates);
  assert.deepEqual(alice, {
    protocolVersion: 1,
    matchId: "match-1",
    sequence: 3,
    tick: 2,
    publicState,
    self: { playerId: "alice", privateState: { hand: ["ALICE_SECRET"] } },
  });
  assert.equal(JSON.stringify(alice).includes("BOB_SECRET"), false);
  assert.equal(JSON.stringify(bob).includes("ALICE_SECRET"), false);
  publicState.positions.alice[0] = 9;
  privateStates.get("alice").hand.push("LATER");
  assert.equal(alice.publicState.positions.alice[0], 1);
  assert.deepEqual(alice.self.privateState.hand, ["ALICE_SECRET"]);
});

test("Agent observations fail closed for absent views and malformed metadata", () => {
  const context = { matchId: "m", sequence: 0, tick: 0, playerId: "a" };
  assert.throws(() => createAgentObservation(context, {}, new Map()), /private view is unavailable/);
  const states = new Map([["a", null]]);
  assert.equal(createAgentObservation(context, null, states).self.privateState, null);
  for (const invalid of [
    { matchId: "" }, { playerId: " " }, { sequence: -1 }, { sequence: 1.5 },
    { tick: Number.MAX_SAFE_INTEGER + 1 }, { tick: NaN },
  ]) {
    assert.throws(() => createAgentObservation({ ...context, ...invalid }, {}, states));
  }
});

test("Agent observations reject non-JSON and cyclic input rather than silently changing it", () => {
  const context = { matchId: "m", sequence: 0, tick: 0, playerId: "a" };
  const privateStates = new Map([["a", {}]]);
  const cycle = {};
  cycle.self = cycle;
  for (const state of [cycle, { x: undefined }, { x: Infinity }, [, 1], new Date()]) {
    assert.throws(() => createAgentObservation(context, state, privateStates));
  }
});
