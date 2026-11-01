package roomhost

import (
	"bytes"
	"encoding/json"
	"fmt"
	"math"
	"strings"
)

const maxSafeInteger int64 = 1<<53 - 1

func validManifest(manifest GameManifest) bool {
	if strings.TrimSpace(manifest.Title) == "" || strings.TrimSpace(manifest.Version) == "" {
		return false
	}
	if manifest.MinPlayers < 1 || manifest.MaxPlayers < manifest.MinPlayers || manifest.MaxPlayers > 100 {
		return false
	}
	if math.IsNaN(manifest.TickRateHz) || math.IsInf(manifest.TickRateHz, 0) ||
		manifest.TickRateHz < 1 || manifest.TickRateHz > 60 {
		return false
	}
	if len(manifest.OptionsSchema) > 0 && !validOptionsSchema(manifest.OptionsSchema) {
		return false
	}
	return manifest.ResultDefinition == nil || manifest.ResultDefinition.Validate() == nil
}

func validMatchConfig(config MatchConfig, data []byte) bool {
	if strings.TrimSpace(config.MatchID) == "" || strings.TrimSpace(config.GameID) == "" {
		return false
	}
	if config.MatchSeq < 1 || config.MatchSeq > maxSafeInteger {
		return false
	}
	if len(config.Players) == 0 || len(config.Players) > 100 {
		return false
	}
	players := make(map[string]struct{}, len(config.Players))
	for _, playerID := range config.Players {
		if strings.TrimSpace(playerID) == "" {
			return false
		}
		if _, exists := players[playerID]; exists {
			return false
		}
		players[playerID] = struct{}{}
	}
	if config.TickRateHz != nil &&
		(math.IsNaN(*config.TickRateHz) || math.IsInf(*config.TickRateHz, 0) ||
			*config.TickRateHz < 1 || *config.TickRateHz > 60) {
		return false
	}
	fields, ok := objectFields(data)
	if !ok {
		return false
	}
	if raw, exists := fields["tickRateHz"]; exists && isJSONNull(raw) {
		return false
	}
	if len(config.Options) == 0 {
		return true
	}
	options, ok := objectFields(config.Options)
	if !ok {
		return false
	}
	seed, exists := options["seed"]
	if !exists {
		return true
	}
	if isJSONNull(seed) {
		return false
	}
	var value float64
	if err := json.Unmarshal(seed, &value); err != nil {
		return false
	}
	return !math.IsNaN(value) && !math.IsInf(value, 0)
}

func validTickRequest(request TickRequest, data []byte) bool {
	root, ok := objectFields(data)
	if !ok {
		return false
	}
	state, hasState := root["state"]
	ctx, hasContext := root["ctx"]
	if !hasState || !json.Valid(state) || !hasContext || !isJSONObject(ctx) {
		return false
	}
	contextFields, ok := objectFields(ctx)
	if !ok {
		return false
	}
	tick, hasTick := contextFields["tick"]
	if !hasTick || isJSONNull(tick) {
		return false
	}
	inputs, hasInputs := contextFields["inputs"]
	elapsed, hasElapsed := contextFields["elapsedSeconds"]
	if !hasInputs || !isJSONArray(inputs) || !hasElapsed || isJSONNull(elapsed) {
		return false
	}
	if request.Ctx.Tick < 0 || request.Ctx.Tick > maxSafeInteger ||
		request.Ctx.ElapsedSeconds < 0 ||
		math.IsNaN(request.Ctx.ElapsedSeconds) || math.IsInf(request.Ctx.ElapsedSeconds, 0) {
		return false
	}
	var rawInputs []json.RawMessage
	if err := json.Unmarshal(inputs, &rawInputs); err != nil || len(rawInputs) != len(request.Ctx.Inputs) {
		return false
	}
	for index, input := range request.Ctx.Inputs {
		if strings.TrimSpace(input.PlayerID) == "" || len(input.Data) == 0 || !json.Valid(input.Data) {
			return false
		}
		if input.Timestamp != nil && (*input.Timestamp < 0 || *input.Timestamp > maxSafeInteger) {
			return false
		}
		var inputFields map[string]json.RawMessage
		if err := json.Unmarshal(rawInputs[index], &inputFields); err != nil {
			return false
		}
		if timestamp, exists := inputFields["timestamp"]; exists && isJSONNull(timestamp) {
			return false
		}
	}
	if commands, exists := contextFields["commands"]; exists && !isJSONArray(commands) {
		return false
	}
	for _, command := range request.Ctx.Commands {
		if !validCommand(command) {
			return false
		}
	}
	return true
}

func validPlayerEventRequest(request PlayerEventRequest, data []byte) bool {
	fields, ok := objectFields(data)
	state, hasState := fields["state"]
	return ok && strings.TrimSpace(request.PlayerID) != "" && hasState && json.Valid(state)
}

func validIsGameOverRequest(request IsGameOverRequest, data []byte) bool {
	fields, ok := objectFields(data)
	state, hasState := fields["state"]
	return ok && hasState && len(request.State) > 0 && json.Valid(state)
}

func validCommandRequest(request CommandRequest, data []byte) bool {
	fields, ok := objectFields(data)
	state, hasState := fields["state"]
	command, hasCommand := fields["command"]
	return ok && hasState && len(request.State) > 0 && json.Valid(state) &&
		hasCommand && isJSONObject(command) && validCommand(request.Command)
}

func validCommand(command Command) bool {
	if strings.TrimSpace(command.Type) == "" || len(command.Data) == 0 || !json.Valid(command.Data) {
		return false
	}
	return true
}

func validInitResult(result InitResult) bool {
	return validJSON(result.State) && validBroadcasts(result.Broadcasts) &&
		validateGameResultUnits(result.ResultUnits, nil) == nil
}

func validTickResult(result TickResult) bool {
	return validJSON(result.State) && validBroadcasts(result.Broadcasts) &&
		validateGameResultUnits(result.ResultUnits, nil) == nil
}

func validPlayerEventResult(result PlayerEventResult) bool {
	return validJSON(result.State) && validBroadcasts(result.Broadcasts) &&
		validateGameResultUnits(result.ResultUnits, nil) == nil
}

func validCommandResult(result CommandResult) bool {
	return validJSON(result.State) && validBroadcasts(result.Broadcasts) &&
		validateGameResultUnits(result.ResultUnits, nil) == nil
}

func validGameResult(result GameResult) bool {
	if validateGameResultUnits(result.ResultUnits, nil) != nil {
		return false
	}
	if result.Over {
		if !validMatchSummary(result.Summary) || result.NextMatchPolicy == nil {
			return false
		}
	}
	if result.Summary != nil && !json.Valid(result.Summary) {
		return false
	}
	if result.NextMatchPolicy == nil {
		return !result.Over
	}
	switch result.NextMatchPolicy.Mode {
	case NextMatchAutoNextMatch, NextMatchWaitForReady, NextMatchCloseRoom:
	default:
		return false
	}
	if result.NextMatchPolicy.TimeoutMS < 0 {
		return false
	}
	return len(result.NextMatchPolicy.Carryover) == 0 || json.Valid(result.NextMatchPolicy.Carryover)
}

// Validate checks event shape and, when provided, its declared units and metrics.
func (e GameResultEvent) Validate(definition *GameResultDefinition) error {
	if e.SchemaVersion != 1 {
		return fmt.Errorf("schemaVersion must be 1")
	}
	for _, field := range []struct{ name, value string }{
		{"eventId", e.EventID}, {"matchId", e.MatchID}, {"unitId", e.UnitID},
		{"unitType", e.UnitType}, {"rulesVersion", e.RulesVersion},
		{"definitionVersion", e.DefinitionVersion},
	} {
		if err := validateIdentifier(field.value); err != nil {
			return fmt.Errorf("%s: %w", field.name, err)
		}
	}
	if e.Participants == nil {
		return fmt.Errorf("participants must be an array")
	}
	if len(e.Participants) > 1000 {
		return fmt.Errorf("participants exceed the 1000-item limit")
	}
	if len(e.Teams) > 256 {
		return fmt.Errorf("teams exceed the 256-item limit")
	}
	retraction := e.Correction != nil && e.Correction.Action == "retract"
	if len(e.Participants) == 0 && len(e.Teams) == 0 && !retraction {
		return fmt.Errorf("event must include participants or teams")
	}
	if retraction && (len(e.Participants) != 0 || len(e.Teams) != 0) {
		return fmt.Errorf("retraction must not include participants or teams")
	}
	if e.OccurredAt != nil && (*e.OccurredAt < 0 || *e.OccurredAt > maxSafeInteger) {
		return fmt.Errorf("occurredAt must be a non-negative safe integer timestamp")
	}
	if e.Correction != nil {
		if e.Correction.Action != "replace" && e.Correction.Action != "retract" {
			return fmt.Errorf("correction action must be replace or retract")
		}
		if err := validateIdentifier(e.Correction.SupersedesEventID); err != nil {
			return fmt.Errorf("supersedesEventId: %w", err)
		}
		if e.Correction.SupersedesEventID == e.EventID {
			return fmt.Errorf("correction cannot supersede itself")
		}
	}
	if definition != nil {
		if err := definition.Validate(); err != nil {
			return err
		}
		if e.DefinitionVersion != definition.Version || !contains(definition.UnitTypes, e.UnitType) {
			return fmt.Errorf("event definition version or unit type is undeclared")
		}
	}

	participantIDs := make(map[string]struct{}, len(e.Participants))
	for _, participant := range e.Participants {
		if err := validateIdentifier(participant.PlayerID); err != nil {
			return fmt.Errorf("playerId: %w", err)
		}
		if participant.TeamID != "" {
			if err := validateIdentifier(participant.TeamID); err != nil {
				return fmt.Errorf("teamId: %w", err)
			}
		}
		if _, exists := participantIDs[participant.PlayerID]; exists {
			return fmt.Errorf("duplicate player %q", participant.PlayerID)
		}
		participantIDs[participant.PlayerID] = struct{}{}
		if err := validateResultEntity(
			participant.Outcome, participant.Rank, participant.Metrics,
			ResultMetricParticipant, e.UnitType, definition); err != nil {
			return err
		}
	}
	teamIDs := make(map[string]struct{}, len(e.Teams))
	for _, team := range e.Teams {
		if err := validateIdentifier(team.TeamID); err != nil {
			return fmt.Errorf("teamId: %w", err)
		}
		if _, exists := teamIDs[team.TeamID]; exists {
			return fmt.Errorf("duplicate team %q", team.TeamID)
		}
		teamIDs[team.TeamID] = struct{}{}
		if err := validateResultEntity(
			team.Outcome, team.Rank, team.Metrics,
			ResultMetricTeam, e.UnitType, definition); err != nil {
			return err
		}
	}
	if len(teamIDs) > 0 {
		for _, participant := range e.Participants {
			if participant.TeamID != "" {
				if _, exists := teamIDs[participant.TeamID]; !exists {
					return fmt.Errorf("participant references undeclared team %q", participant.TeamID)
				}
			}
		}
	}
	data, err := json.Marshal(e)
	if err != nil {
		return fmt.Errorf("event is not JSON serializable: %w", err)
	}
	if len(data) > 256*1024 {
		return fmt.Errorf("event exceeds the 256 KiB serialized limit")
	}
	return nil
}

func validateResultEntity(
	outcome ResultOutcome,
	rank *int64,
	metrics map[string]float64,
	scope ResultMetricScope,
	unitType string,
	definition *GameResultDefinition,
) error {
	if outcome != "" && outcome != ResultOutcomeWin && outcome != ResultOutcomeLoss &&
		outcome != ResultOutcomeDraw && outcome != ResultOutcomePush {
		return fmt.Errorf("invalid %s outcome", scope)
	}
	if rank != nil && (*rank < 1 || *rank > maxSafeInteger) {
		return fmt.Errorf("%s rank must be a positive safe integer", scope)
	}
	if metrics == nil {
		return fmt.Errorf("%s metrics must be an object", scope)
	}
	if len(metrics) > 128 {
		return fmt.Errorf("%s metrics exceed the 128-item limit", scope)
	}
	for id, value := range metrics {
		if err := validateIdentifier(id); err != nil {
			return fmt.Errorf("metric ID: %w", err)
		}
		if math.IsNaN(value) || math.IsInf(value, 0) {
			return fmt.Errorf("metric %q must be finite", id)
		}
		if definition != nil {
			var found *GameMetricDefinition
			for index := range definition.Metrics {
				candidate := &definition.Metrics[index]
				if candidate.ID == id {
					found = candidate
					break
				}
			}
			if found == nil || found.Scope != scope || !contains(found.UnitTypes, unitType) ||
				found.Aggregation.Ratio != nil {
				return fmt.Errorf("metric %q is not a declared direct %s metric", id, scope)
			}
		}
	}
	return nil
}

// ValidateGameResultUnits validates the optional event batch for one transition.
func ValidateGameResultUnits(events []GameResultEvent, definition *GameResultDefinition) error {
	return validateGameResultUnits(events, definition)
}

func validateGameResultUnits(events []GameResultEvent, definition *GameResultDefinition) error {
	if len(events) > 32 {
		return fmt.Errorf("resultUnits exceeds the 32-event per-transition limit")
	}
	eventIDs := make(map[string]struct{}, len(events))
	type unitKey struct{ matchID, unitID string }
	unitIDs := make(map[unitKey]struct{}, len(events))
	for _, event := range events {
		if err := event.Validate(definition); err != nil {
			return err
		}
		if _, exists := eventIDs[event.EventID]; exists {
			return fmt.Errorf("duplicate result eventId %q", event.EventID)
		}
		eventIDs[event.EventID] = struct{}{}
		key := unitKey{event.MatchID, event.UnitID}
		if _, exists := unitIDs[key]; exists {
			return fmt.Errorf("duplicate result unit %q/%q", event.MatchID, event.UnitID)
		}
		unitIDs[key] = struct{}{}
	}
	return nil
}

func validMatchSummary(summary json.RawMessage) bool {
	fields, ok := objectFields(summary)
	if !ok {
		return false
	}
	duration, exists := fields["durationSeconds"]
	if !exists || isJSONNull(duration) {
		return false
	}
	var value float64
	if err := json.Unmarshal(duration, &value); err != nil {
		return false
	}
	return !math.IsNaN(value) && !math.IsInf(value, 0) && value >= 0
}

func validBroadcasts(broadcasts []json.RawMessage) bool {
	for _, broadcast := range broadcasts {
		if !validJSON(broadcast) {
			return false
		}
	}
	return true
}

func validJSON(data json.RawMessage) bool {
	return len(data) > 0 && json.Valid(data)
}

func isJSONObject(data []byte) bool {
	fields, ok := objectFields(data)
	return ok && fields != nil
}

func isJSONArray(data []byte) bool {
	trimmed := bytes.TrimSpace(data)
	return len(trimmed) > 0 && trimmed[0] == '[' && json.Valid(trimmed)
}

func isJSONNull(data []byte) bool {
	return bytes.Equal(bytes.TrimSpace(data), []byte("null"))
}

func validOptionsSchema(data []byte) bool {
	if !json.Valid(data) {
		return false
	}
	trimmed := bytes.TrimSpace(data)
	return isJSONObject(trimmed) ||
		bytes.Equal(trimmed, []byte("true")) ||
		bytes.Equal(trimmed, []byte("false"))
}

func objectFields(data []byte) (map[string]json.RawMessage, bool) {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil || fields == nil {
		return nil, false
	}
	return fields, true
}
