import type { AgentGameCapabilities } from "./agent-capabilities.js";

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface GameManifest {
  title: string;
  version: string;
  minPlayers: number;
  maxPlayers: number;
  tickRateHz: number;
  persistentTable: boolean;
  optionsSchema?: Record<string, unknown> | boolean;
}

export interface MatchOptions {
  seed?: number;
  [key: string]: unknown;
}

export interface MatchConfig {
  matchId: string;
  gameId: string;
  players: string[];
  matchSeq: number;
  options?: MatchOptions;
  carryover?: JsonValue;
  tickRateHz?: number;
}

export interface TickInput<TData = unknown> {
  playerId: string;
  data: TData;
  timestamp?: number;
}

export interface GameCommand<TData = unknown> {
  type: string;
  data: TData;
  issuer?: string;
  timestamp?: number;
}

export interface TickContext<TInput = unknown, TCommand = unknown> {
  tick: number;
  inputs: Array<TickInput<TInput>>;
  commands?: Array<GameCommand<TCommand>>;
  elapsedSeconds: number;
}

export interface InitResult<TState = unknown> {
  state: TState;
  broadcasts?: unknown[];
}

export interface TickResult<TState = unknown> {
  state: TState;
  broadcasts?: unknown[];
  persist: boolean;
}

export interface PlayerEventResult<TState = unknown> {
  state: TState;
  broadcasts?: unknown[];
}

export type NextMatchMode = "auto_next_match" | "wait_for_ready" | "close_room";

export interface NextMatchPolicy {
  mode: NextMatchMode;
  timeoutMs: number;
  carryover?: JsonValue;
}

export interface MatchSummary {
  winnerPlayerId?: string;
  scores?: Record<string, number>;
  durationSeconds: number;
  customMetrics?: Record<string, unknown>;
  extra?: Record<string, unknown>;
}

export interface GameResult {
  over: boolean;
  summary?: MatchSummary;
  nextMatchPolicy?: NextMatchPolicy;
}

export interface CommandResult<TState = unknown> {
  state: TState;
  broadcasts?: unknown[];
}

export interface GameServer<TState = unknown, TInput = unknown, TCommand = unknown> {
  agentCapabilities?(): Promise<AgentGameCapabilities | null> | AgentGameCapabilities | null;
  manifest(): Promise<GameManifest> | GameManifest;
  init(config: MatchConfig): Promise<InitResult<TState>> | InitResult<TState>;
  tick(
    ctx: TickContext<TInput, TCommand>,
    state: TState,
  ): Promise<TickResult<TState>> | TickResult<TState>;
  onPlayerJoin(
    playerId: string,
    state: TState,
  ): Promise<PlayerEventResult<TState>> | PlayerEventResult<TState>;
  onPlayerLeave(
    playerId: string,
    state: TState,
  ): Promise<PlayerEventResult<TState>> | PlayerEventResult<TState>;
  isGameOver(state: TState): Promise<GameResult> | GameResult;
  onCommand?(
    cmd: GameCommand<TCommand>,
    state: TState,
  ): Promise<CommandResult<TState>> | CommandResult<TState>;
}

export interface PlatformSqlResponse {
  rows: Array<Record<string, unknown>>;
}

export interface PlatformBroadcastResponse {
  success: boolean;
  recipientCount: number;
}

export interface PlatformSendResponse {
  success: boolean;
  delivered: boolean;
}

export interface PlatformPlayersResponse {
  players: Array<{
    playerId: string;
    joinedAt: number;
  }>;
}

export interface PlatformKvResponse<T = unknown> {
  value: T | null;
}

export interface PlatformKvMutationResponse {
  success: boolean;
}
