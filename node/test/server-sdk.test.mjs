/** Tests for the Node.js server SDK, platform client, PRNG, and replay helpers. */

import { test, describe, before, after } from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { randomBytes } from "node:crypto";
import {
  createGameServer,
  validateGameManifest,
  validateMatchConfig,
  validateTickContext,
  mulberry32,
  mulberry32U32,
  PlatformClient,
  validateReplayDataV2,
  evaluateReplayStorageTier,
  splitReplayForTierB,
  assembleReplayFromTierB,
  executeDeterministicReplay,
} from "../dist/index.js";

describe("Node.js Server SDK", () => {
  let server;
  let serverPort;
  let baseUrl;

  // Use a deterministic game implementation for endpoint tests.
  const mockGame = {
    manifest() {
      return {
        title: "Test Game",
        version: "1.0.0",
        minPlayers: 1,
        maxPlayers: 4,
        tickRateHz: 20,
        persistentTable: true,
        optionsSchema: { seed: { type: "number" } },
      };
    },
    init(config) {
      return {
        state: {
          matchId: config.matchId,
          score: 0,
          tickCount: 0,
          players: [...config.players],
          carryover: config.carryover || null,
        },
        broadcasts: [{ type: "game_initialized", matchId: config.matchId }],
      };
    },
    tick(ctx, state) {
      const isPersist = ctx.tick % 50 === 0;
      const addedScore = ctx.inputs.length > 0 ? 10 : 1;
      return {
        state: {
          ...state,
          tickCount: ctx.tick,
          score: (state.score || 0) + addedScore,
        },
        broadcasts: ctx.inputs.length > 0 ? [{ type: "action", count: ctx.inputs.length }] : [],
        persist: isPersist,
      };
    },
    onPlayerJoin(playerId, state) {
      return {
        state: {
          ...state,
          players: [...(state.players || []), playerId],
        },
        broadcasts: [{ type: "player_joined", playerId }],
      };
    },
    onPlayerLeave(playerId, state) {
      return {
        state: {
          ...state,
          players: (state.players || []).filter((p) => p !== playerId),
        },
        broadcasts: [{ type: "player_left", playerId }],
      };
    },
    isGameOver(state) {
      if ((state.tickCount || 0) >= 100) {
        return {
          over: true,
          summary: {
            winnerPlayerId: state.players?.[0] || "p1",
            scores: { [state.players?.[0] || "p1"]: state.score || 100 },
            durationSeconds: 5.0,
          },
          nextMatchPolicy: {
            mode: "auto_next_match",
            timeoutMs: 0,
            carryover: { lastScore: state.score || 100 },
          },
        };
      }
      return { over: false };
    },
    onCommand(cmd, state) {
      return {
        state: {
          ...state,
          lastCommand: cmd,
        },
        broadcasts: [{ type: "command_applied", cmdType: cmd.type }],
      };
    },
  };

  before(async () => {
    // Listen on an ephemeral port to avoid conflicts with parallel tests.
    server = createGameServer(mockGame, 0, { gameId: "test-card-game" });
    await new Promise((resolve) => server.once("listening", resolve));
    serverPort = server.address().port;
    baseUrl = `http://127.0.0.1:${serverPort}`;
  });

  after(async () => {
    if (server) {
      await new Promise((resolve) => server.close(resolve));
    }
  });

  describe("Room Host Protocol endpoints", () => {
    test("GET /manifest returns the game metadata", async () => {
      const res = await fetch(`${baseUrl}/manifest`);
      assert.equal(res.status, 200);
      const data = await res.json();
      validateGameManifest(data);
      assert.equal(data.title, "Test Game");
      assert.equal(data.tickRateHz, 20);
    });

    test("GET /health returns a healthy status", async () => {
      const res = await fetch(`${baseUrl}/health`);
      assert.equal(res.status, 200);
      const data = await res.json();
      assert.equal(data.status, "ok");
      assert.equal(data.gameId, "test-card-game");
      assert.equal(typeof data.uptime, "number");
    });

    test("POST /init initializes a match", async () => {
      const config = {
        matchId: "m_1001",
        gameId: "test-card-game",
        players: ["player_a", "player_b"],
        matchSeq: 1,
        options: { seed: 123456 },
      };
      validateMatchConfig(config);

      const res = await fetch(`${baseUrl}/init`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(config),
      });
      assert.equal(res.status, 200);
      const data = await res.json();
      assert.equal(data.state.matchId, "m_1001");
      assert.equal(data.state.score, 0);
      assert.equal(data.broadcasts.length, 1);
    });

    test("POST /tick advances the state and returns a persist flag", async () => {
      const reqPayload = {
        ctx: {
          tick: 50,
          inputs: [{ playerId: "player_a", data: { move: "up" } }],
          elapsedSeconds: 2.5,
        },
        state: { matchId: "m_1001", score: 0, tickCount: 49 },
      };
      validateTickContext(reqPayload.ctx);

      const res = await fetch(`${baseUrl}/tick`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(reqPayload),
      });
      assert.equal(res.status, 200);
      const data = await res.json();
      assert.equal(data.state.tickCount, 50);
      assert.equal(data.state.score, 10);
      assert.equal(data.persist, true);
      assert.equal(data.broadcasts.length, 1);
    });

    test("POST /onPlayerJoin processes a joining player", async () => {
      const reqPayload = {
        playerId: "player_c",
        state: { matchId: "m_1001", players: ["player_a", "player_b"] },
      };

      const res = await fetch(`${baseUrl}/onPlayerJoin`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(reqPayload),
      });
      assert.equal(res.status, 200);
      const data = await res.json();
      assert.deepEqual(data.state.players, ["player_a", "player_b", "player_c"]);
      assert.equal(data.broadcasts[0].type, "player_joined");
    });

    test("POST /onPlayerLeave processes a departing player", async () => {
      const reqPayload = {
        playerId: "player_b",
        state: { matchId: "m_1001", players: ["player_a", "player_b", "player_c"] },
      };

      const res = await fetch(`${baseUrl}/onPlayerLeave`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(reqPayload),
      });
      assert.equal(res.status, 200);
      const data = await res.json();
      assert.deepEqual(data.state.players, ["player_a", "player_c"]);
      assert.equal(data.broadcasts[0].type, "player_left");
    });

    test("POST /isGameOver returns over false before the match ends", async () => {
      const res = await fetch(`${baseUrl}/isGameOver`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ state: { tickCount: 20 } }),
      });
      assert.equal(res.status, 200);
      const data = await res.json();
      assert.equal(data.over, false);
    });

    test("POST /isGameOver returns the summary and next-match policy", async () => {
      const res = await fetch(`${baseUrl}/isGameOver`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ state: { tickCount: 100, score: 350, players: ["player_a"] } }),
      });
      assert.equal(res.status, 200);
      const data = await res.json();
      assert.equal(data.over, true);
      assert.equal(data.summary.winnerPlayerId, "player_a");
      assert.equal(data.nextMatchPolicy.mode, "auto_next_match");
    });

    test("POST /onCommand executes a trusted host command", async () => {
      const res = await fetch(`${baseUrl}/onCommand`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          command: { type: "admin_pause", data: { reason: "inspect" } },
          state: { matchId: "m_1001" },
        }),
      });
      assert.equal(res.status, 200);
      const data = await res.json();
      assert.equal(data.state.lastCommand.type, "admin_pause");
    });

    test("unknown routes return 404", async () => {
      const res = await fetch(`${baseUrl}/non_existent_route`);
      assert.equal(res.status, 404);
      const data = await res.json();
      assert.ok(data.error.includes("Endpoint not found"));
    });

    test("invalid request bodies return 400", async () => {
      const res = await fetch(`${baseUrl}/init`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ matchId: "" }),
      });
      assert.equal(res.status, 400);
      const data = await res.json();
      assert.ok(data.error);
    });

    test("invalid match sequences and duplicate players are rejected", async () => {
      for (const config of [
        { matchId: "m1", gameId: "g1", players: ["p1"], matchSeq: 0, options: { seed: 1 } },
        { matchId: "m1", gameId: "g1", players: ["p1", "p1"], matchSeq: 1, options: { seed: 1 } },
      ]) {
        const response = await fetch(`${baseUrl}/init`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(config),
        });
        assert.equal(response.status, 400);
      }
    });

    test("request bodies larger than 1 MiB return 413", async () => {
      const body = JSON.stringify({ payload: "x".repeat(1024 * 1024) });
      const response = await fetch(`${baseUrl}/init`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body,
      });
      assert.equal(response.status, 413);
      assert.deepEqual(await response.json(), {
        error: "Request body exceeds the 1 MiB limit.",
      });
    });

    test("game callback failures return sanitized 500 errors", async () => {
      const failingServer = createGameServer({
        ...mockGame,
        init: () => {
          throw new Error("sensitive internal details");
        },
      }, 0);
      await new Promise((resolve) => failingServer.once("listening", resolve));
      try {
        const response = await fetch(`http://127.0.0.1:${failingServer.address().port}/init`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            matchId: "m1",
            gameId: "g1",
            players: ["p1"],
            matchSeq: 1,
            options: { seed: 1 },
          }),
        });
        assert.equal(response.status, 500);
        assert.deepEqual(await response.json(), { error: "Internal server error." });
      } finally {
        await new Promise((resolve, reject) => {
          failingServer.close((error) => error ? reject(error) : resolve());
        });
      }
    });
  });

  describe("Mulberry32 deterministic PRNG", () => {
    test("the same seed produces the same floating-point sequence", () => {
      const gen1 = mulberry32(987654321);
      const gen2 = mulberry32(987654321);
      for (let i = 0; i < 100; i++) {
        assert.equal(gen1(), gen2());
      }
    });

    test("supports boundary seeds, including negative and fractional values", () => {
      const genZero = mulberry32(0);
      assert.ok(genZero() >= 0 && genZero() < 1);

      const genNeg = mulberry32(-12345);
      assert.ok(genNeg() >= 0 && genNeg() < 1);

      const genFloat = mulberry32(12345.678);
      const genInt = mulberry32(12345);
      assert.equal(genFloat(), genInt());

      const genMaxU32 = mulberry32(0xffffffff);
      assert.ok(genMaxU32() >= 0 && genMaxU32() < 1);
    });

    test("5000 samples remain within a uniform distribution tolerance", () => {
      const gen = mulberry32(42);
      const buckets = new Array(10).fill(0);
      const samples = 5000;
      for (let i = 0; i < samples; i++) {
        const val = gen();
        assert.ok(val >= 0 && val < 1);
        const idx = Math.floor(val * 10);
        buckets[Math.min(idx, 9)]++;
      }
      for (const count of buckets) {
        assert.ok(count >= 350 && count <= 650, `Unexpected bucket count: ${count}`);
      }
    });

    test("mulberry32U32 returns unsigned 32-bit integers", () => {
      const genU32 = mulberry32U32(1337);
      for (let i = 0; i < 50; i++) {
        const n = genU32();
        assert.ok(Number.isInteger(n));
        assert.ok(n >= 0 && n <= 0xffffffff);
      }
    });
  });

  describe("PlatformClient virtual host calls", () => {
    test("SQL requests preserve parameter binding", async () => {
      const mockFetch = async (url, init) => {
        const parsed = JSON.parse(init.body);
        assert.equal(parsed.sql, "SELECT * FROM player_stats WHERE player_id = ?");
        assert.deepEqual(parsed.params, ["user_1001"]);
        return {
          ok: true,
          status: 200,
          json: async () => ({ rows: [{ player_id: "user_1001", rating: 1500 }] }),
        };
      };

      const client = new PlatformClient({ fetch: mockFetch });
      const rows = await client.sql("SELECT * FROM player_stats WHERE player_id = ?", ["user_1001"]);
      assert.equal(rows.length, 1);
      assert.equal(rows[0].rating, 1500);
    });

    test("broadcasts to all connected players", async () => {
      const mockFetch = async (url, init) => {
        const parsed = JSON.parse(init.body);
        assert.deepEqual(parsed.message, { event: "round_start" });
        return {
          ok: true,
          status: 200,
          json: async () => ({ success: true, recipientCount: 3 }),
        };
      };

      const client = new PlatformClient({ fetch: mockFetch });
      const res = await client.broadcast({ event: "round_start" });
      assert.equal(res.success, true);
      assert.equal(res.recipientCount, 3);
    });

    test("sends a direct message to one player", async () => {
      const mockFetch = async (url, init) => {
        const parsed = JSON.parse(init.body);
        assert.equal(parsed.playerId, "player_secret");
        return {
          ok: true,
          status: 200,
          json: async () => ({ success: true, delivered: true }),
        };
      };

      const client = new PlatformClient({ fetch: mockFetch });
      const res = await client.send("player_secret", { privateCard: "Ace" });
      assert.equal(res.success, true);
      assert.equal(res.delivered, true);
    });

    test("lists online players", async () => {
      const mockFetch = async () => ({
        ok: true,
        status: 200,
        json: async () => ({
          players: [
            { playerId: "p1", joinedAt: 1700000000000 },
            { playerId: "p2", joinedAt: 1700000001000 },
          ],
        }),
      });

      const client = new PlatformClient({ fetch: mockFetch });
      const players = await client.getPlayers();
      assert.equal(players.length, 2);
      assert.equal(players[0].playerId, "p1");
    });

    test("reads, writes, and deletes KV values", async () => {
      const kvStore = new Map();
      const mockFetch = async (url, init) => {
        const path = new URL(url).pathname;
        const key = decodeURIComponent(path.replace(/^\//, ""));
        const method = init?.method || "GET";

        if (method === "PUT") {
          const parsed = JSON.parse(init.body);
          kvStore.set(key, parsed.value);
          return { ok: true, status: 200, json: async () => ({ success: true }) };
        }
        if (method === "GET") {
          return { ok: true, status: 200, json: async () => ({ value: kvStore.get(key) ?? null }) };
        }
        if (method === "DELETE") {
          kvStore.delete(key);
          return { ok: true, status: 200, json: async () => ({ success: true }) };
        }
        return { ok: false, status: 400, json: async () => ({ error: "Unknown method" }) };
      };

      const client = new PlatformClient({ fetch: mockFetch });
      // Write the value.
      assert.equal(await client.kvPut("room:phase", "bidding"), true);
      // Read the value.
      assert.equal(await client.kvGet("room:phase"), "bidding");
      // Delete the value.
      assert.equal(await client.kvDelete("room:phase"), true);
      // Confirm the key no longer exists.
      assert.equal(await client.kvGet("room:phase"), null);
    });
  });

  describe("ReplayDataV2 and storage tier decisions", () => {
    const sampleReplay = {
      version: 2,
      gameId: "test-card-game",
      matchId: "m_replay_1",
      matchConfig: {
        matchId: "m_replay_1",
        gameId: "test-card-game",
        players: ["p1", "p2"],
        matchSeq: 1,
        options: { seed: 8888 },
        tickRateHz: 20,
      },
      initState: { score: 0 },
      inputs: [
        { tick: 0, playerId: "p1", data: { action: "ready" } },
        { tick: 10, playerId: "p2", data: { action: "bet", amount: 50 } },
        { tick: 20, playerId: "p1", data: { action: "call" } },
      ],
      commands: [{ tick: 5, type: "admin_notify", data: { msg: "glhf" } }],
      summary: {
        winnerPlayerId: "p1",
        durationSeconds: 1.0,
      },
      createdAt: Date.now(),
    };

    test("ReplayDataV2 validates its shape and input ordering", () => {
      validateReplayDataV2(sampleReplay);
      // Out-of-order ticks must be rejected.
      const invalidReplay = {
        ...sampleReplay,
        inputs: [
          { tick: 10, playerId: "p1", data: {} },
          { tick: 5, playerId: "p2", data: {} },
        ],
      };
      assert.throws(() => validateReplayDataV2(invalidReplay), /inputs must be ordered by non-decreasing tick/);
    });

    test("selects Tier A, Tier B, and Tier C based on compressed size", () => {
      // Small matches fit in Tier A.
      const tierA = evaluateReplayStorageTier(sampleReplay);
      assert.equal(tierA.tier, "TierA");
      assert.ok(tierA.compressionRatio > 0);

      // High-entropy data larger than 1.8 MiB exercises Tier B.
      const highEntropyData = randomBytes(2.2 * 1024 * 1024);
      const tierBReplay = {
        ...sampleReplay,
        initState: { largeBlob: highEntropyData.toString("base64") },
      };
      const tierEval = evaluateReplayStorageTier(tierBReplay);
      assert.ok(tierEval.tier === "TierB" || tierEval.tier === "TierC");
    });

    test("splits and reconstructs Tier B relational rows", () => {
      const { replayRow, inputRows } = splitReplayForTierB(sampleReplay);
      assert.equal(replayRow.match_id, "m_replay_1");
      assert.equal(inputRows.length, 3);

      const assembled = assembleReplayFromTierB(replayRow, inputRows);
      validateReplayDataV2(assembled);
      assert.deepEqual(assembled, sampleReplay);
      assert.throws(() => assembleReplayFromTierB({ ...replayRow, commands_json: undefined }, inputRows), /missing match configuration or commands/);
      assert.throws(() => assembleReplayFromTierB(replayRow, [{ ...inputRows[0], match_id: "other" }]), /different match/);
    });

    test("replays commands after the final input and in command-only matches", async () => {
      for (const inputs of [[], [{ tick: 1, playerId: "p1", data: {} }]]) {
        const replay = {
          ...sampleReplay,
          inputs,
          commands: [{ tick: 5, type: "add", data: 7 }],
        };
        assert.equal(splitReplayForTierB(replay).replayRow.total_ticks, 5);
        const state = await executeDeterministicReplay(replay, (ctx, current) => ({
          state: { score: current.score + (ctx.commands ?? []).reduce((sum, command) => sum + command.data, 0) },
          persist: false,
        }));
        assert.equal(state.score, 7);
      }
    });

    test("executeDeterministicReplay reproduces a match deterministically", async () => {
      const finalState = await executeDeterministicReplay(sampleReplay, (ctx, state) => {
        const added = ctx.inputs.length > 0 ? 10 : 0;
        return {
          state: {
            score: (state.score || 0) + added,
          },
          persist: false,
        };
      });
      // Inputs at ticks 0, 10, and 20 add a total of 30 points.
      assert.equal(finalState.score, 30);
    });
  });
});
