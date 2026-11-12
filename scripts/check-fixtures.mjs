import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const canonical = readFileSync(new URL("protocol/fixtures/room-host-v1.json", root), "utf8");
const rustCopy = readFileSync(
  new URL("rust/randseed-game-sdk/tests/fixtures/room-host-v1.json", root),
  "utf8",
);
const resultEvent = readFileSync(
  new URL("protocol/fixtures/game-result-event-v1.json", root),
  "utf8",
);
const rustResultEvent = readFileSync(
  new URL("rust/randseed-game-sdk/tests/fixtures/game-result-event-v1.json", root),
  "utf8",
);
const agentPlayer = readFileSync(new URL("protocol/fixtures/agent-player-v1.json", root), "utf8");
const rustAgentPlayer = readFileSync(
  new URL("rust/randseed-game-sdk/tests/fixtures/agent-player-v1.json", root),
  "utf8",
);
const errorCodes = readFileSync(new URL("protocol/fixtures/sdk-error-codes-v1.json", root), "utf8");
const rustErrorCodes = readFileSync(
  new URL("rust/randseed-game-sdk/tests/fixtures/sdk-error-codes-v1.json", root),
  "utf8",
);
const streamFixture = readFileSync(new URL("protocol/fixtures/session-stream-v1.json", root), "utf8");
const rustStreamFixture = readFileSync(
  new URL("rust/randseed-game-sdk/tests/fixtures/session-stream-v1.json", root),
  "utf8",
);

assert.deepEqual(JSON.parse(rustCopy), JSON.parse(canonical), "Rust's packaged fixture must match the canonical fixture");
assert.deepEqual(
  JSON.parse(rustResultEvent),
  JSON.parse(resultEvent),
  "Rust's Result Unit fixture must match the canonical fixture",
);
assert.deepEqual(
  JSON.parse(rustAgentPlayer),
  JSON.parse(agentPlayer),
  "Rust's Agent player fixture must match the canonical fixture",
);
assert.deepEqual(
  JSON.parse(rustErrorCodes),
  JSON.parse(errorCodes),
  "Rust's error-code fixture must match the canonical fixture",
);
assert.deepEqual(
  JSON.parse(rustStreamFixture),
  JSON.parse(streamFixture),
  "Rust's Session Stream fixture must match the canonical fixture",
);
console.log("Shared room-host fixture copies match.");
