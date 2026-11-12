import crypto from "node:crypto";
import type http from "node:http";
import type { Duplex } from "node:stream";
import type {
  GameActionResult,
  GameCommand,
  GameResult,
  GameResultEvent,
  GameManifest,
  GameServer,
  JsonValue,
  StreamDurability,
  TickInput,
  TickContext,
} from "./types.js";
import { SDK_ERROR_METADATA, SdkProtocolError } from "./errors.js";
import {
  validateGameResult,
  validateGameStateResult,
  validateGameTickResult,
} from "./contract-validation.js";
import {
  isRecord,
  normalizeGameCommand,
  normalizeTickContext,
  validateGameCommand,
  validateGameManifest,
  validateJsonValue,
  validatePlayerRoster,
  validateTickContext,
} from "./validators.js";

export const STREAM_PROTOCOL_V1 = "session-v1";
const WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const MAX_WS_PAYLOAD_BYTES = 1024 * 1024;

export type { StreamDurability };

export interface StreamStepEventJoin {
  type: "join";
  playerId: string;
}

export interface StreamStepEventLeave {
  type: "leave";
  playerId: string;
}

export interface StreamStepEventCommand {
  type: "command";
  command: GameCommand;
}

export type StreamStepEvent =
  | StreamStepEventJoin
  | StreamStepEventLeave
  | StreamStepEventCommand;

export interface StreamReplayStep {
  tick: number;
  inputs?: TickInput[];
  events?: StreamStepEvent[];
}

export interface HydrateMessage {
  type: "hydrate";
  matchId: string;
  tickRateHz: number;
  tick: number;
  state: JsonValue;
  players?: string[];
  playerViews?: Record<string, JsonValue>;
  replay?: StreamReplayStep[];
  resyncPlayers?: string[];
}

export interface StepMessage {
  type: "step";
  tick: number;
  inputs?: TickInput[];
  events?: StreamStepEvent[];
  resyncPlayers?: string[];
  wantState?: boolean;
}

function encodeWsFrame(opcode: number, payload: Buffer): Buffer {
  const len = payload.length;
  let header: Buffer;
  if (len < 126) {
    header = Buffer.allocUnsafe(2);
    header[0] = 0x80 | opcode;
    header[1] = len;
  } else if (len < 65536) {
    header = Buffer.allocUnsafe(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(len, 2);
  } else {
    header = Buffer.allocUnsafe(10);
    header[0] = 0x80 | opcode;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(len), 2);
  }
  return Buffer.concat([header, payload]);
}

function sendWsJson(socket: Duplex, body: unknown): void {
  if (socket.destroyed || !socket.writable) return;
  const raw = Buffer.from(JSON.stringify(body), "utf8");
  socket.write(encodeWsFrame(0x01, raw));
}

interface AppliedStreamStep<TState> {
  state: TState;
  players: string[];
  broadcasts: JsonValue[];
  persist: boolean;
  joinable?: boolean;
  playerViews?: Record<string, JsonValue>;
  resultUnits: GameResultEvent[];
  actionResults?: GameActionResult[];
  gameResult: GameResult;
}

interface ParsedReplayStep {
  tick: number;
  inputs: TickInput[];
  events: StreamStepEvent[];
}

function streamError(code: ConstructorParameters<typeof SdkProtocolError>[0], message?: string): never {
  throw new SdkProtocolError(code, message);
}

function assertAllowedKeys(
  value: Record<string, unknown>,
  allowed: readonly string[],
  label: string,
): void {
  if (Object.keys(value).some(key => !allowed.includes(key))) {
    streamError("INVALID_STREAM_MESSAGE", `${label} contains an unsupported field.`);
  }
}

function parseArray(value: unknown, code: "INVALID_ACTION" | "INVALID_STREAM_MESSAGE", field: string): unknown[] {
  if (value === undefined) return [];
  if (!Array.isArray(value)) streamError(code, `${field} must be an array when provided.`);
  return value;
}

function parseInputs(value: unknown): TickInput[] {
  const inputs = parseArray(value, "INVALID_ACTION", "inputs");
  try {
    return normalizeTickContext({ tick: 0, elapsedSeconds: 0, inputs, commands: [] }).inputs;
  } catch (error) {
    streamError("INVALID_ACTION", error instanceof Error ? error.message : undefined);
  }
}

function parseEvent(value: unknown): StreamStepEvent {
  if (!isRecord(value)) streamError("INVALID_STREAM_MESSAGE", "Each event must be an object.");
  if (value.type === "join" || value.type === "leave") {
    assertAllowedKeys(value, ["type", "playerId"], "Lifecycle event");
    if (typeof value.playerId !== "string" || value.playerId.trim().length === 0) {
      streamError("INVALID_STREAM_MESSAGE", "Lifecycle events require a non-empty playerId.");
    }
    return { type: value.type, playerId: value.playerId };
  }
  if (value.type === "command") {
    assertAllowedKeys(value, ["type", "command"], "Command event");
    if (!isRecord(value.command)) streamError("INVALID_ACTION", "command must be an object.");
    assertAllowedKeys(value.command, ["type", "data", "payload", "issuer", "timestamp"], "Command");
    try {
      validateGameCommand(value.command);
    } catch (error) {
      streamError("INVALID_ACTION", error instanceof Error ? error.message : undefined);
    }
    return { type: "command", command: normalizeGameCommand(value.command) };
  }
  return streamError("INVALID_STREAM_MESSAGE", "events contains an unsupported event type.");
}

function parseEvents(value: unknown): StreamStepEvent[] {
  return parseArray(value, "INVALID_STREAM_MESSAGE", "events").map(parseEvent);
}

function parseResyncPlayers(value: unknown): string[] {
  const players = parseArray(value, "INVALID_STREAM_MESSAGE", "resyncPlayers");
  if (players.some(player => typeof player !== "string" || player.trim().length === 0)
    || new Set(players).size !== players.length) {
    streamError("INVALID_STREAM_MESSAGE", "resyncPlayers must contain unique, non-empty player IDs.");
  }
  return players as string[];
}

function parseReplayStep(value: unknown): ParsedReplayStep {
  if (!isRecord(value) || !Number.isSafeInteger(value.tick) || (value.tick as number) < 0) {
    streamError("INVALID_STREAM_MESSAGE", "Each replay step must have a non-negative safe-integer tick.");
  }
  assertAllowedKeys(value, ["tick", "inputs", "events"], "Replay step");
  return {
    tick: value.tick as number,
    inputs: parseInputs(value.inputs),
    events: parseEvents(value.events),
  };
}

async function invokeGame<T>(name: string, callback: () => T | Promise<T>): Promise<T> {
  try {
    return await callback();
  } catch (error) {
    if (error instanceof SdkProtocolError) throw error;
    console.error(`[server-sdk] Stream game callback failed: ${name}.`);
    throw new SdkProtocolError("CALLBACK_FAILED");
  }
}

function validateStreamManifest(value: unknown): asserts value is GameManifest {
  try {
    validateGameManifest(value);
  } catch (error) {
    if (error instanceof SdkProtocolError) throw error;
    streamError("INVALID_MANIFEST");
  }
}

function validateRosterBounds(players: readonly string[], manifest: GameManifest): void {
  if (players.length > manifest.maxPlayers) {
    streamError("WRONG_ROSTER", "The active roster exceeds manifest.maxPlayers.");
  }
}

function assertActiveRoster(players: readonly string[], inputs: readonly TickInput[]): void {
  const active = new Set(players);
  const actionIds = new Set<string>();
  for (const input of inputs) {
    if (!active.has(input.playerId)) streamError("WRONG_ROSTER");
    if (input.actionId !== undefined && actionIds.has(input.actionId)) {
      streamError("INVALID_ACTION", "A tick must not repeat an actionId.");
    }
    if (input.actionId !== undefined) actionIds.add(input.actionId);
  }
}

async function applySingleStep<TState>(
  game: GameServer<TState>,
  manifest: GameManifest,
  tickRateHz: number,
  curState: TState,
  currentPlayers: readonly string[],
  tick: number,
  inputs: TickInput[],
  events: StreamStepEvent[],
  resyncPlayers: readonly string[],
): Promise<AppliedStreamStep<TState>> {
  let state = curState;
  const players = [...currentPlayers];
  const broadcasts: JsonValue[] = [];
  const resultUnits: GameResultEvent[] = [];
  let joinable: boolean | undefined;
  let playerViews: Record<string, JsonValue> | undefined;
  const privateViewsRequired = manifest.playerViewMode === "private";

  const expectedPlayers = [...players];
  for (const event of events) {
    if (event.type === "join") {
      if (!expectedPlayers.includes(event.playerId)) {
        expectedPlayers.push(event.playerId);
        validateRosterBounds(expectedPlayers, manifest);
      }
    } else if (event.type === "leave") {
      const index = expectedPlayers.indexOf(event.playerId);
      if (index < 0) streamError("WRONG_ROSTER");
      expectedPlayers.splice(index, 1);
    }
  }
  assertActiveRoster(expectedPlayers, inputs);
  for (const playerId of resyncPlayers) {
    if (!expectedPlayers.includes(playerId)) streamError("WRONG_ROSTER");
  }

  function applyStateResult(
    result: unknown,
  ): asserts result is Record<string, unknown> & { state: TState } {
    validateGameStateResult(result, players, privateViewsRequired);
  }

  for (const event of events) {
    if (event.type === "join") {
      if (!players.includes(event.playerId)) {
        players.push(event.playerId);
        validateRosterBounds(players, manifest);
      }
      const result = await invokeGame("onPlayerJoin", () => game.onPlayerJoin(event.playerId, state));
      applyStateResult(result);
      state = result.state;
      if (result.broadcasts) broadcasts.push(...(result.broadcasts as JsonValue[]));
      if (result.resultUnits) resultUnits.push(...result.resultUnits as GameResultEvent[]);
      if (result.joinable !== undefined) joinable = result.joinable as boolean;
      if (result.playerViews !== undefined) playerViews = result.playerViews as Record<string, JsonValue>;
    } else if (event.type === "leave") {
      const playerIndex = players.indexOf(event.playerId);
      if (playerIndex < 0) streamError("WRONG_ROSTER");
      players.splice(playerIndex, 1);
      const result = await invokeGame("onPlayerLeave", () => game.onPlayerLeave(event.playerId, state));
      applyStateResult(result);
      state = result.state;
      if (result.broadcasts) broadcasts.push(...(result.broadcasts as JsonValue[]));
      if (result.resultUnits) resultUnits.push(...result.resultUnits as GameResultEvent[]);
      if (result.joinable !== undefined) joinable = result.joinable as boolean;
      if (result.playerViews !== undefined) playerViews = result.playerViews as Record<string, JsonValue>;
    } else {
      if (typeof game.onCommand !== "function") streamError("UNSUPPORTED_FEATURE");
      const result = await invokeGame("onCommand", () => game.onCommand!(event.command, state));
      applyStateResult(result);
      state = result.state;
      if (result.broadcasts) broadcasts.push(...(result.broadcasts as JsonValue[]));
      if (result.resultUnits) resultUnits.push(...result.resultUnits as GameResultEvent[]);
      if (result.joinable !== undefined) joinable = result.joinable as boolean;
      if (result.playerViews !== undefined) playerViews = result.playerViews as Record<string, JsonValue>;
    }
  }

  let tickContext: TickContext = {
    tick,
    elapsedSeconds: tick / tickRateHz,
    inputs,
    commands: [],
  };
  try {
    validateTickContext(tickContext);
  } catch (error) {
    streamError("INVALID_ACTION", error instanceof Error ? error.message : undefined);
  }
  const tickResult = await invokeGame("tick", () => game.tick(tickContext, state));
  validateGameTickResult(tickResult, inputs, players, privateViewsRequired);
  state = tickResult.state;
  if (tickResult.broadcasts) broadcasts.push(...(tickResult.broadcasts as JsonValue[]));
  if (tickResult.resultUnits) resultUnits.push(...tickResult.resultUnits as GameResultEvent[]);
  if (tickResult.joinable !== undefined) joinable = tickResult.joinable;
  if (tickResult.playerViews !== undefined) playerViews = tickResult.playerViews as Record<string, JsonValue>;

  const gameResult = { ...(await invokeGame("isGameOver", () => game.isGameOver(state))) };
  validateGameResult(gameResult);
  if (gameResult.resultUnits) {
    resultUnits.push(...gameResult.resultUnits as GameResultEvent[]);
    delete gameResult.resultUnits;
  }
  if (gameResult.over && joinable !== undefined) joinable = false;
  validateGameStateResult(
    { state, ...(playerViews === undefined ? {} : { playerViews }) },
    players,
    privateViewsRequired,
  );

  return {
    state,
    players,
    broadcasts,
    persist: tickResult.persist,
    ...(joinable === undefined ? {} : { joinable }),
    ...(playerViews === undefined ? {} : { playerViews }),
    resultUnits,
    ...(tickResult.actionResults === undefined
      ? {}
      : { actionResults: tickResult.actionResults as GameActionResult[] }),
    gameResult,
  };
}

function errorFrame(error: unknown, tick?: number): Record<string, unknown> {
  if (error instanceof SdkProtocolError) {
    return {
      type: "error",
      ...(tick === undefined ? {} : { tick }),
      code: error.code,
      retryable: error.retryable,
      creatorAction: SDK_ERROR_METADATA[error.code].creatorAction,
      error: error.message,
    };
  }
  console.error("[server-sdk] Session Stream handler failed.");
  const internalError = new SdkProtocolError("INTERNAL_ERROR");
  return {
    type: "error",
    ...(tick === undefined ? {} : { tick }),
    code: internalError.code,
    retryable: internalError.retryable,
    creatorAction: SDK_ERROR_METADATA[internalError.code].creatorAction,
    error: internalError.message,
  };
}

class NodeStreamSession<TState = unknown> {
  private tickRateHz = 20;
  private tick = 0;
  private state: TState | undefined;
  private players: string[] = [];
  private manifest: GameManifest | undefined;
  private hydrated = false;
  private gameOver = false;

  constructor(private readonly game: GameServer<TState>) {}

  async handleMessage(raw: unknown): Promise<Record<string, unknown>> {
    if (!isRecord(raw)) return errorFrame(new SdkProtocolError("INVALID_STREAM_MESSAGE"));
    if (raw.type === "hydrate") return this.hydrate(raw);
    if (raw.type === "step") return this.step(raw);
    return errorFrame(new SdkProtocolError("INVALID_STREAM_MESSAGE", "Unsupported stream message type."));
  }

  private reset(): void {
    this.state = undefined;
    this.players = [];
    this.manifest = undefined;
    this.tick = 0;
    this.tickRateHz = 20;
    this.gameOver = false;
    this.hydrated = false;
  }

  private async hydrate(message: Record<string, unknown>): Promise<Record<string, unknown>> {
    this.reset();
    const messageTick = Number.isSafeInteger(message.tick) ? message.tick as number : undefined;
    try {
      assertAllowedKeys(
        message,
        ["type", "matchId", "tickRateHz", "tick", "state", "players", "playerViews", "replay", "resyncPlayers"],
        "Hydrate",
      );
      if (typeof message.matchId !== "string"
        || message.matchId.trim().length === 0
        || typeof message.tickRateHz !== "number"
        || !Number.isFinite(message.tickRateHz)
        || message.tickRateHz < 1
        || message.tickRateHz > 60
        || messageTick === undefined
        || messageTick < 0
        || !Object.hasOwn(message, "state")) {
        streamError("INVALID_STREAM_MESSAGE", "Hydrate requires matchId, tickRateHz, tick, and state.");
      }
      const tickRateHz = message.tickRateHz as number;
      const replayRaw = parseArray(message.replay, "INVALID_STREAM_MESSAGE", "replay");
      const replay = replayRaw.map(parseReplayStep);
      const resyncPlayers = parseResyncPlayers(message.resyncPlayers);
      const manifest = await invokeGame("manifest", () => this.game.manifest());
      validateStreamManifest(manifest);

      const players = Object.hasOwn(message, "players") ? message.players : [];
      if (!Object.hasOwn(message, "players")) {
        streamError("WRONG_ROSTER", "Hydrate requires the active players roster.");
      }
      validatePlayerRoster(players);
      const activePlayers = players as string[];
      validateRosterBounds(activePlayers, manifest);
      if (Object.hasOwn(message, "resyncPlayers")) {
        for (const playerId of resyncPlayers) {
          if (!activePlayers.includes(playerId)) streamError("WRONG_ROSTER");
        }
      }
      try {
        validateJsonValue(message.state);
      } catch {
        streamError("INVALID_STATE");
      }

      let curTick = messageTick;
      let curState = message.state as TState;
      let curPlayers = [...activePlayers];
      const broadcasts: JsonValue[] = [];
      const resultUnits: GameResultEvent[] = [];
      const actionResults: GameActionResult[] = [];
      let joinable: boolean | undefined;
      let lastViews: Record<string, JsonValue> | undefined;
      const privateViewsRequired = manifest.playerViewMode === "private";

      if (message.playerViews !== undefined) {
        validateGameStateResult(
          { state: curState, playerViews: message.playerViews },
          curPlayers,
          privateViewsRequired && replay.length === 0,
        );
        lastViews = message.playerViews as Record<string, JsonValue>;
      } else if (privateViewsRequired && replay.length === 0) {
        validateGameStateResult({ state: curState }, curPlayers, true);
      }

      const initialResult = { ...(await invokeGame("isGameOver", () => this.game.isGameOver(curState))) };
      validateGameResult(initialResult);
      if (initialResult.resultUnits) resultUnits.push(...initialResult.resultUnits);
      delete initialResult.resultUnits;
      let gameResult: GameResult = initialResult;

      for (const step of replay) {
        if (gameResult.over || step.tick !== curTick + 1) {
          streamError("REPLAY_MISMATCH", "Replay must continue from the checkpoint with contiguous ticks before game over.");
        }
        const out = await applySingleStep(
          this.game,
          manifest,
          tickRateHz,
          curState,
          curPlayers,
          step.tick,
          step.inputs,
          step.events,
          [],
        );
        curTick = step.tick;
        curState = out.state;
        curPlayers = out.players;
        broadcasts.push(...out.broadcasts);
        resultUnits.push(...out.resultUnits);
        if (out.actionResults) actionResults.push(...out.actionResults);
        if (out.joinable !== undefined) joinable = out.joinable;
        lastViews = out.playerViews;
        gameResult = out.gameResult;
      }

      this.tickRateHz = tickRateHz;
      this.tick = curTick;
      this.state = curState;
      this.players = curPlayers;
      this.manifest = manifest;
      this.hydrated = true;
      this.gameOver = gameResult.over;
      return {
        type: "hydrated",
        matchId: message.matchId,
        tick: curTick,
        ...(replay.length > 0 ? { state: curState } : {}),
        ...(lastViews === undefined ? {} : { playerViews: lastViews }),
        ...(broadcasts.length === 0 ? {} : { broadcasts }),
        ...(joinable === undefined ? {} : { joinable }),
        ...(resultUnits.length === 0 ? {} : { resultUnits }),
        ...(actionResults.length === 0 ? {} : { actionResults }),
        gameResult,
      };
    } catch (error) {
      this.reset();
      return errorFrame(error, messageTick);
    }
  }

  private async step(message: Record<string, unknown>): Promise<Record<string, unknown>> {
    const tick = Number.isSafeInteger(message.tick) ? message.tick as number : undefined;
    try {
      assertAllowedKeys(message, ["type", "tick", "inputs", "events", "resyncPlayers", "wantState"], "Step");
      if (tick === undefined || tick < 0) {
        streamError("INVALID_STREAM_MESSAGE", "step.tick must be a non-negative safe integer.");
      }
      const inputs = parseInputs(message.inputs);
      const events = parseEvents(message.events);
      const resyncPlayers = parseResyncPlayers(message.resyncPlayers);
      if (message.wantState !== undefined && typeof message.wantState !== "boolean") {
        streamError("INVALID_STREAM_MESSAGE", "wantState must be a boolean when provided.");
      }
      if (!this.hydrated || this.state === undefined) {
        streamError("INVALID_STREAM_MESSAGE", "Stream session is not hydrated.");
      }
      if (this.gameOver) return { type: "ignoredAfterGameOver", tick };
      if (this.tick >= Number.MAX_SAFE_INTEGER || tick !== this.tick + 1) {
        streamError("TICK_SEQUENCE_MISMATCH", `Expected tick ${this.tick + 1}.`);
      }
      const manifest = this.manifest;
      if (manifest === undefined) streamError("INVALID_STREAM_MESSAGE", "Stream manifest is unavailable.");
      const out = await applySingleStep(
        this.game,
        manifest,
        this.tickRateHz,
        structuredClone(this.state),
        this.players,
        tick,
        inputs,
        events,
        resyncPlayers,
      );
      const includeState =
        message.wantState === true
        || events.length > 0
        || out.persist
        || out.resultUnits.length > 0
        || out.gameResult.over;
      this.tick = tick;
      this.state = out.state;
      this.players = out.players;
      this.gameOver = out.gameResult.over;
      return {
        type: "stepResult",
        tick,
        ...(includeState ? { state: out.state } : {}),
        ...(out.playerViews === undefined ? {} : { playerViews: out.playerViews }),
        broadcasts: out.broadcasts,
        persist: out.persist,
        ...(out.joinable === undefined ? {} : { joinable: out.joinable }),
        ...(out.resultUnits.length === 0 ? {} : { resultUnits: out.resultUnits }),
        ...(out.actionResults === undefined ? {} : { actionResults: out.actionResults }),
        gameResult: out.gameResult,
      };
    } catch (error) {
      if (error instanceof SdkProtocolError
        && ["CALLBACK_FAILED", "INVALID_RESULT", "INVALID_STATE", "INVALID_GAME_OVER_POLICY", "RESOURCE_LIMIT_EXCEEDED", "INTERNAL_ERROR"].includes(error.code)) {
        this.reset();
      }
      return errorFrame(error, tick);
    }
  }
}

export function resolveStreamDurability(game: GameServer): StreamDurability | undefined {
  const raw = typeof game.durability === "function" ? game.durability() : game.durability;
  return raw === "checkpoint" || raw === "strict" ? raw : undefined;
}

export function attachStreamUpgradeHandler(server: http.Server, game: GameServer): void {
  const activeSockets = new Set<Duplex>();
  server.on("close", () => {
    for (const sock of activeSockets) {
      sock.destroy();
    }
    activeSockets.clear();
  });

  server.on("upgrade", (req, socket, head) => {
    const pathname = new URL(req.url || "/", `http://${req.headers.host || "localhost"}`).pathname;
    const key = req.headers["sec-websocket-key"];
    if (pathname !== "/stream" || typeof key !== "string" || key.length === 0) {
      socket.write("HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
      socket.destroy();
      return;
    }

    activeSockets.add(socket);
    socket.on("close", () => activeSockets.delete(socket));

    const accept = crypto.createHash("sha1").update(key + WS_GUID).digest("base64");
    const durability = resolveStreamDurability(game);
    const durabilityHeader = durability ? `X-RandSeed-Stream-Durability: ${durability}\r\n` : "";
    socket.write(
      `HTTP/1.1 101 Switching Protocols\r\n` +
        `Upgrade: websocket\r\n` +
        `Connection: Upgrade\r\n` +
        `Sec-WebSocket-Accept: ${accept}\r\n` +
        `X-RandSeed-Stream-Protocol: ${STREAM_PROTOCOL_V1}\r\n` +
        `${durabilityHeader}\r\n`,
    );

    const session = new NodeStreamSession(game);
    let buffer = head.length > 0 ? Buffer.from(head) : Buffer.alloc(0);
    let fragmentChunks: Buffer[] = [];
    let fragmentBytes = 0;
    let fragmentOpcode = 0;
    let queue = Promise.resolve();

    const dispatchMessage = (payload: Buffer) => {
      const text = payload.toString("utf8");
      queue = queue.then(async () => {
        let parsed: unknown;
        try {
          parsed = JSON.parse(text);
        } catch {
          sendWsJson(socket, errorFrame(new SdkProtocolError("INVALID_STREAM_MESSAGE")));
          return;
        }
        const reply = await session.handleMessage(parsed);
        sendWsJson(socket, reply);
      });
    };

    const rejectOversizedFrame = () => {
      sendWsJson(socket, errorFrame(new SdkProtocolError("RESOURCE_LIMIT_EXCEEDED")));
      const closePayload = Buffer.allocUnsafe(2);
      closePayload.writeUInt16BE(1009, 0);
      socket.end(encodeWsFrame(0x08, closePayload));
    };

    const processFrames = () => {
      while (buffer.length >= 2) {
        const first = buffer[0]!;
        const second = buffer[1]!;
        const fin = (first & 0x80) !== 0;
        const opcode = first & 0x0f;
        const masked = (second & 0x80) !== 0;
        let payloadLen = second & 0x7f;
        let offset = 2;

        if (payloadLen === 126) {
          if (buffer.length < 4) return;
          payloadLen = buffer.readUInt16BE(2);
          offset = 4;
        } else if (payloadLen === 127) {
          if (buffer.length < 10) return;
          const bigLen = buffer.readBigUInt64BE(2);
          if (bigLen > BigInt(MAX_WS_PAYLOAD_BYTES)) {
            rejectOversizedFrame();
            return;
          }
          payloadLen = Number(bigLen);
          offset = 10;
        }

        if (payloadLen > MAX_WS_PAYLOAD_BYTES) {
          rejectOversizedFrame();
          return;
        }

        const maskBytes = masked ? 4 : 0;
        if (buffer.length < offset + maskBytes + payloadLen) return;

        const payload = Buffer.from(buffer.subarray(offset + maskBytes, offset + maskBytes + payloadLen));
        if (masked) {
          const mask = buffer.subarray(offset, offset + 4);
          for (let i = 0; i < payload.length; i++) {
            payload[i]! ^= mask[i & 3]!;
          }
        }
        buffer = buffer.subarray(offset + maskBytes + payloadLen);

        if (opcode === 0x08) {
          socket.write(encodeWsFrame(0x08, Buffer.alloc(0)));
          socket.end();
          return;
        }
        if (opcode === 0x09) {
          socket.write(encodeWsFrame(0x0a, payload));
          continue;
        }
        if (opcode === 0x0a) {
          continue;
        }
        if (opcode === 0x01 || opcode === 0x02) {
          if (fin) {
            dispatchMessage(payload);
          } else {
            fragmentOpcode = opcode;
            fragmentChunks = [payload];
            fragmentBytes = payload.length;
          }
        } else if (opcode === 0x00 && fragmentOpcode !== 0) {
          fragmentBytes += payload.length;
          if (fragmentBytes > MAX_WS_PAYLOAD_BYTES) {
            rejectOversizedFrame();
            return;
          }
          fragmentChunks.push(payload);
          if (fin) {
            const full = Buffer.concat(fragmentChunks, fragmentBytes);
            fragmentChunks = [];
            fragmentBytes = 0;
            fragmentOpcode = 0;
            dispatchMessage(full);
          }
        }
      }
    };

    if (buffer.length > 0) processFrames();
    socket.on("data", (chunk: Buffer) => {
      buffer = buffer.length === 0 ? chunk : Buffer.concat([buffer, chunk]);
      processFrames();
    });
    socket.on("error", () => socket.destroy());
  });
}
