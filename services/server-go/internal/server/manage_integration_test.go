package server

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/config"
)

// TestRunManageIntegration drives the v0.3 management endpoints against the
// real stack (bbolt, IAM, review, event bus). It only runs with
// WT_INTEGRATION=1.
func TestRunManageIntegration(t *testing.T) {
	if os.Getenv("WT_INTEGRATION") == "" {
		t.Skip("set WT_INTEGRATION=1 to run the server integration test")
	}
	var logs syncBuffer
	addrCh := make(chan net.Addr, 1)
	dataDir := t.TempDir()
	cfg := config.Config{
		Addr:              "127.0.0.1:0",
		DataDir:           dataDir,
		AdminUser:         "admin",
		AdminPassword:     "integration-password",
		AllowRegistration: false,
		LogLevel:          slog.LevelWarn,
		LogFormat:         "text",
		ShutdownTimeout:   5 * time.Second,
		Version:           "9.9.9-manage",
		Stdout:            io.Discard,
		LogOutput:         &logs,
		OnListen:          func(a net.Addr) { addrCh <- a },
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	runErr := make(chan error, 1)
	go func() { runErr <- Run(ctx, cfg) }()
	var base string
	select {
	case a := <-addrCh:
		base = "http://" + a.String()
	case err := <-runErr:
		t.Fatalf("Run exited early: %v\nlogs:\n%s", err, logs.String())
	case <-time.After(15 * time.Second):
		t.Fatalf("server did not start\nlogs:\n%s", logs.String())
	}

	call := func(method, path, token string, body any, wantStatus int, out any) {
		t.Helper()
		var rd io.Reader
		if body != nil {
			buf, err := json.Marshal(body)
			if err != nil {
				t.Fatal(err)
			}
			rd = bytes.NewReader(buf)
		}
		req, err := http.NewRequest(method, base+path, rd)
		if err != nil {
			t.Fatal(err)
		}
		if body != nil {
			req.Header.Set("Content-Type", "application/json")
		}
		if token != "" {
			req.Header.Set("Authorization", "Bearer "+token)
		}
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatal(err)
		}
		defer resp.Body.Close()
		raw, _ := io.ReadAll(resp.Body)
		if resp.StatusCode != wantStatus {
			t.Fatalf("%s %s: status %d, want %d; body %s", method, path, resp.StatusCode, wantStatus, raw)
		}
		if out != nil {
			if err := json.Unmarshal(raw, out); err != nil {
				t.Fatalf("%s %s: decode: %v; body %s", method, path, err, raw)
			}
		}
	}
	type session struct {
		Token string `json:"token"`
	}
	login := func(username, password string, wantStatus int) string {
		t.Helper()
		var s session
		call("POST", "/api/v1/auth/login", "", map[string]string{"username": username, "password": password}, wantStatus, &s)
		return s.Token
	}

	admin := login("admin", "integration-password", 200)

	// System info reflects the configuration wiring.
	var sys struct {
		Version           string `json:"version"`
		AllowRegistration bool   `json:"allow_registration"`
		DataDir           string `json:"data_dir"`
		Storage           struct {
			MetaDBBytes int64 `json:"meta_db_bytes"`
		} `json:"storage"`
	}
	call("GET", "/api/v1/admin/system", admin, nil, 200, &sys)
	abs, _ := filepath.Abs(dataDir)
	if sys.Version != cfg.Version || sys.AllowRegistration || sys.DataDir != abs || sys.Storage.MetaDBBytes <= 0 {
		t.Fatalf("system = %+v", sys)
	}

	// Registration is closed, but admins can still create accounts.
	call("POST", "/api/v1/auth/register", "", map[string]string{"username": "erin", "email": "erin@example.com", "password": "erin password 1"}, 403, nil)
	var acct struct {
		Username string `json:"username"`
		IsAdmin  bool   `json:"is_admin"`
	}
	call("POST", "/api/v1/admin/accounts", admin, map[string]any{"username": "erin", "email": "erin@example.com", "password": "erin password 1", "is_admin": false}, 201, &acct)
	if acct.Username != "erin" || acct.IsAdmin {
		t.Fatalf("account = %+v", acct)
	}
	erin := login("erin", "erin password 1", 200)
	erinOther := login("erin", "erin password 1", 200)

	// Password change keeps the current token and revokes the others.
	call("POST", "/api/v1/auth/password", erin, map[string]string{"current_password": "wrong password", "new_password": "erin password 2"}, 403, nil)
	call("POST", "/api/v1/auth/password", erin, map[string]string{"current_password": "erin password 1", "new_password": "erin password 2"}, 204, nil)
	call("GET", "/api/v1/auth/me", erin, nil, 200, nil)
	call("GET", "/api/v1/auth/me", erinOther, nil, 401, nil)
	var sessions []json.RawMessage
	call("GET", "/api/v1/auth/sessions", erin, nil, 200, &sessions)
	if len(sessions) != 1 {
		t.Fatalf("erin has %d sessions, want 1 (the current one)", len(sessions))
	}
	login("erin", "erin password 1", 401)
	erin2 := login("erin", "erin password 2", 200)

	// Admin reset revokes every token of the account.
	call("POST", "/api/v1/admin/accounts/erin/password", admin, map[string]string{"password": "erin password 3"}, 204, nil)
	call("GET", "/api/v1/auth/me", erin2, nil, 401, nil)
	erin = login("erin", "erin password 3", 200)

	// Branches and tags on a real tree.
	call("POST", "/api/v1/tenants/erin/trees", erin, map[string]string{"name": "app"}, 201, nil)
	call("POST", "/api/v1/trees/erin/app/branches", erin, map[string]string{"name": "feature/one"}, 201, nil)
	call("POST", "/api/v1/trees/erin/app/branches", erin, map[string]string{"name": "feature/one"}, 409, nil)
	call("POST", "/api/v1/trees/erin/app/tags", erin, map[string]string{"name": "v1", "target": "main"}, 400, nil) // no snapshots yet
	call("DELETE", "/api/v1/trees/erin/app/branches/main", erin, nil, 403, nil)                                    // default branch
	call("DELETE", "/api/v1/trees/erin/app/branches/feature/one", erin, nil, 204, nil)

	// Tenant update and search.
	var tenant struct {
		Name string `json:"name"`
		Plan string `json:"plan"`
	}
	call("PATCH", "/api/v1/tenants/erin", erin, map[string]string{"plan": "pro"}, 403, nil)
	call("PATCH", "/api/v1/tenants/erin", erin, map[string]string{"name": "Erin's space"}, 200, &tenant)
	call("PATCH", "/api/v1/tenants/erin", admin, map[string]string{"plan": "pro"}, 200, &tenant)
	if tenant.Name != "Erin's space" || tenant.Plan != "pro" {
		t.Fatalf("tenant = %+v", tenant)
	}
	var res struct {
		Tenants  []json.RawMessage `json:"tenants"`
		Trees    []json.RawMessage `json:"trees"`
		Accounts []json.RawMessage `json:"accounts"`
	}
	call("GET", "/api/v1/search?q=ERIN", admin, nil, 200, &res)
	if len(res.Tenants) != 1 || len(res.Accounts) != 1 {
		t.Fatalf("search = %+v", res)
	}
	call("GET", "/api/v1/search?q=app", erin, nil, 200, &res)
	if len(res.Trees) != 1 || len(res.Accounts) != 0 {
		t.Fatalf("search = %+v", res)
	}

	cancel()
	select {
	case err := <-runErr:
		if err != nil {
			t.Fatalf("Run: %v\nlogs:\n%s", err, logs.String())
		}
	case <-time.After(15 * time.Second):
		t.Fatal("Run did not return after cancel")
	}
}
