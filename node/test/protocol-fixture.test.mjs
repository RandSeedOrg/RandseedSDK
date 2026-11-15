import assert from "node:assert/strict";
import crypto from "node:crypto";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import {
  ChaCha20Rng,
  createGameServer,
  deriveSeedHex,
  legacySeedFromBytes,
  mulberry32U32,
} from "../dist/index.js";

const fixture = JSON.parse(
  await readFile(new URL("../../protocol/fixtures/room-host-v1.json", import.meta.url), "utf8"),
);

test("Node SDK matches the shared room-host v1 fixture", async (t) => {
  const game = {
    manifest: () => fixture.manifest,
    init: () => fixture.initResponse,
    tick: () => fixture.tickResponse,
    onPlayerJoin: () => fixture.playerEventResponse,
    onPlayerLeave: () => fixture.playerEventResponse,
    isGameOver: () => fixture.gameOverResponse,
    onCommand: () => fixture.commandResponse,
  };
  const server = createGameServer(game, 0);
  await new Promise((resolve) => server.once("listening", resolve));
  t.after(() => new Promise((resolve, reject) => {
    server.close((error) => error ? reject(error) : resolve());
  }));

  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const endpoints = [
    ["GET", "/manifest", undefined, fixture.manifest],
    ["POST", "/init", fixture.config, fixture.initResponse],
    ["POST", "/tick", fixture.tickRequest, fixture.tickResponse],
    ["POST", "/onPlayerJoin", fixture.playerEventRequest, fixture.playerEventResponse],
    ["POST", "/onPlayerLeave", fixture.playerEventRequest, fixture.playerEventResponse],
    ["POST", "/isGameOver", fixture.isGameOverRequest, fixture.gameOverResponse],
    ["POST", "/onCommand", fixture.commandRequest, fixture.commandResponse],
  ];

  for (const [method, path, body, expected] of endpoints) {
    const response = await fetch(`${baseUrl}${path}`, {
      method,
      ...(body === undefined
        ? {}
        : {
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(body),
          }),
    });

    assert.equal(response.status, 200, `${method} ${path}`);
    assert.deepEqual(await response.json(), expected, `${method} ${path}`);
  }
});

test("match options may be omitted but must be an object when supplied", async (t) => {
  let received;
  const server = createGameServer({
    manifest: () => fixture.manifest,
    init: (config) => { received = config; return fixture.initResponse; },
  }, 0);
  await new Promise((resolve) => server.once("listening", resolve));
  t.after(() => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())));
  const url = `http://127.0.0.1:${server.address().port}/init`;
  const { options, ...config } = fixture.config;
  for (const [body, expected] of [[config, 200], [{ ...config, options: null }, 400], [{ ...config, options }, 200]]) {
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    assert.equal(response.status, expected);
    if (expected === 200) assert.deepEqual(received, body);
  }
});

test("Mulberry32 matches every shared protocol vector", () => {
  for (const vector of fixture.prngVectors) {
    const next = mulberry32U32(vector.seed);
    assert.deepEqual(vector.u32.map(() => next()), vector.u32);
  }
});

test("ChaCha20Rng and deriveSeed match every shared protocol vector", () => {
  for (const vector of fixture.chacha20Vectors) {
    const rng = new ChaCha20Rng(vector.seedHex, {
      stream: vector.stream,
      wordPos: vector.wordPos,
    });
    assert.deepEqual(
      vector.u32.map(() => rng.nextU32()),
      vector.u32,
    );

    const boundedRng = ChaCha20Rng.fromState({
      seedHex: vector.seedHex,
      stream: vector.stream,
      wordPos: vector.wordPos,
    });
    assert.deepEqual(
      vector.bounded100.map(() => boundedRng.nextBounded(100)),
      vector.bounded100,
    );

    const midState = boundedRng.getState();
    const restored = ChaCha20Rng.fromState(midState);
    assert.equal(restored.nextU32(), boundedRng.nextU32());
  }

  for (const vector of fixture.deriveSeedVectors) {
    const derived = deriveSeedHex(vector.rootSeedHex, ...vector.domains);
    assert.equal(derived, vector.derivedSeedHex);
    assert.equal(new ChaCha20Rng(derived).nextU32(), vector.firstU32);

    // Cross-verify against Node's built-in SHA-256 implementation
    const hash = crypto.createHash("sha256");
    hash.update(Buffer.from("randseed-chacha20-derive-v1\0", "utf8"));
    hash.update(Buffer.from(vector.rootSeedHex, "hex"));
    const countBuf = Buffer.alloc(4);
    countBuf.writeUInt32LE(vector.domains.length, 0);
    hash.update(countBuf);
    for (const domain of vector.domains) {
      const domainBuf = Buffer.from(domain, "utf8");
      const lenBuf = Buffer.alloc(4);
      lenBuf.writeUInt32LE(domainBuf.length, 0);
      hash.update(lenBuf);
      hash.update(domainBuf);
    }
    assert.equal(derived, hash.digest("hex"));
  }

  assert.equal(
    legacySeedFromBytes("0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"),
    0x04030201,
  );
});

test("match options validate randomSeed and randomSeedIndex strictly", async (t) => {
  let received;
  const server = createGameServer({
    manifest: () => fixture.manifest,
    init: (config) => { received = config; return fixture.initResponse; },
  }, 0);
  await new Promise((resolve) => server.once("listening", resolve));
  t.after(() => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())));
  const url = `http://127.0.0.1:${server.address().port}/init`;

  const validSeed = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
  const okResponse = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      ...fixture.config,
      options: { randomSeed: validSeed, randomSeedIndex: 0, seed: 50462976 },
    }),
  });
  assert.equal(okResponse.status, 200);
  assert.equal(received.options.randomSeed, validSeed);
  assert.equal(received.options.randomSeedIndex, 0);

  const invalidOptionsList = [
    { randomSeed: null },
    { randomSeed: "short" },
    { randomSeed: validSeed.toUpperCase() },
    { randomSeed: "g".repeat(64) },
    { randomSeedIndex: null },
    { randomSeedIndex: -1 },
    { randomSeedIndex: 1.5 },
    { randomSeedIndex: 9007199254740992 },
    { randomSeedIndex: "0" },
  ];

  for (const badOptions of invalidOptionsList) {
    const response = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ...fixture.config, options: badOptions }),
    });
    assert.equal(response.status, 400, JSON.stringify(badOptions));
    const body = await response.json();
    assert.equal(body.code, "INVALID_OPTIONS", JSON.stringify(badOptions));
  }
});
