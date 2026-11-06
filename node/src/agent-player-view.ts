import {
  isValidActionSchemaId,
  isValidProtocolKey,
} from "./protocol-identifiers.js";

const safeInteger = (value: unknown, minimum: number): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= minimum;

const key = isValidProtocolKey;
const schemaIdentifier = isValidActionSchemaId;

export type AgentPlayerMode = "follow" | "autoplay";

export type AgentPlayerJsonValue =
  | null
  | boolean
  | number
  | string
  | AgentPlayerJsonValue[]
  | { [key: string]: AgentPlayerJsonValue };

export interface AgentPlayerContract {
  playerViewProtocolVersion: 1;
  actionProtocolVersion: 1;
  playerViewSchemaId: string;
  actionSchemaId: string;
  modes: AgentPlayerMode[];
  maxDecisionHz: number;
  independentSeats: boolean;
  backgroundAutoplay: boolean;
}

export interface AgentPlayerViewScope {
  gameId: string;
  environment: string;
  releaseVersion: string;
}

export interface AgentPlayerViewContext {
  scope: AgentPlayerViewScope;
  roomId: string;
  matchId: string;
  matchSeq: number;
  playerId: string;
  sequence: number;
  tick: number;
}

export interface AgentPlayerViewRequest {
  context: AgentPlayerViewContext;
  players: string[];
  state: AgentPlayerJsonValue;
  committedEvents: AgentPlayerJsonValue[];
}

export interface AgentPlayerViewProjection {
  publicState: AgentPlayerJsonValue;
  privateState: AgentPlayerJsonValue;
  visibleEvents: AgentPlayerJsonValue[];
  decision: {
    canAct: boolean;
    deadlineAt: number | null;
  };
}

export interface AgentPlayerView extends AgentPlayerViewContext {
  protocolVersion: 1;
  schemaId: string;
  publicState: AgentPlayerJsonValue;
  self: {
    playerId: string;
    privateState: AgentPlayerJsonValue;
  };
  visibleEvents: AgentPlayerJsonValue[];
  decision: {
    canAct: boolean;
    deadlineAt: number | null;
  };
}

export interface AgentControllerLease {
  roomId: string;
  matchId: string;
  playerId: string;
  controller: { type: "human" } | { type: "agent"; runId: string };
  epoch: number;
  expiresAt: number;
}

export interface AgentActionRequest {
  protocolVersion: 1;
  scope: AgentPlayerViewScope;
  roomId: string;
  matchId: string;
  matchSeq: number;
  playerId: string;
  actionId: string;
  controller: { type: "human" } | { type: "agent"; runId: string };
  controllerEpoch: number;
  observationSequence: number;
  expectedTick: number;
  actionSchemaId: string;
  action: AgentPlayerJsonValue;
}

export type AgentActionReceiptStatus = "queued" | "applied" | "rejected" | "unresolved";

export interface AgentActionReceipt {
  protocolVersion: 1;
  actionId: string;
  roomId: string;
  matchId: string;
  playerId: string;
  controllerEpoch: number;
  expectedTick: number;
  status: AgentActionReceiptStatus;
  committedTick?: number;
  code?: string;
}

export type AgentSkillDecision =
  | { kind: "act"; action: AgentPlayerJsonValue; memory: AgentPlayerJsonValue }
  | { kind: "wait"; resumeAt: number | null; memory: AgentPlayerJsonValue };

export const MAX_AGENT_PLAYER_VIEW_BYTES = 6_000;
export const MAX_AGENT_PLAYER_ACTION_BYTES = 2_000;
export const MAX_AGENT_PLAYER_MEMORY_BYTES = 16_000;

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function copyJson(value: unknown, ancestors = new Set<object>()): AgentPlayerJsonValue {
  if (value === null || typeof value === "string" || typeof value === "boolean") return value;
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value !== "object" || ancestors.has(value)) {
    throw new TypeError("Agent player data must be finite JSON without cycles.");
  }
  ancestors.add(value);
  try {
    if (Array.isArray(value)) {
      return Array.from({ length: value.length }, (_, index) => {
        if (!Object.hasOwn(value, index)) throw new TypeError("Agent player arrays must not contain holes.");
        return copyJson(value[index], ancestors);
      });
    }
    const prototype = Object.getPrototypeOf(value);
    if (prototype !== Object.prototype && prototype !== null) {
      throw new TypeError("Agent player objects must be plain JSON objects.");
    }
    return Object.fromEntries(Object.entries(value).map(([field, item]) => [field, copyJson(item, ancestors)]));
  } finally {
    ancestors.delete(value);
  }
}

function serializedBytes(value: unknown): number {
  return new TextEncoder().encode(JSON.stringify(value)).byteLength;
}

function validController(value: unknown): value is AgentActionRequest["controller"] {
  return isRecord(value)
    && ((value.type === "human" && Object.keys(value).length === 1)
      || (value.type === "agent" && Object.keys(value).length === 2 && key(value.runId)));
}

export function validateAgentPlayerContract(value: unknown): AgentPlayerContract {
  if (!isRecord(value)
    || value.playerViewProtocolVersion !== 1
    || value.actionProtocolVersion !== 1
    || !schemaIdentifier(value.playerViewSchemaId)
    || !schemaIdentifier(value.actionSchemaId)
    || !Array.isArray(value.modes)
    || value.modes.length === 0
    || value.modes.some(mode => mode !== "follow" && mode !== "autoplay")
    || new Set(value.modes).size !== value.modes.length
    || typeof value.maxDecisionHz !== "number"
    || !Number.isFinite(value.maxDecisionHz)
    || value.maxDecisionHz <= 0
    || value.maxDecisionHz > 10
    || typeof value.independentSeats !== "boolean"
    || typeof value.backgroundAutoplay !== "boolean"
    || (value.backgroundAutoplay && (!value.modes.includes("autoplay") || !value.independentSeats))) {
    throw new TypeError("Invalid Agent player contract.");
  }
  return {
    playerViewProtocolVersion: 1,
    actionProtocolVersion: 1,
    playerViewSchemaId: value.playerViewSchemaId,
    actionSchemaId: value.actionSchemaId,
    modes: [...value.modes] as AgentPlayerMode[],
    maxDecisionHz: value.maxDecisionHz,
    independentSeats: value.independentSeats,
    backgroundAutoplay: value.backgroundAutoplay,
  };
}

export function validateAgentPlayerViewRequest(value: unknown): AgentPlayerViewRequest {
  if (!isRecord(value)) {
    throw new TypeError("Invalid Agent player view request.");
  }
  const context = value.context;
  const players = value.players;
  const committedEventsInput = value.committedEvents;
  if (!isRecord(context) || !isRecord(context.scope)
    || !Array.isArray(players) || !Array.isArray(committedEventsInput)
    || !Object.hasOwn(value, "state")) {
    throw new TypeError("Invalid Agent player view request.");
  }
  const scope = context.scope;
  if (!key(scope.gameId) || !key(scope.environment) || !key(scope.releaseVersion)
    || !key(context.roomId) || !key(context.matchId) || !key(context.playerId)
    || !safeInteger(context.matchSeq, 1) || !safeInteger(context.sequence, 1)
    || !safeInteger(context.tick, 0)
    || players.length === 0 || players.length > 100
    || players.some(player => !key(player))
    || new Set(players).size !== players.length
    || !players.includes(context.playerId)) {
    throw new TypeError("Invalid Agent player view context or roster.");
  }
  const state = copyJson(value.state);
  const committedEvents = copyJson(committedEventsInput);
  if (!Array.isArray(committedEvents)
    || serializedBytes({
      context,
      players,
      state,
      committedEvents,
    }) > 1_048_576) {
    throw new TypeError("Agent player view request exceeds the supported JSON limit.");
  }
  return {
    context: {
      scope: {
        gameId: scope.gameId,
        environment: scope.environment,
        releaseVersion: scope.releaseVersion,
      },
      roomId: context.roomId,
      matchId: context.matchId,
      matchSeq: context.matchSeq,
      playerId: context.playerId,
      sequence: context.sequence,
      tick: context.tick,
    },
    players: [...players],
    state,
    committedEvents,
  };
}

export function createAgentPlayerView(
  contract: AgentPlayerContract,
  request: AgentPlayerViewRequest,
  projection: AgentPlayerViewProjection,
): AgentPlayerView {
  validateAgentPlayerContract(contract);
  const safeRequest = validateAgentPlayerViewRequest(request);
  if (!safeRequest.players.includes(safeRequest.context.playerId)
    || !isRecord(projection)
    || !Object.hasOwn(projection, "publicState")
    || !Object.hasOwn(projection, "privateState")
    || !Array.isArray(projection.visibleEvents)
    || !isRecord(projection.decision)
    || typeof projection.decision.canAct !== "boolean"
    || (projection.decision.deadlineAt !== null
      && !safeInteger(projection.decision.deadlineAt, 0))) {
    throw new TypeError("Agent player projection is incomplete.");
  }
  const view: AgentPlayerView = {
    protocolVersion: 1,
    ...safeRequest.context,
    schemaId: contract.playerViewSchemaId,
    publicState: copyJson(projection.publicState),
    self: {
      playerId: safeRequest.context.playerId,
      privateState: copyJson(projection.privateState),
    },
    visibleEvents: copyJson(projection.visibleEvents) as AgentPlayerJsonValue[],
    decision: {
      canAct: projection.decision.canAct,
      deadlineAt: projection.decision.deadlineAt,
    },
  };
  if (serializedBytes(view) > MAX_AGENT_PLAYER_VIEW_BYTES) {
    throw new TypeError("Agent player view exceeds the 6000-byte limit.");
  }
  return view;
}

export function validateAgentControllerLease(value: unknown): AgentControllerLease {
  if (!isRecord(value) || !key(value.roomId) || !key(value.matchId) || !key(value.playerId)
    || !validController(value.controller)
    || !safeInteger(value.epoch, 1)
    || !safeInteger(value.expiresAt, 1)) {
    throw new TypeError("Invalid Agent controller lease.");
  }
  return {
    roomId: value.roomId,
    matchId: value.matchId,
    playerId: value.playerId,
    controller: value.controller,
    epoch: value.epoch,
    expiresAt: value.expiresAt,
  };
}

export function validateAgentActionRequest(value: unknown): AgentActionRequest {
  if (!isRecord(value) || !isRecord(value.scope)
    || value.protocolVersion !== 1
    || !key(value.scope.gameId) || !key(value.scope.environment) || !key(value.scope.releaseVersion)
    || !key(value.roomId) || !key(value.matchId) || !key(value.playerId) || !key(value.actionId)
    || !validController(value.controller)
    || !safeInteger(value.matchSeq, 1)
    || !safeInteger(value.controllerEpoch, 1)
    || !safeInteger(value.observationSequence, 1)
    || !safeInteger(value.expectedTick, 0)
    || !schemaIdentifier(value.actionSchemaId)
    || !Object.hasOwn(value, "action")) {
    throw new TypeError("Invalid Agent action request.");
  }
  const action = copyJson(value.action);
  if (serializedBytes(action) > MAX_AGENT_PLAYER_ACTION_BYTES) {
    throw new TypeError("Agent action exceeds the 2000-byte limit.");
  }
  return {
    protocolVersion: 1,
    scope: {
      gameId: value.scope.gameId,
      environment: value.scope.environment,
      releaseVersion: value.scope.releaseVersion,
    },
    roomId: value.roomId,
    matchId: value.matchId,
    matchSeq: value.matchSeq,
    playerId: value.playerId,
    actionId: value.actionId,
    controller: value.controller,
    controllerEpoch: value.controllerEpoch,
    observationSequence: value.observationSequence,
    expectedTick: value.expectedTick,
    actionSchemaId: value.actionSchemaId,
    action,
  };
}

export function validateAgentActionReceipt(value: unknown): AgentActionReceipt {
  const statuses: AgentActionReceiptStatus[] = ["queued", "applied", "rejected", "unresolved"];
  if (!isRecord(value) || value.protocolVersion !== 1
    || !key(value.actionId) || !key(value.roomId) || !key(value.matchId) || !key(value.playerId)
    || !safeInteger(value.controllerEpoch, 1)
    || !safeInteger(value.expectedTick, 0)
    || !statuses.includes(value.status as AgentActionReceiptStatus)
    || (value.committedTick !== undefined && !safeInteger(value.committedTick, 0))
    || (value.code !== undefined && (typeof value.code !== "string"
      || !/^[A-Za-z0-9._:-]{1,128}$/.test(value.code)))
    || ((value.status === "applied" || value.status === "rejected")
      && !safeInteger(value.committedTick, 0))) {
    throw new TypeError("Invalid Agent action receipt.");
  }
  return {
    protocolVersion: 1,
    actionId: value.actionId,
    roomId: value.roomId,
    matchId: value.matchId,
    playerId: value.playerId,
    controllerEpoch: value.controllerEpoch,
    expectedTick: value.expectedTick,
    status: value.status as AgentActionReceiptStatus,
    ...(value.committedTick === undefined ? {} : { committedTick: value.committedTick }),
    ...(value.code === undefined ? {} : { code: value.code }),
  };
}

export function validateAgentSkillDecision(value: unknown): AgentSkillDecision {
  if (!isRecord(value) || !Object.hasOwn(value, "memory")) {
    throw new TypeError("Invalid Agent Skill decision.");
  }
  const memory = copyJson(value.memory);
  if (serializedBytes(memory) > MAX_AGENT_PLAYER_MEMORY_BYTES) {
    throw new TypeError("Agent memory exceeds the 16000-byte limit.");
  }
  if (value.kind === "act" && Object.hasOwn(value, "action")) {
    const action = copyJson(value.action);
    if (serializedBytes(action) > MAX_AGENT_PLAYER_ACTION_BYTES) {
      throw new TypeError("Agent action exceeds the 2000-byte limit.");
    }
    return { kind: "act", action, memory };
  }
  if (value.kind === "wait"
    && (value.resumeAt === null || safeInteger(value.resumeAt, 0))) {
    return { kind: "wait", resumeAt: value.resumeAt, memory };
  }
  throw new TypeError("Invalid Agent Skill decision.");
}
