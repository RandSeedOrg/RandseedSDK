import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PlatformClient, PlatformDataError } from '../dist/index.js';

test('platform.data v1 covers schema, CAS receipts, indexed queries, and bounded aggregates', async () => {
  const calls = [];
  const client = new PlatformClient({ vhostMap: { 'platform.data': 'http://data.test/' }, fetch: async (url, init) => {
    const request = JSON.parse(init.body);
    calls.push({ url: String(url), request });
    const operation = new URL(String(url)).pathname.slice(1);
    const data = operation === 'schema' ? { registered: true }
      : operation === 'get' ? { id: 'alice', value: { score: 7 }, revision: 3, deleted: false }
        : operation === 'query' ? { items: [], nextCursor: null }
          : operation === 'aggregate' ? { value: 7, scanned: 1 }
            : { idempotencyKey: request.request.idempotencyKey, items: [{ collection: 'scores', id: 'alice', revision: 4, deleted: operation === 'delete' }] };
    return { ok: true, status: 200, json: async () => ({ success: true, data }) };
  } });
  await client.data.registerSchema('scores', { version: 1, fields: { score: 'number', season: 'number' }, indexes: ['score'], compoundIndexes: [['season', 'score']] });
  assert.equal((await client.data.get('scores', 'alice')).revision, 3);
  assert.equal((await client.data.put({ idempotencyKey: 'put-one', collection: 'scores', id: 'alice', expectedRevision: 3, value: { score: 8 } })).items[0].revision, 4);
  await client.data.delete({ idempotencyKey: 'delete-one', collection: 'scores', id: 'alice', expectedRevision: 4 });
  await client.data.mutate({ idempotencyKey: 'batch-one', mutations: [{ op: 'put', collection: 'scores', id: 'bob', expectedRevision: 0, value: { score: 2 } }] });
  await client.data.query({ collection: 'scores', where: [{ field: 'season', op: 'eq', value: 4 }], orderBy: { field: 'score', direction: 'desc' }, limit: 10 });
  assert.deepEqual(await client.data.aggregate({ collection: 'scores', metric: { op: 'sum', field: 'score' }, scanLimit: 100 }), { value: 7, scanned: 1 });
  assert.equal(calls.length, 7);
  assert.equal(calls[0].request.request.schema.compoundIndexes[0][0], 'season');
  assert.equal(calls[0].request.request.schema.compoundIndexes[0][1], 'score');
  assert.equal(calls[5].request.request.where[0].field, 'season');
  assert.equal(calls[5].request.request.orderBy.field, 'score');
  for (const call of calls) assert.equal(call.request.protocolVersion, 1);
  assert.deepEqual(calls.map(call => new URL(call.url).pathname), ['/schema', '/get', '/mutate', '/mutate', '/mutate', '/query', '/aggregate']);
});

test('platform.data enforces bounds and turns service failures into sanitized errors', async () => {
  assert.equal(new PlatformDataError(503, "GAME_DATA_DISABLED").retryable, false);
  assert.equal(new PlatformDataError(507, "DATA_CAPACITY_REACHED").retryable, false);
  const client = new PlatformClient({ fetch: async () => ({ ok: false, status: 429, json: async () => ({ code: 'DATA_RATE_LIMITED', message: 'private backend detail' }) }) });
  await assert.rejects(client.data.query({ collection: 'scores', limit: 101 }), /limit must be an integer/);
  await assert.rejects(client.data.get('scores', 'alice'), error => {
    assert.ok(error instanceof PlatformDataError);
    assert.equal(error.status, 429);
    assert.equal(error.code, 'DATA_RATE_LIMITED');
    assert.equal(error.retryable, true);
    assert.doesNotMatch(error.message, /private backend detail/);
    return true;
  });
});


test("platform.data rejects missing success data but accepts explicit null data", async () => {
  const envelope = { success: true };
  const client = new PlatformClient({ fetch: async () => ({ ok: true, status: 200, json: async () => envelope }) });
  await assert.rejects(client.data.get("scores", "alice"), error => {
    assert.ok(error instanceof PlatformDataError);
    assert.equal(error.status, 200);
    assert.equal(error.code, "GAME_DATA_INVALID_RESPONSE");
    return true;
  });
  envelope.data = null;
  assert.equal(await client.data.get("scores", "alice"), null);
});

test('platform.data rejects non-finite and unsafe integer numbers before fetch and preserves stored values', async () => {
  const records = new Map();
  const requests = [];
  const client = new PlatformClient({ fetch: async (url, init) => {
    requests.push({ url: String(url), body: JSON.parse(init.body) });
    const operation = new URL(String(url)).pathname.slice(1);
    const request = requests.at(-1).body.request;
    if (operation === 'mutate') {
      for (const mutation of request.mutations) {
        if (mutation.op === 'put') records.set(mutation.id, mutation.value);
        else records.delete(mutation.id);
      }
      return { ok: true, status: 200, json: async () => ({ success: true, data: { idempotencyKey: request.idempotencyKey, items: [] } }) };
    }
    return { ok: true, status: 200, json: async () => ({ success: true, data: { id: 'alice', value: records.get('alice') ?? null, revision: 1, deleted: false } }) };
  } });
  await client.data.put({ idempotencyKey: 'safe-write', collection: 'scores', id: 'alice', expectedRevision: 0, value: { score: 7 } });
  assert.equal(requests.length, 1);
  await assert.rejects(client.data.put({ idempotencyKey: 'nan-write', collection: 'scores', id: 'alice', expectedRevision: 1, value: { score: Number.NaN } }), /finite/);
  await assert.rejects(client.data.put({ idempotencyKey: 'infinity-write', collection: 'scores', id: 'alice', expectedRevision: 1, value: { score: Number.POSITIVE_INFINITY } }), /finite/);
  await assert.rejects(client.data.put({ idempotencyKey: 'unsafe-integer', collection: 'scores', id: 'alice', expectedRevision: 1, value: { score: Number.MAX_SAFE_INTEGER + 1 } }), /safe JavaScript integers/);
  await assert.rejects(client.data.put({ idempotencyKey: 'negative-zero', collection: 'scores', id: 'alice', expectedRevision: 1, value: { score: -0 } }), /negative zero/);
  await assert.rejects(client.data.query({ collection: 'scores', where: [{ field: 'score', op: 'eq', value: Number.NEGATIVE_INFINITY }], limit: 1 }), /finite/);
  assert.equal(requests.length, 1, 'invalid values must not cross the network boundary');
  assert.deepEqual((await client.data.get('scores', 'alice')).value, { score: 7 }, 'failed writes must preserve prior data');
  assert.equal(requests.length, 2);
});
