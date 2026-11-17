package roomhost

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"time"
)

func validPlatformGameID(gameID string) bool {
	if len(gameID) == 0 || len(gameID) > 64 {
		return false
	}
	for _, char := range gameID {
		if !(char >= 97 && char <= 122 || char >= 65 && char <= 90 || char >= 48 && char <= 57 || char == 95 || char == 45) {
			return false
		}
	}
	return true
}

const (
	// DefaultPort is the port used by ListenAndServe.
	DefaultPort = 8080
	// DefaultMaxRequestBodyBytes limits JSON request bodies to one mebibyte.
	DefaultMaxRequestBodyBytes int64 = 1 << 20
)

// HandlerOptions configures the HTTP handler.
type HandlerOptions struct {
	// GameID is returned by GET /health.
	GameID string
}

type httpHandler struct {
	server    GameServer
	serverV1  GameServerV1
	gameID    string
	startedAt time.Time
}

// NewHandler creates an HTTP handler for the room-host v1 endpoints.
func NewHandler(server GameServer) http.Handler {
	return NewHandlerWithOptions(server, HandlerOptions{})
}

// NewHandlerWithOptions creates an HTTP handler with the supplied configuration.
func NewHandlerWithOptions(server GameServer, options HandlerOptions) http.Handler {
	gameID := options.GameID
	if gameID == "" {
		if provider, ok := server.(GameIDProvider); ok {
			if providedID, err := safeValue(provider.GameID); err == nil {
				gameID = providedID
			}
		}
	}
	return newHandler(server, nil, options, gameID)
}

// NewHandlerV1 serves an opt-in GameServerV1 while preserving GameServer APIs.
func NewHandlerV1(server GameServerV1) http.Handler {
	return NewHandlerV1WithOptions(server, HandlerOptions{})
}

// NewHandlerV1WithOptions serves an opt-in GameServerV1 with handler options.
func NewHandlerV1WithOptions(server GameServerV1, options HandlerOptions) http.Handler {
	gameID := options.GameID
	if gameID == "" {
		if provider, ok := server.(GameIDProvider); ok {
			if providedID, err := safeValue(provider.GameID); err == nil {
				gameID = providedID
			}
		}
	}
	return newHandler(nil, server, options, gameID)
}

func newHandler(server GameServer, serverV1 GameServerV1, _ HandlerOptions, gameID string) http.Handler {
	return &httpHandler{server: server, serverV1: serverV1, gameID: gameID, startedAt: time.Now()}
}

// ListenAndServe starts the room-host HTTP server on the default port, 8080.
func ListenAndServe(server GameServer) error {
	return ListenAndServeWithOptions(server, HandlerOptions{})
}

// ListenAndServeWithOptions starts the server on the default port with handler options.
func ListenAndServeWithOptions(server GameServer, options HandlerOptions) error {
	address := ":" + strconv.Itoa(DefaultPort)
	return http.ListenAndServe(address, NewHandlerWithOptions(server, options))
}

// ListenAndServeV1 serves an opt-in GameServerV1 on port 8080.
func ListenAndServeV1(server GameServerV1) error {
	return ListenAndServeV1WithOptions(server, HandlerOptions{})
}

// ListenAndServeV1WithOptions serves an opt-in GameServerV1 with handler options.
func ListenAndServeV1WithOptions(server GameServerV1, options HandlerOptions) error {
	address := ":" + strconv.Itoa(DefaultPort)
	return http.ListenAndServe(address, NewHandlerV1WithOptions(server, options))
}

func (h *httpHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	switch r.URL.Path {
	case "/manifest":
		if !h.requireMethod(w, r, http.MethodGet) {
			return
		}
		h.serveManifest(w)
	case "/health":
		if !h.requireMethod(w, r, http.MethodGet) {
			return
		}
		if !validPlatformGameID(h.gameID) {
			writeError(w, http.StatusServiceUnavailable, "the platform RANDSEED_GAME_ID is unavailable or invalid")
			return
		}
		h.writeJSON(w, http.StatusOK, struct {
			Status string  `json:"status"`
			GameID string  `json:"gameId"`
			Uptime float64 `json:"uptime"`
		}{
			Status: "ok",
			GameID: h.gameID,
			Uptime: time.Since(h.startedAt).Seconds(),
		})
	case "/init":
		if !h.requireMethod(w, r, http.MethodPost) {
			return
		}
		h.serveInit(w, r)
	case "/tick":
		if !h.requireMethod(w, r, http.MethodPost) {
			return
		}
		h.serveTick(w, r)
	case "/onPlayerJoin":
		if !h.requireMethod(w, r, http.MethodPost) {
			return
		}
		h.servePlayerEvent(w, r, true)
	case "/onPlayerLeave":
		if !h.requireMethod(w, r, http.MethodPost) {
			return
		}
		h.servePlayerEvent(w, r, false)
	case "/isGameOver":
		if !h.requireMethod(w, r, http.MethodPost) {
			return
		}
		h.serveIsGameOver(w, r)
	case "/onCommand":
		if !h.requireMethod(w, r, http.MethodPost) {
			return
		}
		h.serveCommand(w, r)
	default:
		writeError(w, http.StatusNotFound, "not found")
	}
}

func (h *httpHandler) requireMethod(w http.ResponseWriter, r *http.Request, method string) bool {
	if r.Method == method {
		return true
	}
	w.Header().Set("Allow", method)
	writeError(w, http.StatusMethodNotAllowed, "method not allowed")
	return false
}

func (h *httpHandler) getValidManifest() (GameManifestV1, bool) {
	if h.serverV1 != nil {
		manifest, err := safeValue(h.serverV1.ManifestV1)
		return manifest, err == nil && validManifestV1(manifest)
	}
	if h.server == nil {
		return GameManifestV1{}, false
	}
	manifest, err := safeValue(h.server.Manifest)
	wrapped := GameManifestV1{GameManifest: manifest}
	return wrapped, err == nil && validManifest(manifest)
}

func (h *httpHandler) serveManifest(w http.ResponseWriter) {
	manifest, ok := h.getValidManifest()
	if !ok {
		writeInternalError(w)
		return
	}
	if h.serverV1 != nil {
		h.writeJSON(w, http.StatusOK, manifest)
	} else {
		h.writeJSON(w, http.StatusOK, manifest.GameManifest)
	}
}

func (h *httpHandler) serveInit(w http.ResponseWriter, r *http.Request) {
	var config MatchConfig
	data, ok := h.decodeRequest(w, r, &config)
	if !ok {
		return
	}
	if !validMatchConfig(config, data) {
		writeError(w, http.StatusBadRequest, "invalid match configuration")
		return
	}
	manifest, ok := h.getValidManifest()
	if !ok {
		writeInternalError(w)
		return
	}
	if len(config.Players) < manifest.MinPlayers || len(config.Players) > manifest.MaxPlayers {
		writeError(w, http.StatusBadRequest, "players must satisfy the game manifest roster limits")
		return
	}
	playersSnapshot := clonePlayers(config.Players)
	callbackConfig := config
	callbackConfig.Players = clonePlayers(playersSnapshot)
	var result InitResultV1
	var err error
	if h.serverV1 != nil {
		result, err = safeCall(func() (InitResultV1, error) {
			return h.serverV1.InitV1(r.Context(), callbackConfig)
		})
	} else if h.server != nil {
		var legacy InitResult
		legacy, err = safeCall(func() (InitResult, error) { return h.server.Init(r.Context(), callbackConfig) })
		result = InitResultV1{State: legacy.State, Broadcasts: legacy.Broadcasts, ResultUnits: legacy.ResultUnits}
	} else {
		writeInternalError(w)
		return
	}
	if err != nil || !validInitResultV1ForRoster(result, playersSnapshot, true, manifest.PlayerViewMode == PlayerViewPrivate) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, result)
}

func (h *httpHandler) serveTick(w http.ResponseWriter, r *http.Request) {
	var request TickRequestV1
	data, ok := h.decodeRequest(w, r, &request)
	if !ok {
		return
	}
	if !validTickRequest(request, data) {
		writeError(w, http.StatusBadRequest, "invalid tick request")
		return
	}
	manifest, ok := h.getValidManifest()
	if !ok {
		writeInternalError(w)
		return
	}
	rosterValid, rosterPresent := validRequestRoster(data, request.Players,
		manifest.PlayerViewMode == PlayerViewPrivate, manifest.MaxPlayers, true)
	if !rosterValid {
		writeError(w, http.StatusBadRequest, "players must include the active roster for private views")
		return
	}
	playersSnapshot := clonePlayers(request.Players)
	if rosterPresent {
		for _, input := range request.Ctx.Inputs {
			if !playersContain(playersSnapshot, input.PlayerID) {
				writeError(w, http.StatusBadRequest, "input playerId must belong to the active roster")
				return
			}
		}
	}
	if request.Ctx.Commands == nil {
		request.Ctx.Commands = []Command{}
	}
	callbackRequest := request
	callbackRequest.Players = clonePlayers(playersSnapshot)
	var result TickResultV1
	var err error
	if h.serverV1 != nil {
		result, err = safeCall(func() (TickResultV1, error) {
			return h.serverV1.TickV1(r.Context(), callbackRequest)
		})
	} else if h.server != nil {
		var legacy TickResult
		legacy, err = safeCall(func() (TickResult, error) {
			return h.server.Tick(r.Context(), TickRequest{Ctx: request.Ctx, State: request.State})
		})
		result = TickResultV1{State: legacy.State, Broadcasts: legacy.Broadcasts, Persist: legacy.Persist, ResultUnits: legacy.ResultUnits}
	} else {
		writeInternalError(w)
		return
	}
	if err != nil || !validTickResultV1ForRoster(result, playersSnapshot, rosterPresent, manifest.PlayerViewMode == PlayerViewPrivate) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, result)
}

func (h *httpHandler) servePlayerEvent(w http.ResponseWriter, r *http.Request, joining bool) {
	var request PlayerEventRequestV1
	data, ok := h.decodeRequest(w, r, &request)
	if !ok {
		return
	}
	if !validPlayerEventRequest(request, data) {
		writeError(w, http.StatusBadRequest, "invalid player event request")
		return
	}
	manifest, ok := h.getValidManifest()
	if !ok {
		writeInternalError(w)
		return
	}
	rosterValid, rosterPresent := validRequestRoster(data, request.Players,
		manifest.PlayerViewMode == PlayerViewPrivate, manifest.MaxPlayers, true)
	if !rosterValid {
		writeError(w, http.StatusBadRequest, "players must include the resulting roster for private views")
		return
	}
	if rosterPresent && playersContain(request.Players, request.PlayerID) != joining {
		writeError(w, http.StatusBadRequest, "players must describe the resulting join or leave roster")
		return
	}
	playersSnapshot := clonePlayers(request.Players)
	callbackRequest := request
	callbackRequest.Players = clonePlayers(playersSnapshot)
	var result PlayerEventResultV1
	var err error
	if h.serverV1 != nil {
		if joining {
			result, err = safeCall(func() (PlayerEventResultV1, error) {
				return h.serverV1.OnPlayerJoinV1(r.Context(), callbackRequest)
			})
		} else {
			result, err = safeCall(func() (PlayerEventResultV1, error) {
				return h.serverV1.OnPlayerLeaveV1(r.Context(), callbackRequest)
			})
		}
	} else if h.server != nil {
		var legacy PlayerEventResult
		if joining {
			legacy, err = safeCall(func() (PlayerEventResult, error) {
				return h.server.OnPlayerJoin(r.Context(), PlayerEventRequest{PlayerID: request.PlayerID, State: request.State})
			})
		} else {
			legacy, err = safeCall(func() (PlayerEventResult, error) {
				return h.server.OnPlayerLeave(r.Context(), PlayerEventRequest{PlayerID: request.PlayerID, State: request.State})
			})
		}
		result = PlayerEventResultV1{State: legacy.State, Broadcasts: legacy.Broadcasts, ResultUnits: legacy.ResultUnits}
	} else {
		writeInternalError(w)
		return
	}
	if err != nil || !validPlayerEventResultV1ForRoster(result, playersSnapshot, rosterPresent, manifest.PlayerViewMode == PlayerViewPrivate) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, result)
}

func (h *httpHandler) serveIsGameOver(w http.ResponseWriter, r *http.Request) {
	var request IsGameOverRequest
	data, ok := h.decodeRequest(w, r, &request)
	if !ok {
		return
	}
	if !validIsGameOverRequest(request, data) {
		writeError(w, http.StatusBadRequest, "invalid game-over request")
		return
	}
	if h.server == nil && h.serverV1 == nil {
		writeInternalError(w)
		return
	}
	var result GameResult
	var err error
	if h.serverV1 != nil {
		result, err = safeCall(func() (GameResult, error) { return h.serverV1.IsGameOverV1(r.Context(), request) })
	} else {
		result, err = safeCall(func() (GameResult, error) { return h.server.IsGameOver(r.Context(), request) })
	}
	if err != nil || !validGameResult(result) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, result)
}

func (h *httpHandler) serveCommand(w http.ResponseWriter, r *http.Request) {
	var request CommandRequestV1
	data, ok := h.decodeRequest(w, r, &request)
	if !ok {
		return
	}
	if !validCommandRequest(request, data) {
		writeError(w, http.StatusBadRequest, "invalid command request")
		return
	}
	manifest, ok := h.getValidManifest()
	if !ok {
		writeInternalError(w)
		return
	}
	rosterValid, rosterPresent := validRequestRoster(data, request.Players,
		manifest.PlayerViewMode == PlayerViewPrivate, manifest.MaxPlayers, true)
	if !rosterValid {
		writeError(w, http.StatusBadRequest, "players must include the active roster for private views")
		return
	}
	playersSnapshot := clonePlayers(request.Players)
	callbackRequest := request
	callbackRequest.Players = clonePlayers(playersSnapshot)
	var result CommandResultV1
	var err error
	if h.serverV1 != nil {
		result, err = safeCall(func() (CommandResultV1, error) {
			return h.serverV1.OnCommandV1(r.Context(), callbackRequest)
		})
	} else if h.server != nil {
		var legacy CommandResult
		legacy, err = safeCall(func() (CommandResult, error) {
			return h.server.OnCommand(r.Context(), CommandRequest{Command: request.Command, State: request.State})
		})
		result = CommandResultV1{State: legacy.State, Broadcasts: legacy.Broadcasts, ResultUnits: legacy.ResultUnits}
	} else {
		writeInternalError(w)
		return
	}
	if err != nil || !validCommandResultV1ForRoster(result, playersSnapshot, rosterPresent, manifest.PlayerViewMode == PlayerViewPrivate) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, result)
}

func (h *httpHandler) decodeRequest(w http.ResponseWriter, r *http.Request, target any) ([]byte, bool) {
	body := http.MaxBytesReader(w, r.Body, DefaultMaxRequestBodyBytes)
	data, err := io.ReadAll(body)
	if err != nil {
		var maxBytesError *http.MaxBytesError
		if errors.As(err, &maxBytesError) {
			writeError(w, http.StatusRequestEntityTooLarge, "request body too large")
		} else {
			writeError(w, http.StatusBadRequest, "invalid request body")
		}
		return nil, false
	}
	if !isJSONObject(data) {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return nil, false
	}
	if err := json.Unmarshal(data, target); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return nil, false
	}
	return data, true
}

func (h *httpHandler) writeJSON(w http.ResponseWriter, status int, value any) {
	data, err := json.Marshal(value)
	if err != nil {
		writeInternalError(w)
		return
	}
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.WriteHeader(status)
	_, _ = w.Write(data)
}

func writeError(w http.ResponseWriter, status int, message string) {
	data, _ := json.Marshal(struct {
		Error string `json:"error"`
	}{Error: message})
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.WriteHeader(status)
	_, _ = w.Write(data)
}

func writeInternalError(w http.ResponseWriter) {
	writeError(w, http.StatusInternalServerError, "internal server error")
}

func clonePlayers(players []string) []string {
	if players == nil {
		return nil
	}
	return append([]string{}, players...)
}

func safeCall[T any](callback func() (T, error)) (value T, err error) {
	defer func() {
		if recover() != nil {
			err = errors.New("game server callback failed")
		}
	}()
	return callback()
}

func safeValue[T any](callback func() T) (value T, err error) {
	defer func() {
		if recover() != nil {
			err = errors.New("game server callback failed")
		}
	}()
	return callback(), nil
}
