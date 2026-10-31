package main

import (
	"context"
	"encoding/json"
	"errors"
	"log"

	"github.com/randseedorg/randseedsdk/go/roomhost"
)

type counterServer struct{}

type counterState struct {
	Count           int64           `json:"count"`
	Players         []string        `json:"players,omitempty"`
	DurationSeconds float64         `json:"durationSeconds,omitempty"`
	Carryover       json.RawMessage `json:"carryover,omitempty"`
}

func (counterServer) GameID() string {
	return "cooperative-counter"
}

func (counterServer) Manifest() roomhost.GameManifest {
	return roomhost.GameManifest{
		Title:           "Cooperative Counter",
		Version:         "1.0.0",
		MinPlayers:      1,
		MaxPlayers:      4,
		TickRateHz:      5,
		PersistentTable: false,
	}
}

func (counterServer) Init(_ context.Context, config roomhost.MatchConfig) (roomhost.InitResult, error) {
	state := counterState{
		Players:   append([]string(nil), config.Players...),
		Carryover: append(json.RawMessage(nil), config.Carryover...),
	}
	if len(config.Carryover) > 0 && string(config.Carryover) != "null" {
		_ = json.Unmarshal(config.Carryover, &state.Count)
	}
	stateJSON, err := encode(state)
	if err != nil {
		return roomhost.InitResult{}, err
	}
	broadcast, err := encode(map[string]any{"type": "started"})
	if err != nil {
		return roomhost.InitResult{}, err
	}
	return roomhost.InitResult{
		State:      stateJSON,
		Broadcasts: []json.RawMessage{broadcast},
	}, nil
}

func (counterServer) Tick(_ context.Context, request roomhost.TickRequest) (roomhost.TickResult, error) {
	state, err := decodeState(request.State)
	if err != nil {
		return roomhost.TickResult{}, err
	}
	previousCount := state.Count
	state.DurationSeconds += request.Ctx.ElapsedSeconds
	for _, input := range request.Ctx.Inputs {
		var data struct {
			Delta *int64 `json:"delta"`
		}
		if err := json.Unmarshal(input.Data, &data); err != nil {
			return roomhost.TickResult{}, err
		}
		delta := int64(1)
		if data.Delta != nil {
			delta = *data.Delta
		}
		state.Count += delta
	}
	stateJSON, err := encode(state)
	if err != nil {
		return roomhost.TickResult{}, err
	}
	result := roomhost.TickResult{
		State:   stateJSON,
		Persist: state.Count != previousCount,
	}
	if state.Count != previousCount {
		broadcast, err := encode(map[string]any{
			"type":  "counter.changed",
			"count": state.Count,
		})
		if err != nil {
			return roomhost.TickResult{}, err
		}
		result.Broadcasts = []json.RawMessage{broadcast}
	}
	return result, nil
}

func (counterServer) OnPlayerJoin(_ context.Context, request roomhost.PlayerEventRequest) (roomhost.PlayerEventResult, error) {
	state, err := decodeState(request.State)
	if err != nil {
		return roomhost.PlayerEventResult{}, err
	}
	found := false
	for _, playerID := range state.Players {
		if playerID == request.PlayerID {
			found = true
			break
		}
	}
	if !found {
		state.Players = append(state.Players, request.PlayerID)
	}
	stateJSON, err := encode(state)
	if err != nil {
		return roomhost.PlayerEventResult{}, err
	}
	broadcast, err := encode(map[string]any{
		"type":     "player_joined",
		"playerId": request.PlayerID,
	})
	if err != nil {
		return roomhost.PlayerEventResult{}, err
	}
	return roomhost.PlayerEventResult{
		State:      stateJSON,
		Broadcasts: []json.RawMessage{broadcast},
	}, nil
}

func (counterServer) OnPlayerLeave(_ context.Context, request roomhost.PlayerEventRequest) (roomhost.PlayerEventResult, error) {
	state, err := decodeState(request.State)
	if err != nil {
		return roomhost.PlayerEventResult{}, err
	}
	remaining := make([]string, 0, len(state.Players))
	for _, playerID := range state.Players {
		if playerID != request.PlayerID {
			remaining = append(remaining, playerID)
		}
	}
	state.Players = remaining
	stateJSON, err := encode(state)
	if err != nil {
		return roomhost.PlayerEventResult{}, err
	}
	broadcast, err := encode(map[string]any{
		"type":     "player_left",
		"playerId": request.PlayerID,
	})
	if err != nil {
		return roomhost.PlayerEventResult{}, err
	}
	return roomhost.PlayerEventResult{
		State:      stateJSON,
		Broadcasts: []json.RawMessage{broadcast},
	}, nil
}

func (counterServer) IsGameOver(_ context.Context, request roomhost.IsGameOverRequest) (roomhost.GameResult, error) {
	state, err := decodeState(request.State)
	if err != nil {
		return roomhost.GameResult{}, err
	}
	if state.Count < 20 {
		return roomhost.GameResult{Over: false}, nil
	}
	summary, err := encode(map[string]any{"durationSeconds": state.DurationSeconds})
	if err != nil {
		return roomhost.GameResult{}, err
	}
	carryover := state.Carryover
	if len(carryover) == 0 {
		carryover, err = encode(state.Count)
		if err != nil {
			return roomhost.GameResult{}, err
		}
	}
	timeout := int64(0)
	return roomhost.GameResult{
		Over:    true,
		Summary: summary,
		NextMatchPolicy: &roomhost.NextMatchPolicy{
			Mode:      roomhost.NextMatchAutoNextMatch,
			TimeoutMS: &timeout,
			Carryover: carryover,
		},
	}, nil
}

func (counterServer) OnCommand(_ context.Context, request roomhost.CommandRequest) (roomhost.CommandResult, error) {
	if request.Command.Type != "reset" {
		return roomhost.CommandResult{}, errors.New("unsupported command")
	}
	state, err := decodeState(request.State)
	if err != nil {
		return roomhost.CommandResult{}, err
	}
	var data struct {
		Count *int64 `json:"count"`
	}
	if err := json.Unmarshal(request.Command.Data, &data); err != nil {
		return roomhost.CommandResult{}, err
	}
	state.Count = 0
	if data.Count != nil {
		state.Count = *data.Count
	}
	stateJSON, err := encode(state)
	if err != nil {
		return roomhost.CommandResult{}, err
	}
	broadcast, err := encode(map[string]any{"type": "counter.reset"})
	if err != nil {
		return roomhost.CommandResult{}, err
	}
	return roomhost.CommandResult{
		State:      stateJSON,
		Broadcasts: []json.RawMessage{broadcast},
	}, nil
}

func decodeState(data json.RawMessage) (counterState, error) {
	var state counterState
	if err := json.Unmarshal(data, &state); err != nil {
		return counterState{}, err
	}
	return state, nil
}

func encode(value any) (json.RawMessage, error) {
	data, err := json.Marshal(value)
	return json.RawMessage(data), err
}

func main() {
	if err := roomhost.ListenAndServeWithOptions(counterServer{}, roomhost.HandlerOptions{
		GameID: "cooperative-counter",
	}); err != nil {
		log.Fatal(err)
	}
}
