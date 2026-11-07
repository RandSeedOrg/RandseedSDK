import crypto from "node:crypto";
import type http from "node:http";
import type { Duplex } from "node:stream";
import type {
  GameCommand,
  GameResult,
  GameResultEvent,
  GameServer,
  JsonValue,
  StreamDurability,
  TickInput,
} from "./types.js";
import { validateGameResultUnits } from "./result-units.js";

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

async function applySingleStep<TState>(
  game: GameServer<TState>,
  tickRateHz: number,
  curState: TState,
  tick: number,
  inputs: TickInput[],
  events: StreamStepEvent[],
): Promise<{
  state: TState;
  broadcasts: JsonValue[];
  persist: boolean;
  joinable?: boolean;
  playerViews?: Record<string, JsonValue>;
  resultUnits: GameResultEvent[];
  gameResult: GameResult;
}> {
  let state = curState;
  const broadcasts: JsonValue[] = [];
  const resultUnits: GameResultEvent[] = [];
  let joinable: boolean | undefined;
  let playerViews: Record<string, JsonValue> | undefined;

  for (const ev of events) {
    if (ev.type === "join") {
      if (typeof ev.playerId !== "string" || ev.playerId.trim().length === 0) {
        throw new StreamValidationError("playerId must be non-empty.");
      }
      const res = await game.onPlayerJoin(ev.playerId, state);
      validateGameResultUnits(res.resultUnits);
      state = res.state;
      if (res.broadcasts) broadcasts.push(...(res.broadcasts as JsonValue[]));
      if (res.resultUnits) resultUnits.push(...res.resultUnits);
      if (res.joinable !== undefined) joinable = res.joinable;
      if (res.playerViews !== undefined) playerViews = res.playerViews as Record<string, JsonValue>;
    } else if (ev.type === "leave") {
      if (typeof ev.playerId !== "string" || ev.playerId.trim().length === 0) {
        throw new StreamValidationError("playerId must be non-empty.");
      }
      const res = await game.onPlayerLeave(ev.playerId, state);
      validateGameResultUnits(res.resultUnits);
      state = res.state;
      if (res.broadcasts) broadcasts.push(...(res.broadcasts as JsonValue[]));
      if (res.resultUnits) resultUnits.push(...res.resultUnits);
      if (res.joinable !== undefined) joinable = res.joinable;
      if (res.playerViews !== undefined) playerViews = res.playerViews as Record<string, JsonValue>;
    } else if (ev.type === "command") {
      if (!ev.command || typeof ev.command.type !== "string" || ev.command.type.trim().length === 0) {
        throw new StreamValidationError("command.type must be a non-empty string.");
      }
      if (typeof game.onCommand !== "function") {
        throw new Error("The onCommand handler is not implemented.");
      }
      const res = await game.onCommand(ev.command, state);
      validateGameResultUnits(res.resultUnits);
      state = res.state;
      if (res.broadcasts) broadcasts.push(...(res.broadcasts as JsonValue[]));
      if (res.resultUnits) resultUnits.push(...res.resultUnits);
      if (res.joinable !== undefined) joinable = res.joinable;
      if (res.playerViews !== undefined) playerViews = res.playerViews as Record<string, JsonValue>;
    }
  }

  const tickRes = await game.tick(
    {
      tick,
      elapsedSeconds: tick / tickRateHz,
      inputs,
      commands: [],
    },
    state,
  );
  validateGameResultUnits(tickRes.resultUnits);
  state = tickRes.state;
  if (tickRes.broadcasts) broadcasts.push(...(tickRes.broadcasts as JsonValue[]));
  if (tickRes.resultUnits) resultUnits.push(...tickRes.resultUnits);
  if (tickRes.joinable !== undefined) joinable = tickRes.joinable;
  if (tickRes.playerViews !== undefined) playerViews = tickRes.playerViews as Record<string, JsonValue>;

  const gameResult = { ...(await game.isGameOver(state)) };
  validateGameResultUnits(gameResult.resultUnits);
  if (gameResult.resultUnits) {
    resultUnits.push(...gameResult.resultUnits);
    delete gameResult.resultUnits;
  }
  if (gameResult.over && joinable !== undefined) {
    joinable = false;
  }

  return {
    state,
    broadcasts,
    persist: Boolean(tickRes.persist),
    joinable,
    playerViews,
    resultUnits,
    gameResult,
  };
}

class StreamValidationError extends Error {}

class NodeStreamSession<TState = unknown> {
  private tickRateHz = 20;
  private tick = 0;
  private state: TState | undefined;
  private hydrated = false;
  private gameOver = false;

  constructor(private readonly game: GameServer<TState>) {}

  async handleMessage(raw: unknown): Promise<Record<string, unknown>> {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
      return { type: "error", code: "VALIDATION_ERROR", error: "Stream message must be an object." };
    }
    const msg = raw as Record<string, unknown>;
    if (msg.type === "hydrate") {
      if (
        typeof msg.matchId !== "string" ||
        msg.matchId.trim().length === 0 ||
        typeof msg.tickRateHz !== "number" ||
        !Number.isFinite(msg.tickRateHz) ||
        msg.tickRateHz < 1 ||
        msg.tickRateHz > 60 ||
        typeof msg.tick !== "number" ||
        !Number.isInteger(msg.tick) ||
        msg.tick < 0 ||
        msg.state === undefined
      ) {
        return {
          type: "error",
          ...(typeof msg.tick === "number" ? { tick: msg.tick } : {}),
          code: "VALIDATION_ERROR",
          error: "Invalid hydrate payload.",
        };
      }
      const hydrate = msg as unknown as HydrateMessage;
      let curTick = hydrate.tick;
      let curState = hydrate.state as TState;
      const broadcasts: JsonValue[] = [];
      const resultUnits: GameResultEvent[] = [];
      let joinable: boolean | undefined;
      let lastViews: Record<string, JsonValue> | undefined;
      const replay = Array.isArray(hydrate.replay) ? hydrate.replay : [];

      try {
        let gameResult = { ...(await this.game.isGameOver(curState)) };
        validateGameResultUnits(gameResult.resultUnits);
        if (gameResult.resultUnits) {
          resultUnits.push(...gameResult.resultUnits);
          delete gameResult.resultUnits;
        }
        for (const step of replay) {
          if (gameResult.over) break;
          if (step.tick !== curTick + 1) {
            return {
              type: "error",
              tick: step.tick,
              code: "VALIDATION_ERROR",
              error: "Replay ticks must be contiguous.",
            };
          }
          const out = await applySingleStep(
            this.game,
            hydrate.tickRateHz,
            curState,
            step.tick,
            step.inputs ?? [],
            step.events ?? [],
          );
          curTick = step.tick;
          curState = out.state;
          broadcasts.push(...out.broadcasts);
          resultUnits.push(...out.resultUnits);
          if (out.joinable !== undefined) joinable = out.joinable;
          lastViews = out.playerViews;
          gameResult = out.gameResult;
        }
        this.tickRateHz = hydrate.tickRateHz;
        this.tick = curTick;
        this.state = curState;
        this.hydrated = true;
        this.gameOver = gameResult.over;
        return {
          type: "hydrated",
          matchId: hydrate.matchId,
          tick: curTick,
          ...(replay.length > 0 ? { state: curState } : {}),
          ...(lastViews !== undefined ? { playerViews: lastViews } : {}),
          ...(broadcasts.length > 0 ? { broadcasts } : {}),
          ...(joinable !== undefined ? { joinable } : {}),
          ...(resultUnits.length > 0 ? { resultUnits } : {}),
          gameResult,
        };
      } catch (err) {
        this.hydrated = false;
        if (err instanceof StreamValidationError) {
          return { type: "error", tick: curTick, code: "VALIDATION_ERROR", error: err.message };
        }
        return { type: "error", tick: curTick, code: "INTERNAL_ERROR", error: "Internal server error." };
      }
    }

    if (msg.type === "step") {
      const tick = typeof msg.tick === "number" ? msg.tick : undefined;
      if (!this.hydrated || this.state === undefined) {
        return {
          type: "error",
          ...(tick !== undefined ? { tick } : {}),
          code: "VALIDATION_ERROR",
          error: "Stream session is not hydrated.",
        };
      }
      if (this.gameOver) {
        return { type: "ignoredAfterGameOver", tick: tick ?? this.tick };
      }
      if (tick !== this.tick + 1) {
        return {
          type: "error",
          ...(tick !== undefined ? { tick } : {}),
          code: "VALIDATION_ERROR",
          error: `Expected tick ${this.tick + 1}.`,
        };
      }
      const events = Array.isArray(msg.events) ? (msg.events as StreamStepEvent[]) : [];
      try {
        const out = await applySingleStep(
          this.game,
          this.tickRateHz,
          this.state,
          tick,
          Array.isArray(msg.inputs) ? (msg.inputs as TickInput[]) : [],
          events,
        );
        const includeState =
          Boolean(msg.wantState) ||
          events.length > 0 ||
          out.persist ||
          out.resultUnits.length > 0 ||
          out.gameResult.over;
        this.tick = tick;
        this.state = out.state;
        this.gameOver = out.gameResult.over;
        return {
          type: "stepResult",
          tick,
          ...(includeState ? { state: out.state } : {}),
          ...(out.playerViews !== undefined ? { playerViews: out.playerViews } : {}),
          broadcasts: out.broadcasts,
          persist: out.persist,
          ...(out.joinable !== undefined ? { joinable: out.joinable } : {}),
          ...(out.resultUnits.length > 0 ? { resultUnits: out.resultUnits } : {}),
          gameResult: out.gameResult,
        };
      } catch (err) {
        if (err instanceof StreamValidationError) {
          return { type: "error", tick, code: "VALIDATION_ERROR", error: err.message };
        }
        this.hydrated = false;
        return { type: "error", tick, code: "INTERNAL_ERROR", error: "Internal server error." };
      }
    }

    return { type: "error", code: "VALIDATION_ERROR", error: "Unsupported stream message type." };
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
          sendWsJson(socket, { type: "error", code: "VALIDATION_ERROR", error: "Invalid stream JSON." });
          return;
        }
        const reply = await session.handleMessage(parsed);
        sendWsJson(socket, reply);
      });
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
            socket.write(encodeWsFrame(0x08, Buffer.alloc(0)));
            socket.destroy();
            return;
          }
          payloadLen = Number(bigLen);
          offset = 10;
        }

        if (payloadLen > MAX_WS_PAYLOAD_BYTES) {
          socket.write(encodeWsFrame(0x08, Buffer.alloc(0)));
          socket.destroy();
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
            socket.write(encodeWsFrame(0x08, Buffer.alloc(0)));
            socket.destroy();
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
