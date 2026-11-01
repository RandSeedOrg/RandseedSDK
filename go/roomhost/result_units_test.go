package roomhost

import (
	"encoding/json"
	"os"
	"reflect"
	"testing"
)

type resultFixture struct {
	Definition json.RawMessage `json:"definition"`
	Event      json.RawMessage `json:"event"`
}

func loadResultFixture(t *testing.T) (GameResultDefinition, GameResultEvent) {
	t.Helper()
	data, err := os.ReadFile("../../protocol/fixtures/game-result-event-v1.json")
	if err != nil {
		t.Fatalf("read Result Unit fixture: %v", err)
	}
	var fixture resultFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("decode Result Unit fixture: %v", err)
	}
	var definition GameResultDefinition
	if err := json.Unmarshal(fixture.Definition, &definition); err != nil {
		t.Fatalf("decode result definition: %v", err)
	}
	var event GameResultEvent
	if err := json.Unmarshal(fixture.Event, &event); err != nil {
		t.Fatalf("decode result event: %v", err)
	}
	return definition, event
}

func TestGameResultUnitV1MatchesSharedFixture(t *testing.T) {
	definition, event := loadResultFixture(t)
	if err := definition.Validate(); err != nil {
		t.Fatalf("validate result definition: %v", err)
	}
	if err := event.Validate(&definition); err != nil {
		t.Fatalf("validate result event: %v", err)
	}
	if err := ValidateGameResultUnits([]GameResultEvent{event}, &definition); err != nil {
		t.Fatalf("validate result event batch: %v", err)
	}
	encoded, err := json.Marshal(event)
	if err != nil {
		t.Fatalf("encode event: %v", err)
	}
	var want, got any
	fixture, _ := os.ReadFile("../../protocol/fixtures/game-result-event-v1.json")
	var wrapper map[string]json.RawMessage
	_ = json.Unmarshal(fixture, &wrapper)
	_ = json.Unmarshal(wrapper["event"], &want)
	_ = json.Unmarshal(encoded, &got)
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("event round trip differs from shared fixture")
	}
}

func TestGameResultUnitCorrectionsAndInvalidFields(t *testing.T) {
	definition, event := loadResultFixture(t)
	replacement := event
	replacement.EventID = "result-event-replacement"
	replacement.Correction = &GameResultCorrection{
		Action: "replace", SupersedesEventID: event.EventID,
	}
	if err := replacement.Validate(&definition); err != nil {
		t.Fatalf("valid replacement rejected: %v", err)
	}

	retraction := event
	retraction.EventID = "result-event-retraction"
	retraction.Participants = []GameResultParticipant{}
	retraction.Teams = []GameResultTeam{}
	retraction.Correction = &GameResultCorrection{
		Action: "retract", SupersedesEventID: event.EventID,
	}
	if err := retraction.Validate(&definition); err != nil {
		t.Fatalf("valid retraction rejected: %v", err)
	}
	retraction.Participants = event.Participants
	if err := retraction.Validate(&definition); err == nil {
		t.Fatal("retraction with participant data was accepted")
	}

	event.Participants[0].Metrics["winRate"] = 1
	if err := event.Validate(&definition); err == nil {
		t.Fatal("derived ratio metric was accepted as an event value")
	}
}

func TestGameResultUnitLimitsAndHostIdentityAreEnforced(t *testing.T) {
	definition, event := loadResultFixture(t)
	for index := 0; index < 32; index++ {
		event.EventID = "event-" + string(rune('a'+index))
		event.UnitID = "unit-" + string(rune('a'+index))
		if err := event.Validate(&definition); err != nil {
			t.Fatalf("event %d rejected: %v", index, err)
		}
	}
	events := make([]GameResultEvent, 33)
	for index := range events {
		events[index] = event
		events[index].EventID = "event-" + string(rune('a'+index))
		events[index].UnitID = "unit-" + string(rune('a'+index))
	}
	if err := ValidateGameResultUnits(events, &definition); err == nil {
		t.Fatal("more than 32 events in one transition were accepted")
	}

	raw, err := os.ReadFile("../../protocol/fixtures/game-result-event-v1.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture map[string]json.RawMessage
	_ = json.Unmarshal(raw, &fixture)
	var eventObject map[string]json.RawMessage
	_ = json.Unmarshal(fixture["event"], &eventObject)
	eventObject["gameId"] = json.RawMessage(`"forged-game"`)
	forged, _ := json.Marshal(eventObject)
	if err := json.Unmarshal(forged, &event); err == nil {
		t.Fatal("host-bound gameId field was accepted")
	}
}

func TestLifecycleResultUnitsAreValidated(t *testing.T) {
	definition, event := loadResultFixture(t)
	if !validInitResult(InitResult{
		State: json.RawMessage(`{}`), ResultUnits: []GameResultEvent{event},
	}) {
		t.Fatal("valid result unit was rejected from init response")
	}
	event.Participants[0].Rank = new(int64)
	if validTickResult(TickResult{
		State: json.RawMessage(`{}`), ResultUnits: []GameResultEvent{event},
	}) {
		t.Fatal("invalid result unit was accepted from tick response")
	}
	if err := definition.Validate(); err != nil {
		t.Fatal(err)
	}
}
