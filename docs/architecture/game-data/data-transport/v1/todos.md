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

- **Status:** complete. **Dependencies:** none. **Owner:** Luna.
- **Files and budget:** `node/src/game-data.ts` (120 lines, final 137;
  TypeScript public contract/application); `node/test/game-data.test.mjs`
  (89 lines, final 166; contract tests). 98 changed lines total.
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
- **Evidence:** Coder commit `fc9081dbc0684a84fb885907fd4418d94b6990ff`,
  based on `f5ceb808`, integrated as `2218063340e0dcf0951f44b668701550b96bc3ae`.
  Integrated source and test SHA-256 values match the tested coder snapshot:
  `e4b8c740bf34a94497171d6a97e6292041ef6838f95c89264c0da0730a59ce91`
  and `d8c9d8037cefdc03d6fa8643f47a8f9f94e091251258d816d8d26441100e026d`.
  With Node v24.14.1 and npm 11.11.0, the build and focused tests passed
  (8/8), `npm test` passed (101/101), and `npm pack --dry-run` passed.
  The new constructor is 4 lines, classifier 9, `call` 24, and added test
  callbacks 10–20 lines; nesting is at most two levels. Deployed behavior and
  committed-write/lost-response recovery remain UNVERIFIED.

## GDT-SDK-02: Align public recovery guidance

- **Status:** complete. **Dependencies:** GDT-SDK-01 integrated and checked.
  **Owner:** Luna.
- **Files and budget:** `node/README.md` (432 lines, final 473; public SDK
  guidance); `protocol/game-data-v1.md` (184 lines, final 223; protocol
  guidance). 106 changed lines total.
- **Goal:** Explain actual TypeScript fetch versus HTTP, local and body error
  behavior. A failed read is not absence; tombstones have revisions; on 409
  reconcile original business intent or stop instead of blindly using a newer
  revision with stale data. For an indeterminate write, describe the identical
  full request including `expectedRevision`, same game/environment and
  idempotency key, 30-day receipt retention and uncertainty afterward. Note
  separate Room/data commits and lack of complete receipt validation. Preserve
  Rust and wire behavior. The design and tracker link each other; the shipped
  protocol must not use relative links to these files because they are excluded
  from the published npm package.
- **Acceptance (Luna):** Inspect guidance against integrated GDT-SDK-01 and
  existing Worker contract; verify relative links using existing repository
  tooling if available. Report exact commands, snapshot and results. Docs alone
  do not prove runtime recovery.
- **Stop:** Do not invent application merge/retry rules or modify other files.
- **Evidence:** Initial coder commit `16aa5d41` was integrated as `e1175523`.
  The package dry-run revealed the protocol's new relative design/tracker
  links would be broken because `package.json` excludes `docs/architecture`.
  Initial-validation correction `aaf15aca` was integrated as `6d50e26`; its
  protocol SHA-256 is
  `d6fbf76a7ecc5b2b575abcc2ba62345482666baa76bd2da8ff36ebe8d885af41`.
  `npm pack --dry-run` passed on `6d50e26` (51 files); the protocol has no
  `docs/architecture` references. The coder's targeted README protocol link
  and recovery anchor checks passed. `git diff --check origin/main...HEAD`
  passed. Deployed behavior remains UNVERIFIED.

## Final evidence

SDK local and package checks passed as recorded above. Combined review and
publication remain pending. Deployed acceptance and committed-write/lost-
response recovery remain UNVERIFIED. One initial review and at most one
in-scope correction/re-review are authorized.
