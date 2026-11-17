package roomhost

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"strings"
	"testing"
)

type privateFixture struct {
	ManifestCases []struct {
		Name     string          `json:"name"`
		Valid    bool            `json:"valid"`
		Manifest json.RawMessage `json:"manifest"`
	} `json:"manifestCases"`
	Players            []string                              `json:"players"`
	PlayerViews        map[string]json.RawMessage            `json:"playerViews"`
	InvalidPlayerViews map[string]map[string]json.RawMessage `json:"invalidPlayerViews"`
}

func loadPrivateFixture(t *testing.T) privateFixture {
	t.Helper()
	data, err := os.ReadFile("../../protocol/fixtures/room-host-private-v1.json")
	if err != nil {
		t.Fatalf("read private-view fixture: %v", err)
	}
	var fixture privateFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("decode private-view fixture: %v", err)
	}
	return fixture
}

func TestHealthRequiresPlatformAssignedGameID(t *testing.T) {
	fixture := loadFixture(t)
	for name, handler := range map[string]http.Handler{
		"missing": NewHandler(fixtureServer{fixture: fixture}),
		"invalid": NewHandlerWithOptions(fixtureServer{fixture: fixture}, HandlerOptions{GameID: "invalid game id"}),
	} {
		t.Run(name, func(t *testing.T) {
			response := serveRequest(t, handler, http.MethodGet, "/health", nil)
			if response.Code != http.StatusServiceUnavailable {
				t.Fatalf("health status=%d body=%s", response.Code, response.Body)
			}
		})
	}
}

func TestManifestOptionsSchemaAcceptsOnlyJSONSchemaObjectOrBoolean(t *testing.T) {
	base := `{"title":"Options","version":"1.0.0","minPlayers":1,"maxPlayers":2,"tickRateHz":5,"persistentTable":false}`
	for _, schema := range []string{
		`{"type":"object","properties":{"mode":{"type":"string"}}}`,
		`true`,
		`false`,
	} {
		var manifest GameManifest
		data := strings.TrimSuffix(base, "}") + `,"optionsSchema":` + schema + `}`
		if err := json.Unmarshal([]byte(data), &manifest); err != nil || !validManifest(manifest) {
			t.Fatalf("valid optionsSchema rejected (%s): err=%v", schema, err)
		}
	}
	for _, schema := range []string{`null`, `[]`, `"object"`, `1`} {
		var manifest GameManifest
		data := strings.TrimSuffix(base, "}") + `,"optionsSchema":` + schema + `}`
		if err := json.Unmarshal([]byte(data), &manifest); err == nil && validManifest(manifest) {
			t.Errorf("invalid optionsSchema accepted: %s", schema)
		}
	}
}

func TestPrivateManifestMatchesSharedVectors(t *testing.T) {
	fixture := loadPrivateFixture(t)
	for _, testCase := range fixture.ManifestCases {
		t.Run(testCase.Name, func(t *testing.T) {
			var manifest GameManifestV1
			err := json.Unmarshal(testCase.Manifest, &manifest)
			if testCase.Valid {
				if err != nil || !validManifestV1(manifest) {
					t.Fatalf("valid manifest rejected: err=%v manifest=%+v", err, manifest)
				}
				if manifest.PlayerViewMode == PlayerViewPrivate && manifest.PlayerViewSchemaVersion != nil && *manifest.PlayerViewSchemaVersion != 1 {
					t.Fatalf("unexpected explicit schema version: %+v", manifest.PlayerViewSchemaVersion)
				}
			} else if err == nil && validManifestV1(manifest) {
				t.Fatalf("invalid manifest accepted: %+v", manifest)
			}
		})
	}
}

func TestGameManifestV1NumericSchemaVersionRepresentations(t *testing.T) {
	for _, representation := range []string{"1", "1.0", "1e0"} {
		data := `{"title":"v1","version":"1","minPlayers":1,"maxPlayers":2,"tickRateHz":5,"persistentTable":false,"playerViewMode":"private","playerViewSchemaVersion":` + representation + `}`
		var manifest GameManifestV1
		if err := json.Unmarshal([]byte(data), &manifest); err != nil {
			t.Fatalf("numeric representation %s rejected: %v", representation, err)
		}
		if !validManifestV1(manifest) || manifest.PlayerViewSchemaVersion == nil || *manifest.PlayerViewSchemaVersion != 1 {
			t.Fatalf("numeric representation %s was not normalized to 1: %+v", representation, manifest)
		}
	}
	for _, declaration := range []string{`null`, `2`, `1.5`} {
		data := `{"title":"v1","version":"1","minPlayers":1,"maxPlayers":2,"tickRateHz":5,"persistentTable":false,"playerViewMode":"private","playerViewSchemaVersion":` + declaration + `}`
		var manifest GameManifestV1
		if err := json.Unmarshal([]byte(data), &manifest); err == nil {
			t.Errorf("invalid version declaration %s was accepted", declaration)
		}
	}
}

type privateViewGame struct {
	manifest GameManifestV1
	views    map[string]json.RawMessage
}

func (g privateViewGame) ManifestV1() GameManifestV1 { return g.manifest }
func (g privateViewGame) InitV1(context.Context, MatchConfig) (InitResultV1, error) {
	return InitResultV1{State: json.RawMessage(`{"canonical":{"seat-a":"SECRET-seat-a","seat-b":"SECRET-seat-b"}}`), PlayerViews: g.views}, nil
}
func (g privateViewGame) TickV1(_ context.Context, request TickRequestV1) (TickResultV1, error) {
	return TickResultV1{State: json.RawMessage(`{"tick":1}`), PlayerViews: g.views, Persist: false}, nil
}
func (g privateViewGame) OnPlayerJoinV1(_ context.Context, request PlayerEventRequestV1) (PlayerEventResultV1, error) {
	return PlayerEventResultV1{State: json.RawMessage(`{"joined":true}`), PlayerViews: viewsFor(request.Players)}, nil
}
func (g privateViewGame) OnPlayerLeaveV1(_ context.Context, request PlayerEventRequestV1) (PlayerEventResultV1, error) {
	return PlayerEventResultV1{State: json.RawMessage(`{"left":true}`), PlayerViews: viewsFor(request.Players)}, nil
}
func (g privateViewGame) IsGameOverV1(context.Context, IsGameOverRequest) (GameResult, error) {
	return GameResult{Over: false}, nil
}
func (g privateViewGame) OnCommandV1(_ context.Context, request CommandRequestV1) (CommandResultV1, error) {
	return CommandResultV1{State: json.RawMessage(`{"command":true}`), PlayerViews: viewsFor(request.Players)}, nil
}

func viewsFor(players []string) map[string]json.RawMessage {
	if players == nil {
		return nil
	}
	views := make(map[string]json.RawMessage, len(players))
	for _, playerID := range players {
		data, _ := json.Marshal(map[string]string{"viewer": playerID, "secret": "SECRET-" + playerID})
		views[playerID] = data
	}
	return views
}

func privateManifest(maxPlayers int) GameManifestV1 {
	return GameManifestV1{GameManifest: GameManifest{Title: "Private fixture", Version: "1.0.0", MinPlayers: 1, MaxPlayers: maxPlayers, TickRateHz: 5}, PlayerViewMode: PlayerViewPrivate}
}

func privateHandler(t *testing.T, game privateViewGame) http.Handler {
	t.Helper()
	return NewHandlerV1WithOptions(game, HandlerOptions{GameID: "fixture-game_1"})
}

func TestGoHTTPManifestEmitsExplicitPrivateV1AndPublicOmission(t *testing.T) {
	fixture := loadPrivateFixture(t)
	version := 1
	private := privateManifest(4)
	private.PlayerViewSchemaVersion = &version
	response := serveRequest(t, privateHandler(t, privateViewGame{manifest: private, views: fixture.PlayerViews}), http.MethodGet, "/manifest", nil)
	if response.Code != http.StatusOK {
		t.Fatalf("explicit private manifest status=%d body=%s", response.Code, response.Body)
	}
	var privateWire map[string]json.RawMessage
	if err := json.Unmarshal(response.Body.Bytes(), &privateWire); err != nil {
		t.Fatal(err)
	}
	if string(privateWire["playerViewSchemaVersion"]) != "1" {
		t.Fatalf("explicit v1 missing from manifest: %s", response.Body)
	}

	public := GameManifestV1{GameManifest: GameManifest{Title: "Public", Version: "1.0.0", MinPlayers: 1, MaxPlayers: 2, TickRateHz: 5}}
	publicResponse := serveRequest(t, privateHandler(t, privateViewGame{manifest: public}), http.MethodGet, "/manifest", nil)
	if publicResponse.Code != http.StatusOK {
		t.Fatalf("public manifest status=%d body=%s", publicResponse.Code, publicResponse.Body)
	}
	var publicWire map[string]json.RawMessage
	if err := json.Unmarshal(publicResponse.Body.Bytes(), &publicWire); err != nil {
		t.Fatal(err)
	}
	if _, present := publicWire["playerViewMode"]; present {
		t.Fatalf("public manifest unexpectedly includes private mode: %s", publicResponse.Body)
	}
}

func TestPrivateLifecycleHTTPAndRecipientProjections(t *testing.T) {
	fixture := loadPrivateFixture(t)
	game := privateViewGame{manifest: privateManifest(4), views: fixture.PlayerViews}
	handler := privateHandler(t, game)
	manifestResponse := serveRequest(t, handler, http.MethodGet, "/manifest", nil)
	if manifestResponse.Code != http.StatusOK {
		t.Fatalf("manifest status=%d body=%s", manifestResponse.Code, manifestResponse.Body)
	}
	var manifestJSON map[string]json.RawMessage
	if err := json.Unmarshal(manifestResponse.Body.Bytes(), &manifestJSON); err != nil {
		t.Fatal(err)
	}
	if string(manifestJSON["playerViewMode"]) != `"private"` || len(manifestJSON["playerViewSchemaVersion"]) != 0 {
		t.Fatalf("unexpected omitted-v1 manifest wire: %s", manifestResponse.Body)
	}

	config := MatchConfig{MatchID: "match-private-v1", GameID: "fixture-game_1", Players: fixture.Players, MatchSeq: 1}
	initResponse := serveRequest(t, handler, http.MethodPost, "/init", config)
	assertPrivateViewsResponse(t, initResponse, fixture.Players)
	var initBody struct {
		PlayerViews map[string]json.RawMessage `json:"playerViews"`
	}
	if err := json.Unmarshal(initResponse.Body.Bytes(), &initBody); err != nil {
		t.Fatal(err)
	}
	for playerID, expectedRaw := range fixture.PlayerViews {
		var actual, expected any
		if err := json.Unmarshal(initBody.PlayerViews[playerID], &actual); err != nil {
			t.Fatal(err)
		}
		if err := json.Unmarshal(expectedRaw, &expected); err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(actual, expected) {
			t.Fatalf("%s projection=%v want shared fixture %v", playerID, actual, expected)
		}
	}
	if strings.Contains(string(initBody.PlayerViews["seat-a"]), "KING_OF_CLUBS") {
		t.Fatalf("seat-a projection included another seats secret: %s", initBody.PlayerViews["seat-a"])
	}

	tickBody := map[string]any{"ctx": map[string]any{"tick": 1, "inputs": []any{}, "commands": []any{}, "elapsedSeconds": 0.2}, "state": map[string]any{"round": 1}, "players": fixture.Players}
	assertPrivateViewsResponse(t, serveRequest(t, handler, http.MethodPost, "/tick", tickBody), fixture.Players)
	joinPlayers := []string{"seat-a", "seat-b", "seat-c"}
	assertPrivateViewsResponse(t, serveRequest(t, handler, http.MethodPost, "/onPlayerJoin", PlayerEventRequestV1{PlayerID: "seat-c", State: json.RawMessage(`{"round":1}`), Players: joinPlayers}), joinPlayers)
	leavePlayers := []string{"seat-a"}
	assertPrivateViewsResponse(t, serveRequest(t, handler, http.MethodPost, "/onPlayerLeave", PlayerEventRequestV1{PlayerID: "seat-b", State: json.RawMessage(`{"round":1}`), Players: leavePlayers}), leavePlayers)
	commandBody := map[string]any{"command": map[string]any{"type": "reset", "data": map[string]any{}}, "state": map[string]any{"round": 1}, "players": fixture.Players}
	assertPrivateViewsResponse(t, serveRequest(t, handler, http.MethodPost, "/onCommand", commandBody), fixture.Players)
}

func mustJSON(t *testing.T, value any) []byte {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func assertPrivateViewsResponse(t *testing.T, response *httptest.ResponseRecorder, players []string) {
	t.Helper()
	if response.Code != http.StatusOK {
		t.Fatalf("private lifecycle status=%d body=%s", response.Code, response.Body)
	}
	var body struct {
		PlayerViews map[string]json.RawMessage `json:"playerViews"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if len(body.PlayerViews) != len(players) {
		t.Fatalf("view count=%d want=%d", len(body.PlayerViews), len(players))
	}
	for _, playerID := range players {
		if _, ok := body.PlayerViews[playerID]; !ok {
			t.Fatalf("missing view for %s", playerID)
		}
	}
}

func TestPrivateLifecycleRequestsFailClosed(t *testing.T) {
	players := []string{"seat-a", "seat-b"}
	handler := privateHandler(t, privateViewGame{manifest: privateManifest(4), views: viewsFor(players)})
	missingRoster := map[string]any{"ctx": map[string]any{"tick": 1, "inputs": []any{}, "commands": []any{}, "elapsedSeconds": 0.2}, "state": map[string]any{}}
	for _, body := range []any{missingRoster, map[string]any{"ctx": map[string]any{"tick": 1, "inputs": []any{}, "commands": []any{}, "elapsedSeconds": 0.2}, "state": map[string]any{}, "players": nil}} {
		response := serveRequest(t, handler, http.MethodPost, "/tick", body)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("missing/null roster status=%d body=%s", response.Code, response.Body)
		}
	}
	for _, pathAndBody := range []struct {
		path string
		body any
	}{
		{"/onPlayerJoin", map[string]any{"playerId": "seat-c", "state": map[string]any{}}},
		{"/onPlayerLeave", map[string]any{"playerId": "seat-b", "state": map[string]any{}, "players": players}},
		{"/onPlayerJoin", map[string]any{"playerId": "seat-c", "state": map[string]any{}, "players": players}},
		{"/onCommand", map[string]any{"command": map[string]any{"type": "reset", "data": map[string]any{}}, "state": map[string]any{}}},
	} {
		response := serveRequest(t, handler, http.MethodPost, pathAndBody.path, pathAndBody.body)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("%s bad roster status=%d body=%s", pathAndBody.path, response.Code, response.Body)
		}
	}
}

func TestPrivateLifecycleRejectsMissingMalformedAndOversizedViews(t *testing.T) {
	players := []string{"seat-a", "seat-b"}
	baseConfig := MatchConfig{MatchID: "m1", GameID: "fixture-game_1", Players: players, MatchSeq: 1}
	cases := []struct {
		name  string
		views map[string]json.RawMessage
		max   int
	}{
		{"missing", map[string]json.RawMessage{"seat-a": json.RawMessage(`{"viewer":"seat-a"}`)}, 4},
		{"extra", map[string]json.RawMessage{"seat-a": json.RawMessage(`{}`), "seat-b": json.RawMessage(`{}`), "seat-c": json.RawMessage(`{}`)}, 4},
		{"malformed-json", map[string]json.RawMessage{"seat-a": json.RawMessage(`{`), "seat-b": json.RawMessage(`{}`)}, 4},
		{"oversized-view", map[string]json.RawMessage{"seat-a": mustJSON(t, strings.Repeat("x", maxPlayerViewBytes)), "seat-b": json.RawMessage(`{}`)}, 4},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			handler := privateHandler(t, privateViewGame{manifest: privateManifest(testCase.max), views: testCase.views})
			response := serveRequest(t, handler, http.MethodPost, "/init", baseConfig)
			if response.Code != http.StatusInternalServerError {
				t.Fatalf("status=%d body=%s", response.Code, response.Body)
			}
		})
	}
	manyPlayers := make([]string, 17)
	manyViews := make(map[string]json.RawMessage, len(manyPlayers))
	for i := range manyPlayers {
		manyPlayers[i] = fmt.Sprintf("seat-%02d", i)
		manyViews[manyPlayers[i]] = mustJSON(t, strings.Repeat("x", 62000))
	}
	largeConfig := MatchConfig{MatchID: "m2", GameID: "fixture-game_1", Players: manyPlayers, MatchSeq: 1}
	largeResponse := serveRequest(t, privateHandler(t, privateViewGame{manifest: privateManifest(20), views: manyViews}), http.MethodPost, "/init", largeConfig)
	if largeResponse.Code != http.StatusInternalServerError {
		t.Fatalf("oversized map status=%d", largeResponse.Code)
	}
}

func TestPublicLegacyGoLifecycleRemainsUnchanged(t *testing.T) {
	manifest := GameManifestV1{GameManifest: GameManifest{Title: "Public", Version: "1", MinPlayers: 1, MaxPlayers: 2, TickRateHz: 5}}
	server := privateViewGame{manifest: manifest, views: nil}
	handler := privateHandler(t, server)
	config := MatchConfig{MatchID: "m1", GameID: "fixture-game_1", Players: []string{"seat-a"}, MatchSeq: 1}
	if response := serveRequest(t, handler, http.MethodPost, "/init", config); response.Code != http.StatusOK {
		t.Fatalf("public init status=%d body=%s", response.Code, response.Body)
	}
	if response := serveRequest(t, handler, http.MethodPost, "/tick", map[string]any{"ctx": map[string]any{"tick": 1, "inputs": []any{}, "commands": []any{}, "elapsedSeconds": 0.2}, "state": map[string]any{}}); response.Code != http.StatusOK {
		t.Fatalf("public tick status=%d body=%s", response.Code, response.Body)
	}
}
