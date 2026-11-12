import type {
  GameActionResult,
  GameCommand,
  GameManifest,
  JsonValue,
  MatchConfig,
  TickContext,
  TickInput,
} from "./types.js";
import {
  isValidActionCode,
  isValidActionSchemaId,
  isValidProtocolKey,
} from "./protocol-identifiers.js";
import { validateGameResultDefinition, validateGameResultUnits } from "./result-units.js";

const NEXT_MATCH_MODES = new Set(["auto_next_match", "wait_for_ready", "close_room"]);

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function validateJsonValue(value: unknown): asserts value is JsonValue {
  const ancestors = new Set<object>();
  const visit = (item: unknown): void => {
    if (item === null || typeof item === "string" || typeof item === "boolean") return;
    if (typeof item === "number" && Number.isFinite(item)) return;
    if (typeof item !== "object") {
      throw new Error("Value must be valid JSON.");
    }
    if (ancestors.has(item)) {
      throw new Error("JSON values must not contain cycles.");
    }

    ancestors.add(item);
    try {
      if (Array.isArray(item)) {
        for (let index = 0; index < item.length; index += 1) {
          if (!Object.hasOwn(item, index)) {
            throw new Error("JSON arrays must not contain holes.");
          }
          visit(item[index]);
        }
        return;
      }

      const prototype = Object.getPrototypeOf(item);
      if (prototype !== Object.prototype && prototype !== null) {
        throw new Error("JSON objects must be plain objects.");
      }
      for (const entry of Object.values(item)) visit(entry);
    } finally {
      ancestors.delete(item);
    }
  };

  visit(value);
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isSafeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value);
}

function isFiniteRate(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 1 && value <= 60;
}

export function validateGameManifest(manifest: unknown): asserts manifest is GameManifest {
  if (!isRecord(manifest)) {
    throw new Error("GameManifest must be an object.");
  }
  validateJsonValue(manifest);
  if (!isNonEmptyString(manifest.title)) {
    throw new Error("title must be a non-empty string.");
  }
  if (!isNonEmptyString(manifest.version)) {
    throw new Error("version must be a non-empty string.");
  }
  const minPlayers = manifest.minPlayers;
  const maxPlayers = manifest.maxPlayers;
  if (!isSafeInteger(minPlayers) || minPlayers < 1) {
    throw new Error("minPlayers must be a positive safe integer.");
  }
  if (
    !isSafeInteger(maxPlayers) ||
    maxPlayers < minPlayers ||
    maxPlayers > 100
  ) {
    throw new Error("maxPlayers must be between minPlayers and 100.");
  }
  if (!isFiniteRate(manifest.tickRateHz)) {
    throw new Error("tickRateHz must be a finite number between 1 and 60.");
  }
  if (typeof manifest.persistentTable !== "boolean") {
    throw new Error("persistentTable must be a boolean.");
  }
  if (Object.hasOwn(manifest, "optionsSchema")) {
    const schema = manifest.optionsSchema;
    if (typeof schema !== "boolean" && !isRecord(schema)) {
      throw new Error("optionsSchema must be a JSON Schema object or boolean.");
    }
    validateJsonValue(schema);
  }
  if (Object.hasOwn(manifest, "resultDefinition")) {
    validateGameResultDefinition(manifest.resultDefinition);
  }
}

export function validateMatchConfig(config: unknown): asserts config is MatchConfig {
  if (!isRecord(config)) {
    throw new Error("MatchConfig must be an object.");
  }
  if (!isNonEmptyString(config.matchId)) {
    throw new Error("matchId must be a non-empty string.");
  }
  if (!isNonEmptyString(config.gameId)) {
    throw new Error("gameId must be a non-empty string.");
  }
  if (!Array.isArray(config.players) || config.players.length === 0 || config.players.length > 100) {
    throw new Error("players must contain between 1 and 100 player IDs.");
  }
  const players = new Set<string>();
  for (const playerId of config.players) {
    if (!isNonEmptyString(playerId) || players.has(playerId)) {
      throw new Error("players must contain unique, non-empty player IDs.");
    }
    players.add(playerId);
  }
  if (!isSafeInteger(config.matchSeq) || config.matchSeq < 1) {
    throw new Error("matchSeq must be a positive safe integer.");
  }
  const hasOptions = Object.hasOwn(config, "options");
  if (hasOptions && !isRecord(config.options)) {
    throw new Error("options must be an object.");
  }
  if (
    isRecord(config.options) &&
    Object.hasOwn(config.options, "seed") &&
    (typeof config.options.seed !== "number" || !Number.isFinite(config.options.seed))
  ) {
    throw new Error("options.seed must be a finite number.");
  }
  if (hasOptions) {
    validateJsonValue(config.options);
  }
  if (Object.hasOwn(config, "tickRateHz") && !isFiniteRate(config.tickRateHz)) {
    throw new Error("tickRateHz must be a finite number between 1 and 60.");
  }
  if (Object.hasOwn(config, "carryover")) {
    validateJsonValue(config.carryover);
  }
}

export function validateTickContext(ctx: unknown): asserts ctx is TickContext {
  if (!isRecord(ctx)) {
    throw new Error("TickContext must be an object.");
  }
  if (!isSafeInteger(ctx.tick) || ctx.tick < 0) {
    throw new Error("tick must be a non-negative safe integer.");
  }
  if (!Array.isArray(ctx.inputs)) {
    throw new Error("inputs must be an array.");
  }
  const actionIds = new Set<string>();
  for (const input of ctx.inputs) {
    if (!isRecord(input)) {
      throw new Error("Each input must be an object.");
    }
    if (!isNonEmptyString(input.playerId)) {
      throw new Error("input.playerId must be a non-empty string.");
    }
    if (!Object.hasOwn(input, "data")) {
      throw new Error("input.data is required.");
    }
    validateJsonValue(input.data);
    if (
      Object.hasOwn(input, "timestamp") &&
      (!isSafeInteger(input.timestamp) || input.timestamp < 0)
    ) {
      throw new Error("input.timestamp must be a non-negative safe integer.");
    }
    const hasActionId = Object.hasOwn(input, "actionId");
    const hasActionSchemaId = Object.hasOwn(input, "actionSchemaId");
    if (hasActionId !== hasActionSchemaId) {
      throw new Error("input.actionId and input.actionSchemaId must be supplied together.");
    }
    if (hasActionId) {
      const actionId = input.actionId;
      if (!isValidProtocolKey(actionId) || !isValidActionSchemaId(input.actionSchemaId)) {
        throw new Error("input action identifiers are invalid.");
      }
      if (actionIds.has(actionId)) {
        throw new Error("input actionId values must be unique within a tick.");
      }
      actionIds.add(actionId);
    }
  }
  if (Object.hasOwn(ctx, "commands")) {
    if (!Array.isArray(ctx.commands)) {
      throw new Error("commands must be an array when provided.");
    }
    for (const command of ctx.commands) {
      validateGameCommand(command);
    }
  }
  if (typeof ctx.elapsedSeconds !== "number" || !Number.isFinite(ctx.elapsedSeconds) || ctx.elapsedSeconds < 0) {
    throw new Error("elapsedSeconds must be a finite, non-negative number.");
  }
}

export function validateGameCommand(command: unknown): asserts command is GameCommand {
  if (!isRecord(command) || !isNonEmptyString(command.type)) {
    throw new Error("command.type must be a non-empty string.");
  }
  if (!Object.hasOwn(command, "data")) {
    throw new Error("command.data is required.");
  }
  validateJsonValue(command.data);
  if (
    Object.hasOwn(command, "timestamp") &&
    (!isSafeInteger(command.timestamp) || command.timestamp < 0)
  ) {
    throw new Error("command.timestamp must be a non-negative safe integer.");
  }
}

export function validateStateResult(value: unknown): void {
  if (!isRecord(value) || !Object.hasOwn(value, "state")) {
    throw new Error("Game callback result must contain state.");
  }
  validateJsonValue(value);
  if (Object.hasOwn(value, "playerViews") && value.playerViews !== undefined) {
    if (!isRecord(value.playerViews)) {
      throw new Error("playerViews must be an object keyed by playerId when provided.");
    }
    for (const [playerId, view] of Object.entries(value.playerViews)) {
      if (playerId.trim().length === 0) {
        throw new Error("playerViews keys must be non-empty playerId strings.");
      }
      validateJsonValue(view);
    }
  }
  if (Object.hasOwn(value, "broadcasts")) {
    if (!Array.isArray(value.broadcasts)) {
      throw new Error("broadcasts must be an array when provided.");
    }
    validateJsonValue(value.broadcasts);
  }
  if (Object.hasOwn(value, "joinable") && typeof value.joinable !== "boolean") {
    throw new Error("joinable must be a boolean when provided.");
  }
  validateGameResultUnits(value.resultUnits);
}

export function validateTickResult(
  value: unknown,
  inputs: TickInput[],
): asserts value is {
  state: unknown;
  broadcasts?: unknown[];
  persist: boolean;
  actionResults?: GameActionResult[];
} {
  validateStateResult(value);
  if (!isRecord(value) || typeof value.persist !== "boolean") {
    throw new Error("TickResult.persist must be a boolean.");
  }
  const expectedActionIds = new Set(
    inputs.flatMap(input => input.actionId === undefined ? [] : [input.actionId]),
  );
  const providedActionIds = new Set<string>();
  if (Object.hasOwn(value, "actionResults")) {
    if (!Array.isArray(value.actionResults)) {
      throw new Error("TickResult.actionResults must be an array when provided.");
    }
    for (const result of value.actionResults) {
      if (!isRecord(result)
        || !isValidProtocolKey(result.actionId)
        || (result.status !== "applied" && result.status !== "rejected")
        || (Object.hasOwn(result, "code") && !isValidActionCode(result.code))) {
        throw new Error("TickResult.actionResults contains an invalid result.");
      }
      if (!expectedActionIds.has(result.actionId) || providedActionIds.has(result.actionId)) {
        throw new Error("TickResult.actionResults must contain one result per action ID in the tick.");
      }
      providedActionIds.add(result.actionId);
    }
  }
  if (providedActionIds.size !== expectedActionIds.size) {
    throw new Error("TickResult.actionResults must contain one result per action ID in the tick.");
  }
}

export function validateGameResult(value: unknown): asserts value is {
  over: boolean;
  summary?: Record<string, unknown>;
  nextMatchPolicy?: Record<string, unknown>;
} {
  if (!isRecord(value) || typeof value.over !== "boolean") {
    throw new Error("GameResult.over must be a boolean.");
  }
  validateJsonValue(value);
  validateGameResultUnits(value.resultUnits);

  if (Object.hasOwn(value, "summary")) {
    if (
      !isRecord(value.summary) ||
      typeof value.summary.durationSeconds !== "number" ||
      !Number.isFinite(value.summary.durationSeconds) ||
      value.summary.durationSeconds < 0
    ) {
      throw new Error("summary must contain a finite, non-negative durationSeconds.");
    }
    validateJsonValue(value.summary);
  }

  if (Object.hasOwn(value, "nextMatchPolicy")) {
    const policy = value.nextMatchPolicy;
    const timeoutMs = isRecord(policy) ? policy.timeoutMs : undefined;
    if (
      !isRecord(policy) ||
      typeof policy.mode !== "string" ||
      !NEXT_MATCH_MODES.has(policy.mode) ||
      !Number.isSafeInteger(timeoutMs) ||
      typeof timeoutMs !== "number" ||
      timeoutMs < 0
    ) {
      throw new Error("nextMatchPolicy must contain a supported mode and non-negative integer timeoutMs.");
    }
    if (Object.hasOwn(policy, "carryover")) {
      validateJsonValue(policy.carryover);
    }
  }

  if (value.over && (!Object.hasOwn(value, "summary") || !Object.hasOwn(value, "nextMatchPolicy"))) {
    throw new Error("A completed game must include summary and nextMatchPolicy.");
  }
}
