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
	ProtocolVersion       int              `json:"protocolVersion"`
	Manifest              GameManifest     `json:"manifest"`
	Config                MatchConfig      `json:"config"`
	InitResponse          InitResult       `json:"initResponse"`
	TickRequest           TickRequest      `json:"tickRequest"`
	TickResponse          TickResult       `json:"tickResponse"`
	PlayerEventRequest    PlayerEventRequest `json:"playerEventRequest"`
	PlayerEventResponse   PlayerEventResult  `json:"playerEventResponse"`
	IsGameOverRequest     IsGameOverRequest  `json:"isGameOverRequest"`
	GameOverResponse      GameResult         `json:"gameOverResponse"`
	CommandRequest        CommandRequest     `json:"commandRequest"`
	CommandResponse       CommandResult      `json:"commandResponse"`
	PRNGVectors           []prngFixtureVector `json:"prngVectors"`
}

type prngFixtureVector struct {
	Seed int64    `json:"seed"`
	U32  []uint32 `json:"u32"`
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
		Over: true,
		Summary: s.summary,
		NextMatchPolicy: &NextMatchPolicy{Mode: NextMatchCloseRoom},
	}, nil
}

func TestGameOverSummaryValidation(t *testing.T) {
	fixture := loadFixture(t)
	tests := []struct {
		name string
		summary string
		status int
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
		Status  string  `json:"status"`
		GameID  string  `json:"gameId"`
		Uptime  float64 `json:"uptime"`
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
	var request TickRequest
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
