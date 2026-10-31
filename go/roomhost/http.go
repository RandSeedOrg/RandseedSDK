package roomhost

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"time"
)

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
	return &httpHandler{
		server:    server,
		gameID:    gameID,
		startedAt: time.Now(),
	}
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
		h.writeJSON(w, http.StatusOK, struct {
			Status  string  `json:"status"`
			GameID  string  `json:"gameId"`
			Uptime  float64 `json:"uptime"`
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

func (h *httpHandler) serveManifest(w http.ResponseWriter) {
	if h.server == nil {
		writeInternalError(w)
		return
	}
	manifest, err := safeValue(h.server.Manifest)
	if err != nil || !validManifest(manifest) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, manifest)
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
	if h.server == nil {
		writeInternalError(w)
		return
	}
	result, err := safeCall(func() (InitResult, error) {
		return h.server.Init(r.Context(), config)
	})
	if err != nil || !validInitResult(result) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, result)
}

func (h *httpHandler) serveTick(w http.ResponseWriter, r *http.Request) {
	var request TickRequest
	data, ok := h.decodeRequest(w, r, &request)
	if !ok {
		return
	}
	if !validTickRequest(request, data) {
		writeError(w, http.StatusBadRequest, "invalid tick request")
		return
	}
	if request.Ctx.Commands == nil {
		request.Ctx.Commands = []Command{}
	}
	if h.server == nil {
		writeInternalError(w)
		return
	}
	result, err := safeCall(func() (TickResult, error) {
		return h.server.Tick(r.Context(), request)
	})
	if err != nil || !validTickResult(result) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, result)
}

func (h *httpHandler) servePlayerEvent(w http.ResponseWriter, r *http.Request, joining bool) {
	var request PlayerEventRequest
	data, ok := h.decodeRequest(w, r, &request)
	if !ok {
		return
	}
	if !validPlayerEventRequest(request, data) {
		writeError(w, http.StatusBadRequest, "invalid player event request")
		return
	}
	if h.server == nil {
		writeInternalError(w)
		return
	}
	var (
		result PlayerEventResult
		err    error
	)
	if joining {
		result, err = safeCall(func() (PlayerEventResult, error) {
			return h.server.OnPlayerJoin(r.Context(), request)
		})
	} else {
		result, err = safeCall(func() (PlayerEventResult, error) {
			return h.server.OnPlayerLeave(r.Context(), request)
		})
	}
	if err != nil || !validPlayerEventResult(result) {
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
	if h.server == nil {
		writeInternalError(w)
		return
	}
	result, err := safeCall(func() (GameResult, error) {
		return h.server.IsGameOver(r.Context(), request)
	})
	if err != nil || !validGameResult(result) {
		writeInternalError(w)
		return
	}
	h.writeJSON(w, http.StatusOK, result)
}

func (h *httpHandler) serveCommand(w http.ResponseWriter, r *http.Request) {
	var request CommandRequest
	data, ok := h.decodeRequest(w, r, &request)
	if !ok {
		return
	}
	if !validCommandRequest(request, data) {
		writeError(w, http.StatusBadRequest, "invalid command request")
		return
	}
	if h.server == nil {
		writeInternalError(w)
		return
	}
	result, err := safeCall(func() (CommandResult, error) {
		return h.server.OnCommand(r.Context(), request)
	})
	if err != nil || !validCommandResult(result) {
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
