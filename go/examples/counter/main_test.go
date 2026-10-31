package main

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/randseedorg/randseedsdk/go/roomhost"
)

func TestDurationUsesCumulativeElapsedSeconds(t *testing.T) {
	server := counterServer{}
	state, err := encode(counterState{Count: 19})
	if err != nil {
		t.Fatal(err)
	}

	for tick, elapsedSeconds := range []float64{0.2, 0.4, 0.6} {
		result, err := server.Tick(context.Background(), roomhost.TickRequest{
			Ctx: roomhost.TickContext{
				Tick:           int64(tick + 1),
				Inputs:         []roomhost.PlayerInput{{PlayerID: "player-a", Data: json.RawMessage(`{"delta":1}`)}},
				ElapsedSeconds: elapsedSeconds,
			},
			State: state,
		})
		if err != nil {
			t.Fatal(err)
		}

		next, err := decodeState(result.State)
		if err != nil {
			t.Fatal(err)
		}
		if next.DurationSeconds != elapsedSeconds {
			t.Fatalf("tick %d duration = %v, want %v", tick+1, next.DurationSeconds, elapsedSeconds)
		}
		state = result.State
	}
}

func TestGameOverIncludesRequiredZeroTimeout(t *testing.T) {
	state, err := encode(counterState{Count: 20, DurationSeconds: 4})
	if err != nil {
		t.Fatal(err)
	}
	result, err := (counterServer{}).IsGameOver(context.Background(), roomhost.IsGameOverRequest{State: state})
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(result)
	if err != nil {
		t.Fatal(err)
	}
	var response map[string]json.RawMessage
	if err := json.Unmarshal(encoded, &response); err != nil {
		t.Fatal(err)
	}
	var policy map[string]json.RawMessage
	if err := json.Unmarshal(response["nextMatchPolicy"], &policy); err != nil {
		t.Fatal(err)
	}
	timeout, ok := policy["timeoutMs"]
	if !ok {
		t.Fatal("zero timeoutMs must be serialized")
	}
	var timeoutMs int64
	if err := json.Unmarshal(timeout, &timeoutMs); err != nil {
		t.Fatal(err)
	}
	if timeoutMs != 0 {
		t.Fatalf("timeoutMs = %d, want 0", timeoutMs)
	}
}
