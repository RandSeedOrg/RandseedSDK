# AI-Assisted Frontend-to-Multiplayer Migration

## English

### What the Server SDK does

The Node.js and Rust Server SDKs implement the container-side Room Host
protocol. They do not automatically convert a browser game or replace the
browser `RoomClient`. Use the server SDK for authoritative rules and the
browser SDK for connecting to rooms, receiving snapshots, and submitting
player intents. Never expose the container's `/stream` or lifecycle endpoints
to browsers.

Start with the paired [Room Host Protocol v1](./room-host-v1.md) and
[Session Stream v1](./session-stream-v1.md) contracts. The
[Node.js guide](../node/README.md) links to a runnable
[cooperative-counter example](../node/examples/cooperative-counter.mjs), and
the [Rust guide](../rust/randseed-game-sdk/README.md) links to its cooperative
counter example.

### Migration sequence

1. **Inventory the frontend game.** List canonical rules/state, render-only
   state, player actions, turn order, join/leave behavior, hidden information,
   victory/defeat rules, and persistence. Do not copy React component state
   wholesale into the server.
2. **Make the server authoritative.** Move rule evaluation and all mutations
   of canonical state into `init`, `tick`, `onPlayerJoin`, `onPlayerLeave`,
   `onCommand`, and `isGameOver`. The browser sends intent data only; it must
   not submit replacement state, scores, authenticated player IDs, or results.
3. **Define the manifest and state.** Set player bounds, tick rate,
   `persistentTable`, and `optionsSchema`. `optionsSchema` is a declaration,
   not a substitute for validating game-specific options in `init`. Add an
   explicit state schema version and a migration path for state that may be
   hydrated after a deployment.
4. **Implement deterministic transitions.** Treat the supplied state and
   `TickContext` as the complete simulation inputs. Validate every action
   against the host-supplied player ID, roster, turn, and game rules. Use the
   tick and seeded deterministic PRNG; do not use wall-clock time,
   `Math.random`, process-global authoritative state, or unrecorded network
   responses.
5. **Define lifecycle semantics.** Make `onPlayerJoin` idempotent because it
   may represent a reconnect. Decide whether a leave/disconnect is temporary,
   a timeout, or a forfeit; do not infer a forfeit from a single disconnect.
   Handle trusted `onCommand` events separately from player inputs.
6. **Connect the browser client.** Keep React responsible for rendering and
   ephemeral UI state. Use the browser `RoomClient` to join, receive
   authoritative snapshots, submit intents, and show pending/applied/rejected
   outcomes. The server-side `PlatformClient` is for host-provided services,
   not a replacement for the room client.
7. **Finish every match.** `isGameOver` must be deterministic. When `over` is
   true, return a valid summary and `nextMatchPolicy`; disable further
   joinability and use only a policy supported by the deployed host. Browser
   ready signaling for `wait_for_ready` is not implemented by Session Stream
   v1 yet.

### Release-blocking checks

- Manifest, options, state, inputs, commands, and callback results are valid
  JSON; numbers are finite and IDs are within protocol limits.
- Every submitted action ID is unique within a tick and receives exactly one
  `applied` or `rejected` result. Never acknowledge an action merely because a
  later tick advanced.
- Replaying the same checkpoint and ordered inputs reproduces the same state,
  broadcasts, result events, and action results. Persist enough state/version
  information to recover across container restarts and releases.
- Duplicate joins, reconnects, leaves, stale ticks, duplicate actions,
  wrong-turn actions, and unknown commands have explicit, tested outcomes.
- Terminal transitions include a valid summary and next-match policy. Result
  event IDs are stable across retries; result units match the declared
  definition and authenticated roster.
- If a game has private state, test every seat with canary secrets. The current
  generic `playerViews` host path may fall back to canonical state for a
  missing key; until the host enforces fail-closed delivery, keep canonical
  state public and provide a view for every active recipient whenever
  `playerViews` is used. Agent `/agent/player-view` is a separate
  recipient-specific contract.
- Treat validation errors (HTTP 400 / stream `VALIDATION_ERROR`) as source or
  protocol defects, payload-limit errors (HTTP 413) as a size defect, and
  sanitized internal errors (HTTP 500 / stream `INTERNAL_ERROR`) as game or
  infrastructure failures. Do not retry non-idempotent game side effects.

## 中文

### Server SDK 的职责

Node.js 和 Rust Server SDK 实现的是容器侧 Room Host 协议；它们不会自动把
浏览器游戏转换成多人游戏，也不能替代浏览器端 `RoomClient`。Server SDK
负责权威规则和状态变更，浏览器 SDK 负责加入房间、接收快照及提交玩家意图。
浏览器不得直接访问容器的 `/stream` 或生命周期接口。

开始前先对照 [Room Host Protocol v1](./room-host-v1.md) 和
[Session Stream v1](./session-stream-v1.md)。[Node.js 指南](../node/README.md)
链接到可运行的[合作计数器示例](../node/examples/cooperative-counter.mjs)，
[Rust 指南](../rust/randseed-game-sdk/README.md)也链接到对应示例。

### 迁移步骤

1. **盘点前端游戏。** 列出权威规则/状态、纯渲染状态、玩家操作、回合顺序、
   加入/离开行为、隐藏信息、胜负规则和持久化需求。不要把 React 组件状态整体搬到服务器。
2. **让服务器成为权威。** 将规则判断和权威状态变更放入 `init`、`tick`、
   `onPlayerJoin`、`onPlayerLeave`、`onCommand` 和 `isGameOver`。浏览器只发送意图数据；
   不得提交替换后的状态、分数、认证玩家 ID 或比赛结果。
3. **定义 manifest 和状态结构。** 设置玩家数范围、tick 频率、
   `persistentTable` 和 `optionsSchema`。`optionsSchema` 只是声明，不能替代 `init`
   中的游戏选项校验。显式加入状态 schema 版本，并为部署后 hydrate 的旧状态提供迁移路径。
4. **实现确定性状态转换。** 将传入的 state 和 `TickContext` 视为模拟所需的完整输入。
   每个操作都要按 Host 提供的玩家 ID、玩家名单、回合和规则校验。使用 tick 和确定性 PRNG；
   不要在权威模拟中使用墙上时钟、`Math.random`、进程全局状态或未记录的网络响应。
5. **定义生命周期语义。** `onPlayerJoin` 必须幂等，因为它可能表示重连。明确区分临时断线、
   超时和弃权；不能将一次断线直接视为弃权。可信的 `onCommand` 事件要与玩家输入分开处理。
6. **连接浏览器客户端。** React 继续负责渲染和临时 UI 状态。使用浏览器 `RoomClient`
   加入房间、接收权威快照、提交意图，并展示 pending/applied/rejected 状态。
   服务端 `PlatformClient` 用于调用 Host 服务，不是 RoomClient 的替代品。
7. **完整结束每场比赛。** `isGameOver` 必须确定性执行。`over` 为 true 时返回合法的 summary
   和 `nextMatchPolicy`，并停止接收新玩家；只能使用当前 Host 支持的策略。
   Session Stream v1 尚未实现浏览器对 `wait_for_ready` 的 ready 信号。

### 发布阻断检查

- Manifest、options、state、inputs、commands 和回调结果都是合法 JSON；
  数值必须有限，标识符必须符合协议限制。
- 每个 tick 内 action ID 唯一；每个带 ID 的操作必须且只能得到一个
  `applied` 或 `rejected` 结果。不能仅因后续 tick 前进就声称操作已应用。
- 用相同检查点和有序输入回放，必须得到相同状态、广播、结果事件和操作结果。
  持久化足够的状态版本信息，以支持容器重启和发布后恢复。
- 重复 join、重连、leave、过期 tick、重复操作、错误回合操作和未知命令都必须有明确且经过测试的结果。
- 终局转换必须带有合法 summary 和 next-match policy。结果事件 ID 在重试间保持稳定，
  result units 必须符合声明的 definition 和认证玩家名单。
- 游戏含有私有状态时，对每个玩家使用 canary secret 测试隔离。当前通用
  `playerViews` Host 路径在缺少 key 时可能回退到 canonical state；在 Host 实现
  fail-closed 前，canonical state 必须保持公开，且只要使用 `playerViews`，就要为每个活跃接收者提供视图。
  Agent `/agent/player-view` 是独立的逐接收者协议。
- 将校验错误（HTTP 400 / stream `VALIDATION_ERROR`）视为代码或协议错误；
  payload 超限（HTTP 413）视为大小错误；脱敏内部错误（HTTP 500 /
  stream `INTERNAL_ERROR`）视为游戏或基础设施故障。不要重试非幂等的游戏副作用。
