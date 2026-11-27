export const GAME_DATA_PROTOCOL_VERSION = 1 as const;
export type GameDataJsonValue = null | boolean | number | string | GameDataJsonValue[] | { [key: string]: GameDataJsonValue };
export type GameDataFieldType = 'string' | 'number' | 'boolean';
export interface GameDataSchema { version: number; fields: Record<string, GameDataFieldType>; indexes?: string[]; compoundIndexes?: [string, string][]; }
export interface GameDataRecord<T = GameDataJsonValue> { id: string; value: T | null; revision: number; deleted: boolean; }
export interface GameDataPutMutation<T = GameDataJsonValue> { op: 'put'; collection: string; id: string; expectedRevision: number; value: T; }
export interface GameDataDeleteMutation { op: 'delete'; collection: string; id: string; expectedRevision: number; }
export type GameDataMutation<T = GameDataJsonValue> = GameDataPutMutation<T> | GameDataDeleteMutation;
export interface GameDataWriteRequest<T = GameDataJsonValue> { idempotencyKey: string; mutations: GameDataMutation<T>[]; }
export interface GameDataWriteReceiptItem { collection: string; id: string; revision: number; deleted: boolean; }
export interface GameDataWriteReceipt { idempotencyKey: string; items: GameDataWriteReceiptItem[]; }
export type GameDataFilterOperator = 'eq' | 'lt' | 'lte' | 'gt' | 'gte';
export interface GameDataFilter { field: string; op: GameDataFilterOperator; value: string | number | boolean | null; }
export interface GameDataSort { field: string; direction: 'asc' | 'desc'; }
export interface GameDataQuery { collection: string; where?: GameDataFilter[]; orderBy?: GameDataSort; limit: number; cursor?: string; }
export interface GameDataQueryResult<T = GameDataJsonValue> { items: GameDataRecord<T>[]; nextCursor: string | null; }
export interface GameDataAggregateMetric { op: 'count' | 'sum' | 'min' | 'max' | 'avg'; field?: string; }
export interface GameDataAggregate { collection: string; where?: GameDataFilter[]; metric: GameDataAggregateMetric; scanLimit: number; }
export interface GameDataAggregateResult { value: number | null; scanned: number; }

export class PlatformDataError extends Error {
  readonly status: number;
  readonly code: string;
  readonly retryable: boolean;
  constructor(status: number, code: string) {
    super('Game data request failed (' + status + ', ' + code + '). Check the request, schema, and documented limits.');
    this.name = 'PlatformDataError'; this.status = status; this.code = code;
    this.retryable = status === 429 || (status >= 500 && status < 507 && code !== 'GAME_DATA_DISABLED');
  }
}
export class PlatformDataTransportError extends TypeError {
  readonly code = 'GAME_DATA_TRANSPORT_ERROR';
  constructor() {
    super('Game data fetch failed with a TypeError; request outcome is unknown.');
    this.name = 'PlatformDataTransportError';
  }
}
export interface PlatformDataClientOptions { fetch?: typeof fetch; vhostMap?: Record<string, string>; }

function assertJsonNumbers(value: unknown, seen = new WeakSet<object>()): void {
  if (typeof value === 'number') {
    if (Object.is(value, -0)) {
      throw new TypeError('Game data does not support negative zero because JSON serialization changes it to zero.');
    }
    if (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value))) {
      throw new TypeError('Game data numbers must be finite and integer values must be safe JavaScript integers.');
    }
    return;
  }
  if (typeof value !== 'object' || value === null || seen.has(value)) return;
  seen.add(value);
  for (const child of Array.isArray(value) ? value : Object.values(value)) assertJsonNumbers(child, seen);
}

export class PlatformDataClient {
  private readonly customFetch: typeof fetch;
  private readonly vhostMap: Record<string, string>;
  constructor(options: PlatformDataClientOptions = {}) {
    this.customFetch = options.fetch || globalThis.fetch;
    this.vhostMap = options.vhostMap || {};
  }
  private async call<T>(operation: string, request: object): Promise<T> {
    assertJsonNumbers(request);
    const base = (this.vhostMap["platform.data"] || "http://platform.data").replace(/\/+$/, "");
    const url = base + '/' + operation;
    const init = { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ protocolVersion: GAME_DATA_PROTOCOL_VERSION, request }) };
    let response: Response;
    try {
      response = await this.customFetch(url, init);
    } catch (error) {
      let isTypeError: boolean;
      try { isTypeError = error instanceof TypeError; } catch { throw error; }
      if (isTypeError) throw new PlatformDataTransportError();
      throw error;
    }
    let payload: unknown;
    try { payload = await response.json(); } catch { payload = null; }
    if (!response.ok) {
      const object = typeof payload === 'object' && payload !== null ? payload as Record<string, unknown> : {};
      const candidate = object.code;
      const code = typeof candidate === 'string' && /^[A-Z0-9_]{1,64}$/.test(candidate) ? candidate : 'GAME_DATA_HTTP_ERROR';
      throw new PlatformDataError(response.status, code);
    }
    if (typeof payload !== 'object' || payload === null || (payload as Record<string, unknown>).success !== true || !('data' in payload)) throw new PlatformDataError(response.status, 'GAME_DATA_INVALID_RESPONSE');
    return (payload as { data: T }).data;
  }
  async registerSchema(collection: string, schema: GameDataSchema): Promise<void> {
    this.assertName(collection, 'collection'); this.assertCompoundIndexes(schema); await this.call('schema', { collection, schema });
  }
  async get<T = GameDataJsonValue>(collection: string, id: string): Promise<GameDataRecord<T> | null> {
    this.assertName(collection, 'collection'); this.assertName(id, 'id'); return this.call('get', { collection, id });
  }
  async put<T = GameDataJsonValue>(input: { idempotencyKey: string; collection: string; id: string; expectedRevision: number; value: T }): Promise<GameDataWriteReceipt> {
    return this.mutate({ idempotencyKey: input.idempotencyKey, mutations: [{ op: 'put', collection: input.collection, id: input.id, expectedRevision: input.expectedRevision, value: input.value }] });
  }
  async delete(input: { idempotencyKey: string; collection: string; id: string; expectedRevision: number }): Promise<GameDataWriteReceipt> {
    return this.mutate({ idempotencyKey: input.idempotencyKey, mutations: [{ op: 'delete', collection: input.collection, id: input.id, expectedRevision: input.expectedRevision }] });
  }
  async mutate<T = GameDataJsonValue>(request: GameDataWriteRequest<T>): Promise<GameDataWriteReceipt> {
    if (typeof request.idempotencyKey !== 'string' || request.idempotencyKey.length < 1 || request.idempotencyKey.length > 128 || !/^[A-Za-z0-9._:-]+$/.test(request.idempotencyKey)) throw new TypeError('A valid idempotencyKey is required.');
    if (!Array.isArray(request.mutations) || request.mutations.length < 1 || request.mutations.length > 25) throw new TypeError('A write must contain between 1 and 25 mutations.');
    for (const mutation of request.mutations) {
      this.assertName(mutation.collection, 'collection'); this.assertName(mutation.id, 'id');
      if (!Number.isSafeInteger(mutation.expectedRevision) || mutation.expectedRevision < 0) throw new TypeError('expectedRevision must be a non-negative safe integer.');
    }
    return this.call('mutate', request);
  }
  async query<T = GameDataJsonValue>(request: GameDataQuery): Promise<GameDataQueryResult<T>> {
    this.assertName(request.collection, 'collection');
    if (!Number.isInteger(request.limit) || request.limit < 1 || request.limit > 100) throw new TypeError('limit must be an integer from 1 to 100.');
    return this.call('query', request);
  }
  async aggregate(request: GameDataAggregate): Promise<GameDataAggregateResult> {
    this.assertName(request.collection, 'collection');
    if (!Number.isInteger(request.scanLimit) || request.scanLimit < 1 || request.scanLimit > 1000) throw new TypeError('scanLimit must be an integer from 1 to 1000.');
    return this.call('aggregate', request);
  }
  private assertCompoundIndexes(schema: GameDataSchema): void {
    const pairs = schema?.compoundIndexes ?? [];
    if (!Array.isArray(pairs) || pairs.length > 16) throw new TypeError('compoundIndexes must contain at most 16 declared field pairs.');
    const fields = schema?.fields;
    const seen = new Set<string>();
    for (const pair of pairs) {
      if (!Array.isArray(pair) || pair.length !== 2 || pair[0] === pair[1]
        || pair.some(field => typeof field !== 'string' || !/^[A-Za-z][A-Za-z0-9_]{0,63}$/.test(field) || !Object.hasOwn(fields ?? {}, field))) {
        throw new TypeError('Each compound index must declare two different schema fields.');
      }
      const key = pair[0] + String.fromCharCode(0) + pair[1];
      if (seen.has(key)) throw new TypeError('compoundIndexes must not contain duplicate pairs.');
      seen.add(key);
    }
  }

  private assertName(value: string, name: string): void {
    if (typeof value !== 'string' || value.length < 1 || value.trim() !== value) throw new TypeError('A non-empty ' + name + ' is required.');
  }
}
