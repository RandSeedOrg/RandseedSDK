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

assert.deepEqual(JSON.parse(rustCopy), JSON.parse(canonical), "Rust's packaged fixture must match the canonical fixture");
assert.deepEqual(
  JSON.parse(rustResultEvent),
  JSON.parse(resultEvent),
  "Rust's Result Unit fixture must match the canonical fixture",
);
console.log("Shared room-host fixture copies match.");
