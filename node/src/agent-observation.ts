export type AgentJsonValue =
  | null
  | boolean
  | number
  | string
  | AgentJsonValue[]
  | { [key: string]: AgentJsonValue };

export interface AgentObservationContext {
  matchId: string;
  sequence: number;
  tick: number;
  playerId: string;
}

export interface AgentObservation {
  protocolVersion: 1;
  matchId: string;
  sequence: number;
  tick: number;
  publicState: AgentJsonValue;
  self: {
    playerId: string;
    privateState: AgentJsonValue;
  };
}

function snapshotJson(value: AgentJsonValue, ancestors = new Set<object>()): AgentJsonValue {
  if (value === null || typeof value === "boolean" || typeof value === "string") return value;
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value !== "object") throw new Error("Observation data must be valid JSON.");
  if (ancestors.has(value)) throw new Error("Observation data must not contain cycles.");
  ancestors.add(value);
  try {
    if (Array.isArray(value)) {
      return Array.from({ length: value.length }, (_, index) => {
        if (!Object.hasOwn(value, index)) throw new Error("Observation arrays must not contain holes.");
        return snapshotJson(value[index], ancestors);
      });
    }
    const prototype = Object.getPrototypeOf(value);
    if (prototype !== Object.prototype && prototype !== null) {
      throw new Error("Observation objects must be plain JSON objects.");
    }
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, snapshotJson(item, ancestors)]));
  } finally {
    ancestors.delete(value);
  }
}

/**
 * Builds a snapshot for a player selected by the trusted room, not by an Agent.
 * The game must classify public and private data correctly before calling this.
 */
export function createAgentObservation(
  context: AgentObservationContext,
  publicState: AgentJsonValue,
  privateStates: ReadonlyMap<string, AgentJsonValue>,
): AgentObservation {
  if (typeof context.matchId !== "string" || !context.matchId.trim()) throw new Error("matchId must be a non-empty string.");
  if (typeof context.playerId !== "string" || !context.playerId.trim()) throw new Error("playerId must be a non-empty string.");
  if (!Number.isSafeInteger(context.sequence) || context.sequence < 0) throw new Error("sequence must be a non-negative safe integer.");
  if (!Number.isSafeInteger(context.tick) || context.tick < 0) throw new Error("tick must be a non-negative safe integer.");
  if (!privateStates.has(context.playerId)) throw new Error("The player's private view is unavailable.");
  const privateState = privateStates.get(context.playerId);
  if (privateState === undefined) throw new Error("The player's private view is invalid.");
  return {
    protocolVersion: 1,
    matchId: context.matchId,
    sequence: context.sequence,
    tick: context.tick,
    publicState: snapshotJson(publicState),
    self: { playerId: context.playerId, privateState: snapshotJson(privateState) },
  };
}
