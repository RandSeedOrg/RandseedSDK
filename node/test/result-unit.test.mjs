import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import {
  createGameServer,
  validateGameResultDefinition,
  validateGameResultEvent,
  validateGameResultUnits,
} from "../dist/index.js";

const fixture = JSON.parse(
  await readFile(new URL("../../protocol/fixtures/game-result-event-v1.json", import.meta.url), "utf8"),
);

test("validates the shared Result Unit v1 fixture and declared metrics", () => {
  validateGameResultDefinition(fixture.definition);
  validateGameResultEvent(fixture.event, fixture.definition);
  validateGameResultUnits([fixture.event], fixture.definition);
});

test("validates append-only replacement and retraction payloads", () => {
  const replacement = {
    ...fixture.event,
    eventId: "replacement-event",
    correction: { action: "replace", supersedesEventId: fixture.event.eventId },
  };
  validateGameResultEvent(replacement, fixture.definition);

  const retraction = {
    ...fixture.event,
    eventId: "retraction-event",
    participants: [],
    teams: [],
    correction: { action: "retract", supersedesEventId: fixture.event.eventId },
  };
  validateGameResultEvent(retraction, fixture.definition);
  assert.throws(() => validateGameResultEvent({
    ...retraction, participants: fixture.event.participants,
  }, fixture.definition), /must not include/);
  assert.throws(() => validateGameResultEvent({
    ...replacement,
    correction: { action: "replace", supersedesEventId: replacement.eventId },
  }, fixture.definition), /cannot supersede itself/);
});

test("rejects forged identities, undeclared metrics and invalid result metadata", () => {
  for (const event of [
    { ...fixture.event, gameId: "forged-game" },
    { ...fixture.event, unitType: "unknown" },
    { ...fixture.event, definitionVersion: "old" },
    { ...fixture.event, participants: [{ ...fixture.event.participants[0], metrics: { winRate: 1 } }] },
    { ...fixture.event, participants: [{ ...fixture.event.participants[0], rank: 0 }] },
    { ...fixture.event, occurredAt: Number.MAX_SAFE_INTEGER + 1 },
  ]) {
    assert.throws(() => validateGameResultEvent(event, fixture.definition));
  }
});

test("enforces definition, event and per-transition limits", () => {
  const metric = fixture.definition.metrics[0];
  const fullDefinition = {
    ...fixture.definition,
    metrics: Array.from({ length: 128 }, (_, index) => ({ ...metric, id: `metric-${index}` })),
  };
  validateGameResultDefinition(fullDefinition);
  assert.throws(() => validateGameResultDefinition({
    ...fullDefinition,
    metrics: [...fullDefinition.metrics, { ...metric, id: "metric-128" }],
  }), /128-metric limit/);

  const events = Array.from({ length: 32 }, (_, index) => ({
    ...fixture.event,
    eventId: `event-${index}`,
    unitId: `unit-${index}`,
  }));
  validateGameResultUnits(events);
  assert.throws(() => validateGameResultUnits([
    ...events,
    { ...fixture.event, eventId: "event-32", unitId: "unit-32" },
  ]), /32-event/);

  assert.throws(() => validateGameResultEvent({
    ...fixture.event,
    eventId: "😀".repeat(129),
  }), /Unicode code points/);
});

test("the Node host returns valid transition results and rejects invalid result output", async t => {
  const game = {
    manifest: () => ({
      title: "Result Unit test", version: "1.0.0", minPlayers: 1, maxPlayers: 2,
      tickRateHz: 10, persistentTable: false, resultDefinition: fixture.definition,
    }),
    init: () => ({ state: {}, resultUnits: [fixture.event] }),
  };
  const server = createGameServer(game, 0);
  await new Promise(resolve => server.once("listening", resolve));
  t.after(() => new Promise((resolve, reject) =>
    server.close(error => error ? reject(error) : resolve())));

  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const response = await fetch(`${baseUrl}/init`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      matchId: "match-001", gameId: "game-1", players: ["player-a"],
      matchSeq: 1, options: { seed: 1 },
    }),
  });
  assert.equal(response.status, 200);
  assert.deepEqual((await response.json()).resultUnits, [fixture.event]);

  game.init = () => ({
    state: {},
    resultUnits: [{ ...fixture.event, participants: [{ ...fixture.event.participants[0], rank: 0 }] }],
  });
  const invalidResponse = await fetch(`${baseUrl}/init`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      matchId: "match-001", gameId: "game-1", players: ["player-a"],
      matchSeq: 1, options: { seed: 1 },
    }),
  });
  assert.equal(invalidResponse.status, 500);
  assert.deepEqual(await invalidResponse.json(), {
    error: "Internal server error.",
  });
});
