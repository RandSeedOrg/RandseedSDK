import { SdkProtocolError } from "./errors.js";
import { isValidActionCode, isValidProtocolKey } from "./protocol-identifiers.js";
import { validateGameResultUnits } from "./result-units.js";
import type { GameActionResult, TickInput } from "./types.js";
import { isRecord, validateJsonValue } from "./validators.js";

export const MAX_PLAYER_VIEW_BYTES = 64 * 1024;
const MAX_PLAYER_VIEWS_BYTES = 1024 * 1024;

function validateJson(value: unknown, code: "INVALID_STATE" | "INVALID_RESULT"): void {
  try {
    validateJsonValue(value);
  } catch {
    throw new SdkProtocolError(code);
  }
}

function validateUnits(value: unknown): void {
  try {
    validateGameResultUnits(value);
  } catch (error) {
    const message = error instanceof Error ? error.message : "";
    throw new SdkProtocolError(
      /limit|exceeds/i.test(message) ? "RESOURCE_LIMIT_EXCEEDED" : "INVALID_RESULT",
    );
  }
}

function validatePlayerViews(
  value: unknown,
  players?: readonly string[],
  required = false,
): void {
  if (value === undefined) {
    if (required) throw new SdkProtocolError("INVALID_RESULT");
    return;
  }
  if (!isRecord(value)) {
    throw new SdkProtocolError("INVALID_RESULT", "playerViews must be an object keyed by playerId.");
  }
  if (players === undefined) {
    throw new SdkProtocolError("WRONG_ROSTER", "playerViews require an active players roster.");
  }
  const expected = new Set(players);
  const actual = Object.keys(value);
  if (actual.length !== expected.size || actual.some(playerId => !expected.has(playerId))) {
    throw new SdkProtocolError("INVALID_RESULT", "playerViews must contain exactly one entry per active roster seat.");
  }

  let totalBytes = 0;
  for (const [playerId, view] of Object.entries(value)) {
    if (!playerId.trim()) throw new SdkProtocolError("INVALID_RESULT", "playerViews keys must be non-empty player IDs.");
    validateJson(view, "INVALID_RESULT");
    const bytes = new TextEncoder().encode(JSON.stringify(view)).byteLength;
    if (bytes > MAX_PLAYER_VIEW_BYTES) throw new SdkProtocolError("RESOURCE_LIMIT_EXCEEDED");
    totalBytes += bytes;
  }
  if (totalBytes > MAX_PLAYER_VIEWS_BYTES) {
    throw new SdkProtocolError("RESOURCE_LIMIT_EXCEEDED");
  }
}

export function validateGameStateResult(
  value: unknown,
  players?: readonly string[],
  requirePlayerViews = false,
): asserts value is Record<string, unknown> & { state: unknown } {
  if (!isRecord(value) || !Object.hasOwn(value, "state")) {
    throw new SdkProtocolError("INVALID_STATE", "A game callback must return a state property.");
  }
  validateJson(value.state, "INVALID_STATE");
  validatePlayerViews(value.playerViews, players, requirePlayerViews);
  if (value.broadcasts !== undefined) {
    if (!Array.isArray(value.broadcasts)) {
      throw new SdkProtocolError("INVALID_RESULT", "broadcasts must be an array when provided.");
    }
    validateJson(value.broadcasts, "INVALID_RESULT");
  }
  if (value.joinable !== undefined && typeof value.joinable !== "boolean") {
    throw new SdkProtocolError("INVALID_RESULT", "joinable must be a boolean when provided.");
  }
  validateUnits(value.resultUnits);
  validateJson(value, "INVALID_RESULT");
}

export function validateGameTickResult(
  value: unknown,
  inputs: readonly TickInput[] = [],
  players?: readonly string[],
  requirePlayerViews = false,
): asserts value is Record<string, unknown> & {
  state: unknown;
  persist: boolean;
  actionResults?: GameActionResult[];
} {
  validateGameStateResult(value, players, requirePlayerViews);
  if (!isRecord(value) || typeof value.persist !== "boolean") {
    throw new SdkProtocolError("INVALID_RESULT", "TickResult.persist must be a boolean.");
  }
  if (value.actionResults === undefined) return;
  if (!Array.isArray(value.actionResults)) {
    throw new SdkProtocolError("INVALID_RESULT", "actionResults must be an array when provided.");
  }
  const submittedIds = new Set(
    inputs.flatMap(input => input.actionId === undefined ? [] : [input.actionId]),
  );
  if (players !== undefined && inputs.some(input => !players.includes(input.playerId))) {
    throw new SdkProtocolError("WRONG_ROSTER");
  }
  const reportedIds = new Set<string>();
  for (const result of value.actionResults) {
    if (!isRecord(result)
      || !isValidProtocolKey(result.actionId)
      || (result.status !== "applied" && result.status !== "rejected")
      || (result.code !== undefined && !isValidActionCode(result.code))
      || reportedIds.has(result.actionId)
      || !submittedIds.has(result.actionId)) {
      throw new SdkProtocolError("INVALID_RESULT", "actionResults must refer to unique submitted action IDs.");
    }
    reportedIds.add(result.actionId);
  }
}

export function validateGameResult(value: unknown): asserts value is {
  over: boolean;
  summary?: Record<string, unknown>;
  nextMatchPolicy?: Record<string, unknown>;
  resultUnits?: unknown;
} {
  if (!isRecord(value) || typeof value.over !== "boolean") {
    throw new SdkProtocolError("INVALID_RESULT", "GameResult.over must be a boolean.");
  }
  validateJson(value, "INVALID_RESULT");
  validateUnits(value.resultUnits);

  if (value.summary !== undefined) {
    if (!isRecord(value.summary)
      || typeof value.summary.durationSeconds !== "number"
      || !Number.isFinite(value.summary.durationSeconds)
      || value.summary.durationSeconds < 0) {
      throw new SdkProtocolError("INVALID_GAME_OVER_POLICY", "summary must include a finite non-negative durationSeconds.");
    }
  }

  if (value.nextMatchPolicy !== undefined) {
    const policy = value.nextMatchPolicy;
    if (!isRecord(policy)
      || !["auto_next_match", "wait_for_ready", "close_room"].includes(String(policy.mode))
      || !Number.isSafeInteger(policy.timeoutMs)
      || typeof policy.timeoutMs !== "number"
      || policy.timeoutMs < 0) {
      throw new SdkProtocolError("INVALID_GAME_OVER_POLICY");
    }
  }

  if (value.over && (value.summary === undefined || value.nextMatchPolicy === undefined)) {
    throw new SdkProtocolError("INVALID_GAME_OVER_POLICY");
  }
}
