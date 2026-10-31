// Package roomhost implements the RandSeed room-host v1 game-server protocol.
package roomhost

import (
	"context"
	"encoding/json"
)

// GameManifest describes a game server and its supported room configuration.
type GameManifest struct {
	Title           string          `json:"title"`
	Version         string          `json:"version"`
	MinPlayers      int             `json:"minPlayers"`
	MaxPlayers      int             `json:"maxPlayers"`
	TickRateHz      int             `json:"tickRateHz"`
	PersistentTable bool            `json:"persistentTable"`
	OptionsSchema   json.RawMessage `json:"optionsSchema,omitempty"`
}

// MatchConfig contains the host-owned configuration for one match.
type MatchConfig struct {
	MatchID    string          `json:"matchId"`
	GameID     string          `json:"gameId"`
	Players    []string        `json:"players"`
	MatchSeq   int64           `json:"matchSeq"`
	Options    json.RawMessage `json:"options,omitempty"`
	Carryover  json.RawMessage `json:"carryover,omitempty"`
	TickRateHz *int            `json:"tickRateHz,omitempty"`
}

// PlayerInput is one player's JSON payload for a simulation tick.
type PlayerInput struct {
	PlayerID  string          `json:"playerId"`
	Data      json.RawMessage `json:"data"`
	Timestamp *int64          `json:"timestamp,omitempty"`
}

// Command is a trusted host command delivered to the game server.
type Command struct {
	Type   string          `json:"type"`
	Data   json.RawMessage `json:"data"`
	Issuer string          `json:"issuer,omitempty"`
}

// TickContext contains deterministic host-supplied simulation inputs.
type TickContext struct {
	Tick            int64         `json:"tick"`
	Inputs          []PlayerInput `json:"inputs"`
	Commands        []Command     `json:"commands,omitempty"`
	ElapsedSeconds  float64       `json:"elapsedSeconds"`
}

// TickRequest supplies the current state and host context for one tick.
type TickRequest struct {
	Ctx   TickContext    `json:"ctx"`
	State json.RawMessage `json:"state"`
}

// PlayerEventRequest supplies a player lifecycle event and current state.
type PlayerEventRequest struct {
	PlayerID string          `json:"playerId"`
	State    json.RawMessage `json:"state"`
}

// IsGameOverRequest supplies the state to evaluate for match completion.
type IsGameOverRequest struct {
	State json.RawMessage `json:"state"`
}

// CommandRequest supplies a trusted host command and current state.
type CommandRequest struct {
	Command Command         `json:"command"`
	State   json.RawMessage `json:"state"`
}

// InitResult is the initial authoritative state and optional broadcasts.
type InitResult struct {
	State      json.RawMessage   `json:"state"`
	Broadcasts []json.RawMessage `json:"broadcasts,omitempty"`
}

// TickResult contains the updated state, optional broadcasts, and persistence hint.
type TickResult struct {
	State      json.RawMessage   `json:"state"`
	Broadcasts []json.RawMessage `json:"broadcasts,omitempty"`
	Persist    bool              `json:"persist"`
}

// PlayerEventResult contains the state after a join or leave event.
type PlayerEventResult struct {
	State      json.RawMessage   `json:"state"`
	Broadcasts []json.RawMessage `json:"broadcasts,omitempty"`
}

// NextMatchMode identifies how the room host should proceed after a match.
type NextMatchMode string

const (
	// NextMatchAutoNextMatch asks the host to create the next match automatically.
	NextMatchAutoNextMatch NextMatchMode = "auto_next_match"
	// NextMatchWaitForReady asks the host to wait for browser-ready signaling.
	NextMatchWaitForReady NextMatchMode = "wait_for_ready"
	// NextMatchCloseRoom asks the host to close the room.
	NextMatchCloseRoom NextMatchMode = "close_room"
)

// NextMatchPolicy describes the host's next-match behavior.
type NextMatchPolicy struct {
	Mode       NextMatchMode   `json:"mode"`
	TimeoutMS  *int64          `json:"timeoutMs,omitempty"`
	Carryover  json.RawMessage `json:"carryover,omitempty"`
}

// GameResult reports whether the match has ended and, if so, its outcome.
type GameResult struct {
	Over            bool             `json:"over"`
	Summary         json.RawMessage  `json:"summary,omitempty"`
	NextMatchPolicy *NextMatchPolicy `json:"nextMatchPolicy,omitempty"`
}

// CommandResult contains the state after a trusted command.
type CommandResult struct {
	State      json.RawMessage   `json:"state"`
	Broadcasts []json.RawMessage `json:"broadcasts,omitempty"`
}

// GameServer implements all game-specific room-host lifecycle operations.
type GameServer interface {
	// Manifest returns the static manifest for this game.
	Manifest() GameManifest
	// Init creates the initial state for a match.
	Init(context.Context, MatchConfig) (InitResult, error)
	// Tick advances the supplied state using the host-provided tick context.
	Tick(context.Context, TickRequest) (TickResult, error)
	// OnPlayerJoin handles a player joining the match.
	OnPlayerJoin(context.Context, PlayerEventRequest) (PlayerEventResult, error)
	// OnPlayerLeave handles a player leaving the match.
	OnPlayerLeave(context.Context, PlayerEventRequest) (PlayerEventResult, error)
	// IsGameOver determines whether the supplied state ends the match.
	IsGameOver(context.Context, IsGameOverRequest) (GameResult, error)
	// OnCommand handles a trusted host command.
	OnCommand(context.Context, CommandRequest) (CommandResult, error)
}

// GameIDProvider optionally supplies the identifier returned by GET /health.
type GameIDProvider interface {
	// GameID returns the stable identifier for this game.
	GameID() string
}
