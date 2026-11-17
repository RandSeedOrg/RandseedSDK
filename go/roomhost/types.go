// Package roomhost implements the RandSeed room-host v1 game-server protocol.
package roomhost

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"unicode/utf8"
)

// GameManifest describes a game server and its supported room configuration.
// Its field layout is kept stable for existing unkeyed literals.
type GameManifest struct {
	Title            string                `json:"title"`
	Version          string                `json:"version"`
	MinPlayers       int                   `json:"minPlayers"`
	MaxPlayers       int                   `json:"maxPlayers"`
	TickRateHz       float64               `json:"tickRateHz"`
	PersistentTable  bool                  `json:"persistentTable"`
	OptionsSchema    json.RawMessage       `json:"optionsSchema,omitempty"`
	ResultDefinition *GameResultDefinition `json:"resultDefinition,omitempty"`
}

// PlayerViewMode declares recipient-specific projections in room-host v1.
type PlayerViewMode string

const PlayerViewPrivate PlayerViewMode = "private"

// GameManifestV1 opts into private-view manifest extensions without changing GameManifest.
type GameManifestV1 struct {
	GameManifest
	PlayerViewMode          PlayerViewMode `json:"playerViewMode,omitempty"`
	PlayerViewSchemaVersion *int           `json:"playerViewSchemaVersion,omitempty"`
}

// UnmarshalJSON accepts integer-valued JSON representations 1, 1.0, and 1e0.
func (manifest *GameManifestV1) UnmarshalJSON(data []byte) error {
	var decoded struct {
		GameManifest
		PlayerViewMode          json.RawMessage `json:"playerViewMode"`
		PlayerViewSchemaVersion json.RawMessage `json:"playerViewSchemaVersion"`
	}
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	fields, ok := objectFields(data)
	if !ok {
		return fmt.Errorf("manifest must be an object")
	}
	var mode PlayerViewMode
	if raw, exists := fields["playerViewMode"]; exists {
		if isJSONNull(raw) || json.Unmarshal(raw, &mode) != nil || mode != PlayerViewPrivate {
			return fmt.Errorf("playerViewMode must be private when provided")
		}
	}
	var version *int
	if raw, exists := fields["playerViewSchemaVersion"]; exists {
		var numeric float64
		if isJSONNull(raw) || json.Unmarshal(raw, &numeric) != nil || numeric != 1 || mode != PlayerViewPrivate {
			return fmt.Errorf("playerViewSchemaVersion must be 1 and requires playerViewMode private")
		}
		normalized := 1
		version = &normalized
	}
	*manifest = GameManifestV1{GameManifest: decoded.GameManifest, PlayerViewMode: mode, PlayerViewSchemaVersion: version}
	return nil
}

// ResultMetricScope identifies the subject aggregated by a metric.
type ResultMetricScope string

const (
	// ResultMetricParticipant records one value per player.
	ResultMetricParticipant ResultMetricScope = "participant"
	// ResultMetricTeam records one value per team.
	ResultMetricTeam ResultMetricScope = "team"
)

// ResultOutcome is an optional game-defined participant or team outcome.
type ResultOutcome string

const (
	ResultOutcomeWin  ResultOutcome = "win"
	ResultOutcomeLoss ResultOutcome = "loss"
	ResultOutcomeDraw ResultOutcome = "draw"
	ResultOutcomePush ResultOutcome = "push"
)

// RatioMetricAggregation defines a ratio derived from cumulative sums.
type RatioMetricAggregation struct {
	Type                string `json:"type"`
	NumeratorMetricID   string `json:"numeratorMetricId"`
	DenominatorMetricID string `json:"denominatorMetricId"`
}

// GameMetricAggregation is either a simple operation or a derived ratio.
type GameMetricAggregation struct {
	Simple string
	Ratio  *RatioMetricAggregation
}

// MarshalJSON encodes simple aggregations as strings and ratios as objects.
func (a GameMetricAggregation) MarshalJSON() ([]byte, error) {
	if a.Ratio != nil {
		return json.Marshal(a.Ratio)
	}
	return json.Marshal(a.Simple)
}

// UnmarshalJSON decodes the wire union for metric aggregations.
func (a *GameMetricAggregation) UnmarshalJSON(data []byte) error {
	data = bytes.TrimSpace(data)
	if len(data) == 0 {
		return fmt.Errorf("aggregation is required")
	}
	if data[0] == '"' {
		return json.Unmarshal(data, &a.Simple)
	}
	var ratio RatioMetricAggregation
	if err := decodeStrictJSON(data, &ratio); err != nil {
		return err
	}
	a.Ratio = &ratio
	return nil
}

// GameMetricDefinition declares a direct or derived result metric.
type GameMetricDefinition struct {
	ID          string                `json:"id"`
	Scope       ResultMetricScope     `json:"scope"`
	Unit        string                `json:"unit"`
	UnitTypes   []string              `json:"unitTypes"`
	Aggregation GameMetricAggregation `json:"aggregation"`
}

// UnmarshalJSON rejects unsupported metric declaration fields.
func (m *GameMetricDefinition) UnmarshalJSON(data []byte) error {
	type alias GameMetricDefinition
	return decodeStrictJSON(data, (*alias)(m))
}

// GameResultDefinition declares the result-unit types and metric meanings.
type GameResultDefinition struct {
	Version   string                 `json:"version"`
	UnitTypes []string               `json:"unitTypes"`
	Metrics   []GameMetricDefinition `json:"metrics"`
}

// UnmarshalJSON rejects unknown fields and requires each wire field.
func (d *GameResultDefinition) UnmarshalJSON(data []byte) error {
	type alias GameResultDefinition
	if err := decodeStrictJSON(data, (*alias)(d)); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	for _, field := range []string{"version", "unitTypes", "metrics"} {
		raw, exists := fields[field]
		if !exists || isJSONNull(raw) {
			return fmt.Errorf("%s is required and must not be null", field)
		}
	}
	return nil
}

// Validate checks a result definition and its ratio references.
func (d GameResultDefinition) Validate() error {
	if err := validateIdentifier(d.Version); err != nil {
		return fmt.Errorf("result definition version: %w", err)
	}
	unitTypes, err := validateUniqueIdentifiers(d.UnitTypes)
	if err != nil {
		return fmt.Errorf("result definition unitTypes: %w", err)
	}
	if len(d.Metrics) > 128 {
		return fmt.Errorf("result definition exceeds the 128-metric limit")
	}
	metrics := make(map[string]GameMetricDefinition, len(d.Metrics))
	for _, metric := range d.Metrics {
		if err := validateIdentifier(metric.ID); err != nil {
			return fmt.Errorf("metric ID: %w", err)
		}
		if err := validateIdentifier(metric.Unit); err != nil {
			return fmt.Errorf("metric %q unit: %w", metric.ID, err)
		}
		if metric.Scope != ResultMetricParticipant && metric.Scope != ResultMetricTeam {
			return fmt.Errorf("metric %q has an invalid scope", metric.ID)
		}
		metricUnits, err := validateUniqueIdentifiers(metric.UnitTypes)
		if err != nil {
			return fmt.Errorf("metric %q unitTypes: %w", metric.ID, err)
		}
		for unitType := range metricUnits {
			if _, exists := unitTypes[unitType]; !exists {
				return fmt.Errorf("metric %q references undeclared unit type %q", metric.ID, unitType)
			}
		}
		if _, exists := metrics[metric.ID]; exists {
			return fmt.Errorf("duplicate metric ID %q", metric.ID)
		}
		if metric.Aggregation.Ratio == nil && metric.Aggregation.Simple != "sum" &&
			metric.Aggregation.Simple != "min" && metric.Aggregation.Simple != "max" {
			return fmt.Errorf("metric %q has an unsupported aggregation", metric.ID)
		}
		metrics[metric.ID] = metric
	}
	for _, metric := range d.Metrics {
		ratio := metric.Aggregation.Ratio
		if ratio == nil {
			continue
		}
		if ratio.Type != "ratio" || ratio.NumeratorMetricID == metric.ID ||
			ratio.DenominatorMetricID == metric.ID {
			return fmt.Errorf("ratio metric %q has an invalid definition", metric.ID)
		}
		numerator, numeratorOK := metrics[ratio.NumeratorMetricID]
		denominator, denominatorOK := metrics[ratio.DenominatorMetricID]
		if !numeratorOK || !denominatorOK || numerator.Scope != metric.Scope ||
			denominator.Scope != metric.Scope || numerator.Aggregation.Ratio != nil ||
			denominator.Aggregation.Ratio != nil || numerator.Aggregation.Simple != "sum" ||
			denominator.Aggregation.Simple != "sum" {
			return fmt.Errorf("ratio metric %q must reference same-scope sum metrics", metric.ID)
		}
		for _, unitType := range metric.UnitTypes {
			if !contains(numerator.UnitTypes, unitType) || !contains(denominator.UnitTypes, unitType) {
				return fmt.Errorf("ratio metric %q uses a unit type unsupported by its source metrics", metric.ID)
			}
		}
	}
	return nil
}

// GameResultParticipant contains one player's result in a unit.
type GameResultParticipant struct {
	PlayerID string             `json:"playerId"`
	TeamID   string             `json:"teamId,omitempty"`
	Outcome  ResultOutcome      `json:"outcome,omitempty"`
	Rank     *int64             `json:"rank,omitempty"`
	Metrics  map[string]float64 `json:"metrics"`
}

// UnmarshalJSON rejects unknown fields and explicit null optional values.
func (p *GameResultParticipant) UnmarshalJSON(data []byte) error {
	type alias GameResultParticipant
	if err := decodeStrictJSON(data, (*alias)(p)); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	for _, field := range []string{"teamId", "outcome", "rank"} {
		if raw, exists := fields[field]; exists && isJSONNull(raw) {
			return fmt.Errorf("%s must be omitted rather than null", field)
		}
	}
	return nil
}

// GameResultTeam contains one team's result in a unit.
type GameResultTeam struct {
	TeamID  string             `json:"teamId"`
	Outcome ResultOutcome      `json:"outcome,omitempty"`
	Rank    *int64             `json:"rank,omitempty"`
	Metrics map[string]float64 `json:"metrics"`
}

// UnmarshalJSON rejects unknown fields and explicit null optional values.
func (t *GameResultTeam) UnmarshalJSON(data []byte) error {
	type alias GameResultTeam
	if err := decodeStrictJSON(data, (*alias)(t)); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	for _, field := range []string{"outcome", "rank"} {
		if raw, exists := fields[field]; exists && isJSONNull(raw) {
			return fmt.Errorf("%s must be omitted rather than null", field)
		}
	}
	return nil
}

// GameResultCorrection refers to an earlier event replaced or retracted.
type GameResultCorrection struct {
	Action            string `json:"action"`
	SupersedesEventID string `json:"supersedesEventId"`
}

// UnmarshalJSON rejects unknown correction fields.
func (c *GameResultCorrection) UnmarshalJSON(data []byte) error {
	type alias GameResultCorrection
	return decodeStrictJSON(data, (*alias)(c))
}

// GameResultEvent is an immutable result unit emitted by authoritative game logic.
type GameResultEvent struct {
	SchemaVersion     int                     `json:"schemaVersion"`
	EventID           string                  `json:"eventId"`
	MatchID           string                  `json:"matchId"`
	UnitID            string                  `json:"unitId"`
	UnitType          string                  `json:"unitType"`
	RulesVersion      string                  `json:"rulesVersion"`
	DefinitionVersion string                  `json:"definitionVersion"`
	OccurredAt        *int64                  `json:"occurredAt,omitempty"`
	Participants      []GameResultParticipant `json:"participants"`
	Teams             []GameResultTeam        `json:"teams,omitempty"`
	Correction        *GameResultCorrection   `json:"correction,omitempty"`
}

// UnmarshalJSON rejects unknown event fields, including host-bound identities.
func (e *GameResultEvent) UnmarshalJSON(data []byte) error {
	type alias GameResultEvent
	if err := decodeStrictJSON(data, (*alias)(e)); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	for _, field := range []string{"occurredAt", "teams", "correction"} {
		if raw, exists := fields[field]; exists && isJSONNull(raw) {
			return fmt.Errorf("%s must be omitted rather than null", field)
		}
	}
	return nil
}

// marshalOptionalProtocolField preserves explicitly empty values while omitting absent ones.
func marshalOptionalProtocolField(value any, field string, present bool, fieldValue any) ([]byte, error) {
	encoded, err := json.Marshal(value)
	if err != nil || !present {
		return encoded, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(encoded, &fields); err != nil {
		return nil, err
	}
	encodedField, err := json.Marshal(fieldValue)
	if err != nil {
		return nil, err
	}
	fields[field] = encodedField
	return json.Marshal(fields)
}

// MatchConfig contains the host-owned configuration for one match.
type MatchConfig struct {
	MatchID    string          `json:"matchId"`
	GameID     string          `json:"gameId"`
	Players    []string        `json:"players"`
	MatchSeq   int64           `json:"matchSeq"`
	Options    json.RawMessage `json:"options,omitempty"`
	Carryover  json.RawMessage `json:"carryover,omitempty"`
	TickRateHz *float64        `json:"tickRateHz,omitempty"`
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
	Tick           int64         `json:"tick"`
	Inputs         []PlayerInput `json:"inputs"`
	Commands       []Command     `json:"commands,omitempty"`
	ElapsedSeconds float64       `json:"elapsedSeconds"`
}

// TickRequest supplies the current state and host context for one tick.
// The field layout remains unchanged for existing callers.
type TickRequest struct {
	Ctx   TickContext     `json:"ctx"`
	State json.RawMessage `json:"state"`
}

// TickRequestV1 opts into active-roster metadata. A non-nil empty Players slice
// serializes as an explicit empty roster; nil omits the field.
type TickRequestV1 struct {
	Ctx     TickContext     `json:"ctx"`
	State   json.RawMessage `json:"state"`
	Players []string        `json:"players,omitempty"`
}

func (request TickRequestV1) MarshalJSON() ([]byte, error) {
	type wire TickRequestV1
	return marshalOptionalProtocolField(wire(request), "players", request.Players != nil, request.Players)
}

// PlayerEventRequest supplies a player lifecycle event and current state.
type PlayerEventRequest struct {
	PlayerID string          `json:"playerId"`
	State    json.RawMessage `json:"state"`
}

// PlayerEventRequestV1 carries optional resulting-roster metadata.
type PlayerEventRequestV1 struct {
	PlayerID string          `json:"playerId"`
	State    json.RawMessage `json:"state"`
	Players  []string        `json:"players,omitempty"`
}

func (request PlayerEventRequestV1) MarshalJSON() ([]byte, error) {
	type wire PlayerEventRequestV1
	return marshalOptionalProtocolField(wire(request), "players", request.Players != nil, request.Players)
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

// CommandRequestV1 carries optional active-roster metadata.
type CommandRequestV1 struct {
	Command Command         `json:"command"`
	State   json.RawMessage `json:"state"`
	Players []string        `json:"players,omitempty"`
}

func (request CommandRequestV1) MarshalJSON() ([]byte, error) {
	type wire CommandRequestV1
	return marshalOptionalProtocolField(wire(request), "players", request.Players != nil, request.Players)
}

// InitResult is the initial authoritative state and optional broadcasts.
type InitResult struct {
	State       json.RawMessage   `json:"state"`
	Broadcasts  []json.RawMessage `json:"broadcasts,omitempty"`
	ResultUnits []GameResultEvent `json:"resultUnits,omitempty"`
}

// InitResultV1 opts into recipient-specific private views.
type InitResultV1 struct {
	State       json.RawMessage            `json:"state"`
	PlayerViews map[string]json.RawMessage `json:"playerViews,omitempty"`
	Broadcasts  []json.RawMessage          `json:"broadcasts,omitempty"`
	ResultUnits []GameResultEvent          `json:"resultUnits,omitempty"`
}

func (result InitResultV1) MarshalJSON() ([]byte, error) {
	type wire InitResultV1
	return marshalOptionalProtocolField(wire(result), "playerViews", result.PlayerViews != nil, result.PlayerViews)
}

// TickResult contains the updated state, optional broadcasts, and persistence hint.
type TickResult struct {
	State       json.RawMessage   `json:"state"`
	Broadcasts  []json.RawMessage `json:"broadcasts,omitempty"`
	Persist     bool              `json:"persist"`
	ResultUnits []GameResultEvent `json:"resultUnits,omitempty"`
}

// TickResultV1 opts into recipient-specific private views.
type TickResultV1 struct {
	State       json.RawMessage            `json:"state"`
	PlayerViews map[string]json.RawMessage `json:"playerViews,omitempty"`
	Broadcasts  []json.RawMessage          `json:"broadcasts,omitempty"`
	Persist     bool                       `json:"persist"`
	ResultUnits []GameResultEvent          `json:"resultUnits,omitempty"`
}

func (result TickResultV1) MarshalJSON() ([]byte, error) {
	type wire TickResultV1
	return marshalOptionalProtocolField(wire(result), "playerViews", result.PlayerViews != nil, result.PlayerViews)
}

// PlayerEventResult contains the state after a join or leave event.
type PlayerEventResult struct {
	State       json.RawMessage   `json:"state"`
	Broadcasts  []json.RawMessage `json:"broadcasts,omitempty"`
	ResultUnits []GameResultEvent `json:"resultUnits,omitempty"`
}

// PlayerEventResultV1 opts into recipient-specific private views.
type PlayerEventResultV1 struct {
	State       json.RawMessage            `json:"state"`
	PlayerViews map[string]json.RawMessage `json:"playerViews,omitempty"`
	Broadcasts  []json.RawMessage          `json:"broadcasts,omitempty"`
	ResultUnits []GameResultEvent          `json:"resultUnits,omitempty"`
}

func (result PlayerEventResultV1) MarshalJSON() ([]byte, error) {
	type wire PlayerEventResultV1
	return marshalOptionalProtocolField(wire(result), "playerViews", result.PlayerViews != nil, result.PlayerViews)
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
	Mode      NextMatchMode   `json:"mode"`
	TimeoutMS int64           `json:"timeoutMs"`
	Carryover json.RawMessage `json:"carryover,omitempty"`
}

// GameResult reports whether the match has ended and, if so, its outcome.
type GameResult struct {
	Over            bool              `json:"over"`
	Summary         json.RawMessage   `json:"summary,omitempty"`
	NextMatchPolicy *NextMatchPolicy  `json:"nextMatchPolicy,omitempty"`
	ResultUnits     []GameResultEvent `json:"resultUnits,omitempty"`
}

// CommandResult contains the state after a trusted command.
type CommandResult struct {
	State       json.RawMessage   `json:"state"`
	Broadcasts  []json.RawMessage `json:"broadcasts,omitempty"`
	ResultUnits []GameResultEvent `json:"resultUnits,omitempty"`
}

// CommandResultV1 opts into recipient-specific private views.
type CommandResultV1 struct {
	State       json.RawMessage            `json:"state"`
	PlayerViews map[string]json.RawMessage `json:"playerViews,omitempty"`
	Broadcasts  []json.RawMessage          `json:"broadcasts,omitempty"`
	ResultUnits []GameResultEvent          `json:"resultUnits,omitempty"`
}

func (result CommandResultV1) MarshalJSON() ([]byte, error) {
	type wire CommandResultV1
	return marshalOptionalProtocolField(wire(result), "playerViews", result.PlayerViews != nil, result.PlayerViews)
}

func decodeStrictJSON(data []byte, target any) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		return err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return fmt.Errorf("unexpected trailing JSON data")
	}
	return nil
}

func validateIdentifier(value string) error {
	if strings.TrimSpace(value) == "" || !utf8.ValidString(value) ||
		utf8.RuneCountInString(value) > 128 {
		return fmt.Errorf("must be non-empty and at most 128 Unicode code points")
	}
	return nil
}

func validateUniqueIdentifiers(values []string) (map[string]struct{}, error) {
	if len(values) == 0 {
		return nil, fmt.Errorf("must be a non-empty string array")
	}
	unique := make(map[string]struct{}, len(values))
	for _, value := range values {
		if err := validateIdentifier(value); err != nil {
			return nil, err
		}
		if _, exists := unique[value]; exists {
			return nil, fmt.Errorf("must not contain duplicate identifiers")
		}
		unique[value] = struct{}{}
	}
	return unique, nil
}

func contains(values []string, value string) bool {
	for _, candidate := range values {
		if candidate == value {
			return true
		}
	}
	return false
}

func isJSONNull(data []byte) bool {
	return bytes.Equal(bytes.TrimSpace(data), []byte("null"))
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

// GameServerV1 opts into room-host v1 roster and private-view request/response fields.
// Implement this interface with NewHandlerV1; GameServer remains source-compatible.
type GameServerV1 interface {
	ManifestV1() GameManifestV1
	InitV1(context.Context, MatchConfig) (InitResultV1, error)
	TickV1(context.Context, TickRequestV1) (TickResultV1, error)
	OnPlayerJoinV1(context.Context, PlayerEventRequestV1) (PlayerEventResultV1, error)
	OnPlayerLeaveV1(context.Context, PlayerEventRequestV1) (PlayerEventResultV1, error)
	IsGameOverV1(context.Context, IsGameOverRequest) (GameResult, error)
	OnCommandV1(context.Context, CommandRequestV1) (CommandResultV1, error)
}

// GameIDProvider optionally supplies the identifier returned by GET /health.
type GameIDProvider interface {
	// GameID returns the stable identifier for this game.
	GameID() string
}
