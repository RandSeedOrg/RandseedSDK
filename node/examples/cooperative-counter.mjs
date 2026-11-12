import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createGameServer } from "../dist/index.js";

export const cooperativeCounter = {
  manifest: () => ({
    title: "Cooperative Counter",
    version: "1.0.0",
    minPlayers: 2,
    maxPlayers: 4,
    tickRateHz: 5,
    persistentTable: false,
  }),

  init: config => ({
    state: {
      schemaVersion: 1,
      matchId: config.matchId,
      players: [...config.players],
      score: 0,
      elapsedSeconds: 0,
    },
    joinable: config.players.length < 4,
    broadcasts: [{ type: "counter.started", score: 0 }],
  }),

  tick: (ctx, currentState) => {
    const state = {
      ...currentState,
      players: [...currentState.players],
      elapsedSeconds: ctx.elapsedSeconds,
    };
    const actionResults = [];

    for (const input of ctx.inputs) {
      let result;
      if (!state.players.includes(input.playerId)) {
        result = { status: "rejected", code: "not_in_match" };
      } else {
        const delta = input.data?.delta;
        if (!Number.isSafeInteger(delta) || delta < 1 || delta > 3) {
          result = { status: "rejected", code: "invalid_delta" };
        } else {
          state.score += delta;
          if (!Number.isSafeInteger(state.score)) {
            throw new Error("The counter score is out of range.");
          }
          result = { status: "applied" };
        }
      }

      if (input.actionId) {
        actionResults.push({ actionId: input.actionId, ...result });
      }
    }

    return {
      state,
      persist: state.score >= 20,
      joinable: state.players.length < 4 && state.score < 20,
      broadcasts: ctx.inputs.length > 0
        ? [{ type: "counter.changed", score: state.score }]
        : [],
      ...(actionResults.length > 0 ? { actionResults } : {}),
    };
  },

  onPlayerJoin: (playerId, currentState) => {
    const players = currentState.players.includes(playerId)
      ? [...currentState.players]
      : [...currentState.players, playerId];
    return {
      state: { ...currentState, players },
      joinable: players.length < 4 && currentState.score < 20,
      broadcasts: [{ type: "player.joined", playerId }],
    };
  },

  onPlayerLeave: (playerId, currentState) => {
    const players = currentState.players.filter(id => id !== playerId);
    return {
      state: { ...currentState, players },
      joinable: players.length < 4 && currentState.score < 20,
      broadcasts: [{ type: "player.left", playerId }],
    };
  },

  isGameOver: state => state.score >= 20
    ? {
        over: true,
        summary: { durationSeconds: state.elapsedSeconds },
        nextMatchPolicy: { mode: "close_room", timeoutMs: 0 },
      }
    : { over: false },

  onCommand: (command, currentState) => {
    if (command.type !== "reset") {
      throw new Error("Unsupported trusted command.");
    }
    return {
      state: { ...currentState, score: 0, elapsedSeconds: 0 },
      joinable: currentState.players.length < 4,
      broadcasts: [{ type: "counter.reset" }],
    };
  },
};

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const port = Number(process.env.PORT ?? 8080);
  if (!Number.isSafeInteger(port) || port < 1 || port > 65535) {
    throw new Error("PORT must be an integer between 1 and 65535.");
  }

  createGameServer(cooperativeCounter, port, {
    ...(process.env.RANDSEED_GAME_ID ? { gameId: process.env.RANDSEED_GAME_ID } : {}),
  });
}
