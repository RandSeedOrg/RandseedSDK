import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createGameServer, validateGameManifest } from "../dist/index.js";

const fixture = JSON.parse(readFileSync(new URL("../../protocol/fixtures/room-host-private-v1.json", import.meta.url), "utf8"));
const gamecreatorRequests = JSON.parse(readFileSync(new URL("../../protocol/fixtures/gamecreator-room-host-private-requests-v1.json", import.meta.url), "utf8"));
const emptyRosterFixture = JSON.parse(readFileSync(new URL("../../protocol/fixtures/room-host-private-empty-roster-v1.json", import.meta.url), "utf8"));
const originalGameId = process.env.RANDSEED_GAME_ID;
process.env.RANDSEED_GAME_ID = "fixture-game_1";

test("manifest defaults and optional explicit schema v1 match shared vectors", () => {
  for (const testCase of fixture.manifestCases) {
    if (testCase.valid) assert.doesNotThrow(() => validateGameManifest(testCase.manifest), testCase.name);
    else assert.throws(() => validateGameManifest(testCase.manifest), testCase.name);
  }
  assert.equal(fixture.manifestCases[1].manifest.playerViewSchemaVersion, undefined);
  assert.equal(fixture.manifestCases[2].manifest.playerViewSchemaVersion, 1);
});

test("manifest optionsSchema accepts JSON Schema objects and booleans only", () => {
  const base = { ...fixture.manifestCases[0].manifest };
  for (const schema of [{ type: "object", properties: { mode: { type: "string" } } }, true, false]) {
    assert.doesNotThrow(() => validateGameManifest({ ...base, optionsSchema: schema }));
  }
  for (const schema of [null, [], "object", 1]) {
    assert.throws(() => validateGameManifest({ ...base, optionsSchema: schema }));
  }
});

async function withServer(game, run) {
  const server = createGameServer(game, 0, { gameId: "fixture-game_1" });
  await new Promise((resolve) => server.once("listening", resolve));
  try {
    await run(`http://127.0.0.1:${server.address().port}`);
  } finally {
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
}


async function withWebSocket(port, run) {
  const ws = new WebSocket(`ws://127.0.0.1:${port}/stream`);
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });
  try {
    const send = payload => new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("stream response timed out")), 5000);
      ws.addEventListener("message", event => {
        clearTimeout(timer);
        resolve(JSON.parse(String(event.data)));
      }, { once: true });
      ws.send(JSON.stringify(payload));
    });
    await run(send);
  } finally {
    if (ws.readyState === WebSocket.OPEN) {
      await new Promise(resolve => {
        ws.addEventListener("close", resolve, { once: true });
        ws.close();
      });
    }
  }
}

function viewsFor(players) {
  return Object.fromEntries(players.map((playerId) => [playerId, {
    viewer: playerId,
    hand: playerId === "seat-a" ? ["ACE_OF_SPADES"] : playerId === "seat-b" ? ["KING_OF_CLUBS"] : [],
  }]));
}

function privateGame(manifest = fixture.manifestCases[1].manifest, overrides = {}) {
  return {
    manifest: () => manifest,
    init: (config) => ({ state: { canonicalHands: { "seat-a": ["ACE_OF_SPADES"], "seat-b": ["KING_OF_CLUBS"] } }, playerViews: viewsFor(config.players) }),
    tick: (_ctx, _state, players) => ({ state: { tick: 1 }, playerViews: viewsFor(players), persist: false }),
    onPlayerJoin: (_playerId, _state, players) => ({ state: { joined: true }, playerViews: viewsFor(players) }),
    onPlayerLeave: (_playerId, _state, players) => ({ state: { left: true }, playerViews: viewsFor(players) }),
    isGameOver: () => ({ over: false }),
    onCommand: (_command, _state, players) => ({ state: { commanded: true }, playerViews: viewsFor(players) }),
    ...overrides,
  };
}

async function post(baseUrl, route, body) {
  return fetch(`${baseUrl}${route}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: typeof body === "string" ? body : JSON.stringify(body),
  });
}

test("real Node HTTP lifecycle validates and returns recipient-keyed private projections", async () => {
  const callbacks = [];
  const game = privateGame(undefined, {
    tick: (_ctx, _state, players) => { callbacks.push(["tick", players]); return { state: { tick: 1 }, playerViews: viewsFor(players), persist: false }; },
    onPlayerJoin: (_playerId, _state, players) => { callbacks.push(["join", players]); return { state: {}, playerViews: viewsFor(players) }; },
    onPlayerLeave: (_playerId, _state, players) => { callbacks.push(["leave", players]); return { state: {}, playerViews: viewsFor(players) }; },
    onCommand: (_command, _state, players) => { callbacks.push(["command", players]); return { state: {}, playerViews: viewsFor(players) }; },
  });
  await withServer(game, async (baseUrl) => {
    const manifestResponse = await fetch(`${baseUrl}/manifest`);
    assert.equal(manifestResponse.status, 200);
    const manifest = await manifestResponse.json();
    assert.equal(manifest.playerViewMode, "private");
    assert.equal(manifest.playerViewSchemaVersion, undefined);

    const life = fixture.lifecycle;
    const init = await post(baseUrl, "/init", life.initRequest);
    assert.equal(init.status, 200);
    const initBody = await init.json();
    assert.deepEqual(Object.keys(initBody.playerViews).sort(), fixture.players);
    assert.deepEqual(initBody.playerViews["seat-a"], { hand: ["ACE_OF_SPADES"], viewer: "seat-a" });
    assert.equal(JSON.stringify(initBody.playerViews["seat-a"]).includes("KING_OF_CLUBS"), false);

    const tick = await post(baseUrl, "/tick", life.tickRequest);
    assert.equal(tick.status, 200);
    const joined = await post(baseUrl, "/onPlayerJoin", life.joinRequest);
    assert.equal(joined.status, 200);
    const left = await post(baseUrl, "/onPlayerLeave", life.leaveRequest);
    assert.equal(left.status, 200);
    const command = await post(baseUrl, "/onCommand", life.commandRequest);
    assert.equal(command.status, 200);
    assert.deepEqual(callbacks.map(([name, roster]) => [name, [...roster]]), [
      ["tick", ["seat-a", "seat-b"]],
      ["join", ["seat-a", "seat-b", "seat-c"]],
      ["leave", ["seat-a"]],
      ["command", ["seat-a", "seat-b"]],
    ]);
  });
});

test("Gamecreator-generated private HTTP payloads interoperate with the Node host", async () => {
  const rosters = [];
  const remember = (name, players) => rosters.push([name, [...players]]);
  await withServer(privateGame(undefined, {
    tick: (_ctx, _state, players) => {
      remember("tick", players);
      return { state: {}, playerViews: viewsFor(players), persist: false };
    },
    onCommand: (_command, _state, players) => {
      remember("command", players);
      return { state: {}, playerViews: viewsFor(players) };
    },
    onPlayerJoin: (_id, _state, players) => {
      remember("join", players);
      return { state: {}, playerViews: viewsFor(players) };
    },
    onPlayerLeave: (_id, _state, players) => {
      remember("leave", players);
      return { state: {}, playerViews: viewsFor(players) };
    },
  }), async baseUrl => {
    const requests = [
      ["/tick", gamecreatorRequests.tickActive],
      ["/onCommand", gamecreatorRequests.command],
      ["/onPlayerJoin", gamecreatorRequests.join],
      ["/onPlayerLeave", gamecreatorRequests.leaveJoined],
      ["/onPlayerLeave", gamecreatorRequests.leaveFirst],
      ["/onPlayerLeave", gamecreatorRequests.leaveLast],
      ["/tick", gamecreatorRequests.tickEmpty],
    ];
    const responses = [];
    for (const [route, body] of requests) {
      const response = await post(baseUrl, route, body);
      assert.equal(response.status, 200, `${route} body=${JSON.stringify(body)}`);
      responses.push(await response.json());
    }
    assert.deepEqual(responses[5].playerViews, {});
    assert.deepEqual(responses[6].playerViews, {});
  });
  assert.deepEqual(rosters, [
    ["tick", ["seat-a", "seat-b"]],
    ["command", ["seat-a", "seat-b"]],
    ["join", ["seat-a", "seat-b", "seat-c"]],
    ["leave", ["seat-a", "seat-b"]],
    ["leave", ["seat-b"]],
    ["leave", []],
    ["tick", []],
  ]);
});

test("explicit v1 and legacy public manifests are emitted without changing lifecycle interfaces", async () => {
  const explicit = fixture.manifestCases[2].manifest;
  await withServer(privateGame(explicit), async (baseUrl) => {
    const response = await fetch(`${baseUrl}/manifest`);
    assert.equal(response.status, 200);
    assert.equal((await response.json()).playerViewSchemaVersion, 1);
  });
  const publicLegacy = fixture.manifestCases[0].manifest;
  const publicRosterArguments = [];
  await withServer(privateGame(publicLegacy, {
    init: () => ({ state: { public: true } }),
    tick: (_ctx, _state, players) => {
      publicRosterArguments.push(players);
      return { state: { public: true }, persist: false };
    },
  }), async (baseUrl) => {
    const response = await fetch(`${baseUrl}/manifest`);
    assert.equal(response.status, 200);
    const manifest = await response.json();
    assert.equal(Object.hasOwn(manifest, "playerViewMode"), false);
    assert.equal((await post(baseUrl, "/init", fixture.lifecycle.initRequest)).status, 200);
    assert.equal((await post(baseUrl, "/tick", {
      ctx: { tick: 1, inputs: [], commands: [], elapsedSeconds: 0.2 }, state: { public: true },
    })).status, 200);
    assert.deepEqual(publicRosterArguments, [undefined]);
  });
});

test("public HTTP roster distinguishes explicit empty, omitted, and null values", async () => {
  const callbackRosters = [];
  const game = {
    manifest: () => fixture.manifestCases[0].manifest,
    init: () => ({ state: {} }),
    tick: (_ctx, state, players) => {
      callbackRosters.push(players);
      return { state, persist: false };
    },
    onPlayerJoin: (_playerId, state, players) => {
      callbackRosters.push(players);
      return { state };
    },
    onPlayerLeave: (_playerId, state, players) => {
      callbackRosters.push(players);
      return { state };
    },
    onCommand: (_command, state, players) => {
      callbackRosters.push(players);
      return { state };
    },
    isGameOver: () => ({ over: false }),
  };
  await withServer(game, async baseUrl => {
    const request = { ctx: { tick: 1, inputs: [], commands: [], elapsedSeconds: 0.1 }, state: {} };
    assert.equal((await post(baseUrl, "/tick", { ...request, players: [] })).status, 200);
    assert.equal((await post(baseUrl, "/tick", request)).status, 200);
    assert.equal((await post(baseUrl, "/tick", { ...request, players: null })).status, 400);
    assert.equal((await post(baseUrl, "/onPlayerLeave", { playerId: "seat-a", state: {}, players: [] })).status, 200);
  });
  assert.deepEqual(callbackRosters[0], []);
  assert.equal(callbackRosters[1], undefined);
  assert.deepEqual(callbackRosters[2], []);
});

test("private lifecycle rejects missing/null rosters and inconsistent event results", async () => {
  const players = fixture.players;
  await withServer(privateGame(), async (baseUrl) => {
    const tick = { ctx: { tick: 1, inputs: [], commands: [], elapsedSeconds: 0.2 }, state: {} };
    assert.equal((await post(baseUrl, "/tick", tick)).status, 400);
    assert.equal((await post(baseUrl, "/tick", { ...tick, players: null })).status, 400);
    assert.equal((await post(baseUrl, "/onCommand", { command: { type: "reset", data: {} }, state: {} })).status, 400);
    assert.equal((await post(baseUrl, "/onPlayerJoin", { playerId: "seat-c", state: {} })).status, 400);
    assert.equal((await post(baseUrl, "/onPlayerJoin", { playerId: "seat-c", state: {}, players })).status, 400);
    assert.equal((await post(baseUrl, "/onPlayerLeave", { playerId: "seat-b", state: {}, players })).status, 400);
    assert.equal((await post(baseUrl, "/tick", { ...tick, players, ctx: { ...tick.ctx, inputs: [{ playerId: "outsider", data: {} }] } })).status, 400);
  });
});

test("private lifecycle rejects missing, malformed, oversized, and wrong-key view maps", async () => {
  const cases = [
    { name: "missing", playerViews: { "seat-a": { hand: ["ACE_OF_SPADES"] } }, status: 500 },
    { name: "extra", playerViews: { "seat-a": {}, "seat-b": {}, "seat-c": {} }, status: 500 },
    { name: "null-map", playerViews: null, status: 500 },
    { name: "not-an-object", playerViews: [], status: 500 },
    { name: "malformed-json-value", playerViews: { "seat-a": 1n, "seat-b": {} }, status: 500 },
    { name: "oversized-view", playerViews: { "seat-a": "x".repeat(65 * 1024), "seat-b": {} }, status: 413 },
  ];
  for (const testCase of cases) {
    await withServer(privateGame(undefined, { init: () => ({ state: {}, playerViews: testCase.playerViews }) }), async (baseUrl) => {
      const response = await post(baseUrl, "/init", fixture.lifecycle.initRequest);
      assert.equal(response.status, testCase.status, testCase.name);
    });
  }

  const manyPlayers = Array.from({ length: 17 }, (_, index) => `seat-${String(index).padStart(2, "0")}`);
  const views = Object.fromEntries(manyPlayers.map((playerId) => [playerId, "x".repeat(62000)]));
  const config = { ...fixture.lifecycle.initRequest, players: manyPlayers };
  await withServer(privateGame({ ...fixture.manifestCases[1].manifest, maxPlayers: 20 }, { init: () => ({ state: {}, playerViews: views }) }), async (baseUrl) => {
    const response = await post(baseUrl, "/init", config);
    assert.equal(response.status, 413, "whole map over 1 MiB");
  });
});


test("real HTTP and WebSocket callbacks share defensive post-event rosters through replay and empty-seat recovery", async () => {
  const callbackRosters = [];
  const remember = (name, players) => {
    callbackRosters.push([name, players === undefined ? undefined : [...players]]);
  };
  const game = privateGame(undefined, {
    init: config => ({ state: { players: [...config.players] }, playerViews: viewsFor(config.players) }),
    tick: (_ctx, _state, players) => {
      remember("tick", players);
      return { state: { players: [...players] }, playerViews: viewsFor(players), persist: false };
    },
    onPlayerJoin: (_playerId, state, players) => {
      remember("join", players);
      return { state, playerViews: viewsFor(players) };
    },
    onPlayerLeave: (_playerId, state, players) => {
      remember("leave", players);
      return { state, playerViews: viewsFor(players) };
    },
    onCommand: (_command, state, players) => {
      remember("command", players);
      return { state, playerViews: viewsFor(players) };
    },
    isGameOver: state => state.players.length === 0
      ? { over: true, summary: { durationSeconds: 0 }, nextMatchPolicy: { mode: "close_room", timeoutMs: 0 } }
      : { over: false },
  });

  await withServer(game, async baseUrl => {
    const tickRequest = { ctx: { tick: 1, inputs: [], commands: [], elapsedSeconds: 0.2 }, state: {}, players: [] };
    const emptyTick = await post(baseUrl, "/tick", tickRequest);
    assert.equal(emptyTick.status, 200);
    assert.equal((await post(baseUrl, "/tick", { ...tickRequest, players: null })).status, 400);
    const emptyTickBody = await emptyTick.json();
    assert.deepEqual(emptyTickBody.playerViews, {});
    assert.equal(Object.hasOwn(emptyTickBody, "playerViews"), true);
    assert.equal((await post(baseUrl, "/tick", { ...tickRequest, players: undefined })).status, 400);
    const emptyLeave = await post(baseUrl, "/onPlayerLeave", { playerId: "seat-a", state: {}, players: [] });
    assert.equal(emptyLeave.status, 200);
    assert.deepEqual((await emptyLeave.json()).playerViews, {});
    const emptyCommand = await post(baseUrl, "/onCommand", {
      command: { type: "reset", data: {} }, state: {}, players: [],
    });
    assert.equal(emptyCommand.status, 200);

    await withWebSocket(new URL(baseUrl).port, async send => {
      const hydrated = await send({
        type: "hydrate", matchId: "private-replay", tickRateHz: 10, tick: 0,
        state: { players: ["seat-a", "seat-b"] }, players: ["seat-a", "seat-b"],
        playerViews: viewsFor(["seat-a", "seat-b"]),
        replay: [{ tick: 1, events: [
          { type: "join", playerId: "seat-c" },
          { type: "leave", playerId: "seat-a" },
          { type: "command", command: { type: "reset", data: {} } },
        ] }],
      });
      assert.equal(hydrated.type, "hydrated");
      const terminal = await send({
        type: "step", tick: 2, events: [
          { type: "leave", playerId: "seat-b" },
          { type: "leave", playerId: "seat-c" },
        ],
      });
      assert.equal(terminal.type, "stepResult");
      assert.deepEqual(terminal.playerViews, {});
      assert.equal(terminal.gameResult.over, true);
      const recovered = await send(emptyRosterFixture.hydrate);
      assert.equal(recovered.type, "hydrated");
      assert.deepEqual(recovered.playerViews, {});
      assert.equal(recovered.gameResult.over, true);
      const omittedRoster = await send({
        type: "hydrate", matchId: "missing-roster", tickRateHz: 10, tick: 2,
        state: { players: [] }, playerViews: {},
      });
      assert.equal(omittedRoster.code, "WRONG_ROSTER");
      const nullRoster = await send({
        type: "hydrate", matchId: "null-roster", tickRateHz: 10, tick: 2,
        state: { players: [] }, players: null, playerViews: {},
      });
      assert.equal(nullRoster.code, "WRONG_ROSTER");
    });
  });

  assert.deepEqual(callbackRosters, [
    ["tick", []], ["leave", []], ["command", []],
    ["join", ["seat-a", "seat-b", "seat-c"]],
    ["leave", ["seat-b", "seat-c"]], ["command", ["seat-b", "seat-c"]],
    ["tick", ["seat-b", "seat-c"]], ["leave", ["seat-c"]], ["leave", []], ["tick", []],
  ]);
});

test("malformed request JSON and invalid manifest versions fail closed over HTTP", async () => {
  await withServer(privateGame(), async (baseUrl) => {
    assert.equal((await post(baseUrl, "/tick", "{" )).status, 400);
  });
  const invalid = { ...fixture.manifestCases[2].manifest, playerViewSchemaVersion: null };
  await withServer(privateGame(invalid), async (baseUrl) => {
    assert.equal((await fetch(`${baseUrl}/manifest`)).status, 500);
  });
});

process.on("exit", () => {
  if (originalGameId === undefined) delete process.env.RANDSEED_GAME_ID;
  else process.env.RANDSEED_GAME_ID = originalGameId;
});
