import type {
  GameMetricAggregation,
  GameResultDefinition,
  GameResultEvent,
  GameResultParticipant,
  GameResultTeam,
} from "./types.js";

const outcomes = new Set(["win", "loss", "draw", "push"]);
const simpleAggregations = new Set(["sum", "min", "max"]);
const maxIdentifierCodePoints = 128;
const maxMetrics = 128;
const maxParticipants = 1000;
const maxTeams = 256;
const maxMetricsPerEntity = 128;
const maxEventsPerTransition = 32;
const maxEventBytes = 256 * 1024;
const maxSafeInteger = Number.MAX_SAFE_INTEGER;

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function assertOnlyKeys(value: Record<string, unknown>, allowed: readonly string[], label: string): void {
  const keys = new Set(allowed);
  for (const key of Object.keys(value)) {
    if (!keys.has(key)) throw new Error(`${label} contains unsupported field ${key}`);
  }
}

function validateIdentifier(value: unknown, label: string): asserts value is string {
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new Error(`${label} must be a non-empty string`);
  }
  if (Array.from(value).length > maxIdentifierCodePoints) {
    throw new Error(`${label} exceeds ${maxIdentifierCodePoints} Unicode code points`);
  }
}

function validateIdentifiers(value: unknown, label: string): asserts value is string[] {
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error(`${label} must be a non-empty string array`);
  }
  const ids = new Set<string>();
  for (const item of value) {
    validateIdentifier(item, label);
    if (ids.has(item)) throw new Error(`${label} must not contain duplicate identifiers`);
    ids.add(item);
  }
}

function isRatio(value: GameMetricAggregation): value is Extract<GameMetricAggregation, { type: "ratio" }> {
  return typeof value === "object" && value !== null;
}

/** Validate a versioned declaration of result-unit types and aggregate metrics. */
export function validateGameResultDefinition(value: unknown): asserts value is GameResultDefinition {
  if (!isRecord(value)) throw new Error("GameResultDefinition must be an object");
  assertOnlyKeys(value, ["version", "unitTypes", "metrics"], "GameResultDefinition");
  validateIdentifier(value.version, "GameResultDefinition.version");
  validateIdentifiers(value.unitTypes, "GameResultDefinition.unitTypes");
  if (!Array.isArray(value.metrics)) throw new Error("GameResultDefinition.metrics must be an array");
  if (value.metrics.length > maxMetrics) {
    throw new Error(`GameResultDefinition exceeds the ${maxMetrics}-metric limit`);
  }

  const unitTypes = new Set(value.unitTypes as string[]);
  const metrics = new Map<string, Record<string, unknown>>();
  for (const [index, raw] of value.metrics.entries()) {
    const label = `GameResultDefinition.metrics[${index}]`;
    if (!isRecord(raw)) throw new Error(`${label} must be an object`);
    assertOnlyKeys(raw, ["id", "scope", "unit", "unitTypes", "aggregation"], label);
    validateIdentifier(raw.id, `${label}.id`);
    if (metrics.has(raw.id)) throw new Error(`Duplicate metric ID: ${raw.id}`);
    if (raw.scope !== "participant" && raw.scope !== "team") {
      throw new Error(`${label}.scope must be participant or team`);
    }
    validateIdentifier(raw.unit, `${label}.unit`);
    validateIdentifiers(raw.unitTypes, `${label}.unitTypes`);
    if ((raw.unitTypes as string[]).some(unitType => !unitTypes.has(unitType))) {
      throw new Error(`${label}.unitTypes must be declared result-unit types`);
    }
    const aggregation = raw.aggregation;
    if (typeof aggregation === "string") {
      if (!simpleAggregations.has(aggregation)) throw new Error(`${label}.aggregation is unsupported`);
    } else if (isRecord(aggregation)) {
      assertOnlyKeys(aggregation, ["type", "numeratorMetricId", "denominatorMetricId"], `${label}.aggregation`);
      if (aggregation.type !== "ratio") throw new Error(`${label}.aggregation ratio definition is invalid`);
      validateIdentifier(aggregation.numeratorMetricId, `${label}.aggregation.numeratorMetricId`);
      validateIdentifier(aggregation.denominatorMetricId, `${label}.aggregation.denominatorMetricId`);
    } else {
      throw new Error(`${label}.aggregation definition is invalid`);
    }
    metrics.set(raw.id, raw);
  }

  for (const [id, metric] of metrics) {
    if (!isRatio(metric.aggregation as GameMetricAggregation)) continue;
    const aggregation = metric.aggregation as Extract<GameMetricAggregation, { type: "ratio" }>;
    const numerator = metrics.get(aggregation.numeratorMetricId);
    const denominator = metrics.get(aggregation.denominatorMetricId);
    if (!numerator || !denominator || numerator.scope !== metric.scope || denominator.scope !== metric.scope
      || numerator.aggregation !== "sum" || denominator.aggregation !== "sum"
      || aggregation.numeratorMetricId === id || aggregation.denominatorMetricId === id) {
      throw new Error(`Ratio metric ${id} must reference sum metrics with the same scope`);
    }
    if ((metric.unitTypes as string[]).some(unitType =>
      !(numerator.unitTypes as string[]).includes(unitType)
      || !(denominator.unitTypes as string[]).includes(unitType))) {
      throw new Error(`Ratio metric ${id} unit types must be supported by both numerator and denominator`);
    }
  }
}

function validateOutcomeAndRank(value: Record<string, unknown>, label: string): void {
  if (value.outcome !== undefined && !outcomes.has(value.outcome as string)) {
    throw new Error(`${label}.outcome is invalid`);
  }
  if (value.rank !== undefined
    && (typeof value.rank !== "number" || !Number.isSafeInteger(value.rank) || value.rank < 1)) {
    throw new Error(`${label}.rank must be a positive safe integer`);
  }
}

function validateMetrics(
  value: unknown,
  label: string,
  scope: "participant" | "team",
  unitType: string,
  definition?: GameResultDefinition,
): asserts value is Record<string, number> {
  if (!isRecord(value)) throw new Error(`${label} must be an object`);
  const entries = Object.entries(value);
  if (entries.length > maxMetricsPerEntity) {
    throw new Error(`${label} exceeds the ${maxMetricsPerEntity}-metric limit`);
  }
  const declared = definition?.metrics ?? [];
  for (const [id, metricValue] of entries) {
    validateIdentifier(id, `${label} metric ID`);
    if (typeof metricValue !== "number" || !Number.isFinite(metricValue)) {
      throw new Error(`${label}.${id} must be a finite number`);
    }
    if (definition) {
      const metric = declared.find(item => item.id === id);
      if (!metric || metric.scope !== scope || !metric.unitTypes.includes(unitType)
        || isRatio(metric.aggregation)) {
        throw new Error(`${label}.${id} is not a declared direct ${scope} metric for ${unitType}`);
      }
    }
  }
}

function validateEntity<T>(
  value: unknown,
  label: string,
  scope: "participant" | "team",
  unitType: string,
  definition: GameResultDefinition | undefined,
): T {
  if (!isRecord(value)) throw new Error(`${label} must be an object`);
  if (scope === "participant") {
    assertOnlyKeys(value, ["playerId", "teamId", "outcome", "rank", "metrics"], label);
    validateIdentifier(value.playerId, `${label}.playerId`);
    if (value.teamId !== undefined) validateIdentifier(value.teamId, `${label}.teamId`);
  } else {
    assertOnlyKeys(value, ["teamId", "outcome", "rank", "metrics"], label);
    validateIdentifier(value.teamId, `${label}.teamId`);
  }
  validateOutcomeAndRank(value, label);
  validateMetrics(value.metrics, `${label}.metrics`, scope, unitType, definition);
  return value as T;
}

/** Validate a result event and, optionally, its declared unit and metric IDs. */
export function validateGameResultEvent(
  value: unknown,
  definition?: GameResultDefinition,
): asserts value is GameResultEvent {
  if (!isRecord(value)) throw new Error("GameResultEvent must be an object");
  assertOnlyKeys(value, [
    "schemaVersion", "eventId", "matchId", "unitId", "unitType", "rulesVersion",
    "definitionVersion", "occurredAt", "participants", "teams", "correction",
  ], "GameResultEvent");
  if (value.schemaVersion !== 1) throw new Error("GameResultEvent.schemaVersion must be 1");
  for (const field of ["eventId", "matchId", "unitId", "unitType", "rulesVersion", "definitionVersion"]) {
    validateIdentifier(value[field], `GameResultEvent.${field}`);
  }
  if (value.occurredAt !== undefined
    && (typeof value.occurredAt !== "number" || !Number.isSafeInteger(value.occurredAt)
      || value.occurredAt < 0 || value.occurredAt > maxSafeInteger)) {
    throw new Error("GameResultEvent.occurredAt must be a non-negative safe integer timestamp in milliseconds");
  }
  let correctionAction: "replace" | "retract" | undefined;
  if (value.correction !== undefined) {
    if (!isRecord(value.correction)) throw new Error("GameResultEvent.correction must be an object");
    assertOnlyKeys(value.correction, ["action", "supersedesEventId"], "GameResultEvent.correction");
    if (value.correction.action !== "replace" && value.correction.action !== "retract") {
      throw new Error("GameResultEvent.correction.action must be replace or retract");
    }
    validateIdentifier(value.correction.supersedesEventId, "GameResultEvent.correction.supersedesEventId");
    if (value.correction.supersedesEventId === value.eventId) {
      throw new Error("A correction cannot supersede itself");
    }
    correctionAction = value.correction.action;
  }
  if (!Array.isArray(value.participants) || (value.teams !== undefined && !Array.isArray(value.teams))) {
    throw new Error("GameResultEvent.participants and teams must be arrays");
  }
  const teams = (value.teams ?? []) as unknown[];
  if (value.participants.length > maxParticipants) {
    throw new Error(`GameResultEvent exceeds the ${maxParticipants}-participant limit`);
  }
  if (teams.length > maxTeams) throw new Error(`GameResultEvent exceeds the ${maxTeams}-team limit`);
  if (value.participants.length === 0 && teams.length === 0 && correctionAction !== "retract") {
    throw new Error("GameResultEvent must include a participant or team");
  }
  if (correctionAction === "retract" && (value.participants.length > 0 || teams.length > 0)) {
    throw new Error("A retraction must not include participants or teams");
  }

  if (definition) {
    validateGameResultDefinition(definition);
    if (value.definitionVersion !== definition.version) {
      throw new Error("GameResultEvent.definitionVersion does not match the published definition");
    }
    if (!(definition.unitTypes as string[]).includes(value.unitType as string)) {
      throw new Error("GameResultEvent.unitType is not declared");
    }
  }

  const participants = value.participants.map((participant, index) =>
    validateEntity<GameResultParticipant>(participant, `GameResultEvent.participants[${index}]`, "participant",
      value.unitType as string, definition));
  const participantIds = new Set<string>();
  for (const participant of participants) {
    if (participantIds.has(participant.playerId)) {
      throw new Error(`Player appears more than once in a result unit: ${participant.playerId}`);
    }
    participantIds.add(participant.playerId);
  }
  const typedTeams = teams.map((team, index) =>
    validateEntity<GameResultTeam>(team, `GameResultEvent.teams[${index}]`, "team",
      value.unitType as string, definition));
  const teamIds = new Set<string>();
  for (const team of typedTeams) {
    if (teamIds.has(team.teamId)) throw new Error(`Team appears more than once in a result unit: ${team.teamId}`);
    teamIds.add(team.teamId);
  }
  if (teamIds.size > 0) {
    for (const participant of participants) {
      if (participant.teamId !== undefined && !teamIds.has(participant.teamId)) {
        throw new Error(`Participant references undeclared teamId: ${participant.teamId}`);
      }
    }
  }

  let serialized: string | undefined;
  try {
    serialized = JSON.stringify(value);
  } catch {
    throw new Error("GameResultEvent must be serializable as JSON");
  }
  if (serialized === undefined || new TextEncoder().encode(serialized).byteLength > maxEventBytes) {
    throw new Error(`GameResultEvent exceeds the ${maxEventBytes}-byte limit`);
  }
}

/** Validate optional result events emitted in one lifecycle transition. */
export function validateGameResultUnits(
  value: unknown,
  definition?: GameResultDefinition,
): asserts value is GameResultEvent[] | undefined {
  if (value === undefined) return;
  if (!Array.isArray(value)) throw new Error("resultUnits must be an array");
  if (value.length > maxEventsPerTransition) {
    throw new Error(`resultUnits exceeds the ${maxEventsPerTransition}-event per-transition limit`);
  }
  const eventIds = new Set<string>();
  const unitIds = new Set<string>();
  for (const event of value) {
    validateGameResultEvent(event, definition);
    if (eventIds.has(event.eventId)) throw new Error(`Duplicate result eventId in one transition: ${event.eventId}`);
    eventIds.add(event.eventId);
    const unitKey = JSON.stringify([event.matchId, event.unitId]);
    if (unitIds.has(unitKey)) throw new Error(`Duplicate result unit in one transition: ${event.matchId}/${event.unitId}`);
    unitIds.add(unitKey);
  }
}
