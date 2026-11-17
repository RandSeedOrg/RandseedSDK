package roomhost

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"strings"
	"testing"
)

type gamecreatorHostRequests struct {
	TickActive  json.RawMessage `json:"tickActive"`
	Command     json.RawMessage `json:"command"`
	Join        json.RawMessage `json:"join"`
	LeaveJoined json.RawMessage `json:"leaveJoined"`
	LeaveFirst  json.RawMessage `json:"leaveFirst"`
	LeaveLast   json.RawMessage `json:"leaveLast"`
	TickEmpty   json.RawMessage `json:"tickEmpty"`
}

func loadGamecreatorHostRequests(t *testing.T) gamecreatorHostRequests {
	t.Helper()
	data, err := os.ReadFile("../../protocol/fixtures/gamecreator-room-host-private-requests-v1.json")
	if err != nil {
		t.Fatalf("read Gamecreator host request fixture: %v", err)
	}
	var fixture gamecreatorHostRequests
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("decode Gamecreator host request fixture: %v", err)
	}
	return fixture
}

type capturedHostRosterGame struct {
	manifest GameManifestV1
	seen     map[string][][]string
}

func (g *capturedHostRosterGame) ManifestV1() GameManifestV1 { return g.manifest }
func (g *capturedHostRosterGame) record(name string, players []string) {
	var captured []string
	if players != nil {
		captured = append([]string{}, players...)
	}
	g.seen[name] = append(g.seen[name], captured)
}
func (g *capturedHostRosterGame) InitV1(_ context.Context, config MatchConfig) (InitResultV1, error) {
	return InitResultV1{State: json.RawMessage(`{}`), PlayerViews: viewsFor(config.Players)}, nil
}
func (g *capturedHostRosterGame) TickV1(_ context.Context, request TickRequestV1) (TickResultV1, error) {
	g.record("tick", request.Players)
	return TickResultV1{State: json.RawMessage(`{}`), PlayerViews: viewsFor(request.Players)}, nil
}
func (g *capturedHostRosterGame) OnPlayerJoinV1(_ context.Context, request PlayerEventRequestV1) (PlayerEventResultV1, error) {
	g.record("join", request.Players)
	return PlayerEventResultV1{State: json.RawMessage(`{}`), PlayerViews: viewsFor(request.Players)}, nil
}
func (g *capturedHostRosterGame) OnPlayerLeaveV1(_ context.Context, request PlayerEventRequestV1) (PlayerEventResultV1, error) {
	g.record("leave", request.Players)
	return PlayerEventResultV1{State: json.RawMessage(`{}`), PlayerViews: viewsFor(request.Players)}, nil
}
func (g *capturedHostRosterGame) IsGameOverV1(context.Context, IsGameOverRequest) (GameResult, error) {
	return GameResult{Over: false}, nil
}
func (g *capturedHostRosterGame) OnCommandV1(_ context.Context, request CommandRequestV1) (CommandResultV1, error) {
	g.record("command", request.Players)
	return CommandResultV1{State: json.RawMessage(`{}`), PlayerViews: viewsFor(request.Players)}, nil
}

type emptyLeaveViewsGame struct {
	*capturedHostRosterGame
	views map[string]json.RawMessage
}

func (g *emptyLeaveViewsGame) OnPlayerLeaveV1(context.Context, PlayerEventRequestV1) (PlayerEventResultV1, error) {
	return PlayerEventResultV1{State: json.RawMessage(`{}`), PlayerViews: g.views}, nil
}

func TestActualGamecreatorHostSerializedRequestsPassGoPrivateHTTP(t *testing.T) {
	fixture := loadGamecreatorHostRequests(t)
	game := &capturedHostRosterGame{manifest: privateManifest(4), seen: make(map[string][][]string)}
	handler := NewHandlerV1WithOptions(game, HandlerOptions{GameID: "fixture-game_1"})
	requests := []struct {
		route string
		body  json.RawMessage
	}{
		{"/tick", fixture.TickActive},
		{"/onCommand", fixture.Command},
		{"/onPlayerJoin", fixture.Join},
		{"/onPlayerLeave", fixture.LeaveJoined},
		{"/onPlayerLeave", fixture.LeaveFirst},
		{"/onPlayerLeave", fixture.LeaveLast},
		{"/tick", fixture.TickEmpty},
	}
	var lastLeave *httptest.ResponseRecorder
	for index, request := range requests {
		response := serveRequest(t, handler, http.MethodPost, request.route, string(request.body))
		if response.Code != http.StatusOK {
			t.Fatalf("Gamecreator request %d %s status=%d body=%s request=%s", index, request.route, response.Code, response.Body, request.body)
		}
		if index == 5 {
			lastLeave = response
		}
	}
	var response map[string]json.RawMessage
	if err := json.Unmarshal(lastLeave.Body.Bytes(), &response); err != nil {
		t.Fatal(err)
	}
	if raw, present := response["playerViews"]; !present || string(raw) != "{}" {
		t.Fatalf("last-player leave must include playerViews: {}, got %s", lastLeave.Body)
	}
	want := map[string][][]string{
		"tick":    {{"seat-a", "seat-b"}, {}},
		"command": {{"seat-a", "seat-b"}},
		"join":    {{"seat-a", "seat-b", "seat-c"}},
		"leave":   {{"seat-a", "seat-b"}, {"seat-b"}, {}},
	}
	if !reflect.DeepEqual(game.seen, want) {
		t.Fatalf("callback rosters=%#v want=%#v", game.seen, want)
	}
}

func TestGoRejectsMissingOrExtraProjectionForLastPlayerLeave(t *testing.T) {
	fixture := loadGamecreatorHostRequests(t)
	for _, testCase := range []struct {
		name  string
		views map[string]json.RawMessage
	}{
		{name: "missing", views: nil},
		{name: "extra", views: map[string]json.RawMessage{"seat-extra": json.RawMessage(`{}`)}},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			base := &capturedHostRosterGame{manifest: privateManifest(4), seen: make(map[string][][]string)}
			game := &emptyLeaveViewsGame{capturedHostRosterGame: base, views: testCase.views}
			handler := NewHandlerV1WithOptions(game, HandlerOptions{GameID: "fixture-game_1"})
			response := serveRequest(t, handler, http.MethodPost, "/onPlayerLeave", string(fixture.LeaveLast))
			if response.Code != http.StatusInternalServerError {
				t.Fatalf("invalid empty-roster projection status=%d body=%s", response.Code, response.Body)
			}
		})
	}
}

type mutatingRosterGame struct {
	*capturedHostRosterGame
}

func mutatedViews(players []string) map[string]json.RawMessage {
	players[0] = "callback-forged-seat"
	return viewsFor(players)
}

func (g *mutatingRosterGame) InitV1(_ context.Context, config MatchConfig) (InitResultV1, error) {
	return InitResultV1{State: json.RawMessage(`{}`), PlayerViews: mutatedViews(config.Players)}, nil
}
func (g *mutatingRosterGame) TickV1(_ context.Context, request TickRequestV1) (TickResultV1, error) {
	return TickResultV1{State: json.RawMessage(`{}`), PlayerViews: mutatedViews(request.Players)}, nil
}
func (g *mutatingRosterGame) OnPlayerJoinV1(_ context.Context, request PlayerEventRequestV1) (PlayerEventResultV1, error) {
	return PlayerEventResultV1{State: json.RawMessage(`{}`), PlayerViews: mutatedViews(request.Players)}, nil
}
func (g *mutatingRosterGame) OnPlayerLeaveV1(_ context.Context, request PlayerEventRequestV1) (PlayerEventResultV1, error) {
	return PlayerEventResultV1{State: json.RawMessage(`{}`), PlayerViews: mutatedViews(request.Players)}, nil
}
func (g *mutatingRosterGame) OnCommandV1(_ context.Context, request CommandRequestV1) (CommandResultV1, error) {
	return CommandResultV1{State: json.RawMessage(`{}`), PlayerViews: mutatedViews(request.Players)}, nil
}

func TestCallbacksCannotRewriteRosterUsedForProjectionValidation(t *testing.T) {
	fixture := loadPrivateFixture(t)
	base := &capturedHostRosterGame{manifest: privateManifest(4), seen: make(map[string][][]string)}
	handler := NewHandlerV1WithOptions(&mutatingRosterGame{capturedHostRosterGame: base}, HandlerOptions{GameID: "fixture-game_1"})
	config := MatchConfig{MatchID: "m1", GameID: "fixture-game_1", Players: []string{"seat-a", "seat-b"}, MatchSeq: 1}
	bodies := []struct {
		route string
		body  any
	}{
		{"/init", config},
		{"/tick", map[string]any{"ctx": map[string]any{"tick": 1, "inputs": []any{}, "elapsedSeconds": 0.1}, "state": map[string]any{}, "players": []string{"seat-a", "seat-b"}}},
		{"/onPlayerJoin", PlayerEventRequestV1{PlayerID: "seat-b", State: json.RawMessage(`{}`), Players: []string{"seat-a", "seat-b"}}},
		{"/onPlayerLeave", PlayerEventRequestV1{PlayerID: "seat-b", State: json.RawMessage(`{}`), Players: []string{"seat-a"}}},
		{"/onCommand", CommandRequestV1{Command: Command{Type: "reset", Data: json.RawMessage(`{}`)}, State: json.RawMessage(`{}`), Players: []string{"seat-a", "seat-b"}}},
	}
	for _, testCase := range bodies {
		t.Run(testCase.route, func(t *testing.T) {
			response := serveRequest(t, handler, http.MethodPost, testCase.route, testCase.body)
			if response.Code != http.StatusInternalServerError {
				t.Fatalf("mutated projection roster was accepted: status=%d body=%s", response.Code, response.Body)
			}
			if strings.Contains(response.Body.String(), "callback-forged-seat") {
				t.Fatalf("callback details escaped: %s", response.Body)
			}
		})
	}
	_ = fixture
}

func TestGoProtocolMarshalPreservesAbsentAndExplicitEmptyValues(t *testing.T) {
	cases := []struct {
		name  string
		value any
		field string
		want  string
	}{
		{"absent tick roster", TickRequest{Ctx: TickContext{}, State: json.RawMessage(`{}`)}, "players", ""},
		{"empty tick roster", TickRequestV1{Ctx: TickContext{}, State: json.RawMessage(`{}`), Players: []string{}}, "players", "[]"},
		{"absent event roster", PlayerEventRequestV1{PlayerID: "seat-a", State: json.RawMessage(`{}`)}, "players", ""},
		{"empty command roster", CommandRequestV1{State: json.RawMessage(`{}`), Players: []string{}}, "players", "[]"},
		{"absent views", PlayerEventResultV1{State: json.RawMessage(`{}`)}, "playerViews", ""},
		{"empty views", PlayerEventResultV1{State: json.RawMessage(`{}`), PlayerViews: map[string]json.RawMessage{}}, "playerViews", "{}"},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			encoded, err := json.Marshal(testCase.value)
			if err != nil {
				t.Fatal(err)
			}
			var fields map[string]json.RawMessage
			if err := json.Unmarshal(encoded, &fields); err != nil {
				t.Fatal(err)
			}
			if testCase.want == "" {
				if _, exists := fields[testCase.field]; exists {
					t.Fatalf("field unexpectedly present: %s", encoded)
				}
			} else if string(fields[testCase.field]) != testCase.want {
				t.Fatalf("field=%s want=%s wire=%s", fields[testCase.field], testCase.want, encoded)
			}
		})
	}
}

type publicEmptyRosterGame struct {
	*capturedHostRosterGame
}

func TestPublicLifecycleDistinguishesExplicitEmptyMissingAndNullRosters(t *testing.T) {
	base := &capturedHostRosterGame{
		manifest: GameManifestV1{GameManifest: GameManifest{Title: "Public test", Version: "1.0.0", MinPlayers: 1, MaxPlayers: 4, TickRateHz: 10}},
		seen:     make(map[string][][]string),
	}
	handler := NewHandlerV1WithOptions(&publicEmptyRosterGame{capturedHostRosterGame: base}, HandlerOptions{GameID: "fixture-game_1"})
	request := `{"ctx":{"tick":1,"inputs":[],"commands":[],"elapsedSeconds":0.1},"state":{}}`
	for _, body := range []struct {
		name string
		json string
		want int
	}{
		{"empty", `{"ctx":{"tick":1,"inputs":[],"commands":[],"elapsedSeconds":0.1},"state":{},"players":[]}`, http.StatusOK},
		{"missing", request, http.StatusOK},
		{"null", `{"ctx":{"tick":1,"inputs":[],"commands":[],"elapsedSeconds":0.1},"state":{},"players":null}`, http.StatusBadRequest},
	} {
		t.Run(body.name, func(t *testing.T) {
			response := serveRequest(t, handler, http.MethodPost, "/tick", body.json)
			if response.Code != body.want {
				t.Fatalf("status=%d want=%d body=%s", response.Code, body.want, response.Body)
			}
		})
	}
	if len(base.seen["tick"]) != 2 || base.seen["tick"][0] == nil || base.seen["tick"][1] != nil {
		t.Fatalf("callbacks did not preserve empty vs missing roster: %#v", base.seen["tick"])
	}
}
