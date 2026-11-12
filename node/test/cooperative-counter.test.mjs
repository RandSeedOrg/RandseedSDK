import assert from "node:assert/strict";
import { test } from "node:test";
import { cooperativeCounter } from "../examples/cooperative-counter.mjs";

test("cooperative counter shows authoritative multiplayer lifecycle and action receipts", () => {
  const initialized = cooperativeCounter.init({
    matchId: "match-1",
    players: ["player-a", "player-b"],
    options: {},
  });
  assert.deepEqual(initialized.state.players, ["player-a", "player-b"]);
  assert.equal(initialized.state.score, 0);

  const ticked = cooperativeCounter.tick({
    tick: 1,
    elapsedSeconds: 0.2,
    commands: [],
    inputs: [
      {
        playerId: "player-a",
        data: { delta: 2 },
        actionId: "action-1",
        actionSchemaId: "counter.delta.v1",
      },
      {
        playerId: "player-b",
        data: { delta: 99 },
        actionId: "action-2",
        actionSchemaId: "counter.delta.v1",
      },
    ],
  }, initialized.state);
  assert.equal(ticked.state.score, 2);
  assert.equal(ticked.state.elapsedSeconds, 0.2);
  assert.deepEqual(ticked.actionResults, [
    { actionId: "action-1", status: "applied" },
    { actionId: "action-2", status: "rejected", code: "invalid_delta" },
  ]);

  const rejoined = cooperativeCounter.onPlayerJoin("player-b", ticked.state);
  assert.deepEqual(rejoined.state.players, ["player-a", "player-b"]);
  const left = cooperativeCounter.onPlayerLeave("player-b", rejoined.state);
  assert.deepEqual(left.state.players, ["player-a"]);
  assert.equal(cooperativeCounter.isGameOver({ ...left.state, score: 20 }).over, true);
});
