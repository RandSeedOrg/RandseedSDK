import type { GameCommand, GameManifest, JsonValue, MatchConfig, TickContext } from "./types.js";
import { SdkProtocolError } from "./errors.js";
import {
  isValidActionSchemaId,
  isValidProtocolKey,
} from "./protocol-identifiers.js";
import { validateGameResultDefinition } from "./result-units.js";

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
  if (Object.hasOwn(manifest, "playerViewMode") && manifest.playerViewMode !== "private") {
    throw new SdkProtocolError("INVALID_MANIFEST", "playerViewMode must be 'private' when provided.");
  }
  if (Object.hasOwn(manifest, "optionsSchema")) {
    const schema = manifest.optionsSchema;
    if (typeof schema !== "boolean" && !isRecord(schema)) {
      throw new Error("optionsSchema must be a JSON Schema object or boolean.");
    }
    validateJsonValue(schema);
  }
  if (Object.hasOwn(manifest, "resultDefinition")) {
    try {
      validateGameResultDefinition(manifest.resultDefinition);
    } catch (error) {
      const message = error instanceof Error ? error.message : "";
      throw new SdkProtocolError(
        /limit|exceeds/i.test(message) ? "RESOURCE_LIMIT_EXCEEDED" : "INVALID_MANIFEST",
      );
    }
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
    throw new SdkProtocolError("WRONG_ROSTER", "players must contain between 1 and 100 player IDs.");
  }
  const players = new Set<string>();
  for (const playerId of config.players) {
    if (!isNonEmptyString(playerId) || players.has(playerId)) {
      throw new SdkProtocolError("WRONG_ROSTER", "players must contain unique, non-empty player IDs.");
    }
    players.add(playerId);
  }
  if (!isSafeInteger(config.matchSeq) || config.matchSeq < 1) {
    throw new Error("matchSeq must be a positive safe integer.");
  }
  const hasOptions = Object.hasOwn(config, "options");
  if (hasOptions && !isRecord(config.options)) {
    throw new SdkProtocolError("INVALID_OPTIONS", "options must be a JSON object.");
  }
  if (
    isRecord(config.options) &&
    Object.hasOwn(config.options, "seed") &&
    (typeof config.options.seed !== "number" || !Number.isFinite(config.options.seed))
  ) {
    throw new SdkProtocolError("INVALID_OPTIONS", "options.seed must be a finite number.");
  }
  if (hasOptions) {
    try {
      validateJsonValue(config.options);
    } catch {
      throw new SdkProtocolError("INVALID_OPTIONS");
    }
  }
  if (Object.hasOwn(config, "tickRateHz") && !isFiniteRate(config.tickRateHz)) {
    throw new Error("tickRateHz must be a finite number between 1 and 60.");
  }
  if (Object.hasOwn(config, "carryover")) {
    validateJsonValue(config.carryover);
  }
}

export function validatePlayerRoster(value: unknown): asserts value is string[] {
  if (!Array.isArray(value) || value.length === 0 || value.length > 100) {
    throw new SdkProtocolError("WRONG_ROSTER", "players must contain between 1 and 100 player IDs.");
  }
  const players = new Set<string>();
  for (const playerId of value) {
    if (typeof playerId !== "string" || playerId.trim().length === 0 || players.has(playerId)) {
      throw new SdkProtocolError("WRONG_ROSTER", "players must contain unique, non-empty player IDs.");
    }
    players.add(playerId);
  }
}

const tickInputFields = new Set([
  "playerId",
  "data",
  "timestamp",
  "actionId",
  "actionSchemaId",
]);

export function normalizeTickContext(ctx: unknown): TickContext {
  if (!isRecord(ctx)) {
    throw new Error("TickContext must be an object.");
  }
  if (!Array.isArray(ctx.inputs)) {
    throw new Error("inputs must be an array.");
  }
  const inputs = ctx.inputs.map(input => {
    if (!isRecord(input)) {
      throw new Error("Each input must be an object.");
    }
    const flattenedData = Object.fromEntries(
      Object.entries(input).filter(([field]) => !tickInputFields.has(field)),
    );
    if (!Object.hasOwn(input, "data") && Object.keys(flattenedData).length === 0) {
      throw new Error("input.data is required.");
    }
    const data = Object.hasOwn(input, "data")
      ? isRecord(input.data) && Object.keys(flattenedData).length > 0
        ? { ...flattenedData, ...input.data }
        : input.data
      : flattenedData;
    const normalized = {
      playerId: input.playerId,
      data,
      ...(Object.hasOwn(input, "timestamp") ? { timestamp: input.timestamp } : {}),
      ...(Object.hasOwn(input, "actionId") ? { actionId: input.actionId } : {}),
      ...(Object.hasOwn(input, "actionSchemaId") ? { actionSchemaId: input.actionSchemaId } : {}),
    };
    if (!isNonEmptyString(normalized.playerId)) {
      throw new Error("input.playerId must be a non-empty string.");
    }
    validateJsonValue(normalized.data);
    if (
      Object.hasOwn(normalized, "timestamp") &&
      (!isSafeInteger(normalized.timestamp) || normalized.timestamp < 0)
    ) {
      throw new Error("input.timestamp must be a non-negative safe integer.");
    }
    const hasActionId = Object.hasOwn(normalized, "actionId");
    const hasActionSchemaId = Object.hasOwn(normalized, "actionSchemaId");
    if (hasActionId !== hasActionSchemaId) {
      throw new Error("input.actionId and input.actionSchemaId must be supplied together.");
    }
    if (
      hasActionId &&
      (!isValidProtocolKey(normalized.actionId) || !isValidActionSchemaId(normalized.actionSchemaId))
    ) {
      throw new Error("input action identifiers are invalid.");
    }
    return normalized;
  });
  const normalizedContext = {
    tick: ctx.tick,
    inputs,
    ...(Object.hasOwn(ctx, "commands")
      ? {
          commands: Array.isArray(ctx.commands)
            ? ctx.commands.map(normalizeGameCommand)
            : ctx.commands,
        }
      : {}),
    elapsedSeconds: ctx.elapsedSeconds,
  };
  validateTickContextShape(normalizedContext);
  return normalizedContext;
}

export function validateTickContext(ctx: unknown): asserts ctx is TickContext {
  normalizeTickContext(ctx);
}

function validateTickContextShape(ctx: unknown): asserts ctx is TickContext {
  if (!isRecord(ctx)) {
    throw new Error("TickContext must be an object.");
  }
  if (!isSafeInteger(ctx.tick) || ctx.tick < 0) {
    throw new Error("tick must be a non-negative safe integer.");
  }
  if (!Array.isArray(ctx.inputs)) {
    throw new Error("inputs must be an array.");
  }
  for (const input of ctx.inputs) {
    if (!isRecord(input) || !isNonEmptyString(input.playerId) || !Object.hasOwn(input, "data")) {
      throw new Error("Each input requires a non-empty playerId and data.");
    }
    validateJsonValue(input.data);
    if (Object.hasOwn(input, "timestamp")
      && (!isSafeInteger(input.timestamp) || input.timestamp < 0)) {
      throw new Error("input.timestamp must be a non-negative safe integer.");
    }
    if (Object.hasOwn(input, "actionId") !== Object.hasOwn(input, "actionSchemaId")) {
      throw new Error("input.actionId and input.actionSchemaId must be supplied together.");
    }
    if (Object.hasOwn(input, "actionId")
      && (!isValidProtocolKey(input.actionId) || !isValidActionSchemaId(input.actionSchemaId))) {
      throw new Error("input action identifiers are invalid.");
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
  const allowedFields = new Set(["type", "data", "payload", "issuer", "timestamp"]);
  if (Object.keys(command).some(field => !allowedFields.has(field))) {
    throw new Error("command contains an unsupported field.");
  }
  if (Object.hasOwn(command, "data") && Object.hasOwn(command, "payload")) {
    throw new Error("command.data and command.payload must not both be supplied.");
  }
  if (Object.hasOwn(command, "data")) validateJsonValue(command.data);
  if (Object.hasOwn(command, "payload")) validateJsonValue(command.payload);
  if (Object.hasOwn(command, "issuer") && typeof command.issuer !== "string") {
    throw new Error("command.issuer must be a string when provided.");
  }
  if (
    Object.hasOwn(command, "timestamp") &&
    (!isSafeInteger(command.timestamp) || command.timestamp < 0)
  ) {
    throw new Error("command.timestamp must be a non-negative safe integer.");
  }
}

export function normalizeGameCommand(value: unknown): GameCommand {
  validateGameCommand(value);
  if (!isRecord(value)) throw new Error("command must be an object.");
  const { payload, ...command } = value;
  return {
    type: command.type as string,
    ...(Object.hasOwn(command, "data")
      ? { data: command.data }
      : Object.hasOwn(value, "payload")
        ? { data: payload }
        : {}),
    ...(Object.hasOwn(command, "issuer") ? { issuer: command.issuer as string } : {}),
    ...(Object.hasOwn(command, "timestamp") ? { timestamp: command.timestamp as number } : {}),
  };
}
