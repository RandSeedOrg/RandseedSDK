import { test } from "node:test";
import assert from "node:assert/strict";
import { createGameServer, validateAgentGameCapabilities } from "../dist/index.js";

const valid = {
  observationProtocolVersion: 1,
  actionProtocolVersion: 1,
  modes: ["autoplay", "follow"],
  maxDecisionHz: 10,
  backgroundAutoplay: false,
};

test("Agent capabilities validate protocols, modes, frequency and background support", () => {
  assert.deepEqual(validateAgentGameCapabilities({ ...valid, extra: "not exposed" }), valid);
  for (const change of [
    { observationProtocolVersion: 2 },
    { actionProtocolVersion: 0 },
    { modes: [] },
    { modes: new Array(1) },
    { modes: [, "follow"] },
    { modes: ["follow", "follow"] },
    { modes: ["admin"] },
    { maxDecisionHz: 0 },
    { maxDecisionHz: Infinity },
    { backgroundAutoplay: true, modes: ["follow"] },
    { backgroundAutoplay: null },
  ]) {
    assert.throws(() => validateAgentGameCapabilities({ ...valid, ...change }));
  }
  const declaration = validateAgentGameCapabilities(valid);
  declaration.modes.push("copilot");
  assert.deepEqual(valid.modes, ["autoplay", "follow"]);
});

test("Game servers advertise Agent support only by explicit valid declaration", async () => {
  let declaration = null;
  const server = createGameServer({ agentCapabilities: () => declaration }, 0);
  await new Promise(resolve => server.once("listening", resolve));
  const url = `http://127.0.0.1:${server.address().port}/agent/capabilities`;
  try {
    let response = await fetch(url);
    assert.equal(response.status, 404);
    declaration = { ...valid, extra: "not exposed" };
    response = await fetch(url);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), valid);
    declaration = { ...valid, observationProtocolVersion: 2 };
    response = await fetch(url);
    assert.equal(response.status, 500);
    const invalidBody = await response.json();
    assert.equal(invalidBody.code, "INVALID_RESULT");
    assert.equal(invalidBody.retryable, false);
    assert.equal(invalidBody.creatorAction, "Correct the game callback result to satisfy its documented schema.");
    declaration = undefined;
    response = await fetch(url);
    assert.equal(response.status, 500);
    assert.equal((await response.json()).code, "INVALID_RESULT");
  } finally {
    await new Promise(resolve => server.close(resolve));
  }
});

test("Agent capability declaration failures do not expose internal errors", async () => {
  const server = createGameServer({ agentCapabilities: () => { throw new Error("private detail"); } }, 0);
  await new Promise(resolve => server.once("listening", resolve));
  try {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/agent/capabilities`);
    assert.equal(response.status, 500);
    const body = await response.json();
    assert.equal(body.code, "CALLBACK_FAILED");
    assert.equal(body.retryable, false);
    assert.equal(body.error.includes("private detail"), false);
  } finally {
    await new Promise(resolve => server.close(resolve));
  }
});

test("Legacy game servers do not opt in to Agent support", async () => {
  const server = createGameServer({}, 0);
  await new Promise(resolve => server.once("listening", resolve));
  try {
    const response = await fetch(`http://127.0.0.1:${server.address().port}/agent/capabilities`);
    assert.equal(response.status, 404);
  } finally {
    await new Promise(resolve => server.close(resolve));
  }
});

test("Health checks require a valid platform or explicit test Game ID", async t => {
  const previous = process.env.RANDSEED_GAME_ID;
  t.after(() => {
    if (previous === undefined) delete process.env.RANDSEED_GAME_ID;
    else process.env.RANDSEED_GAME_ID = previous;
  });
  delete process.env.RANDSEED_GAME_ID;
  const missing = createGameServer({}, 0);
  t.after(() => new Promise(resolve => missing.close(resolve)));
  await new Promise(resolve => missing.once("listening", resolve));
  const unavailable = await fetch(`http://127.0.0.1:${missing.address().port}/health`);
  assert.equal(unavailable.status, 503);
  assert.equal((await unavailable.json()).error, "The platform RANDSEED_GAME_ID is unavailable.");

  process.env.RANDSEED_GAME_ID = "platform-game";
  assert.throws(() => createGameServer({}, 0, { gameId: "other-game" }), /must match/);
  assert.throws(() => createGameServer({}, 0, { gameId: "" }), /valid RandSeed Game ID/);
  const configured = createGameServer({}, 0);
  t.after(() => new Promise(resolve => configured.close(resolve)));
  await new Promise(resolve => configured.once("listening", resolve));
  const response = await fetch(`http://127.0.0.1:${configured.address().port}/health`);
  const health = await response.json();
  assert.equal(response.status, 200);
  assert.equal(health.status, "ok");
  assert.equal(health.gameId, "platform-game");
  assert.equal(typeof health.uptime, "number");

  for (const invalid of ["", "bad id", "a".repeat(65)]) {
    process.env.RANDSEED_GAME_ID = invalid;
    assert.throws(() => createGameServer({}, 0), /valid RandSeed Game ID/);
  }
  delete process.env.RANDSEED_GAME_ID;
  assert.throws(() => createGameServer({}, 0, { gameId: 123 }), /valid RandSeed Game ID/);
});
