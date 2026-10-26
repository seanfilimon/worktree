package httpapi

// Device authorization grant (RFC 8628) and refresh tokens
// (docs/ENGINEERING_PLAN.md §16.6, workstream device-auth).
//
//	POST /api/v1/auth/device                      {client_id?, client_name?, scopes?|scope?} -> device authorization (public)
//	POST /api/v1/auth/device/token                {device_code, grant_type?}               -> token pair | 400 authorization_pending|slow_down|access_denied|expired_token|invalid_grant (public)
//	GET  /api/v1/auth/device/{user_code}                                                   -> device request (signed in)
//	POST /api/v1/auth/device/{user_code}/approve  {code?}                                  -> device request (signed in; code = TOTP when MFA must be verified)
//	POST /api/v1/auth/device/{user_code}/deny                                              -> device request (signed in)
//	POST /api/v1/auth/refresh                     {refresh_token}                          -> token pair | 401 invalid_grant (public)
//
// The iam service implements these through deviceAuthority and
// refreshAuthority (type assertions on Deps.Security); without them the
// routes answer 501 and logins return plain sessions.

import (
	"context"
	"errors"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// deviceGrantType is the RFC 8628 grant type of the token request.
const deviceGrantType = "urn:ietf:params:oauth:grant-type:device_code"

// deviceAuthority is the device flow of iam.Service.
type deviceAuthority interface {
	StartDeviceAuthorization(ctx context.Context, clientID, clientName string, scopes []string, m domain.SessionMeta) (*iam.DeviceAuthorization, error)
	PollDeviceToken(ctx context.Context, deviceCode string, m domain.SessionMeta) (*iam.TokenPair, error)
	DeviceRequest(ctx context.Context, userCode string, p *domain.Principal) (*iam.DeviceRequest, error)
	ApproveDevice(ctx context.Context, userCode string, p *domain.Principal, mfaCode string) (*iam.DeviceRequest, error)
	DenyDevice(ctx context.Context, userCode string, p *domain.Principal) (*iam.DeviceRequest, error)
	PublicURL() string
}

// refreshAuthority issues and rotates token pairs (iam.Service).
type refreshAuthority interface {
	UpgradeSession(ctx context.Context, rawSession string, m domain.SessionMeta) (*iam.TokenPair, error)
	Refresh(ctx context.Context, refreshToken string, m domain.SessionMeta) (*iam.TokenPair, error)
}

type deviceAuthorizationRequest struct {
	ClientID   string   `json:"client_id"`
	ClientName string   `json:"client_name"`
	Scopes     []string `json:"scopes"`
	// Scope is the RFC 6749 space-separated form of Scopes.
	Scope string `json:"scope"`
}

type deviceAuthorizationResponse struct {
	DeviceCode              string `json:"device_code"`
	UserCode                string `json:"user_code"`
	VerificationURI         string `json:"verification_uri"`
	VerificationURIComplete string `json:"verification_uri_complete"`
	ExpiresIn               int    `json:"expires_in"` // seconds
	Interval                int    `json:"interval"`   // seconds between polls
}

type deviceTokenRequest struct {
	DeviceCode string `json:"device_code"`
	GrantType  string `json:"grant_type"`
}

type deviceApproveRequest struct {
	// Code is a TOTP or recovery code, needed when the device request
	// reports mfa_required.
	Code string `json:"code"`
}

type refreshRequest struct {
	RefreshToken string `json:"refresh_token"`
}

// tokenPairResponse answers the device token and refresh endpoints.
type tokenPairResponse struct {
	Token            string          `json:"token"` // access token (a session)
	TokenType        string          `json:"token_type"`
	ExpiresIn        int             `json:"expires_in"` // access token lifetime, seconds
	RefreshToken     string          `json:"refresh_token"`
	RefreshExpiresIn int             `json:"refresh_expires_in"` // seconds
	Account          *domain.Account `json:"account"`
}

func pairResponse(p *iam.TokenPair) tokenPairResponse {
	now := p.Session.CreatedAt // lifetimes count from the issue
	return tokenPairResponse{
		Token:            p.AccessToken,
		TokenType:        "Bearer",
		ExpiresIn:        secondsUntil(p.AccessExpiresAt, now),
		RefreshToken:     p.RefreshToken,
		RefreshExpiresIn: secondsUntil(p.RefreshExpiresAt, now),
		Account:          p.Account,
	}
}

func secondsUntil(t, now time.Time) int {
	d := t.Sub(now).Round(time.Second)
	if d < 0 {
		return 0
	}
	return int(d / time.Second)
}

func (a *api) registerDeviceRoutes() {
	a.handle("POST /api/v1/auth/device", a.handleDeviceAuthorize)
	a.handle("POST /api/v1/auth/device/token", a.handleDeviceToken)
	a.handle("GET /api/v1/auth/device/{user_code}", a.handleDeviceGet)
	a.handle("POST /api/v1/auth/device/{user_code}/approve", a.handleDeviceApprove)
	a.handle("POST /api/v1/auth/device/{user_code}/deny", a.handleDeviceDeny)
	a.handle("POST /api/v1/auth/refresh", a.handleRefresh)
}

func (a *api) deviceAuth() (deviceAuthority, error) {
	if d, ok := a.d.Security.(deviceAuthority); ok {
		return d, nil
	}
	return nil, errIAMNotImplemented("device authorization")
}

func (a *api) refreshAuth() (refreshAuthority, error) {
	if r, ok := a.d.Security.(refreshAuthority); ok {
		return r, nil
	}
	return nil, errIAMNotImplemented("refresh tokens")
}

// deviceError maps iam device-flow errors onto the HTTP envelope.
func deviceError(err error) error {
	var de *iam.DeviceError
	if errors.As(err, &de) {
		return &apiError{status: http.StatusBadRequest, code: de.Code, msg: de.Message}
	}
	if errors.Is(err, iam.ErrRateLimited) {
		return &apiError{status: http.StatusTooManyRequests, code: "rate_limited", msg: err.Error()}
	}
	return err
}

// baseURL is the server's external base URL: the configured public URL,
// else the scheme and host of the request.
func baseURL(r *http.Request, public string) string {
	if public != "" {
		return public
	}
	scheme := "http"
	if r.TLS != nil || strings.EqualFold(r.Header.Get("X-Forwarded-Proto"), "https") {
		scheme = "https"
	}
	host := r.Host
	if host == "" {
		host = "localhost"
	}
	return scheme + "://" + host
}

// handleDeviceAuthorize answers POST /api/v1/auth/device (public).
func (a *api) handleDeviceAuthorize(w http.ResponseWriter, r *http.Request) error {
	dev, err := a.deviceAuth()
	if err != nil {
		return err
	}
	var req deviceAuthorizationRequest
	if r.ContentLength != 0 {
		if err := decodeJSON(w, r, &req); err != nil {
			return err
		}
	}
	if len(req.ClientName) > 200 || len(req.Scope) > 4096 {
		return badRequestf("client_name or scope is too long")
	}
	scopes := req.Scopes
	if len(scopes) == 0 && strings.TrimSpace(req.Scope) != "" {
		scopes = strings.Fields(req.Scope)
	}
	if _, err := validateScopes(scopes, nil); err != nil {
		return err
	}
	if req.ClientName == "" {
		req.ClientName = r.UserAgent()
	}
	res, err := dev.StartDeviceAuthorization(r.Context(), req.ClientID, req.ClientName, scopes, sessionMeta(r))
	if err != nil {
		return deviceError(err)
	}
	verify := baseURL(r, dev.PublicURL()) + "/admin/device"
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, deviceAuthorizationResponse{
		DeviceCode:              res.DeviceCode,
		UserCode:                res.UserCode,
		VerificationURI:         verify,
		VerificationURIComplete: verify + "?code=" + url.QueryEscape(res.UserCode),
		ExpiresIn:               int(res.ExpiresIn / time.Second),
		Interval:                int(res.Interval / time.Second),
	})
	return nil
}

// handleDeviceToken answers POST /api/v1/auth/device/token (public; the
// device polls it).
func (a *api) handleDeviceToken(w http.ResponseWriter, r *http.Request) error {
	dev, err := a.deviceAuth()
	if err != nil {
		return err
	}
	var req deviceTokenRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if req.GrantType != "" && req.GrantType != deviceGrantType {
		return &apiError{status: http.StatusBadRequest, code: "unsupported_grant_type", msg: "grant_type must be " + deviceGrantType}
	}
	if strings.TrimSpace(req.DeviceCode) == "" {
		return badRequestf("device_code is required")
	}
	if len(req.DeviceCode) > 256 {
		return deviceError(iam.ErrInvalidGrant)
	}
	w.Header().Set("Cache-Control", "no-store")
	pair, err := dev.PollDeviceToken(r.Context(), req.DeviceCode, sessionMeta(r))
	if err != nil {
		return deviceError(err)
	}
	acct := pair.Account
	a.auditAs(r, acct, "auth.login", acct.TenantID, "account", acct.ID,
		map[string]string{"method": "device", "session": pair.Session.ID, "client": strings.TrimPrefix(pair.Session.Name, iam.DeviceSessionPrefix)})
	writeJSON(w, http.StatusOK, pairResponse(pair))
	return nil
}

// deviceCaller requires a signed-in user without token scopes (approving
// a device is an account-security change).
func (a *api) deviceCaller(r *http.Request) (deviceAuthority, *domain.Principal, error) {
	p, err := requireFullUser(r)
	if err != nil {
		return nil, nil, err
	}
	dev, err := a.deviceAuth()
	if err != nil {
		return nil, nil, err
	}
	return dev, p, nil
}

// handleDeviceGet answers GET /api/v1/auth/device/{user_code}.
func (a *api) handleDeviceGet(w http.ResponseWriter, r *http.Request) error {
	dev, p, err := a.deviceCaller(r)
	if err != nil {
		return err
	}
	req, err := dev.DeviceRequest(r.Context(), r.PathValue("user_code"), p)
	if err != nil {
		return deviceError(err)
	}
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, req)
	return nil
}

// handleDeviceApprove answers POST /api/v1/auth/device/{user_code}/approve.
func (a *api) handleDeviceApprove(w http.ResponseWriter, r *http.Request) error {
	dev, p, err := a.deviceCaller(r)
	if err != nil {
		return err
	}
	var body deviceApproveRequest
	if r.ContentLength != 0 {
		if err := decodeJSON(w, r, &body); err != nil {
			return err
		}
	}
	if len(body.Code) > 64 {
		return badRequestf("code is too long")
	}
	req, err := dev.ApproveDevice(r.Context(), r.PathValue("user_code"), p, normalizeCode(body.Code))
	if err != nil {
		return deviceError(err)
	}
	a.audit(r, "auth.device_approve", p.TenantID, "device", req.UserCode,
		map[string]string{"client": req.ClientName, "scopes": strings.Join(req.Scopes, ",")})
	writeJSON(w, http.StatusOK, req)
	return nil
}

// handleDeviceDeny answers POST /api/v1/auth/device/{user_code}/deny.
func (a *api) handleDeviceDeny(w http.ResponseWriter, r *http.Request) error {
	dev, p, err := a.deviceCaller(r)
	if err != nil {
		return err
	}
	req, err := dev.DenyDevice(r.Context(), r.PathValue("user_code"), p)
	if err != nil {
		return deviceError(err)
	}
	a.audit(r, "auth.device_deny", p.TenantID, "device", req.UserCode, map[string]string{"client": req.ClientName})
	writeJSON(w, http.StatusOK, req)
	return nil
}

// handleRefresh answers POST /api/v1/auth/refresh (public: the refresh
// token is the credential; send no Authorization header).
func (a *api) handleRefresh(w http.ResponseWriter, r *http.Request) error {
	ra, err := a.refreshAuth()
	if err != nil {
		return err
	}
	var req refreshRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.RefreshToken = strings.TrimSpace(req.RefreshToken)
	if req.RefreshToken == "" {
		return badRequestf("refresh_token is required")
	}
	if len(req.RefreshToken) > 256 {
		return &apiError{status: http.StatusUnauthorized, code: "invalid_grant", msg: "invalid or expired refresh token"}
	}
	w.Header().Set("Cache-Control", "no-store")
	pair, err := ra.Refresh(r.Context(), req.RefreshToken, sessionMeta(r))
	if err != nil {
		if errors.Is(err, iam.ErrRefreshReused) {
			a.auditAs(r, &domain.Account{Username: "anonymous"}, "auth.refresh_reuse", "", "refresh_token", "", nil)
			return &apiError{status: http.StatusUnauthorized, code: "invalid_grant",
				msg: "refresh token reuse detected; the session was revoked, sign in again"}
		}
		if errors.Is(err, domain.ErrUnauthenticated) {
			return &apiError{status: http.StatusUnauthorized, code: "invalid_grant", msg: "invalid or expired refresh token"}
		}
		return err
	}
	writeJSON(w, http.StatusOK, pairResponse(pair))
	return nil
}

// loginSession turns the session token of a completed login into the
// login response. With refresh-token support (iam.Service) the session is
// exchanged for a short-lived access token plus a refresh token; otherwise
// the plain session is returned.
func (a *api) loginSession(r *http.Request, token string, acct *domain.Account) (sessionResponse, error) {
	ra, ok := a.d.Security.(refreshAuthority)
	if !ok || token == "" {
		return sessionResponse{Token: token, Account: acct}, nil
	}
	pair, err := ra.UpgradeSession(r.Context(), token, sessionMeta(r))
	if err != nil {
		return sessionResponse{}, err
	}
	p := pairResponse(pair)
	return sessionResponse{
		Token:            p.Token,
		Account:          acct,
		TokenType:        p.TokenType,
		ExpiresIn:        p.ExpiresIn,
		RefreshToken:     p.RefreshToken,
		RefreshExpiresIn: p.RefreshExpiresIn,
	}, nil
}
