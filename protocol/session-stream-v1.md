# Session Stream v1

## English

### Scope

`Session Stream v1` is the trusted host-to-container WebSocket protocol at
`/stream`. Browser clients must use the browser `RoomClient`; they must never
connect to the container stream directly.

The server advertises `session-v1` in
`X-RandSeed-Stream-Protocol`. `checkpoint` and `strict` are optional durability
hints in `X-RandSeed-Stream-Durability`; the room host remains responsible for
persisting checkpoints, journaling inputs, and recovering a match.

### Host messages

`hydrate` supplies a checkpoint and an optional contiguous replay:

```json
{
  "type": "hydrate",
  "matchId": "match-1",
  "tickRateHz": 20,
  "tick": 12,
  "state": { "score": 4 },
  "replay": [
    { "tick": 13, "inputs": [{ "playerId": "player-a", "data": { "move": "left" } }] }
  ],
  "resyncPlayers": ["player-a"]
}
```

`step` advances exactly one tick. `inputs` and `events` may be omitted and then
mean empty arrays; if supplied, they must be arrays. A tick must be the current
tick plus one. `events` are processed in array order before `tick`; `join`,
`leave`, and trusted `command` events call their corresponding lifecycle
handlers. Stream commands are handled by `onCommand`; `TickContext.commands`
is empty for a streamed step.

Each input's `playerId` is supplied by the trusted host, not by a browser
request. `actionId` and `actionSchemaId` must be supplied together, and action
IDs must be unique within the tick.

### Container messages

- `hydrated` confirms recovery and returns the post-replay state, views,
  broadcasts, result units, action results, joinability, and game-over result
  when present.
- `stepResult` returns the tick result. Full state may be omitted when the host
  can recover it from replay/checkpoints.
- `ignoredAfterGameOver` means a pipelined tick arrived after the terminal tick.
- `error` contains a stable `VALIDATION_ERROR` or `INTERNAL_ERROR` code.

For every input carrying an `actionId`, the tick result must contain exactly one
matching `actionResults` entry with status `applied` or `rejected`. Results for
unknown IDs, duplicate results, or missing results are invalid. A rejected
action should include a stable, non-sensitive code.

Malformed host messages return `VALIDATION_ERROR` and must not advance the
session. Invalid game callback results, panics, and unexpected exceptions
return a sanitized `INTERNAL_ERROR` and require the host to hydrate/recover
before continuing. Do not silently coerce invalid `persist`, inputs, events,
or terminal results.

### Deterministic recovery

The same checkpoint and ordered replay must reproduce the same state,
broadcasts, player views, result events, action results, and terminal status.
Do not use wall-clock time, process-global authoritative state, OS randomness,
or unrecorded network responses in simulation callbacks. Use
`ctx.elapsedSeconds` and the deterministic seeded PRNG. `persist` is a
checkpoint hint, not a declaration that the host does or does not journal
every committed tick.

### Player-view safety

The SDK can validate JSON shape but cannot determine whether a game-authored
field is private. The current general `playerViews` protocol may fall back to
canonical `state` for a missing player key. Until the room host's fail-closed
per-player delivery is deployed, keep canonical state public and return a
complete view for every active recipient whenever a `playerViews` map is used.
The Agent `/agent/player-view` endpoint is separate and must never fall back to
canonical state or another player's private data.

`wait_for_ready` is a declared next-match policy, but browser-ready signaling
is not implemented by this protocol yet. Do not use it as if clients can
currently acknowledge readiness.

## 中文

### 适用范围

`Session Stream v1` 是可信房间 Host 与容器之间的 WebSocket 协议，路径为
`/stream`。浏览器必须使用浏览器侧的 `RoomClient`，不得直接连接容器
stream。

服务器通过 `X-RandSeed-Stream-Protocol` 声明 `session-v1`。可选的
`X-RandSeed-Stream-Durability` 值为 `checkpoint` 或 `strict`；持久化检查点、
记录输入日志和恢复比赛仍由 room host 负责。

### Host 消息

`hydrate` 提供检查点以及可选的连续回放记录。`step` 只推进一个 tick；
`inputs` 和 `events` 可以省略，省略时表示空数组。如果传入，则必须是数组。
tick 必须等于当前 tick 加一。`events` 按数组顺序在 `tick` 回调前执行：
`join`、`leave` 和可信的 `command` 分别调用对应生命周期处理器。
Stream 命令由 `onCommand` 处理；stream 模式下 `TickContext.commands` 为空。

每个 input 的 `playerId` 由可信 Host 提供，不能来自浏览器请求。
`actionId` 与 `actionSchemaId` 必须同时提供；同一个 tick 中的 action ID
必须唯一。

### 容器响应

- `hydrated` 表示恢复完成；若有数据，则返回回放后的状态、视图、广播、
  result units、action results、joinability 和终局结果。
- `stepResult` 返回该 tick 的结果。Host 能从回放/检查点恢复状态时，可以省略完整状态。
- `ignoredAfterGameOver` 表示终局 tick 之后到达的流水线 tick 被忽略。
- `error` 使用稳定的 `VALIDATION_ERROR` 或 `INTERNAL_ERROR` 错误码。

每个带有 `actionId` 的输入都必须有且仅有一个匹配的 `actionResults`，
状态为 `applied` 或 `rejected`。未知 ID、重复结果或缺失结果都属于无效游戏响应。
拒绝操作时应返回稳定且不包含敏感信息的错误码。

无效 Host 消息返回 `VALIDATION_ERROR`，并且不能推进比赛状态。游戏回调返回无效数据、
发生 panic 或抛出未预期异常时，返回经过脱敏的 `INTERNAL_ERROR`；Host 必须先重新
hydrate/恢复，再继续处理。不能把无效 `persist`、inputs、events 或终局结果静默转换为默认值。

### 确定性恢复

相同检查点和有序回放必须生成相同的状态、广播、玩家视图、result events、
action results 和终局结果。模拟回调不得使用墙上时钟、进程全局权威状态、操作系统随机数
或未记录的网络响应；应使用 `ctx.elapsedSeconds` 和确定性种子的 PRNG。
`persist` 是检查点提示，不表示 Host 不记录或不重放已提交 tick。

### 玩家视图安全

SDK 可以校验 JSON 结构，但无法判断游戏字段是否属于私有数据。当前通用
`playerViews` 协议在缺少某玩家 key 时可能回退到完整 `state`。在 room host 的
逐玩家 fail-closed 投递上线前，canonical state 必须保持公开；使用 `playerViews` 时，
必须为每个活跃接收者返回完整视图。Agent `/agent/player-view` 是独立接口，不得回退到
canonical state，也不得暴露其他玩家的私有数据。

`wait_for_ready` 虽然是合法的 next-match policy，但本协议尚未实现浏览器 ready 信号；
不能假设客户端目前可以确认准备完成。
