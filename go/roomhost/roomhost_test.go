package roomhost

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"strings"
	"testing"
)

type fixtureDocument struct {
	ProtocolVersion     int                       `json:"protocolVersion"`
	Manifest            GameManifest              `json:"manifest"`
	Config              MatchConfig               `json:"config"`
	InitResponse        InitResult                `json:"initResponse"`
	TickRequest         TickRequest               `json:"tickRequest"`
	TickResponse        TickResult                `json:"tickResponse"`
	PlayerEventRequest  PlayerEventRequest        `json:"playerEventRequest"`
	PlayerEventResponse PlayerEventResult         `json:"playerEventResponse"`
	IsGameOverRequest   IsGameOverRequest         `json:"isGameOverRequest"`
	GameOverResponse    GameResult                `json:"gameOverResponse"`
	CommandRequest      CommandRequest            `json:"commandRequest"`
	CommandResponse     CommandResult             `json:"commandResponse"`
	PRNGVectors         []prngFixtureVector       `json:"prngVectors"`
	ChaCha20Vectors     []chacha20FixtureVector   `json:"chacha20Vectors"`
	DeriveSeedVectors   []deriveSeedFixtureVector `json:"deriveSeedVectors"`
}

type prngFixtureVector struct {
	Seed int64    `json:"seed"`
	U32  []uint32 `json:"u32"`
}

type chacha20FixtureVector struct {
	SeedHex    string   `json:"seedHex"`
	Stream     uint64   `json:"stream"`
	WordPos    uint64   `json:"wordPos"`
	U32        []uint32 `json:"u32"`
	Bounded100 []uint32 `json:"bounded100"`
}

type deriveSeedFixtureVector struct {
	RootSeedHex    string   `json:"rootSeedHex"`
	Domains        []string `json:"domains"`
	DerivedSeedHex string   `json:"derivedSeedHex"`
	FirstU32       uint32   `json:"firstU32"`
}

type fixtureServer struct {
	fixture fixtureDocument
}

func loadFixture(t *testing.T) fixtureDocument {
	t.Helper()
	data, err := os.ReadFile("../../protocol/fixtures/room-host-v1.json")
	if err != nil {
		t.Fatalf("read shared protocol fixture: %v", err)
	}
	var fixture fixtureDocument
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("decode shared protocol fixture: %v", err)
	}
	return fixture
}

func (s fixtureServer) Manifest() GameManifest {
	return s.fixture.Manifest
}

func (s fixtureServer) Init(_ context.Context, got MatchConfig) (InitResult, error) {
	if err := s.assertRequest("init", got, s.fixture.Config); err != nil {
		return InitResult{}, err
	}
	return s.fixture.InitResponse, nil
}

func (s fixtureServer) Tick(_ context.Context, got TickRequest) (TickResult, error) {
	if err := s.assertRequest("tick", got, s.fixture.TickRequest); err != nil {
		return TickResult{}, err
	}
	return s.fixture.TickResponse, nil
}

func (s fixtureServer) OnPlayerJoin(_ context.Context, got PlayerEventRequest) (PlayerEventResult, error) {
	if err := s.assertRequest("player join", got, s.fixture.PlayerEventRequest); err != nil {
		return PlayerEventResult{}, err
	}
	return s.fixture.PlayerEventResponse, nil
}

func (s fixtureServer) OnPlayerLeave(_ context.Context, got PlayerEventRequest) (PlayerEventResult, error) {
	if err := s.assertRequest("player leave", got, s.fixture.PlayerEventRequest); err != nil {
		return PlayerEventResult{}, err
	}
	return s.fixture.PlayerEventResponse, nil
}

func (s fixtureServer) IsGameOver(_ context.Context, got IsGameOverRequest) (GameResult, error) {
	if err := s.assertRequest("game over", got, s.fixture.IsGameOverRequest); err != nil {
		return GameResult{}, err
	}
	return s.fixture.GameOverResponse, nil
}

func (s fixtureServer) OnCommand(_ context.Context, got CommandRequest) (CommandResult, error) {
	if err := s.assertRequest("command", got, s.fixture.CommandRequest); err != nil {
		return CommandResult{}, err
	}
	return s.fixture.CommandResponse, nil
}

func (s fixtureServer) assertRequest(name string, got, want any) error {
	if !jsonEquivalent(got, want) {
		return fmt.Errorf("%s request did not match the shared fixture", name)
	}
	return nil
}

func TestHandlerRoutesMatchSharedFixture(t *testing.T) {
	fixture := loadFixture(t)
	server := fixtureServer{fixture: fixture}
	handler := NewHandlerWithOptions(server, HandlerOptions{GameID: fixture.Config.GameID})

	cases := []struct {
		name   string
		method string
		path   string
		body   any
		want   any
	}{
		{"manifest", http.MethodGet, "/manifest", nil, fixture.Manifest},
		{"init", http.MethodPost, "/init", fixture.Config, fixture.InitResponse},
		{"tick", http.MethodPost, "/tick", fixture.TickRequest, fixture.TickResponse},
		{"player join", http.MethodPost, "/onPlayerJoin", fixture.PlayerEventRequest, fixture.PlayerEventResponse},
		{"player leave", http.MethodPost, "/onPlayerLeave", fixture.PlayerEventRequest, fixture.PlayerEventResponse},
		{"game over", http.MethodPost, "/isGameOver", fixture.IsGameOverRequest, fixture.GameOverResponse},
		{"trusted command", http.MethodPost, "/onCommand", fixture.CommandRequest, fixture.CommandResponse},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			recorder := serveRequest(t, handler, testCase.method, testCase.path, testCase.body)
			if recorder.Code != http.StatusOK {
				t.Fatalf("status = %d, want %d; response: %s", recorder.Code, http.StatusOK, recorder.Body)
			}
			assertJSONEqual(t, testCase.want, recorder.Body.Bytes())
		})
	}
}

type summaryServer struct {
	fixtureServer
	summary json.RawMessage
}

func (s summaryServer) IsGameOver(context.Context, IsGameOverRequest) (GameResult, error) {
	return GameResult{
		Over:            true,
		Summary:         s.summary,
		NextMatchPolicy: &NextMatchPolicy{Mode: NextMatchCloseRoom},
	}, nil
}

func TestGameOverSummaryValidation(t *testing.T) {
	fixture := loadFixture(t)
	tests := []struct {
		name    string
		summary string
		status  int
	}{
		{"missing", ``, http.StatusInternalServerError},
		{"null", `null`, http.StatusInternalServerError},
		{"array", `[]`, http.StatusInternalServerError},
		{"missing duration", `{}`, http.StatusInternalServerError},
		{"null duration", `{"durationSeconds":null}`, http.StatusInternalServerError},
		{"string duration", `{"durationSeconds":"1"}`, http.StatusInternalServerError},
		{"negative duration", `{"durationSeconds":-1}`, http.StatusInternalServerError},
		{"overflow duration", `{"durationSeconds":1e400}`, http.StatusInternalServerError},
		{"zero duration", `{"durationSeconds":0}`, http.StatusOK},
		{"fractional duration", `{"durationSeconds":2.5}`, http.StatusOK},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			handler := NewHandler(summaryServer{fixtureServer{fixture}, json.RawMessage(tc.summary)})
			response := serveRequest(t, handler, http.MethodPost, "/isGameOver", fixture.IsGameOverRequest)
			if response.Code != tc.status {
				t.Fatalf("status = %d, want %d; response: %s", response.Code, tc.status, response.Body)
			}
			if tc.status == http.StatusInternalServerError && !strings.Contains(response.Body.String(), "internal server error") {
				t.Fatalf("response was not sanitized: %s", response.Body)
			}
		})
	}
}

func TestHealthEndpoint(t *testing.T) {
	fixture := loadFixture(t)
	handler := NewHandlerWithOptions(fixtureServer{fixture: fixture}, HandlerOptions{
		GameID: fixture.Config.GameID,
	})
	recorder := serveRequest(t, handler, http.MethodGet, "/health", nil)
	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusOK)
	}
	var response struct {
		Status string  `json:"status"`
		GameID string  `json:"gameId"`
		Uptime float64 `json:"uptime"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &response); err != nil {
		t.Fatalf("decode health response: %v", err)
	}
	if response.Status != "ok" || response.GameID != fixture.Config.GameID ||
		math.IsNaN(response.Uptime) || math.IsInf(response.Uptime, 0) || response.Uptime < 0 {
		t.Fatalf("unexpected health response: %+v", response)
	}
}

func TestMulberry32MatchesEverySharedVector(t *testing.T) {
	fixture := loadFixture(t)
	for _, vector := range fixture.PRNGVectors {
		t.Run(fmt.Sprintf("seed_%d", vector.Seed), func(t *testing.T) {
			generator := NewMulberry32(float64(vector.Seed))
			for index, want := range vector.U32 {
				if got := generator.NextU32(); got != want {
					t.Fatalf("output %d = %d, want %d", index, got, want)
				}
			}
		})
	}
}

func TestChaCha20AndDeriveSeedMatchEverySharedVector(t *testing.T) {
	fixture := loadFixture(t)
	for idx, vector := range fixture.ChaCha20Vectors {
		t.Run(fmt.Sprintf("chacha20_%d", idx), func(t *testing.T) {
			generator, err := NewChaCha20RngFromState(ChaCha20State{
				SeedHex: vector.SeedHex,
				Stream:  vector.Stream,
				WordPos: vector.WordPos,
			})
			if err != nil {
				t.Fatalf("NewChaCha20RngFromState: %v", err)
			}
			for index, want := range vector.U32 {
				if got := generator.NextU32(); got != want {
					t.Fatalf("u32[%d] = %d, want %d", index, got, want)
				}
			}

			boundedGen, err := NewChaCha20RngFromState(ChaCha20State{
				SeedHex: vector.SeedHex,
				Stream:  vector.Stream,
				WordPos: vector.WordPos,
			})
			if err != nil {
				t.Fatalf("NewChaCha20RngFromState bounded: %v", err)
			}
			for index, want := range vector.Bounded100 {
				if got := boundedGen.NextBounded(100); got != want {
					t.Fatalf("bounded100[%d] = %d, want %d", index, got, want)
				}
			}

			midState, err := boundedGen.State()
			if err != nil {
				t.Fatalf("boundedGen.State: %v", err)
			}
			encodedState, err := json.Marshal(midState)
			if err != nil {
				t.Fatalf("json.Marshal(ChaCha20State): %v", err)
			}
			var decodedState ChaCha20State
			if err := json.Unmarshal(encodedState, &decodedState); err != nil {
				t.Fatalf("json.Unmarshal(ChaCha20State): %v", err)
			}
			restored, err := NewChaCha20RngFromState(decodedState)
			if err != nil {
				t.Fatalf("NewChaCha20RngFromState(decodedState): %v", err)
			}
			if got, want := restored.NextU32(), boundedGen.NextU32(); got != want {
				t.Fatalf("restored NextU32 = %d, want %d", got, want)
			}
		})
	}

	for idx, vector := range fixture.DeriveSeedVectors {
		t.Run(fmt.Sprintf("derive_%d", idx), func(t *testing.T) {
			derivedHex, err := DeriveSeedHex(vector.RootSeedHex, vector.Domains...)
			if err != nil {
				t.Fatalf("DeriveSeedHex: %v", err)
			}
			if derivedHex != vector.DerivedSeedHex {
				t.Fatalf("derivedSeedHex = %q, want %q", derivedHex, vector.DerivedSeedHex)
			}
			rng, err := NewChaCha20RngFromHex(derivedHex)
			if err != nil {
				t.Fatalf("NewChaCha20RngFromHex: %v", err)
			}
			if got := rng.NextU32(); got != vector.FirstU32 {
				t.Fatalf("firstU32 = %d, want %d", got, vector.FirstU32)
			}
		})
	}
}

func TestChaCha20StateSafeCursorAndJSONValidation(t *testing.T) {
	validSeed := strings.Repeat("1a", RandomSeedBytes)
	rng, err := NewChaCha20RngFromHex(validSeed)
	if err != nil {
		t.Fatalf("NewChaCha20RngFromHex: %v", err)
	}
	rng.SetStream(MaxSafeCursor)
	rng.SetWordPos(MaxSafeCursor)
	maxState, err := rng.State()
	if err != nil {
		t.Fatalf("State() at MaxSafeCursor failed: %v", err)
	}
	raw, err := json.Marshal(maxState)
	if err != nil {
		t.Fatalf("json.Marshal at MaxSafeCursor failed: %v", err)
	}
	var roundTripped ChaCha20State
	if err := json.Unmarshal(raw, &roundTripped); err != nil {
		t.Fatalf("json.Unmarshal at MaxSafeCursor failed: %v", err)
	}
	if roundTripped != maxState {
		t.Fatalf("roundTripped = %+v, want %+v", roundTripped, maxState)
	}

	// Advancing one word past MaxSafeCursor works in memory,
	// but State() and json.Marshal reject unsafe JSON cursors.
	_ = rng.NextU32()
	if _, err := rng.State(); err == nil {
		t.Fatal("expected State() error when wordPos exceeds MaxSafeCursor")
	}

	streamOverflowRng, err := NewChaCha20RngFromHex(validSeed)
	if err != nil {
		t.Fatalf("NewChaCha20RngFromHex: %v", err)
	}
	streamOverflowRng.SetStream(MaxSafeCursor + 1)
	if _, err := streamOverflowRng.State(); err == nil {
		t.Fatal("expected State() error when stream exceeds MaxSafeCursor")
	}

	invalidStates := []ChaCha20State{
		{SeedHex: "abcd", Stream: 0, WordPos: 0},
		{SeedHex: strings.ToUpper(validSeed), Stream: 0, WordPos: 0},
		{SeedHex: validSeed, Stream: MaxSafeCursor + 1, WordPos: 0},
		{SeedHex: validSeed, Stream: 0, WordPos: MaxSafeCursor + 1},
	}
	for _, invalid := range invalidStates {
		if _, err := json.Marshal(invalid); err == nil {
			t.Fatalf("expected json.Marshal error for %+v", invalid)
		}
		if _, err := NewChaCha20RngFromState(invalid); err == nil {
			t.Fatalf("expected NewChaCha20RngFromState error for %+v", invalid)
		}
	}

	invalidPayloads := []string{
		`null`,
		`[]`,
		`{"seedHex":"` + validSeed + `","stream":0}`,
		`{"seedHex":"` + validSeed + `","stream":0,"wordPos":0,"extra":1}`,
		`{"seedHex":"` + strings.ToUpper(validSeed) + `","stream":0,"wordPos":0}`,
		`{"seedHex":"` + validSeed + `","stream":-1,"wordPos":0}`,
		`{"seedHex":"` + validSeed + `","stream":1.5,"wordPos":0}`,
		`{"seedHex":"` + validSeed + `","stream":9007199254740992,"wordPos":0}`,
		`{"seedHex":"` + validSeed + `","stream":0,"wordPos":9007199254740992}`,
		`{"seedHex":"` + validSeed + `","stream":null,"wordPos":0}`,
	}
	for _, payload := range invalidPayloads {
		var target ChaCha20State
		if err := json.Unmarshal([]byte(payload), &target); err == nil {
			t.Fatalf("expected json.Unmarshal error for %s", payload)
		}
	}

	overflowRng, err := NewChaCha20RngFromHex(validSeed)
	if err != nil {
		t.Fatalf("NewChaCha20RngFromHex: %v", err)
	}
	overflowRng.SetWordPos(^uint64(0))
	didPanic := false
	func() {
		defer func() {
			if recover() != nil {
				didPanic = true
			}
		}()
		_ = overflowRng.NextU32()
	}()
	if !didPanic {
		t.Fatal("expected NextU32 to panic on uint64 wordPos overflow")
	}
}

func TestToInt32UsesJavaScriptConversion(t *testing.T) {
	tests := []struct {
		seed float64
		want int32
	}{
		{seed: -1.9, want: -1},
		{seed: 4294967297, want: 1},
		{seed: 2147483648, want: -1 << 31},
		{seed: -2147483649, want: 1<<31 - 1},
		{seed: math.NaN(), want: 0},
		{seed: math.Inf(1), want: 0},
	}
	for _, testCase := range tests {
		if got := ToInt32(testCase.seed); got != testCase.want {
			t.Errorf("ToInt32(%v) = %d, want %d", testCase.seed, got, testCase.want)
		}
	}
}

func TestNextFloatUsesUnsignedOutput(t *testing.T) {
	generator := NewMulberry32(0)
	want := float64(uint32(1144304738)) / 4294967296
	if got := generator.NextFloat(); got != want {
		t.Fatalf("NextFloat() = %v, want %v", got, want)
	}
}

func TestInvalidRequestsAreRejected(t *testing.T) {
	fixture := loadFixture(t)
	handler := NewHandler(fixtureServer{fixture: fixture})
	tests := []struct {
		name string
		path string
		body string
	}{
		{
			name: "invalid json",
			path: "/init",
			body: `{"matchId":`,
		},
		{
			name: "duplicate players",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p","p"],"matchSeq":1}`,
		},
		{
			name: "non-numeric seed",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":1,"options":{"seed":null}}`,
		},
		{
			name: "null randomSeed",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":1,"options":{"randomSeed":null}}`,
		},
		{
			name: "short randomSeed",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":1,"options":{"randomSeed":"abcd"}}`,
		},
		{
			name: "uppercase randomSeed",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":1,"options":{"randomSeed":"000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F"}}`,
		},
		{
			name: "negative randomSeedIndex",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":1,"options":{"randomSeedIndex":-1}}`,
		},
		{
			name: "unsafe randomSeedIndex",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":1,"options":{"randomSeedIndex":9007199254740992}}`,
		},
		{
			name: "missing match sequence",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"]}`,
		},
		{
			name: "zero match sequence",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":0}`,
		},
		{
			name: "negative match sequence",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":-1}`,
		},
		{
			name: "match sequence exceeds safe integer",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":9007199254740992}`,
		},
		{
			name: "fractional match sequence",
			path: "/init",
			body: `{"matchId":"m","gameId":"g","players":["p"],"matchSeq":1.5}`,
		},
		{
			name: "missing tick elapsed time",
			path: "/tick",
			body: `{"ctx":{"tick":0,"inputs":[]},"state":{}}`,
		},
		{
			name: "negative tick",
			path: "/tick",
			body: `{"ctx":{"tick":-1,"inputs":[],"elapsedSeconds":0},"state":{}}`,
		},
		{
			name: "tick exceeds safe integer",
			path: "/tick",
			body: `{"ctx":{"tick":9007199254740992,"inputs":[],"elapsedSeconds":0},"state":{}}`,
		},
		{
			name: "negative input timestamp",
			path: "/tick",
			body: `{"ctx":{"tick":0,"inputs":[{"playerId":"p","data":null,"timestamp":-1}],"elapsedSeconds":0},"state":{}}`,
		},
		{
			name: "input timestamp exceeds safe integer",
			path: "/tick",
			body: `{"ctx":{"tick":0,"inputs":[{"playerId":"p","data":null,"timestamp":9007199254740992}],"elapsedSeconds":0},"state":{}}`,
		},
		{
			name: "null input timestamp",
			path: "/tick",
			body: `{"ctx":{"tick":0,"inputs":[{"playerId":"p","data":null,"timestamp":null}],"elapsedSeconds":0},"state":{}}`,
		},
		{
			name: "missing opaque state",
			path: "/isGameOver",
			body: `{}`,
		},
		{
			name: "missing command data",
			path: "/onCommand",
			body: `{"command":{"type":"reset"},"state":{}}`,
		},
	}
	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			recorder := serveRequest(t, handler, http.MethodPost, testCase.path, testCase.body)
			if recorder.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want %d; response: %s", recorder.Code, http.StatusBadRequest, recorder.Body)
			}
			if !strings.Contains(recorder.Header().Get("Content-Type"), "application/json") {
				t.Fatalf("unexpected content type %q", recorder.Header().Get("Content-Type"))
			}
		})
	}
}

func TestRequestBodyLimit(t *testing.T) {
	fixture := loadFixture(t)
	handler := NewHandler(fixtureServer{fixture: fixture})
	base, err := json.Marshal(fixture.Config)
	if err != nil {
		t.Fatalf("encode fixture config: %v", err)
	}
	prefix := strings.TrimSuffix(string(base), "}")
	paddingPrefix := `,"padding":"`
	suffix := `"}`
	paddingLength := int(DefaultMaxRequestBodyBytes) - len(prefix) - len(paddingPrefix) - len(suffix)
	if paddingLength < 0 {
		t.Fatal("fixture config exceeds request body limit")
	}
	exactLimitBody := prefix + paddingPrefix + strings.Repeat("x", paddingLength) + suffix
	if len(exactLimitBody) != int(DefaultMaxRequestBodyBytes) {
		t.Fatalf("built body length = %d, want %d", len(exactLimitBody), DefaultMaxRequestBodyBytes)
	}
	exactLimitResponse := serveRequest(t, handler, http.MethodPost, "/init", exactLimitBody)
	if exactLimitResponse.Code != http.StatusOK {
		t.Fatalf("exact-limit status = %d, want %d; response: %s", exactLimitResponse.Code, http.StatusOK, exactLimitResponse.Body)
	}

	oversizedBody := exactLimitBody + " "
	recorder := serveRequest(t, handler, http.MethodPost, "/init", oversizedBody)
	if recorder.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusRequestEntityTooLarge)
	}
}

func TestLegacyUnkeyedPublicStructLiteralsRemainSourceCompatible(t *testing.T) {
	_ = GameManifest{"title", "1.0", 1, 2, 5, false, nil, nil}
	_ = TickRequest{TickContext{}, json.RawMessage(`{}`)}
	_ = PlayerEventRequest{"player", json.RawMessage(`{}`)}
	_ = CommandRequest{Command{}, json.RawMessage(`{}`)}
	_ = InitResult{json.RawMessage(`{}`), nil, nil}
	_ = TickResult{json.RawMessage(`{}`), nil, false, nil}
	_ = PlayerEventResult{json.RawMessage(`{}`), nil, nil}
	_ = CommandResult{json.RawMessage(`{}`), nil, nil}
}

func TestSafeIntegerUpperBoundsAreAccepted(t *testing.T) {
	matchConfig := []byte(`{"matchId":"m","gameId":"g","players":["p"],"matchSeq":9007199254740991}`)
	var config MatchConfig
	if err := json.Unmarshal(matchConfig, &config); err != nil {
		t.Fatalf("decode max safe match sequence: %v", err)
	}
	if !validMatchConfig(config, matchConfig) {
		t.Fatal("maximum safe match sequence was rejected")
	}

	tickRequest := []byte(`{"ctx":{"tick":9007199254740991,"inputs":[{"playerId":"p","data":null,"timestamp":9007199254740991}],"elapsedSeconds":0},"state":null}`)
	var request TickRequestV1
	if err := json.Unmarshal(tickRequest, &request); err != nil {
		t.Fatalf("decode max safe tick request: %v", err)
	}
	if !validTickRequest(request, tickRequest) {
		t.Fatal("maximum safe tick or timestamp was rejected")
	}
}

type failingServer struct {
	fixtureServer
}

func (s failingServer) Init(context.Context, MatchConfig) (InitResult, error) {
	return InitResult{}, errors.New("private callback details must not be returned")
}

func TestCallbackErrorsAreSanitized(t *testing.T) {
	fixture := loadFixture(t)
	handler := NewHandler(failingServer{fixtureServer: fixtureServer{fixture: fixture}})
	recorder := serveRequest(t, handler, http.MethodPost, "/init", fixture.Config)
	if recorder.Code != http.StatusInternalServerError {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusInternalServerError)
	}
	if bytes.Contains(recorder.Body.Bytes(), []byte("private callback details")) {
		t.Fatalf("callback details leaked in response: %s", recorder.Body)
	}
	assertJSONEqual(t, map[string]string{"error": "internal server error"}, recorder.Body.Bytes())
}

func TestMethodAndUnknownRouteResponses(t *testing.T) {
	fixture := loadFixture(t)
	handler := NewHandler(fixtureServer{fixture: fixture})
	wrongMethod := serveRequest(t, handler, http.MethodPost, "/manifest", map[string]string{})
	if wrongMethod.Code != http.StatusMethodNotAllowed || wrongMethod.Header().Get("Allow") != http.MethodGet {
		t.Fatalf("unexpected method response: status=%d allow=%q", wrongMethod.Code, wrongMethod.Header().Get("Allow"))
	}
	notFound := serveRequest(t, handler, http.MethodGet, "/unknown", nil)
	if notFound.Code != http.StatusNotFound {
		t.Fatalf("unknown route status = %d, want %d", notFound.Code, http.StatusNotFound)
	}
}

func serveRequest(t *testing.T, handler http.Handler, method, path string, body any) *httptest.ResponseRecorder {
	t.Helper()
	var requestBody []byte
	switch value := body.(type) {
	case nil:
	case string:
		requestBody = []byte(value)
	default:
		data, err := json.Marshal(value)
		if err != nil {
			t.Fatalf("encode request body: %v", err)
		}
		requestBody = data
	}
	request := httptest.NewRequest(method, path, bytes.NewReader(requestBody))
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, request)
	return recorder
}

func assertJSONEqual(t *testing.T, want any, gotJSON []byte) {
	t.Helper()
	var got any
	decoder := json.NewDecoder(bytes.NewReader(gotJSON))
	decoder.UseNumber()
	if err := decoder.Decode(&got); err != nil {
		t.Fatalf("decode response JSON: %v", err)
	}
	wantValue, err := jsonValue(want)
	if err != nil {
		t.Fatalf("encode expected JSON: %v", err)
	}
	if !reflect.DeepEqual(got, wantValue) {
		t.Fatalf("response JSON differs:\n got: %#v\nwant: %#v", got, wantValue)
	}
}

func jsonEquivalent(left, right any) bool {
	leftValue, leftErr := jsonValue(left)
	rightValue, rightErr := jsonValue(right)
	return leftErr == nil && rightErr == nil && reflect.DeepEqual(leftValue, rightValue)
}

func jsonValue(value any) (any, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	var result any
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := decoder.Decode(&result); err != nil {
		return nil, err
	}
	return result, nil
}
