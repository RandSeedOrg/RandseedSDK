import http from "node:http";
import type {
  GameCommand,
  GameManifest,
  GameServer,
  JsonValue,
  MatchConfig,
  TickContext,
} from "./types.js";
import { SDK_ERROR_METADATA, SdkProtocolError, type SdkErrorCode } from "./errors.js";
import {
  validateGameResult,
  validateGameStateResult,
  validateGameTickResult,
} from "./contract-validation.js";
import { validateAgentGameCapabilities } from "./agent-capabilities.js";
import {
  createAgentPlayerView,
  validateAgentPlayerContract,
  validateAgentPlayerViewRequest,
} from "./agent-player-view.js";
import {
  attachStreamUpgradeHandler,
  resolveStreamDurability,
  STREAM_PROTOCOL_V1,
} from "./stream.js";
import {
  isRecord,
  normalizeGameCommand,
  normalizeTickContext,
  validateGameCommand,
  validateGameManifest,
  validateJsonValue,
  validateMatchConfig,
  validatePlayerRoster,
  validateTickContext,
} from "./validators.js";

const MAX_REQUEST_BODY_BYTES = 1024 * 1024;
export interface CreateGameServerOptions {
  gameId?: string;
}

class RequestError extends Error {
  constructor(
    readonly statusCode: 400 | 413,
    readonly code: SdkErrorCode,
    message: string,
  ) {
    super(message);
  }

  get retryable(): boolean {
    return false;
  }
}

interface TickRequest {
  ctx: TickContext;
  state: JsonValue;
  players?: string[];
}

interface StateRequest {
  state: JsonValue;
}

function isRequestRecord(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) {
    throw new RequestError(400, "INVALID_REQUEST", "Request body must be a JSON object.");
  }
  return value;
}

function validateRequest<T>(
  value: unknown,
  validator: (input: unknown) => asserts input is T,
  fallbackCode: SdkErrorCode = "INVALID_REQUEST",
): T {
  try {
    validator(value);
  } catch (error) {
    const code = error instanceof SdkProtocolError ? error.code : fallbackCode;
    throw new RequestError(400, code, error instanceof Error ? error.message : "Request validation failed.");
  }
  return value as T;
}

function readJsonBody(req: http.IncomingMessage): Promise<unknown> {
  const contentLength = req.headers["content-length"];
  if (typeof contentLength === "string" && Number(contentLength) > MAX_REQUEST_BODY_BYTES) {
    req.resume();
    throw new RequestError(413, "RESOURCE_LIMIT_EXCEEDED", "Request body exceeds the 1 MiB limit.");
  }

  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let bytesRead = 0;
    let settled = false;
    let oversized = false;

    const fail = (error: Error) => {
      if (!settled) {
        settled = true;
        reject(error);
      }
    };

    req.on("data", (chunk: Buffer) => {
      if (oversized) return;
      bytesRead += chunk.length;
      if (bytesRead > MAX_REQUEST_BODY_BYTES) {
        oversized = true;
        chunks.length = 0;
        fail(new RequestError(413, "RESOURCE_LIMIT_EXCEEDED", "Request body exceeds the 1 MiB limit."));
        return;
      }
      chunks.push(chunk);
    });

    req.on("end", () => {
      if (settled) return;
      settled = true;
      if (chunks.length === 0) {
        resolve(null);
        return;
      }
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown);
      } catch {
        reject(new RequestError(400, "INVALID_REQUEST", "Request body must contain valid JSON."));
      }
    });

    req.on("error", () => fail(new RequestError(400, "INVALID_REQUEST", "Could not read request body.")));
    req.on("aborted", () => fail(new RequestError(400, "INVALID_REQUEST", "Request body was interrupted.")));
  });
}

function validateStateRequest(value: unknown): StateRequest {
  const body = isRequestRecord(value);
  if (!Object.hasOwn(body, "state")) {
    throw new RequestError(400, "INVALID_STATE", "state is required.");
  }
  return {
    state: validateRequest<JsonValue>(body.state, validateJsonValue, "INVALID_STATE"),
  };
}

function validateTickRequest(value: unknown): TickRequest {
  const body = isRequestRecord(value);
  const ctx = validateRequest<TickContext>(body.ctx, validateTickContext, "INVALID_ACTION");
  if (!Object.hasOwn(body, "state")) {
    throw new RequestError(400, "INVALID_STATE", "state is required.");
  }
  if (Object.hasOwn(body, "players")) {
    validateRequest<string[]>(body.players, validatePlayerRoster, "WRONG_ROSTER");
  }
  return {
    ctx: normalizeTickContext(ctx),
    state: validateRequest<JsonValue>(body.state, validateJsonValue, "INVALID_STATE"),
    ...(body.players === undefined ? {} : { players: body.players as string[] }),
  };
}

function validatePlayerEventRequest(value: unknown): { playerId: string; state: JsonValue; players?: string[] } {
  const body = isRequestRecord(value);
  if (typeof body.playerId !== "string" || body.playerId.trim().length === 0) {
    throw new RequestError(400, "WRONG_ROSTER", "playerId must be a non-empty string.");
  }
  if (Object.hasOwn(body, "players")) {
    const players = body.players;
    if (!Array.isArray(players) || players.length > 100
      || players.some(player => typeof player !== "string" || player.trim().length === 0)
      || new Set(players).size !== players.length) {
      throw new RequestError(400, "WRONG_ROSTER", "players must contain unique, non-empty player IDs.");
    }
  }
  const { state } = validateStateRequest(body);
  return {
    playerId: body.playerId,
    state,
    ...(body.players === undefined ? {} : { players: body.players as string[] }),
  };
}

function validateCommandRequest(value: unknown): {
  command: GameCommand;
  state: JsonValue;
  players?: string[];
} {
  const body = isRequestRecord(value);
  if (!Object.hasOwn(body, "command")) {
    throw new RequestError(400, "INVALID_ACTION", "command is required.");
  }
  const command = validateRequest<GameCommand>(body.command, validateCommand, "INVALID_ACTION");
  const { state } = validateStateRequest(body);
  if (Object.hasOwn(body, "players")) {
    validateRequest<string[]>(body.players, validatePlayerRoster, "WRONG_ROSTER");
  }
  return {
    command: normalizeGameCommand(command),
    state,
    ...(body.players === undefined ? {} : { players: body.players as string[] }),
  };
}

function validateCommand(value: unknown): asserts value is GameCommand {
  validateGameCommand(value);
}

function statusForError(code: SdkErrorCode): number {
  if (code === "RESOURCE_LIMIT_EXCEEDED") return 413;
  if (code === "UNSUPPORTED_FEATURE") return 501;
  if (code === "NOT_FOUND") return 404;
  if (code === "INVALID_REQUEST"
    || code === "INVALID_STREAM_MESSAGE"
    || code === "INVALID_OPTIONS"
    || code === "INVALID_STATE"
    || code === "WRONG_ROSTER"
    || code === "INVALID_ACTION"
    || code === "TICK_SEQUENCE_MISMATCH"
    || code === "REPLAY_MISMATCH") return 400;
  return 500;
}

async function callGame<T>(callback: () => T | Promise<T>): Promise<T> {
  try {
    return await callback();
  } catch (error) {
    if (error instanceof SdkProtocolError) throw error;
    console.error("[server-sdk] Game callback failed.");
    throw new SdkProtocolError("CALLBACK_FAILED");
  }
}

function validateManifest(value: unknown): asserts value is import("./types.js").GameManifest {
  try {
    validateGameManifest(value);
  } catch (error) {
    if (error instanceof SdkProtocolError) throw error;
    throw new SdkProtocolError("INVALID_MANIFEST");
  }
}

function assertManifestRoster(config: MatchConfig, manifest: GameManifest): void {
  if (config.players.length < manifest.minPlayers || config.players.length > manifest.maxPlayers) {
    throw new RequestError(
      400,
      "WRONG_ROSTER",
      "players must satisfy the game manifest minPlayers and maxPlayers.",
    );
  }
}

function requireProjectionRoster(
  players: readonly string[] | undefined,
  manifest: GameManifest,
  allowEmpty = false,
): void {
  if (manifest.playerViewMode === "private" && players === undefined) {
    throw new RequestError(
      400,
      "WRONG_ROSTER",
      "Private player views require the active players roster in the request.",
    );
  }
  if (players !== undefined
    && ((!allowEmpty && players.length === 0) || players.length > manifest.maxPlayers)) {
    throw new RequestError(400, "WRONG_ROSTER", "players must satisfy the active game roster limits.");
  }
}

function sendJson(
  res: http.ServerResponse,
  statusCode: number,
  body: unknown,
  extraHeaders?: Record<string, string>,
): void {
  const payload = JSON.stringify(body);
  if (payload === undefined) {
    throw new Error("Response is not JSON serializable.");
  }
  res.writeHead(statusCode, {
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": Buffer.byteLength(payload, "utf8"),
    ...extraHeaders,
  });
  res.end(payload);
}

function sendSdkError(
  res: http.ServerResponse,
  statusCode: number,
  code: SdkErrorCode,
  message: string = SDK_ERROR_METADATA[code].creatorAction,
): void {
  sendJson(res, statusCode, {
    code,
    retryable: SDK_ERROR_METADATA[code].retryable,
    creatorAction: SDK_ERROR_METADATA[code].creatorAction,
    error: message,
  });
}

export function createGameServer(
  game: GameServer,
  port = 8080,
  options: CreateGameServerOptions = {},
): http.Server {
  const getManifest = async (): Promise<GameManifest> => {
    const manifest = await callGame(() => game.manifest());
    validateManifest(manifest);
    return manifest;
  };

  const server = http.createServer(async (req, res) => {
    const method = req.method || "GET";

    try {
      const pathname = new URL(req.url || "/", `http://${req.headers.host || "localhost"}`).pathname;

      if (method === "GET" && pathname === "/agent/capabilities") {
        if (!game.agentCapabilities) {
          sendSdkError(res, 404, "NOT_FOUND", "Agent support is not enabled for this game.");
          return;
        }
        const declaration = await callGame(() => game.agentCapabilities!());
        if (declaration === null) {
          sendSdkError(res, 404, "NOT_FOUND", "Agent support is not enabled for this game.");
          return;
        }
        try {
          sendJson(res, 200, validateAgentGameCapabilities(declaration));
        } catch {
          sendSdkError(res, 500, "INVALID_RESULT");
        }
        return;
      }

      if (method === "GET" && pathname === "/agent/player-contract") {
        if (!game.agentPlayerContract || !game.projectAgentPlayerView) {
          sendSdkError(res, 404, "NOT_FOUND", "Authoritative Agent player views are not enabled for this game.");
          return;
        }
        const declaration = await callGame(() => game.agentPlayerContract!());
        if (declaration === null) {
          sendSdkError(res, 404, "NOT_FOUND", "Authoritative Agent player views are not enabled for this game.");
          return;
        }
        try {
          const contract = validateAgentPlayerContract(declaration);
          sendJson(res, 200, contract);
        } catch {
          sendSdkError(res, 500, "INVALID_RESULT");
        }
        return;
      }

      if (method === "POST" && pathname === "/agent/player-view") {
        let playerRequest: ReturnType<typeof validateAgentPlayerViewRequest>;
        try {
          playerRequest = validateAgentPlayerViewRequest(await readJsonBody(req));
        } catch (error) {
          const code = error instanceof RequestError ? error.code : "INVALID_REQUEST";
          const status = error instanceof RequestError ? error.statusCode : 400;
          sendSdkError(
            res,
            status,
            code,
            error instanceof Error ? error.message : "Invalid Agent player view request.",
          );
          return;
        }
        if (options.gameId && playerRequest.context.scope.gameId !== options.gameId) {
          sendSdkError(res, 400, "INVALID_REQUEST", "Agent player view game scope does not match this server.");
          return;
        }
        if (!game.agentPlayerContract || !game.projectAgentPlayerView) {
          sendSdkError(res, 404, "NOT_FOUND", "Authoritative Agent player views are not enabled for this game.");
          return;
        }
        const declaration = await callGame(() => game.agentPlayerContract!());
        if (declaration === null) {
          sendSdkError(res, 404, "NOT_FOUND", "Authoritative Agent player views are not enabled for this game.");
          return;
        }
        let contract: ReturnType<typeof validateAgentPlayerContract>;
        try {
          contract = validateAgentPlayerContract(declaration);
        } catch {
          sendSdkError(res, 500, "INVALID_RESULT");
          return;
        }
        const projection = await callGame(() =>
          game.projectAgentPlayerView!(structuredClone(playerRequest)),
        );
        try {
          sendJson(res, 200, createAgentPlayerView(contract, playerRequest, projection));
        } catch {
          sendSdkError(res, 500, "INVALID_RESULT");
        }
        return;
      }

      if (method === "GET" && pathname === "/manifest") {
        const manifest = await getManifest();
        const durability = resolveStreamDurability(game);
        const headers: Record<string, string> = {
          "X-RandSeed-Stream-Protocol": STREAM_PROTOCOL_V1,
        };
        if (durability) {
          headers["X-RandSeed-Stream-Durability"] = durability;
        }
        sendJson(res, 200, manifest, headers);
        return;
      }

      if (method === "GET" && pathname === "/health") {
        sendJson(res, 200, {
          status: "ok",
          gameId: options.gameId || "game",
          uptime: process.uptime(),
        });
        return;
      }

      if (method === "POST" && pathname === "/init") {
        const config = validateRequest<MatchConfig>(await readJsonBody(req), validateMatchConfig);
        const manifest = await getManifest();
        assertManifestRoster(config, manifest);
        const result = await callGame(() => game.init(config));
        validateGameStateResult(
          result,
          config.players,
          manifest.playerViewMode === "private",
        );
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/tick") {
        const body = validateTickRequest(await readJsonBody(req));
        const manifest = await getManifest();
        requireProjectionRoster(body.players, manifest);
        if (body.players !== undefined
          && body.ctx.inputs.some(input => !body.players!.includes(input.playerId))) {
          throw new RequestError(400, "WRONG_ROSTER", "Every input playerId must belong to the active roster.");
        }
        const result = await callGame(() => game.tick(body.ctx, body.state));
        validateGameTickResult(
          result,
          body.ctx.inputs,
          body.players,
          manifest.playerViewMode === "private",
        );
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/onPlayerJoin") {
        const body = validatePlayerEventRequest(await readJsonBody(req));
        const manifest = await getManifest();
        requireProjectionRoster(body.players, manifest, true);
        if (body.players !== undefined && !body.players.includes(body.playerId)) {
          throw new RequestError(400, "WRONG_ROSTER", "A joining player must be included in the resulting roster.");
        }
        const result = await callGame(() => game.onPlayerJoin(body.playerId, body.state));
        validateGameStateResult(result, body.players, manifest.playerViewMode === "private");
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/onPlayerLeave") {
        const body = validatePlayerEventRequest(await readJsonBody(req));
        const manifest = await getManifest();
        requireProjectionRoster(body.players, manifest, true);
        if (body.players !== undefined && body.players.includes(body.playerId)) {
          throw new RequestError(400, "WRONG_ROSTER", "A leaving player must be absent from the resulting roster.");
        }
        const result = await callGame(() => game.onPlayerLeave(body.playerId, body.state));
        validateGameStateResult(result, body.players, manifest.playerViewMode === "private");
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/isGameOver") {
        const { state } = validateStateRequest(await readJsonBody(req));
        const result = await callGame(() => game.isGameOver(state));
        validateGameResult(result);
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/onCommand") {
        if (typeof game.onCommand !== "function") {
          sendJson(res, 501, {
            code: "UNSUPPORTED_FEATURE",
            retryable: false,
            creatorAction: "Remove the unsupported feature or implement its required callback.",
            error: "This game server does not implement onCommand.",
          });
          return;
        }
        const body = validateCommandRequest(await readJsonBody(req));
        const manifest = await getManifest();
        requireProjectionRoster(body.players, manifest);
        const result = await callGame(() => game.onCommand!(body.command, body.state));
        validateGameStateResult(result, body.players, manifest.playerViewMode === "private");
        sendJson(res, 200, result);
        return;
      }

      sendJson(res, 404, {
        code: "NOT_FOUND",
        retryable: false,
        creatorAction: "Correct the endpoint or resource identifier.",
        error: `Endpoint not found: ${method} ${pathname}`,
      });
    } catch (error) {
      if (res.destroyed || res.writableEnded) return;
      if (error instanceof RequestError) {
        sendJson(res, error.statusCode, {
          code: error.code,
          retryable: error.retryable,
          creatorAction: SDK_ERROR_METADATA[error.code].creatorAction,
          error: error.message,
        });
        return;
      }
      if (error instanceof SdkProtocolError) {
        sendJson(res, statusForError(error.code), {
          code: error.code,
          retryable: error.retryable,
          creatorAction: SDK_ERROR_METADATA[error.code].creatorAction,
          error: error.message,
        });
        return;
      }
      console.error("[server-sdk] Request handler failed.");
      const internalError = new SdkProtocolError("INTERNAL_ERROR");
      sendJson(res, 500, {
        code: internalError.code,
        retryable: internalError.retryable,
        creatorAction: SDK_ERROR_METADATA[internalError.code].creatorAction,
        error: internalError.message,
      });
    }
  });

  attachStreamUpgradeHandler(server, game);
  server.listen(port);
  return server;
}
