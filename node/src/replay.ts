/** Deterministic replay validation and storage tier helpers. */

import { gzipSync } from "node:zlib";
import type { MatchConfig, MatchSummary, TickContext, TickResult } from "./types.js";
import { validateMatchConfig } from "./validators.js";

/** A player input recorded in a replay. */
export interface ReplayInputItem {
  /** Tick on which the input occurred. */
  tick: number;
  /** Unique player ID. */
  playerId: string;
  /** Player input payload. */
  data: unknown;
  /** Optional report timestamp in milliseconds. */
  timestamp?: number;
}

/** A trusted command recorded in a replay. */
export interface ReplayCommandItem {
  /** Tick on which the command took effect. */
  tick: number;
  /** Command type. */
  type: string;
  /** Command payload. */
  data: unknown;
  /** Optional dispatch timestamp in milliseconds. */
  timestamp?: number;
}

/** Version 2 deterministic game replay data. */
export interface ReplayDataV2 {
  /** Replay format version. */
  version: 2;
  /** Unique game ID. */
  gameId: string;
  /** Unique match ID. */
  matchId: string;
  /** Copy of the match configuration. */
  matchConfig: MatchConfig;
  /** Initial state snapshot. */
  initState: unknown;
  /** Player inputs ordered by non-decreasing tick. */
  inputs: Array<ReplayInputItem>;
  /** Trusted commands. */
  commands: Array<ReplayCommandItem>;
  /** Match summary. */
  summary: MatchSummary;
  /** Replay creation timestamp in milliseconds. */
  createdAt: number;
}

/** Compressed-size threshold for inline Tier A storage. */
export const TIER_A_MAX_COMPRESSED_BYTES = 1.8 * 1024 * 1024;
/** Compressed-size threshold for Tier B storage. */
export const TIER_B_MAX_COMPRESSED_BYTES = 10 * 1024 * 1024;

export interface ReplayStorageTierEvaluation {
  tier: "TierA" | "TierB" | "TierC";
  uncompressedBytes: number;
  compressedBytes: number;
  compressionRatio: number;
  compressedBuffer: Buffer;
}

/** Validates the replay shape and non-decreasing tick order. */
export function validateReplayDataV2(replay: unknown): asserts replay is ReplayDataV2 {
  if (!replay || typeof replay !== "object") {
    throw new Error("ReplayDataV2 must be an object.");
  }
  const r = replay as Record<string, unknown>;
  if (r.version !== 2) {
    throw new Error(`Replay version must be 2; received ${String(r.version)}.`);
  }
  if (typeof r.gameId !== "string" || r.gameId.length === 0) {
    throw new Error("gameId is required.");
  }
  if (typeof r.matchId !== "string" || r.matchId.length === 0) {
    throw new Error("matchId is required.");
  }
  if (!r.matchConfig || typeof r.matchConfig !== "object") {
    throw new Error("matchConfig is required.");
  }
  validateMatchConfig(r.matchConfig);
  if (r.matchConfig.matchId !== r.matchId || r.matchConfig.gameId !== r.gameId) {
    throw new Error("matchConfig must match the replay game and match IDs.");
  }
  if (r.initState === undefined) {
    throw new Error("initState is required.");
  }
  if (!Array.isArray(r.inputs)) {
    throw new Error("inputs must be an array.");
  }
  if (!Array.isArray(r.commands)) {
    throw new Error("commands must be an array.");
  }
  if (!r.summary || typeof r.summary !== "object") {
    throw new Error("summary is required.");
  }
  if (typeof r.createdAt !== "number" || r.createdAt <= 0) {
    throw new Error("createdAt must be a positive timestamp.");
  }

  let prevTick = -1;
  for (const input of r.inputs as Array<Record<string, unknown>>) {
    if (typeof input.tick !== "number" || !Number.isSafeInteger(input.tick) || input.tick < 0) {
      throw new Error("input.tick must be a non-negative safe integer.");
    }
    if (input.tick < prevTick) {
      throw new Error(`inputs must be ordered by non-decreasing tick: ${input.tick} < ${prevTick}.`);
    }
    if (typeof input.playerId !== "string") {
      throw new Error("input.playerId must be a string.");
    }
    if (!("data" in input)) {
      throw new Error("input.data is required.");
    }
    prevTick = input.tick;
  }

  const commands = (r.commands || []) as Array<Record<string, unknown>>;
  for (const cmd of commands) {
    if (typeof cmd.tick !== "number" || !Number.isSafeInteger(cmd.tick) || cmd.tick < 0) {
      throw new Error("command.tick must be a non-negative safe integer.");
    }
    if (typeof cmd.type !== "string") {
      throw new Error("command.type must be a string.");
    }
    if (!("data" in cmd)) {
      throw new Error("command.data is required.");
    }
  }
}

/** Calculates the compressed size and recommended storage tier. */
export function evaluateReplayStorageTier(replay: ReplayDataV2): ReplayStorageTierEvaluation {
  validateReplayDataV2(replay);
  const rawJson = JSON.stringify(replay);
  const uncompressedBytes = Buffer.byteLength(rawJson, "utf8");
  const compressedBuffer = gzipSync(Buffer.from(rawJson, "utf8"));
  const compressedBytes = compressedBuffer.length;

  let tier: "TierA" | "TierB" | "TierC";
  if (compressedBytes < TIER_A_MAX_COMPRESSED_BYTES) {
    tier = "TierA";
  } else if (compressedBytes < TIER_B_MAX_COMPRESSED_BYTES) {
    tier = "TierB";
  } else {
    tier = "TierC";
  }

  return {
    tier,
    uncompressedBytes,
    compressedBytes,
    compressionRatio: uncompressedBytes / compressedBytes,
    compressedBuffer,
  };
}

/** Splits replay data into Tier B relational rows. */
export function splitReplayForTierB(replay: ReplayDataV2) {
  validateReplayDataV2(replay);
  const maxTick = replay.commands.reduce(
    (max, command) => Math.max(max, command.tick),
    replay.inputs.reduce((max, input) => Math.max(max, input.tick), 0),
  );

  const replayRow = {
    match_id: replay.matchId,
    game_id: replay.gameId,
    version: 2,
    seed: replay.matchConfig.options?.seed ?? 0,
    total_ticks: maxTick,
    init_state_json: JSON.stringify(replay.initState),
    match_config_json: JSON.stringify(replay.matchConfig),
    commands_json: JSON.stringify(replay.commands),
    summary_json: JSON.stringify(replay.summary),
    created_at: replay.createdAt,
  };

  const inputRows = replay.inputs.map(input => {
    const payload: { data: unknown; timestamp?: number } = { data: input.data };
    if (input.timestamp !== undefined) {
      payload.timestamp = input.timestamp;
    }
    return {
      match_id: replay.matchId,
      tick: input.tick,
      player_id: input.playerId,
      input_json: JSON.stringify(payload),
    };
  });

  return { replayRow, inputRows };
}

/** Reconstructs replay data from Tier B relational rows. */
export function assembleReplayFromTierB(
  replayRow: {
    match_id: string;
    game_id: string;
    seed: number;
    init_state_json: string;
    match_config_json: string;
    commands_json: string;
    summary_json: string;
    created_at: number;
  },
  inputRows: Array<{
    match_id: string;
    tick: number;
    player_id: string;
    input_json: string;
  }>
): ReplayDataV2 {
  if (!replayRow || !replayRow.match_id) {
    throw new Error("replayRow is required.");
  }

  if (typeof replayRow.match_config_json !== "string" || typeof replayRow.commands_json !== "string") {
    throw new Error("Tier B replay is missing match configuration or commands.");
  }
  const inputs: Array<ReplayInputItem> = inputRows.map(r => {
    if (r.match_id !== replayRow.match_id) {
      throw new Error("Tier B input belongs to a different match.");
    }
    const parsed = JSON.parse(r.input_json) as { data: unknown; timestamp?: number };
    const item: ReplayInputItem = {
      tick: r.tick,
      playerId: r.player_id,
      data: parsed.data,
    };
    if (parsed.timestamp !== undefined) {
      item.timestamp = parsed.timestamp;
    }
    return item;
  });

  inputs.sort((a, b) => a.tick - b.tick);

  const replay: ReplayDataV2 = {
    version: 2,
    gameId: replayRow.game_id,
    matchId: replayRow.match_id,
    matchConfig: JSON.parse(replayRow.match_config_json) as MatchConfig,
    initState: JSON.parse(replayRow.init_state_json),
    inputs,
    commands: JSON.parse(replayRow.commands_json) as Array<ReplayCommandItem>,
    summary: JSON.parse(replayRow.summary_json) as MatchSummary,
    createdAt: replayRow.created_at,
  };
  validateReplayDataV2(replay);
  if (replay.matchConfig.matchId !== replay.matchId || replay.matchConfig.gameId !== replay.gameId) {
    throw new Error("Tier B match configuration does not match replay metadata.");
  }
  return replay;
}

/** Replays recorded inputs through a deterministic tick function. */
export async function executeDeterministicReplay<TState = unknown, TInput = unknown, TCommand = unknown>(
  replay: ReplayDataV2,
  tickFn: (ctx: TickContext<TInput, TCommand>, state: TState) => Promise<TickResult<TState>> | TickResult<TState>
): Promise<TState> {
  validateReplayDataV2(replay);

  const inputsByTick = new Map<number, Array<{ playerId: string; data: TInput; timestamp?: number }>>();
  for (const input of replay.inputs) {
    const list = inputsByTick.get(input.tick) || [];
    list.push(input as { playerId: string; data: TInput; timestamp?: number });
    inputsByTick.set(input.tick, list);
  }

  const commandsByTick = new Map<number, Array<{ type: string; data: TCommand; timestamp?: number }>>();
  for (const cmd of replay.commands || []) {
    const list = commandsByTick.get(cmd.tick) || [];
    list.push(cmd as { type: string; data: TCommand; timestamp?: number });
    commandsByTick.set(cmd.tick, list);
  }

  const maxTick = replay.commands.reduce(
    (max, command) => Math.max(max, command.tick),
    replay.inputs.reduce((max, input) => Math.max(max, input.tick), 0),
  );

  const tickRate = replay.matchConfig.tickRateHz || 20;
  let currentState = structuredClone(replay.initState) as TState;

  for (let tick = 0; tick <= maxTick; tick++) {
    const frameInputs = inputsByTick.get(tick) || [];
    const frameCommands = commandsByTick.get(tick) || [];
    const ctx: TickContext<TInput, TCommand> = {
      tick,
      inputs: frameInputs,
      commands: frameCommands,
      elapsedSeconds: tick / tickRate,
    };
    const res = await tickFn(ctx, currentState);
    currentState = res.state;
  }

  return currentState;
}
