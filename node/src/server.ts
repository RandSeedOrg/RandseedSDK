import http from "node:http";
import type { GameCommand, GameServer, JsonValue, MatchConfig, TickContext } from "./types.js";
import { validateAgentGameCapabilities } from "./agent-capabilities.js";
import {
  isRecord,
  validateGameCommand,
  validateGameManifest,
  validateJsonValue,
  validateMatchConfig,
  validateTickContext,
} from "./validators.js";

const MAX_REQUEST_BODY_BYTES = 1024 * 1024;
const NEXT_MATCH_MODES = new Set(["auto_next_match", "wait_for_ready", "close_room"]);

export interface CreateGameServerOptions {
  gameId?: string;
}

class RequestError extends Error {
  constructor(
    readonly statusCode: 400 | 413,
    message: string,
  ) {
    super(message);
  }
}

interface TickRequest {
  ctx: TickContext;
  state: JsonValue;
}

interface StateRequest {
  state: JsonValue;
}

function isRequestRecord(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) {
    throw new RequestError(400, "Request body must be a JSON object.");
  }
  return value;
}

function validateRequest<T>(
  value: unknown,
  validator: (input: unknown) => asserts input is T,
): T {
  try {
    validator(value);
  } catch (error) {
    throw new RequestError(
      400,
      error instanceof Error ? error.message : "Request validation failed.",
    );
  }
  return value as T;
}

function readJsonBody(req: http.IncomingMessage): Promise<unknown> {
  const contentLength = req.headers["content-length"];
  if (typeof contentLength === "string" && Number(contentLength) > MAX_REQUEST_BODY_BYTES) {
    req.resume();
    throw new RequestError(413, "Request body exceeds the 1 MiB limit.");
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
        fail(new RequestError(413, "Request body exceeds the 1 MiB limit."));
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
        reject(new RequestError(400, "Request body must contain valid JSON."));
      }
    });

    req.on("error", () => fail(new RequestError(400, "Could not read request body.")));
    req.on("aborted", () => fail(new RequestError(400, "Request body was interrupted.")));
  });
}

function validateStateRequest(value: unknown): StateRequest {
  const body = isRequestRecord(value);
  if (!Object.hasOwn(body, "state")) {
    throw new RequestError(400, "state is required.");
  }
  return {
    state: validateRequest<JsonValue>(body.state, validateJsonValue),
  };
}

function validateTickRequest(value: unknown): TickRequest {
  const body = isRequestRecord(value);
  const ctx = validateRequest<TickContext>(body.ctx, validateTickContext);
  if (!Object.hasOwn(body, "state")) {
    throw new RequestError(400, "state is required.");
  }
  return {
    ctx,
    state: validateRequest<JsonValue>(body.state, validateJsonValue),
  };
}

function validatePlayerEventRequest(value: unknown): { playerId: string; state: JsonValue } {
  const body = isRequestRecord(value);
  if (typeof body.playerId !== "string" || body.playerId.trim().length === 0) {
    throw new RequestError(400, "playerId must be a non-empty string.");
  }
  const { state } = validateStateRequest(body);
  return { playerId: body.playerId, state };
}

function validateCommandRequest(value: unknown): {
  command: GameCommand;
  state: JsonValue;
} {
  const body = isRequestRecord(value);
  if (!Object.hasOwn(body, "command")) {
    throw new RequestError(400, "command is required.");
  }
  const command = validateRequest<GameCommand>(body.command, validateCommand);
  const { state } = validateStateRequest(body);
  return { command, state };
}

function validateCommand(value: unknown): asserts value is GameCommand {
  validateGameCommand(value);
}

function validateStateResult(value: unknown): void {
  if (!isRecord(value) || !Object.hasOwn(value, "state")) {
    throw new Error("Game callback result must contain state.");
  }
  validateJsonValue(value);
  if (Object.hasOwn(value, "broadcasts")) {
    if (!Array.isArray(value.broadcasts)) {
      throw new Error("broadcasts must be an array when provided.");
    }
    validateJsonValue(value.broadcasts);
  }
}

function validateTickResult(value: unknown): asserts value is {
  state: unknown;
  broadcasts?: unknown[];
  persist: boolean;
} {
  validateStateResult(value);
  if (!isRecord(value) || typeof value.persist !== "boolean") {
    throw new Error("TickResult.persist must be a boolean.");
  }
}

function validateGameResult(value: unknown): asserts value is {
  over: boolean;
  summary?: Record<string, unknown>;
  nextMatchPolicy?: Record<string, unknown>;
} {
  if (!isRecord(value) || typeof value.over !== "boolean") {
    throw new Error("GameResult.over must be a boolean.");
  }
  validateJsonValue(value);

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

function sendJson(res: http.ServerResponse, statusCode: number, body: unknown): void {
  const payload = JSON.stringify(body);
  if (payload === undefined) {
    throw new Error("Response is not JSON serializable.");
  }
  res.writeHead(statusCode, {
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": Buffer.byteLength(payload, "utf8"),
  });
  res.end(payload);
}

export function createGameServer(
  game: GameServer,
  port = 8080,
  options: CreateGameServerOptions = {},
): http.Server {
  const server = http.createServer(async (req, res) => {
    const method = req.method || "GET";

    try {
      const pathname = new URL(req.url || "/", `http://${req.headers.host || "localhost"}`).pathname;

      if (method === "GET" && pathname === "/agent/capabilities") {
        try {
          if (!game.agentCapabilities) {
            sendJson(res, 404, { error: "Agent support is not enabled for this game." });
            return;
          }
          const declaration = await game.agentCapabilities();
          if (declaration === null) {
            sendJson(res, 404, { error: "Agent support is not enabled for this game." });
            return;
          }
          sendJson(res, 200, validateAgentGameCapabilities(declaration));
        } catch {
          console.error("[server-sdk] Agent capability declaration failed validation or execution.");
          sendJson(res, 500, { error: "Agent capabilities are unavailable." });
        }
        return;
      }

      if (method === "GET" && pathname === "/manifest") {
        const manifest = await game.manifest();
        validateGameManifest(manifest);
        sendJson(res, 200, manifest);
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
        const result = await game.init(config);
        validateStateResult(result);
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/tick") {
        const body = validateTickRequest(await readJsonBody(req));
        const result = await game.tick(body.ctx, body.state);
        validateTickResult(result);
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/onPlayerJoin") {
        const body = validatePlayerEventRequest(await readJsonBody(req));
        const result = await game.onPlayerJoin(body.playerId, body.state);
        validateStateResult(result);
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/onPlayerLeave") {
        const body = validatePlayerEventRequest(await readJsonBody(req));
        const result = await game.onPlayerLeave(body.playerId, body.state);
        validateStateResult(result);
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/isGameOver") {
        const { state } = validateStateRequest(await readJsonBody(req));
        const result = await game.isGameOver(state);
        validateGameResult(result);
        sendJson(res, 200, result);
        return;
      }

      if (method === "POST" && pathname === "/onCommand") {
        if (typeof game.onCommand !== "function") {
          sendJson(res, 501, { error: "This game server does not implement onCommand." });
          return;
        }
        const body = validateCommandRequest(await readJsonBody(req));
        const result = await game.onCommand(body.command, body.state);
        validateStateResult(result);
        sendJson(res, 200, result);
        return;
      }

      sendJson(res, 404, { error: `Endpoint not found: ${method} ${pathname}` });
    } catch (error) {
      if (res.destroyed || res.writableEnded) return;
      if (error instanceof RequestError) {
        sendJson(res, error.statusCode, { error: error.message });
        return;
      }
      console.error("[server-sdk] Request handler failed.");
      sendJson(res, 500, { error: "Internal server error." });
    }
  });

  server.listen(port);
  return server;
}
