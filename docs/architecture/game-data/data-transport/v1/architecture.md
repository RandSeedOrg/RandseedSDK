# Public Server SDK game-data transport errors

Iteration: v1, proposal. The existing [game-data protocol](../../../../../protocol/game-data-v1.md)
remains wire version 1. See the [implementation tracker](./todos.md).
This is the public RandSeedSDK counterpart of the approved Gamecreator GDT-v1
design at commit `55960a5661112bd473ec13e617a1365c3a791ed0`; it does not
version that separate repository's unversioned game-data architecture.

## Design card

- **RC-1:** At `f5ceb808`, `PlatformDataClient` passes a fetch `TypeError`
  through while the TypeScript guidance broadly claims `PlatformDataError`.
  The published Server SDK is sourced from this repository; the Gamecreator
  SDK copy is a distinct implementation. Neither proves BLJ data loss.
- **INV-1:** A failed read is not an absent record. A failed or unreadable
  mutation response leaves its commit outcome unknown.
- **INV-2:** Preserve validation and serialization, HTTP and response-body
  mapping, CAS, receipt idempotency, and other thrown values. Never retry
  automatically or alter the wire contract.
- **INV-3:** Generic game-data writes and Room transitions are separate Durable
  Object commits. No cross-object atomicity or automatic reconciliation exists.
- **Decision:** Export `PlatformDataTransportError extends TypeError` with code
  `GAME_DATA_TRANSPORT_ERROR` and the same fixed sanitized message as the
  Gamecreator SDK. It identifies only a same-realm `TypeError` thrown/rejected
  at the fetch boundary. It has no cause, status or generic retryability claim;
  it does not establish cause, delivery, or server execution. Exact original
  error identity and message are intentionally not preserved for a match.
- **RC-2:** Guard the `instanceof` classification itself: if it throws, rethrow
  the original rejection. This preserves even unclassifiable values such as a
  revoked proxy by identity. Validation and serialization occur before the
  fetch-only catch; response parsing remains outside it.
- **Recovery:** On revision conflict, reread and reconcile the original
  business intent or stop; do not overwrite with stale values and a fresh
  revision. Tombstones have revisions. An intentional retry of an indeterminate
  mutation must use the same game and full environment, idempotency key and
  complete identical request including `expectedRevision` within the existing
  30-day receipt retention window. After that, the outcome may remain unknown.

## Acceptance and architecture self-check

- **AC-1 / INV-1–2:** Focused public SDK tests import the root export and cover
  synchronous and rejected read/write fetch `TypeError`, sanitized shape, one
  fetch, abort/nonmatching/revoked-proxy identity, and local validation and
  cyclic serialization failures outside the wrapper.
- **AC-2 / INV-2:** Existing success and HTTP behavior, 409 mapping, and failed
  response-body mapping remain unchanged. The SDK error is not a wire error;
  Rust and shared wire fixtures are not changed.
- **AC-3 / INV-1–3:** Public README and protocol guidance describe unknown write
  outcomes, scoped identical retry, 409 reconciliation, tombstone revisions,
  and separate commits without claiming complete receipt validation.
- **AC-4:** Run the public SDK build and focused tests, existing Node SDK tests,
  and package dry-run as applicable. Independently record Gamecreator Worker
  CAS/idempotency evidence and which integration/runtime paths remain
  UNVERIFIED; do not infer deployed behavior or lost-response recovery.

- [x] Primary aggregate is game-owned records/receipts; SDK uses the existing
  fetch port and does not change domain ownership.
- N/A: No SDK-owned state machine or persistence transition is added; existing
  server CAS and receipt semantics are unchanged.
- N/A: No value-bearing arithmetic is added or changed.
- [x] Planned source files are below 1,000 lines; changed functions must be at
  most 60 lines with at most three control-flow levels. No new dependency.
- [x] TypeScript public export, error boundary, documentation and tests are
  defined; wire and Rust behavior remain unchanged. Real deployment and a
  committed-write/lost-response path are explicitly UNVERIFIED.
