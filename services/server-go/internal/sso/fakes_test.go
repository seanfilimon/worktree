package sso

import (
	"context"
	"crypto"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"math/big"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// ---- keys (generated once per test binary) ----

var (
	keysOnce sync.Once
	rsaKeyA  *rsa.PrivateKey
	rsaKeyB  *rsa.PrivateKey
	ecKey    *ecdsa.PrivateKey
)

func testKeys(t testing.TB) {
	t.Helper()
	keysOnce.Do(func() {
		var err error
		if rsaKeyA, err = rsa.GenerateKey(rand.Reader, 2048); err != nil {
			panic(err)
		}
		if rsaKeyB, err = rsa.GenerateKey(rand.Reader, 2048); err != nil {
			panic(err)
		}
		if ecKey, err = ecdsa.GenerateKey(elliptic.P256(), rand.Reader); err != nil {
			panic(err)
		}
	})
}

func b64e(b []byte) string { return base64.RawURLEncoding.EncodeToString(b) }

func rsaJWK(kid string, pub *rsa.PublicKey) map[string]any {
	return map[string]any{"kty": "RSA", "kid": kid, "use": "sig", "alg": "RS256",
		"n": b64e(pub.N.Bytes()), "e": b64e(big.NewInt(int64(pub.E)).Bytes())}
}

func ecJWK(kid string, pub *ecdsa.PublicKey) map[string]any {
	raw, err := pub.Bytes()
	if err != nil {
		panic(err)
	}
	return map[string]any{"kty": "EC", "kid": kid, "use": "sig", "alg": "ES256", "crv": "P-256",
		"x": b64e(raw[1:33]), "y": b64e(raw[33:65])}
}

// signJWT produces a compact JWS over claims.
func signJWT(alg, kid string, key crypto.Signer, claims map[string]any) string {
	hdr := map[string]any{"alg": alg, "typ": "JWT"}
	if kid != "" {
		hdr["kid"] = kid
	}
	h, _ := json.Marshal(hdr)
	p, _ := json.Marshal(claims)
	signing := b64e(h) + "." + b64e(p)
	d := sha256.Sum256([]byte(signing))
	var sig []byte
	switch alg {
	case "RS256":
		s, err := rsa.SignPKCS1v15(rand.Reader, key.(*rsa.PrivateKey), crypto.SHA256, d[:])
		if err != nil {
			panic(err)
		}
		sig = s
	case "ES256":
		r, s, err := ecdsa.Sign(rand.Reader, key.(*ecdsa.PrivateKey), d[:])
		if err != nil {
			panic(err)
		}
		sig = make([]byte, 64)
		r.FillBytes(sig[:32])
		s.FillBytes(sig[32:])
	default:
		sig = []byte("not-a-signature")
	}
	return signing + "." + b64e(sig)
}

// ---- fake identity provider ----

type authCode struct {
	challenge   string
	nonce       string
	redirectURI string
	claims      map[string]any
}

type fakeIdP struct {
	srv *httptest.Server

	mu          sync.Mutex
	issuer      string // advertised issuer ("" = srv.URL)
	clientID    string
	secret      string
	authMethods []string
	codes       map[string]*authCode
	signAlg     string
	signKid     string
	signKey     crypto.Signer
	jwks        []map[string]any
	userinfo    map[string]any
	lastAuth    string // "basic" | "post" | "none"
	discHits    int
	jwksHits    int
	tokenHits   int
	uiHits      int
	tokenBody   string // non-empty: returned verbatim with tokenStatus
	tokenStatus int
}

func newFakeIdP(t *testing.T) *fakeIdP {
	t.Helper()
	testKeys(t)
	idp := &fakeIdP{
		clientID: "wt-client",
		secret:   "s3cr:et/+&",
		codes:    map[string]*authCode{},
		signAlg:  "RS256",
		signKid:  "key-a",
		signKey:  rsaKeyA,
		jwks:     []map[string]any{rsaJWK("key-a", &rsaKeyA.PublicKey), ecJWK("key-ec", &ecKey.PublicKey)},
	}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /.well-known/openid-configuration", idp.handleDiscovery)
	mux.HandleFunc("GET /jwks", idp.handleJWKS)
	mux.HandleFunc("POST /token", idp.handleToken)
	mux.HandleFunc("GET /userinfo", idp.handleUserinfo)
	idp.srv = httptest.NewServer(mux)
	t.Cleanup(idp.srv.Close)
	return idp
}

func (f *fakeIdP) URL() string { return f.srv.URL }

func (f *fakeIdP) handleDiscovery(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.discHits++
	iss := f.issuer
	if iss == "" {
		iss = f.srv.URL
	}
	md := map[string]any{
		"issuer":                                iss,
		"authorization_endpoint":                f.srv.URL + "/authorize?tenant=t1",
		"token_endpoint":                        f.srv.URL + "/token",
		"userinfo_endpoint":                     f.srv.URL + "/userinfo",
		"jwks_uri":                              f.srv.URL + "/jwks",
		"id_token_signing_alg_values_supported": []string{"RS256", "ES256"},
	}
	if f.authMethods != nil {
		md["token_endpoint_auth_methods_supported"] = f.authMethods
	}
	_ = json.NewEncoder(w).Encode(md)
}

func (f *fakeIdP) handleJWKS(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.jwksHits++
	_ = json.NewEncoder(w).Encode(map[string]any{"keys": f.jwks})
}

func oauthError(w http.ResponseWriter, status int, code string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]string{"error": code, "error_description": "fake idp: " + code})
}

func (f *fakeIdP) handleToken(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.tokenHits++
	if f.tokenBody != "" {
		w.WriteHeader(f.tokenStatus)
		_, _ = io.WriteString(w, f.tokenBody)
		return
	}
	if err := r.ParseForm(); err != nil {
		oauthError(w, 400, "invalid_request")
		return
	}
	// Client authentication.
	if u, p, ok := r.BasicAuth(); ok {
		u, _ = url.QueryUnescape(u)
		p, _ = url.QueryUnescape(p)
		if u != f.clientID || p != f.secret {
			oauthError(w, 401, "invalid_client")
			return
		}
		f.lastAuth = "basic"
	} else if r.PostForm.Get("client_secret") != "" {
		if r.PostForm.Get("client_id") != f.clientID || r.PostForm.Get("client_secret") != f.secret {
			oauthError(w, 401, "invalid_client")
			return
		}
		f.lastAuth = "post"
	} else {
		if r.PostForm.Get("client_id") != f.clientID || f.secret != "" {
			oauthError(w, 401, "invalid_client")
			return
		}
		f.lastAuth = "none"
	}
	if r.PostForm.Get("grant_type") != "authorization_code" {
		oauthError(w, 400, "unsupported_grant_type")
		return
	}
	code := r.PostForm.Get("code")
	ac := f.codes[code]
	delete(f.codes, code) // single use
	if ac == nil || ac.redirectURI != r.PostForm.Get("redirect_uri") {
		oauthError(w, 400, "invalid_grant")
		return
	}
	sum := sha256.Sum256([]byte(r.PostForm.Get("code_verifier")))
	if b64e(sum[:]) != ac.challenge {
		oauthError(w, 400, "invalid_grant")
		return
	}
	claims := map[string]any{}
	for k, v := range ac.claims {
		claims[k] = v
	}
	if _, ok := claims["nonce"]; !ok {
		claims["nonce"] = ac.nonce
	}
	idt := signJWT(f.signAlg, f.signKid, f.signKey, claims)
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]any{
		"access_token": "at-" + code, "token_type": "Bearer", "expires_in": 300, "id_token": idt,
	})
}

func (f *fakeIdP) handleUserinfo(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.uiHits++
	if !strings.HasPrefix(r.Header.Get("Authorization"), "Bearer at-") || f.userinfo == nil {
		w.WriteHeader(http.StatusUnauthorized)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(f.userinfo)
}

// authorize simulates the user approving the login at the IdP: it records
// a code bound to the PKCE challenge and nonce of authURL.
func (f *fakeIdP) authorize(t *testing.T, authURL string, claims map[string]any) (code, state string) {
	t.Helper()
	u, err := url.Parse(authURL)
	if err != nil {
		t.Fatal(err)
	}
	q := u.Query()
	if q.Get("response_type") != "code" || q.Get("client_id") != f.clientID ||
		q.Get("code_challenge_method") != "S256" || q.Get("tenant") != "t1" {
		t.Fatalf("bad authorization request: %s", authURL)
	}
	code = fmt.Sprintf("code-%d", time.Now().UnixNano())
	f.mu.Lock()
	f.codes[code] = &authCode{challenge: q.Get("code_challenge"), nonce: q.Get("nonce"), redirectURI: q.Get("redirect_uri"), claims: claims}
	f.mu.Unlock()
	return code, q.Get("state")
}

// defaultClaims returns a valid claim set for the IdP and client.
func (f *fakeIdP) defaultClaims(sub string) map[string]any {
	now := time.Now()
	return map[string]any{
		"iss":                f.srv.URL,
		"aud":                f.clientID,
		"sub":                sub,
		"iat":                now.Unix(),
		"exp":                now.Add(5 * time.Minute).Unix(),
		"email":              sub + "@example.com",
		"email_verified":     true,
		"preferred_username": sub,
		"name":               "User " + sub,
	}
}

// ---- fake store / security / account creator ----

type memStore struct {
	domain.MetaStore

	mu       sync.Mutex
	cfg      *domain.SSOConfig
	idents   map[[2]string]domain.SSOIdentity
	accounts map[string]domain.Account
	failGet  error
}

func newMemStore() *memStore {
	return &memStore{idents: map[[2]string]domain.SSOIdentity{}, accounts: map[string]domain.Account{}}
}

func (m *memStore) GetSSOConfig(context.Context) (*domain.SSOConfig, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.failGet != nil {
		return nil, m.failGet
	}
	if m.cfg == nil {
		return nil, fmt.Errorf("sso config: %w", domain.ErrNotFound)
	}
	c := *m.cfg
	return &c, nil
}

func (m *memStore) PutSSOConfig(_ context.Context, c *domain.SSOConfig) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	cp := *c
	m.cfg = &cp
	return nil
}

func (m *memStore) PutSSOIdentity(_ context.Context, id *domain.SSOIdentity) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.idents[[2]string{id.Issuer, id.Subject}] = *id
	return nil
}

func (m *memStore) GetSSOIdentity(_ context.Context, issuer, subject string) (*domain.SSOIdentity, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	id, ok := m.idents[[2]string{issuer, subject}]
	if !ok {
		return nil, fmt.Errorf("identity: %w", domain.ErrNotFound)
	}
	return &id, nil
}

func (m *memStore) ListAccountSSOIdentities(_ context.Context, accountID string) ([]domain.SSOIdentity, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := []domain.SSOIdentity{}
	for _, id := range m.idents {
		if id.AccountID == accountID {
			out = append(out, id)
		}
	}
	return out, nil
}

func (m *memStore) GetAccount(_ context.Context, id string) (*domain.Account, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	a, ok := m.accounts[id]
	if !ok {
		return nil, fmt.Errorf("account: %w", domain.ErrNotFound)
	}
	return &a, nil
}

func (m *memStore) ListAccounts(context.Context) ([]domain.Account, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	out := []domain.Account{}
	for _, a := range m.accounts {
		out = append(out, a)
	}
	return out, nil
}

func (m *memStore) UpdateAccount(_ context.Context, a *domain.Account) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, ok := m.accounts[a.ID]; !ok {
		return fmt.Errorf("account: %w", domain.ErrNotFound)
	}
	m.accounts[a.ID] = *a
	return nil
}

func (m *memStore) addAccount(username, email string, disabled bool) domain.Account {
	m.mu.Lock()
	defer m.mu.Unlock()
	a := domain.Account{ID: "acct-" + username, Username: username, Email: email, DisplayName: username, Disabled: disabled}
	m.accounts[a.ID] = a
	return a
}

func (m *memStore) accountCount() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.accounts)
}

// fakeCreator mimics iam.Service.CreateAccount's uniqueness and validation.
type fakeCreator struct {
	store     *memStore
	passwords map[string]string
}

func (c *fakeCreator) CreateAccount(_ context.Context, username, email, password string, isAdmin bool) (*domain.Account, error) {
	if err := iam.ValidateUsername(username); err != nil {
		return nil, err
	}
	if err := iam.ValidatePassword(password); err != nil {
		return nil, err
	}
	c.store.mu.Lock()
	defer c.store.mu.Unlock()
	for _, a := range c.store.accounts {
		if a.Username == username || strings.EqualFold(a.Email, email) {
			return nil, fmt.Errorf("taken: %w", domain.ErrAlreadyExists)
		}
	}
	a := domain.Account{ID: "acct-" + username, Username: username, Email: email, DisplayName: username, IsAdmin: isAdmin}
	c.store.accounts[a.ID] = a
	c.passwords[username] = password
	return &a, nil
}

type fakeSecurity struct {
	domain.AccountSecurity

	mu       sync.Mutex
	sessions []string // account ids
	metas    []domain.SessionMeta
	fail     error
}

func (f *fakeSecurity) IssueSession(_ context.Context, accountID string, mfa bool, meta domain.SessionMeta) (string, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.fail != nil {
		return "", f.fail
	}
	if mfa {
		return "", fmt.Errorf("unexpected mfa session")
	}
	f.sessions = append(f.sessions, accountID)
	f.metas = append(f.metas, meta)
	return "sess-" + accountID, nil
}

// ---- harness ----

type harness struct {
	idp     *fakeIdP
	store   *memStore
	sec     *fakeSecurity
	creator *fakeCreator
	svc     *Service
	clock   *fakeClock
}

type fakeClock struct {
	mu     sync.Mutex
	offset time.Duration
}

func (c *fakeClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return time.Now().Add(c.offset)
}

func (c *fakeClock) advance(d time.Duration) {
	c.mu.Lock()
	c.offset += d
	c.mu.Unlock()
}

func quietLogger() *slog.Logger { return slog.New(slog.NewTextHandler(io.Discard, nil)) }

func newHarness(t *testing.T, mutate ...func(*domain.SSOConfig, *Options)) *harness {
	t.Helper()
	h := &harness{idp: newFakeIdP(t), store: newMemStore(), sec: &fakeSecurity{}, clock: &fakeClock{}}
	h.creator = &fakeCreator{store: h.store, passwords: map[string]string{}}
	cfg := &domain.SSOConfig{
		Enabled:     true,
		Name:        "Fake IdP",
		Issuer:      h.idp.URL(),
		ClientID:    h.idp.clientID,
		RedirectURL: "http://localhost:7420" + CallbackPath,
		AutoCreate:  true,
	}
	opts := Options{Logger: quietLogger(), Now: h.clock.Now}
	for _, m := range mutate {
		m(cfg, &opts)
	}
	h.svc = New(h.store, h.sec, h.creator, opts)
	secret := h.idp.secret
	if _, err := h.svc.SaveConfig(context.Background(), cfg, &secret); err != nil {
		t.Fatalf("SaveConfig: %v", err)
	}
	return h
}

// login runs a full flow with the given claims (nil = defaults for sub).
func (h *harness) login(t *testing.T, claims map[string]any) (string, *domain.Account, string, error) {
	t.Helper()
	ctx := context.Background()
	authURL, err := h.svc.AuthURL(ctx, "/admin/trees?x=1")
	if err != nil {
		t.Fatalf("AuthURL: %v", err)
	}
	code, state := h.idp.authorize(t, authURL, claims)
	return h.svc.Callback(ctx, code, state, domain.SessionMeta{RemoteIP: "192.0.2.1", UserAgent: "test"})
}
