import { pathToFileURL } from "node:url";
import { createGameServer, SdkProtocolError } from "../dist/index.js";

function playerViews(state) {
  return Object.fromEntries(
    state.players.map(playerId => [
      playerId,
      {
        tick: state.tick,
        count: state.count,
        ownScore: state.privateScores[playerId] ?? 0,
      },
    ]),
  );
}

export const game = {
  manifest: () => ({
    title: "Authoritative Counter",
    version: "1.0.0",
    minPlayers: 2,
    maxPlayers: 4,
    tickRateHz: 10,
    persistentTable: true,
    playerViewMode: "private",
    optionsSchema: {
      type: "object",
      properties: { targetScore: { type: "integer", minimum: 1, maximum: 100 } },
      additionalProperties: false,
    },
  }),

  init(config) {
    const hasTargetScore = Object.hasOwn(config.options ?? {}, "targetScore");
    const targetScore = hasTargetScore ? config.options.targetScore : 5;
    if (hasTargetScore && (!Number.isSafeInteger(targetScore) || targetScore < 1 || targetScore > 100)) {
      throw new SdkProtocolError("INVALID_OPTIONS", "targetScore must be an integer from 1 to 100.");
    }
    const state = {
      version: 1,
      tick: 0,
      count: 0,
      targetScore,
      players: [...config.players],
      privateScores: Object.fromEntries(config.players.map(playerId => [playerId, 0])),
    };
    return { state, playerViews: playerViews(state), joinable: true };
  },

  tick(ctx, state) {
    const next = structuredClone(state);
    next.tick = ctx.tick;
    const actionResults = [];
    for (const input of ctx.inputs) {
      let status = "applied";
      let code;
      const delta = input.data?.delta;
      if (!next.players.includes(input.playerId)) {
        status = "rejected";
        code = "WRONG_ROSTER";
      } else if (!Number.isSafeInteger(delta)) {
        status = "rejected";
        code = "INVALID_MOVE";
      } else {
        next.count += delta;
        next.privateScores[input.playerId] =
          (next.privateScores[input.playerId] ?? 0) + delta;
      }
      if (input.actionId) {
        actionResults.push({
          actionId: input.actionId,
          status,
          ...(code === undefined ? {} : { code }),
        });
      }
    }
    return {
      state: next,
      playerViews: playerViews(next),
      persist: next.count >= next.targetScore,
      actionResults,
    };
  },

  onPlayerJoin(playerId, state) {
    const next = structuredClone(state);
    if (!next.players.includes(playerId)) next.players.push(playerId);
    next.privateScores[playerId] ??= 0;
    return {
      state: next,
      playerViews: playerViews(next),
      joinable: next.players.length < 4,
    };
  },

  onPlayerLeave(playerId, state) {
    const next = structuredClone(state);
    next.players = next.players.filter(activeId => activeId !== playerId);
    delete next.privateScores[playerId];
    return { state: next, playerViews: playerViews(next), joinable: true };
  },

  isGameOver(state) {
    const over = state.count >= state.targetScore;
    return over
      ? {
          over: true,
          summary: { durationSeconds: state.tick / 10 },
          nextMatchPolicy: { mode: "close_room", timeoutMs: 0 },
        }
      : { over: false };
  },
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  createGameServer(game, Number(process.env.PORT ?? 8080));
}
