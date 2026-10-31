package roomhost

import (
	"bytes"
	"encoding/json"
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
	return len(manifest.OptionsSchema) == 0 || validOptionsSchema(manifest.OptionsSchema)
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
	return validJSON(result.State) && validBroadcasts(result.Broadcasts)
}

func validTickResult(result TickResult) bool {
	return validJSON(result.State) && validBroadcasts(result.Broadcasts)
}

func validPlayerEventResult(result PlayerEventResult) bool {
	return validJSON(result.State) && validBroadcasts(result.Broadcasts)
}

func validCommandResult(result CommandResult) bool {
	return validJSON(result.State) && validBroadcasts(result.Broadcasts)
}

func validGameResult(result GameResult) bool {
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
