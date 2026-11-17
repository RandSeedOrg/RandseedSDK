package roomhost

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"
)

type platformRoundTripperFunc func(*http.Request) (*http.Response, error)

func (roundTripper platformRoundTripperFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return roundTripper(request)
}

func platformResponse(status int, body string) *http.Response {
	return &http.Response{
		StatusCode: status,
		Header:     make(http.Header),
		Body:       io.NopCloser(strings.NewReader(body)),
	}
}

func TestPlatformClientGetPlayersProfilesAndStripsUnknownFields(t *testing.T) {
	type contextKey string
	ctx := context.WithValue(context.Background(), contextKey("trace"), "request-context")
	transport := platformRoundTripperFunc(func(request *http.Request) (*http.Response, error) {
		if request.Method != http.MethodGet {
			t.Errorf("method = %q, want GET", request.Method)
		}
		if request.URL.String() != platformPlayersURL {
			t.Errorf("URL = %q, want %q", request.URL, platformPlayersURL)
		}
		if got := request.Context().Value(contextKey("trace")); got != "request-context" {
			t.Errorf("request context value = %v, want request-context", got)
		}
		return platformResponse(http.StatusOK, `{
			"players": [
				{"playerId":"legacy","joinedAt":100},
				{"playerId":"nulls","joinedAt":200,"displayName":null,"avatarUrl":null,"isGameCreator":null},
				{
					"playerId":"creator","joinedAt":300,"displayName":"Display Name",
					"avatarUrl":"https://example.test/avatar.png","isGameCreator":false,
					"principal":"PRIVATE_PRINCIPAL_SENTINEL","accountId":"PRIVATE_ACCOUNT_SENTINEL",
					"unknownField":"PRIVATE_UNKNOWN_SENTINEL"
				}
			]
		}`), nil
	})

	players, err := NewPlatformClient(transport).GetPlayers(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if len(players) != 3 {
		t.Fatalf("got %d players, want 3", len(players))
	}
	if players[0].PlayerID != "legacy" || players[0].JoinedAt != 100 ||
		players[0].DisplayName != nil || players[0].AvatarURL != nil || players[0].IsGameCreator != nil {
		t.Fatalf("legacy player = %#v", players[0])
	}
	if players[1].DisplayName != nil || players[1].AvatarURL != nil || players[1].IsGameCreator != nil {
		t.Fatalf("null profile fields = %#v", players[1])
	}
	if players[2].DisplayName == nil || *players[2].DisplayName != "Display Name" ||
		players[2].AvatarURL == nil || *players[2].AvatarURL != "https://example.test/avatar.png" {
		t.Fatalf("profile fields = %#v", players[2])
	}
	if players[2].IsGameCreator == nil || *players[2].IsGameCreator {
		t.Fatalf("false isGameCreator hint was not preserved: %#v", players[2].IsGameCreator)
	}

	encoded, err := json.Marshal(players)
	if err != nil {
		t.Fatal(err)
	}
	for _, privateValue := range []string{
		"PRIVATE_PRINCIPAL_SENTINEL",
		"PRIVATE_ACCOUNT_SENTINEL",
		"PRIVATE_UNKNOWN_SENTINEL",
	} {
		if strings.Contains(string(encoded), privateValue) {
			t.Fatalf("private or unknown field %q escaped: %s", privateValue, encoded)
		}
	}
}

func TestPlatformClientGetPlayersRejectsMalformedRowsAndOptionalFieldTypes(t *testing.T) {
	cases := []struct {
		name string
		body string
	}{
		{"missing players", `{}`},
		{"null players", `{"players":null}`},
		{"non-array players", `{"players":"bad"}`},
		{"missing playerId", `{"players":[{"joinedAt":1}]}`},
		{"wrong playerId type", `{"players":[{"playerId":4,"joinedAt":1}]}`},
		{"empty playerId", `{"players":[{"playerId":"","joinedAt":1}]}`},
		{"missing joinedAt", `{"players":[{"playerId":"p1"}]}`},
		{"wrong joinedAt type", `{"players":[{"playerId":"p1","joinedAt":"1"}]}`},
		{"wrong displayName type", `{"players":[{"playerId":"p1","joinedAt":1,"displayName":4}]}`},
		{"wrong avatarUrl type", `{"players":[{"playerId":"p1","joinedAt":1,"avatarUrl":[]}]}`},
		{"wrong isGameCreator type", `{"players":[{"playerId":"p1","joinedAt":1,"isGameCreator":"false"}]}`},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			transport := platformRoundTripperFunc(func(*http.Request) (*http.Response, error) {
				return platformResponse(http.StatusOK, testCase.body), nil
			})
			if _, err := NewPlatformClient(transport).GetPlayers(context.Background()); err == nil {
				t.Fatalf("accepted malformed response %s", testCase.body)
			}
		})
	}
}

func TestPlatformClientGetPlayersReturnsHTTPAndTransportErrors(t *testing.T) {
	httpClient := NewPlatformClient(platformRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		return platformResponse(http.StatusBadGateway, `{"error":"unavailable"}`), nil
	}))
	if _, err := httpClient.GetPlayers(context.Background()); err == nil ||
		!strings.Contains(err.Error(), "HTTP status 502") {
		t.Fatalf("HTTP failure error = %v, want status 502", err)
	}

	transportFailure := errors.New("connection refused")
	transportClient := NewPlatformClient(platformRoundTripperFunc(func(*http.Request) (*http.Response, error) {
		return nil, transportFailure
	}))
	if _, err := transportClient.GetPlayers(context.Background()); !errors.Is(err, transportFailure) {
		t.Fatalf("transport failure error = %v, want wrapped connection error", err)
	}
}

func TestPlatformClientGetPlayersHonorsCanceledContext(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := NewPlatformClient(nil).GetPlayers(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("canceled request error = %v, want context.Canceled", err)
	}
}
