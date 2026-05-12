package httpapi

import (
	"encoding/json"
	"net/http"

	"github.com/ramizik/worktree/server-go/internal/auth"
)

type LoginRequest struct {
	TokenID string `json:"token_id"`
	Secret  string `json:"secret"`
}

type LoginResponse struct {
	Token string `json:"token"`
}

func HandleLogin(authenticator auth.Authenticator, jwtAuth auth.JWTAuthenticator) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
			return
		}

		var req LoginRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			writeError(w, http.StatusBadRequest, "InvalidJSON", "invalid json body")
			return
		}

		principal, err := authenticator.AuthenticateBearer(r.Context(), req.Secret)
		if err != nil {
			writeError(w, http.StatusUnauthorized, "Unauthorized", "invalid credentials")
			return
		}

		if req.TokenID != "" && principal.TokenID != "" && req.TokenID != principal.TokenID {
			writeError(w, http.StatusUnauthorized, "Unauthorized", "invalid credentials")
			return
		}

		token, err := jwtAuth.GenerateToken(principal)
		if err != nil {
			writeError(w, http.StatusInternalServerError, "InternalError", "failed to generate token")
			return
		}

		writeJSON(w, http.StatusOK, LoginResponse{Token: token})
	}
}
