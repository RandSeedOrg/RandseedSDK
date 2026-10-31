import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { createGameServer, mulberry32U32 } from "../dist/index.js";

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
