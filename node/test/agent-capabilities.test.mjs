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
    assert.deepEqual(await response.json(), { error: "Agent capabilities are unavailable." });
    declaration = undefined;
    response = await fetch(url);
    assert.equal(response.status, 500);
    assert.deepEqual(await response.json(), { error: "Agent capabilities are unavailable." });
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
    assert.deepEqual(await response.json(), { error: "Agent capabilities are unavailable." });
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
