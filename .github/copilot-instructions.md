# Copilot Instructions — RandSeed SDKs

These requirements apply to every Copilot assistant, custom agent, planner,
coder, and reviewer working in this repository. Preserve the repository's
supported language implementations and shared protocol contracts. Use existing
test fixtures and compatibility checks across languages where applicable; do
not add a language, runtime, third-party dependency, or platform service without
design approval.
They apply alongside role-specific custom-agent prompts. If a role prompt
conflicts with this repository-wide policy, follow this policy and report the
conflict rather than silently relaxing the requirement.

## Requirement-to-Implementation Gate

Follow this sequence for requirement-driven code changes:

1. **Architecture and contracts:** diagnose the request and produce a concise,
   evidence-based design. Do not write implementation code or generate the
   executable TODO breakdown yet.
2. **Human approval:** present the design, scope, risks, and acceptance approach
   for confirmation before implementation planning is authorized. One approval
   may cover the bounded task; routine TODO handoffs do not require repeated
   approval. A material scope, behavior, or contract change requires renewed
   approval.
3. **Atomic TODOs and implementation:** only after design approval, create the
   dependency-ordered TODOs and implement only the approved TODOs. Keep each
   TODO independently verifiable and stop at its agreed boundary.

Scale the design detail to task risk, but do not omit a required section: for a
small repair, keep it concise and mark genuinely inapplicable items `N/A` with a
reason. Never use an inapplicable checklist item to hide an unverified risk.
Keep review bounded to an initial review and at most one correction/re-review
round per task. If that re-review still fails, stop and report evidence and the
smallest plan revision; do not start a second correction round under the same
task.

### Architecture and contract design

Every design must cover these areas:

1. **One primary domain model and boundaries**
   - Identify the single primary aggregate/domain model changed by the task.
     Do not combine unrelated business domains in one service or module.
   - Do not directly read or write another domain's tables/state or use
     cross-domain SQL joins. Communicate through an explicit service/port
     contract or an asynchronous domain event.
   - Preserve one-way dependencies: handler/SDK entry point -> application/use
     case -> domain rules and ports; concrete repositories/adapters implement
     ports and are wired at the composition boundary. Keep domain rules free of
     I/O and do not introduce layers that the task does not need.
2. **Explicit state machine when applicable**
   - For lifecycles, jobs, matches, sessions, or asynchronous workflows, define
     a closed status enum and do not infer status from combinations of unrelated
     booleans. Apply this to payments/orders only when the SDK owns that domain.
   - Include a complete transition table with `From`, `Event`, `Guard`,
     `To`, and deterministic `Side Effects`.
     Use columns: `Current Status (From) | Event | Guard | Target Status (To) |
     Deterministic Side Effects / Idempotency`.
   - Define an idempotency key (or a documented domain-level equivalent) for
     every state-changing command, including retry and duplicate-delivery
     behavior. Persist transitions with expected-status/version checks,
     compare-and-set, or the protocol/runtime's equivalent concurrency guard.
     Specify conflict, failure, and recovery behavior. If no state machine
     applies, state why.
3. **Contracts and deterministic data**
   - Define affected wire schemas, protocol messages, input/output types, and
     machine-readable errors before designing implementation details. Preserve
     cross-language compatibility and update shared fixtures/contracts first.
   - Use integer smallest units or fixed-point/decimal arithmetic for money,
     ledger values, points, rates, multipliers, and inventory. Do not use
     floating-point arithmetic for these calculations.
4. **File and function budgets**
   - Estimate current and final line counts for every new or materially changed
     source file. Target fewer than 500 lines; 1,000 lines is a hard ceiling.
     If a required legacy file is already at/over the ceiling, isolate the
     changed responsibility in a smaller module or obtain approval for a
     narrowly justified exception before coding. Do not refactor unrelated
     code merely to satisfy the limit.
   - Keep each new or materially changed function/method at 60 lines or fewer,
     with no more than three nested control-flow levels. Use guard clauses and
     focused helpers where appropriate.
   - Do not add third-party dependencies without design approval. Do not
     hardcode secrets, URLs, timeouts, or unexplained numeric values; use
     approved configuration, environment bindings, or named constants.

End the design with an architecture self-check. Every item must be `[x]` or
explicitly `N/A` with a reason before approval and TODO creation:

- [ ] One primary domain model is identified; domain boundaries and dependency
      direction are preserved.
- [ ] Applicable state transitions, idempotency, concurrency guards, and
      failure/recovery behavior are defined.
- [ ] Numeric calculations are deterministic and avoid floating-point
      arithmetic for value-bearing quantities.
- [ ] Every planned source file has a line budget below the hard ceiling, and
      changed functions meet the size/nesting limits.
- [ ] Contracts and acceptance checks cover every affected boundary and
      applicable language implementation.

### Atomic TODO format and execution

After architecture approval, create TODOs in dependency order:

1. **Contract layer:** shared protocol fixtures/schemas, public types, status
   enums, and error definitions.
2. **Domain layer:** pure state transitions, invariant checks, and deterministic
   calculations.
3. **Persistence/adapters:** repository contracts and database or
   external-service implementations, only when this repository owns them.
4. **Application/entry points:** SDK/runtime orchestration, public methods, and
   examples.

Skip a layer only when it is genuinely not applicable and record why. Each TODO
must:

- Have a stable ID, one bounded outcome, explicit dependencies, and ownership
  of one or two files. Split work touching more files into dependent TODOs,
  unless a documented atomic contract requires otherwise.
- Limit the total change to at most 300 lines, including tests and related
  documentation. Split larger work into independently reviewable TODOs.
- List each target path, its current line count when it exists, and the
  estimated final count; include its layer and implementation summary.
- State exact acceptance checks: focused unit/contract tests, invalid state
  transition and boundary tests where applicable, the relevant type/lint/build
  checks, and cross-language compatibility or integration checks for affected
  contracts. A unit test alone does not prove a cross-language wire contract.
- Identify test ownership and commands. Reuse unchanged evidence rather than
  rerunning whole suites per TODO or reviewer comment. Mark unavailable runtime
  or language checks `UNVERIFIED`, never passed.

Use the repository's configured line-count/lint checks. If no applicable
`max-lines` lint exists, report measured line counts for changed files and
functions; do not claim a lint rule ran or add tooling without approval. Every
coder works only on assigned TODO IDs. Parallelize only independent TODOs with
disjoint file ownership and stable contracts, within the approved worker limit.

Present approved architecture before TODO decomposition using this structure:

1. **Primary domain model and boundaries:** domain, aggregate, and external
   service/port contracts.
2. **State machine (when applicable):** full transition table using
   `From | Event | Guard | To | Deterministic Side Effects / Idempotency`,
   plus concurrency behavior; otherwise explain `N/A`.
3. **Contracts and file plan:** protocol/types/errors first, then file ownership,
   layer, change summary, and current/estimated final line counts.
4. **Architecture self-check:** every required item satisfied or justified `N/A`.
5. **Atomic TODOs (after approval):** checked/unchecked TODO IDs, dependencies,
   files, change budget, implementation summary, and executable Definition of
   Done.

Reviewers must assess the approved design, TODO boundaries, contracts, relevant
language/compatibility evidence, and measured code-size limits. Report concrete
findings with the TODO ID, invariant/acceptance criterion, failure scenario, and
evidence. Do not expand scope or turn optional suggestions into blocking work.
