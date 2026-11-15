# Game server implementation guidance

Before changing room-game state or results:

- Distinguish room recovery from cross-release persistence. Room callback state,
  checkpoints, and carryover recover a room within its pinned game/environment/
  runtime-version scope; the host and Server SDK do not migrate that state to a
  newly activated runtime version.
- Do not implement all-time rankings, progression, inventory, or durable player
  history only in room state, a bounded recent-history array, process memory,
  or browser storage. Use a game-level durable store with an explicitly verified
  cross-release namespace and retention policy.
- Data schemas are game-defined, but a game can query them only through an
  exposed read API. On current Gamecreator, `PlatformClient.sql` is scoped to
  one Room Durable Object and `PlatformClient` KV is ephemeral room memory;
  neither is a game-wide, cross-release store. Do not recommend them for
  all-time rankings.
- Do not claim Gamecreator Result Units are queryable by game code. The Server
  SDK validates/emits events; the current Gamecreator Archive query is internal
  and there is no general game-facing Server SDK leaderboard query.
- `persist: true` persists room-state snapshots within the room scope, not
  arbitrary application records. If a task requires cross-release game-owned
  data and the target host has no documented game-level store/query API, state
  that platform capability is missing instead of inventing a storage path.
- Keep stable game/player identity keys independent of runtime version unless
  the data is intentionally version-scoped. Define schema migration, deletion,
  idempotency, and backfill behavior before changing persisted data.
- Distinguish publisher `manifest.version`, platform runtime version, and
  `resultDefinition.version`. A Result Unit event is not proof of durable
  archival, delivery, or queryability; the Server SDK does not implement a
  statistics or leaderboard query API. Use an Archive query only when the host
  exposes one; otherwise maintain a durable projection the game can read back.
- For a metric-definition change, explicitly choose whether old and new results
  remain separate, are queried together, or are migrated. Never silently drop
  historical rankings because a release changed its definition version.
- Add an upgrade regression: seed historical records, activate a new runtime
  version, create a new room, and verify the specified retention and ranking
  behavior.

The normative lifecycle and release-upgrade guidance is in
[`protocol/room-host-v1.md`](./protocol/room-host-v1.md#cross-release-data-continuity).
