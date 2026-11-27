# Public SDK game-data transport v1 tracker

Task: GDT-SDK-v1. [Design](./architecture.md). User approved SDK parity and a
separate PR to `main` on 2026-11-28. Base: `origin/main`
`f5ceb8084bb8763b2795d5cf1fcc9cf77514f003`; no package publishing, merge,
deployment, remote migration, Room changes, or BLJ changes are authorized.
Related Gamecreator PR: https://github.com/RandSeedOrg/Gamecreator/pull/395.
Coordinator owns this tracker; coders do not edit it concurrently.

Domain and persistence layers are not applicable: this task changes only the
TypeScript SDK fetch-boundary error contract and directly related guidance.
Wire protocol, Rust implementation, CAS and receipt semantics remain unchanged.

## GDT-SDK-01: Align public SDK fetch error handling

- **Status:** ready. **Dependencies:** none. **Owner:** Luna.
- **Files and budget:** `node/src/game-data.ts` (120 lines, estimated 145;
  TypeScript public contract/application); `node/test/game-data.test.mjs`
  (89 lines, estimated 190; contract tests). At most 300 changed lines total.
- **Flow and goal:** `PlatformDataClient` validates, serializes the existing
  request, invokes its selected fetch once, then parses/maps the response.
  Move serialization outside the fetch-only `try`. Export
  `PlatformDataTransportError extends TypeError` from `game-data.ts` using the
  existing root export, code `GAME_DATA_TRANSPORT_ERROR` and fixed message
  `Game data fetch failed with a TypeError; request outcome is unknown.`.
  No original cause, HTTP status or `retryable` property is exposed. Classify
  only same-realm `TypeError`; guard the classification itself. If `instanceof`
  throws, rethrow the original rejection by identity. Nonmatches, including
  fetch `AbortError`, are also rethrown unchanged.
- **Preserve:** local validation and serialization errors are not wrapped; no
  automatic retry; success, HTTP `PlatformDataError` including 409, and
  existing response-body failure mapping are unchanged. No wire, Rust, schema,
  Room, persistence, dependency or unrelated SDK change. The two repository
  implementations are not identical; do not wholesale replace the file.
- **Acceptance (Luna):** Root export and error shape; synchronous and rejected
  `TypeError` for reads and writes; exactly one fetch; fetch abort and other
  error identity, including a rejected revoked proxy checked with `===`; local
  validation and cyclic serialization before fetch; success, 409, and body
  parse failure mappings. Run `npm run build && node --test node/test/game-data.test.mjs`,
  then `npm test` and `npm pack --dry-run`. Report exact snapshot and results,
  toolchain, changed-line and function-size measurements. Git installation,
  deployed acceptance and committed-write/lost-response recovery remain
  UNVERIFIED unless separately tested.
- **Stop:** Escalate if boundary preservation, behavior, dependencies or file
  budget requires a material design change. No push, PR, merge or deployment.
- **Evidence:** local UNVERIFIED; integration UNVERIFIED; deployed UNVERIFIED.

## GDT-SDK-02: Align public recovery guidance

- **Status:** blocked. **Dependencies:** GDT-SDK-01 integrated and checked.
  **Owner:** Luna.
- **Files and budget:** `node/README.md` (432 lines, estimated 475; public SDK
  guidance); `protocol/game-data-v1.md` (184 lines, estimated 225; protocol
  guidance). At most 300 changed lines total.
- **Goal:** Explain actual TypeScript fetch versus HTTP, local and body error
  behavior. A failed read is not absence; tombstones have revisions; on 409
  reconcile original business intent or stop instead of blindly using a newer
  revision with stale data. For an indeterminate write, describe the identical
  full request including `expectedRevision`, same game/environment and
  idempotency key, 30-day receipt retention and uncertainty afterward. Note
  separate Room/data commits and lack of complete receipt validation. Preserve
  Rust and wire behavior; link the protocol document to this design/tracker.
- **Acceptance (Luna):** Inspect guidance against integrated GDT-SDK-01 and
  existing Worker contract; verify relative links using existing repository
  tooling if available. Report exact commands, snapshot and results. Docs alone
  do not prove runtime recovery.
- **Stop:** Do not invent application merge/retry rules or modify other files.
- **Evidence:** local UNVERIFIED; integration N/A; deployed UNVERIFIED.

## Final evidence

SDK local, package integration, combined review and publication: UNVERIFIED.
Deployed acceptance and committed-write/lost-response recovery: UNVERIFIED.
One initial review and at most one in-scope correction/re-review are authorized.
