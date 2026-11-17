# Game-owned platform data protocol v1

## Status and scope

This versioned Server SDK contract defines explicit game-authored JSON documents
for a host that implements it. The current Gamecreator implementation remains
disabled by default (`GAME_DATA_STORE_ENABLED=false`); only use it after the
deployment owner confirms the gate is enabled. `DataPlugin` is a separate,
disabled capability. SDK source/API presence is not a rollout or production
availability guarantee.

The host derives canonical `gameId` and the complete deployment plus execution
environment exclusively from a verified durable Room admission and startup
fence. The game request cannot select scope: do not send `scope`, `gameId`,
`environment`, `roomId`, or `versionId`. Missing, invalid, stale, unsigned, or
standalone admission fails closed before game initialization/data access. The
host routes the call through the GameRoom `platform.data` virtual host and an
authenticated signed internal Archive request. Different games and full
environments are isolated; all rooms and runtime releases of the same game and
environment share the namespace. Runtime version and room ID do not partition
records.

This store is forward-only. Games must explicitly write new records using the
Server SDK. It does not import historical Room SQLite, snapshots, KV, old
Result Units or Blackjack records, legacy client-reported boards, profiles,
principals, or R2 replay data. It is not a global lobby, real-time cross-room
synchronization channel, general database, Result Unit query API, or public
leaderboard. `platform.data` and a Room transition are separate Durable Object
operations, not one transaction. A callback write is not a durable atomic
outbox; use deterministic idempotency keys and retry the identical payload
when safe. Exact committed-transition mirroring is unavailable in this stage.

## Wire envelope

The Room SDK sends JSON `POST` requests to `http://platform.data/<operation>`:

```json
{"protocolVersion":1,"request":{}}
```

Version 1 accepts exactly `protocolVersion` and `request` at the envelope level;
`request` must be an object. The service rejects scope fields instead of
ignoring them. The encoded write request is limited to 512 KiB. Supported SDK
operations and request shapes are:

- `POST /schema`: `{ "collection": string, "schema": GameDataSchema }`
- `POST /get`: `{ "collection": string, "id": string }`
- `POST /mutate`: `{ "idempotencyKey": string, "mutations": Mutation[] }`
- `POST /query`: `GameDataQuery`
- `POST /aggregate`: `GameDataAggregate`

A successful response is an HTTP 2xx API envelope with `success: true` and a
`data` member. Errors use a non-2xx status and a sanitized stable `code`; clients
must branch on that code, not message text. Common codes include
`GAME_DATA_DISABLED`, `GAME_DATA_SCOPE_UNAVAILABLE`, `GAME_DATA_UNAVAILABLE`,
`GAME_DATA_RETIRED`, `INVALID_INPUT`, `INVALID_SCOPE`, `DATA_SCHEMA_REQUIRED`,
`DATA_SCHEMA_CONFLICT`, `DATA_REVISION_CONFLICT`, `DATA_QUERY_UNSUPPORTED`,
`DATA_CURSOR_INVALID`, `DATA_PAYLOAD_TOO_LARGE`, `DATA_CAPACITY_REACHED`,
`DATA_AGGREGATE_LIMIT`, `DATA_AGGREGATE_OVERFLOW`, and `DATA_RATE_LIMITED`.
TypeScript throws `PlatformDataError` (`status`, `code`, `retryable`); Rust
returns `SdkError::PlatformData` and provides `retryable()` and
`platform_data_code()`.

## Schemas and documents

`GameDataSchema` contains a positive `version`, `fields` mapping top-level field
names to `string`, `number`, or `boolean`, optional single-field `indexes`, and
optional ordered `compoundIndexes` pairs `[equalityField, sortField]`. Stored
values are JSON objects whose fields are declared by the collection schema;
there is no arbitrary SQL or platform-table access. Schema field types are
scalar; undeclared fields and mismatched types are rejected. Indexed strings
are bounded to 256 UTF-8 bytes.

Register a schema before collection access. Evolution is additive: increase the
version, retain previous field names and types, and add compatible fields or
indexes. Unchanged older declarations may re-register after a newer version;
the host does not downgrade shared metadata. Fields added later are sparse so
old writers can omit them, and queries omit documents without an indexed value.
Removing/changing fields or types conflicts. Do not newly index a previously
unindexed string: old releases may have stored strings longer than the index
bound, so adding that index would break their previously valid writes. First
schema index fields are required for writes; see the host author guide for
compatibility details.

## Records and writes

A record is `{id, value, revision, deleted}`. `get` returns a live record, a
deleted record with `value: null`, or `null` when no record exists. New records
start at revision 0. Put and delete use compare-and-swap `expectedRevision`.
Deletion retains a monotonically increasing tombstone revision; recreating a
record must use that tombstone revision, not 0.

A mutation is one of:

```json
{"op":"put","collection":"scores","id":"player-1","expectedRevision":0,"value":{"score":10}}
{"op":"delete","collection":"scores","id":"player-1","expectedRevision":1}
```

`mutate` accepts 1–25 unique record mutations in one game/environment scope;
the batch commits atomically or rolls back. Every write includes a bounded
`idempotencyKey`. Repeating the same key with the identical payload returns the
original receipt. Reusing the key with changed content returns
`DATA_REVISION_CONFLICT`. Retry the exact same request after a transient error;
rebuild a fresh request after a revision conflict.

## Queries and aggregation

`GameDataQuery` has `collection`, at most one optional `where` filter, optional
`orderBy`, `limit` 1–100, and an opaque optional `cursor`. Comparison operators
are `eq`, `lt`, `lte`, `gt`, `gte`. Values must be non-null and match the
indexed scalar type. Supported shapes are:

- One filter and ordering on the same declared single-field index.
- Equality on the first field of a declared ordered compound pair, followed by
  ascending or descending ordering on the second field. For example,
  `season == X ORDER BY score ASC|DESC` requires `compoundIndexes:
  [["season", "score"]]`.
- Unfiltered live-document listing by ID.

The document ID is a deterministic tie-breaker in the requested direction.
Pagination uses an opaque keyset cursor bound to game, environment, query,
order, and page size; cursors expire after 15 minutes. Do not parse or construct
cursors. Null filters, multiple predicates, undeclared cross-field plans, joins,
offsets, arbitrary sorts/expressions/SQL, and unbounded scans are unsupported
and return an explicit error. Do not assume a filter on `season` plus sort on
`score` is supported unless that exact ordered compound pair was registered.

`GameDataAggregate` supports `count` (no field) and `sum`, `min`, `max`, `avg`
over a declared numeric field. It accepts at most one filter on a declared
single-field index and requires an explicit `scanLimit` from 1–1,000. Exceeding
the bound returns `DATA_AGGREGATE_LIMIT`. Sum and average exactly accumulate
stored binary64 values as integer multiples of `2^-1074`, then round the final
value to nearest-even; average divides the exact total by the match count before
final rounding. Results outside the supported numeric range return
`DATA_AGGREGATE_OVERFLOW`. JSON integers outside the JavaScript safe-integer
range are rejected. Node rejects non-finite numbers and negative zero before
transmission; Rust preserves negative-zero sign in documents, while indexes
treat `-0` and `+0` as equal.

## Resource quotas

All scope quotas apply to `(gameId, full environment)` unless specified as
Archive-wide. These are enforced limits, not capacity or latency guarantees.

| Resource | Limit |
| --- | ---: |
| Serialized document value | 64 KiB |
| Serialized write request / batch | 512 KiB |
| Mutations per atomic batch | 25 |
| Query page size | 100 |
| Aggregate scan limit | 1,000 records |
| Collections per scope | 32 |
| Declared fields per collection | 32 |
| Single-field indexes per collection | 16 |
| Compound index pairs per collection | 16 |
| Game-declared query indexes per GameArchive (all environments) | 128 |
| Normalized query-index entries per GameArchive (all environments) | 16,384 |
| Live documents per scope | 8,192 |
| Stored records per scope, including tombstones | 16,384 |
| Live serialized bytes per scope | 128 MiB |
| Idempotency receipts per scope | 4,096; 30-day retention |
| Active pagination cursors per scope | 4,096; 15-minute retention |
| Requests per room | 120 per minute |
| Tracked rooms per scope for rate limiting | 10,000 |

Each live indexed value or complete compound pair contributes a normalized
entry. Fixed document-access indexes are not multiplied by user declarations.
Limits are checked before writes/index registration; capacity errors are
explicit. Expired receipts/cursors are subject to bounded cleanup.

## Lifecycle and privacy

Game deletion/archive durably queues retirement independently of ordinary
artifact cleanup. The fair bounded reconciler retries Archive retirement;
Archive tombstones prevent late room writes from resurrecting data. Lifecycle
unavailability fails data operations closed but does not block ordinary game
deletion cleanup. Existing match/result archives and R2 replay retention are
not altered by this protocol.

Do not store secrets, private player snapshots, profiles, principals, identity
linkage, or client-reported ranking claims. No automatic projection from Room
transition outboxes, stable player identity migration, public authoritative
board, or historical recovery is provided.
