package roomhost

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
)

const platformPlayersURL = "http://platform.players/"

// PlayerInfo describes a connected player and the optional display hints
// returned by the room host.
//
// IsGameCreator is presentation metadata only and must not be used to
// authorize an action.
type PlayerInfo struct {
	PlayerID      string  `json:"playerId"`
	JoinedAt      uint64  `json:"joinedAt"`
	DisplayName   *string `json:"displayName,omitempty"`
	AvatarURL     *string `json:"avatarUrl,omitempty"`
	IsGameCreator *bool   `json:"isGameCreator,omitempty"`
}

// PlatformClient calls room-host virtual-host services.
type PlatformClient struct {
	httpClient *http.Client
}

// NewPlatformClient creates a client for platform.players.
//
// Pass a custom RoundTripper to route or mock requests. A nil transport uses
// the standard library's default transport.
func NewPlatformClient(transport http.RoundTripper) *PlatformClient {
	return &PlatformClient{httpClient: &http.Client{Transport: transport}}
}

type platformPlayersResponse struct {
	Players []json.RawMessage `json:"players"`
}

type platformPlayerRow struct {
	PlayerID      *string `json:"playerId"`
	JoinedAt      *uint64 `json:"joinedAt"`
	DisplayName   *string `json:"displayName"`
	AvatarURL     *string `json:"avatarUrl"`
	IsGameCreator *bool   `json:"isGameCreator"`
}

// GetPlayers returns the connected room roster and optional display-profile
// hints. Missing or null optional values are nil; unknown fields are ignored.
// The request uses the supplied context, and transport and HTTP failures are
// returned as errors.
func (c *PlatformClient) GetPlayers(ctx context.Context) ([]PlayerInfo, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, platformPlayersURL, nil)
	if err != nil {
		return nil, fmt.Errorf("platform.players request: %w", err)
	}

	response, err := c.httpClient.Do(request)
	if err != nil {
		return nil, fmt.Errorf("platform.players request: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		return nil, fmt.Errorf("platform.players returned HTTP status %d", response.StatusCode)
	}

	body, err := io.ReadAll(response.Body)
	if err != nil {
		return nil, fmt.Errorf("read platform.players response: %w", err)
	}
	var decoded platformPlayersResponse
	if err := json.Unmarshal(body, &decoded); err != nil {
		return nil, fmt.Errorf("decode platform.players response: %w", err)
	}
	if decoded.Players == nil {
		return nil, fmt.Errorf("decode platform.players response: players must be an array")
	}

	players := make([]PlayerInfo, 0, len(decoded.Players))
	for index, rawPlayer := range decoded.Players {
		var row platformPlayerRow
		if err := json.Unmarshal(rawPlayer, &row); err != nil {
			return nil, fmt.Errorf("decode platform.players row %d: %w", index, err)
		}
		if row.PlayerID == nil || *row.PlayerID == "" {
			return nil, fmt.Errorf("decode platform.players row %d: playerId is required", index)
		}
		if row.JoinedAt == nil {
			return nil, fmt.Errorf("decode platform.players row %d: joinedAt is required", index)
		}
		players = append(players, PlayerInfo{
			PlayerID:      *row.PlayerID,
			JoinedAt:      *row.JoinedAt,
			DisplayName:   row.DisplayName,
			AvatarURL:     row.AvatarURL,
			IsGameCreator: row.IsGameCreator,
		})
	}
	return players, nil
}
