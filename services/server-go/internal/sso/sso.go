// Package sso implements OpenID Connect single sign-on for the W0rkTree
// server (docs/ENGINEERING_PLAN.md §12.3 [iam-sso]).
//
// It is a self-contained relying party built on the standard library:
// provider discovery (/.well-known/openid-configuration, cached), JWKS
// retrieval (cached, refreshed when an unknown key id appears), the
// authorization code flow with PKCE (S256), single-use state and nonce
// values, token exchange (client_secret_basic, client_secret_post or a
// public client), ID token verification (RS256 / ES256, iss, aud, azp, exp,
// iat, nbf, nonce) and an optional userinfo fallback for the email claim.
//
// Signed-in identities are mapped to accounts: an existing SSOIdentity
// link wins, then an existing account with the same verified email is
// linked, and otherwise (when AutoCreate is on) a new account is created.
// The session itself is issued by domain.AccountSecurity.IssueSession.
package sso

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"net/url"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Defaults.
const (
	DefaultName          = "SSO"
	DefaultUsernameClaim = "preferred_username"
	// CallbackPath is the path of the callback endpoint, appended to
	// Options.PublicURL to derive the default redirect URL.
	CallbackPath = "/api/v1/auth/sso/callback"

	defaultHTTPTimeout   = 10 * time.Second
	defaultDiscoveryTTL  = time.Hour
	defaultJWKSTTL       = time.Hour
	defaultJWKSMinFetch  = time.Minute
	defaultStateTTL      = 10 * time.Minute
	defaultClockSkew     = 2 * time.Minute
	defaultMaxPending    = 10000
	maxNameLen           = 100
	maxClientIDLen       = 256
	maxClientSecretLen   = 1024
	maxURLLen            = 2048
	maxScopes            = 20
	maxScopeLen          = 128
	maxAllowedDomains    = 100
	maxUsernameClaimLen  = 64
	maxUsernameAttempts  = 25
	usernameMaxLen       = 39
	autoPasswordEntropy  = 32 // random bytes of the unusable password
	maxResponseBodyBytes = 1 << 20
)

// DefaultScopes are requested when the configuration names none.
var DefaultScopes = []string{"openid", "email", "profile"}

// AccountCreator creates accounts for first-time SSO users. It is
// satisfied by (*iam.Service).CreateAccount, which creates the personal
// tenant and owner membership as well.
type AccountCreator interface {
	CreateAccount(ctx context.Context, username, email, password string, isAdmin bool) (*domain.Account, error)
}

// Options tune the Service. The zero value is usable.
type Options struct {
	// PublicURL is the externally visible base URL of the server
	// (e.g. "https://wt.example.com"). When set, a configuration without
	// a redirect URL defaults to PublicURL + CallbackPath.
	PublicURL string
	// HTTPClient talks to the identity provider. nil = a client with a
	// 10 second timeout.
	HTTPClient *http.Client
	// Logger receives diagnostics (IdP failures are logged here and
	// reported to users without details). nil = slog.Default().
	Logger *slog.Logger
	// Now is the clock. nil = time.Now.
	Now func() time.Time

	DiscoveryTTL  time.Duration // discovery document cache lifetime (default 1h)
	JWKSTTL       time.Duration // key set cache lifetime (default 1h)
	JWKSMinFetch  time.Duration // minimum interval between key set fetches on unknown kid (default 1m)
	StateTTL      time.Duration // lifetime of a pending login (default 10m)
	ClockSkew     time.Duration // leeway for exp / iat / nbf (default 2m)
	MaxPending    int           // bound on concurrently pending logins (default 10000)
	AllowInsecure bool          // accept http:// issuers on any host (tests only)
}

// Service implements domain.SSOProvider.
type Service struct {
	store    domain.MetaStore
	security domain.AccountSecurity
	accounts AccountCreator
	opts     Options
	client   *http.Client
	log      *slog.Logger
	now      func() time.Time

	pendingMu sync.Mutex
	pending   map[string]*pendingLogin

	discMu sync.Mutex
	disc   map[string]*discoveryEntry // by issuer

	jwksMu sync.Mutex
	jwks   map[string]*jwksEntry // by jwks_uri

	// mapMu serializes account mapping so that concurrent first logins of
	// the same identity cannot create two accounts.
	mapMu sync.Mutex
	// cfgMu serializes SaveConfig's read-modify-write.
	cfgMu sync.Mutex
}

var _ domain.SSOProvider = (*Service)(nil)

// New returns an SSO service. store holds the configuration and identity
// links, security issues sessions and accounts creates new accounts.
func New(store domain.MetaStore, security domain.AccountSecurity, accounts AccountCreator, opts Options) *Service {
	if opts.DiscoveryTTL <= 0 {
		opts.DiscoveryTTL = defaultDiscoveryTTL
	}
	if opts.JWKSTTL <= 0 {
		opts.JWKSTTL = defaultJWKSTTL
	}
	if opts.JWKSMinFetch <= 0 {
		opts.JWKSMinFetch = defaultJWKSMinFetch
	}
	if opts.StateTTL <= 0 {
		opts.StateTTL = defaultStateTTL
	}
	if opts.ClockSkew <= 0 {
		opts.ClockSkew = defaultClockSkew
	}
	if opts.MaxPending <= 0 {
		opts.MaxPending = defaultMaxPending
	}
	opts.PublicURL = strings.TrimRight(strings.TrimSpace(opts.PublicURL), "/")
	s := &Service{
		store:    store,
		security: security,
		accounts: accounts,
		opts:     opts,
		client:   opts.HTTPClient,
		log:      opts.Logger,
		now:      opts.Now,
		pending:  map[string]*pendingLogin{},
		disc:     map[string]*discoveryEntry{},
		jwks:     map[string]*jwksEntry{},
	}
	if s.client == nil {
		s.client = &http.Client{Timeout: defaultHTTPTimeout}
	}
	if s.log == nil {
		s.log = slog.Default()
	}
	if s.now == nil {
		s.now = time.Now
	}
	return s
}

// ---- errors ----

// Error is an SSO failure whose message is safe to show to end users. Kind
// is the domain sentinel (ErrInvalid, ErrUnauthenticated, ErrForbidden,
// ErrNotFound, ...) used to pick an HTTP status; the underlying cause is
// logged, never included in the message.
type Error struct {
	Msg   string
	Kind  error
	Cause error
}

func (e *Error) Error() string { return e.Msg }

// Unwrap exposes the kind and the cause to errors.Is / errors.As.
func (e *Error) Unwrap() []error {
	if e.Cause == nil {
		return []error{e.Kind}
	}
	return []error{e.Kind, e.Cause}
}

func newErr(kind error, format string, args ...any) *Error {
	return &Error{Msg: fmt.Sprintf(format, args...), Kind: kind}
}

func invalidf(format string, args ...any) error {
	return newErr(domain.ErrInvalid, format, args...)
}

// ErrNotEnabled is returned by AuthURL and Callback while SSO is disabled.
var ErrNotEnabled = &Error{Msg: "single sign-on is not enabled", Kind: domain.ErrNotFound}

// ---- configuration ----

// Config returns the stored configuration (with defaults filled in), or a
// zero configuration when none was saved. The client secret is never
// returned; HasSecret reports whether one is set.
func (s *Service) Config(ctx context.Context) (*domain.SSOConfig, error) {
	c, err := s.loadConfig(ctx)
	if err != nil {
		return nil, err
	}
	return publicView(c), nil
}

// loadConfig returns the stored configuration including the secret.
func (s *Service) loadConfig(ctx context.Context) (*domain.SSOConfig, error) {
	c, err := s.store.GetSSOConfig(ctx)
	if errors.Is(err, domain.ErrNotFound) {
		c = &domain.SSOConfig{}
	} else if err != nil {
		return nil, fmt.Errorf("sso: loading configuration: %w", err)
	} else {
		cp := *c
		c = &cp
	}
	if len(c.Scopes) == 0 {
		c.Scopes = slices.Clone(DefaultScopes)
	}
	if c.UsernameClaim == "" {
		c.UsernameClaim = DefaultUsernameClaim
	}
	if c.RedirectURL == "" && s.opts.PublicURL != "" {
		c.RedirectURL = s.opts.PublicURL + CallbackPath
	}
	if c.AllowedDomains == nil {
		c.AllowedDomains = []string{}
	}
	c.HasSecret = c.ClientSecret != ""
	return c, nil
}

func publicView(c *domain.SSOConfig) *domain.SSOConfig {
	out := *c
	out.ClientSecret = ""
	out.HasSecret = c.ClientSecret != ""
	out.Scopes = slices.Clone(c.Scopes)
	out.AllowedDomains = slices.Clone(c.AllowedDomains)
	if out.AllowedDomains == nil {
		out.AllowedDomains = []string{}
	}
	return &out
}

// SaveConfig validates and stores c. clientSecret nil keeps the stored
// secret, a pointer to "" clears it and any other value replaces it (the
// secret is write-only). Validation errors wrap domain.ErrInvalid.
func (s *Service) SaveConfig(ctx context.Context, c *domain.SSOConfig, clientSecret *string) (*domain.SSOConfig, error) {
	if c == nil {
		return nil, invalidf("configuration is required")
	}
	s.cfgMu.Lock()
	defer s.cfgMu.Unlock()

	cur, err := s.store.GetSSOConfig(ctx)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		cur = &domain.SSOConfig{}
	case err != nil:
		return nil, fmt.Errorf("sso: loading configuration: %w", err)
	}

	next, err := s.normalizeConfig(c)
	if err != nil {
		return nil, err
	}
	next.ClientSecret = cur.ClientSecret
	if clientSecret != nil {
		sec := *clientSecret
		if len(sec) > maxClientSecretLen {
			return nil, invalidf("client_secret must be at most %d bytes", maxClientSecretLen)
		}
		if strings.ContainsFunc(sec, unicode.IsControl) || !utf8.ValidString(sec) {
			return nil, invalidf("client_secret contains invalid characters")
		}
		next.ClientSecret = sec
	}
	next.HasSecret = next.ClientSecret != ""
	next.UpdatedAt = s.now().UTC()
	if err := s.store.PutSSOConfig(ctx, next); err != nil {
		return nil, fmt.Errorf("sso: saving configuration: %w", err)
	}
	if cur.Issuer != next.Issuer {
		// Drop cached provider metadata of the old issuer.
		s.discMu.Lock()
		delete(s.disc, cur.Issuer)
		s.discMu.Unlock()
	}
	return publicView(next), nil
}

var (
	scopeRe         = regexp.MustCompile(`^[\x21\x23-\x5B\x5D-\x7E]+$`) // RFC 6749 §3.3 scope-token
	usernameClaimRe = regexp.MustCompile(`^[A-Za-z0-9_.:-]+$`)
	domainLabelRe   = regexp.MustCompile(`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`)
)

// normalizeConfig trims, defaults and validates the editable fields of c.
func (s *Service) normalizeConfig(c *domain.SSOConfig) (*domain.SSOConfig, error) {
	out := &domain.SSOConfig{
		Enabled:       c.Enabled,
		Name:          strings.TrimSpace(c.Name),
		Issuer:        strings.TrimSpace(c.Issuer),
		ClientID:      strings.TrimSpace(c.ClientID),
		RedirectURL:   strings.TrimSpace(c.RedirectURL),
		AutoCreate:    c.AutoCreate,
		UsernameClaim: strings.TrimSpace(c.UsernameClaim),
	}

	if out.Name == "" {
		out.Name = DefaultName
	}
	if utf8.RuneCountInString(out.Name) > maxNameLen || !utf8.ValidString(out.Name) || strings.ContainsFunc(out.Name, unicode.IsControl) {
		return nil, invalidf("name must be 1-%d printable characters", maxNameLen)
	}

	if out.Issuer != "" {
		if err := s.checkEndpoint("issuer", out.Issuer); err != nil {
			return nil, err
		}
	} else if out.Enabled {
		return nil, invalidf("issuer is required when single sign-on is enabled")
	}

	if len(out.ClientID) > maxClientIDLen || strings.ContainsFunc(out.ClientID, func(r rune) bool { return r < 0x21 || r > 0x7e }) {
		return nil, invalidf("client_id must be at most %d printable ASCII characters without spaces", maxClientIDLen)
	}
	if out.Enabled && out.ClientID == "" {
		return nil, invalidf("client_id is required when single sign-on is enabled")
	}

	if out.RedirectURL == "" && s.opts.PublicURL != "" {
		out.RedirectURL = s.opts.PublicURL + CallbackPath
	}
	if out.RedirectURL != "" {
		if err := s.checkRedirectURL(out.RedirectURL); err != nil {
			return nil, err
		}
	} else if out.Enabled {
		return nil, invalidf("redirect_url is required when single sign-on is enabled (e.g. https://wt.example.com%s)", CallbackPath)
	}

	scopes := c.Scopes
	if len(scopes) == 0 {
		scopes = DefaultScopes
	}
	if len(scopes) > maxScopes {
		return nil, invalidf("at most %d scopes are allowed", maxScopes)
	}
	out.Scopes = make([]string, 0, len(scopes))
	for _, sc := range scopes {
		sc = strings.TrimSpace(sc)
		if sc == "" || len(sc) > maxScopeLen || !scopeRe.MatchString(sc) {
			return nil, invalidf("scope %q is not a valid OAuth scope token", sc)
		}
		if !slices.Contains(out.Scopes, sc) {
			out.Scopes = append(out.Scopes, sc)
		}
	}
	if !slices.Contains(out.Scopes, "openid") {
		return nil, invalidf("scopes must include \"openid\"")
	}

	if len(c.AllowedDomains) > maxAllowedDomains {
		return nil, invalidf("at most %d allowed domains are allowed", maxAllowedDomains)
	}
	out.AllowedDomains = make([]string, 0, len(c.AllowedDomains))
	for _, d := range c.AllowedDomains {
		d = strings.ToLower(strings.TrimPrefix(strings.TrimSpace(d), "@"))
		if !validDomain(d) {
			return nil, invalidf("allowed domain %q is not a valid domain name", d)
		}
		if !slices.Contains(out.AllowedDomains, d) {
			out.AllowedDomains = append(out.AllowedDomains, d)
		}
	}

	if out.UsernameClaim == "" {
		out.UsernameClaim = DefaultUsernameClaim
	}
	if len(out.UsernameClaim) > maxUsernameClaimLen || !usernameClaimRe.MatchString(out.UsernameClaim) {
		return nil, invalidf("username_claim must be 1-%d characters of letters, digits, '_', '.', ':' or '-'", maxUsernameClaimLen)
	}
	return out, nil
}

func validDomain(d string) bool {
	if d == "" || len(d) > 253 || !strings.Contains(d, ".") {
		return false
	}
	for _, l := range strings.Split(d, ".") {
		if !domainLabelRe.MatchString(l) {
			return false
		}
	}
	return true
}

// isLoopbackHost reports whether host (without port) is a loopback name,
// for which plain http is accepted (development setups).
func isLoopbackHost(host string) bool {
	if strings.EqualFold(host, "localhost") || strings.HasSuffix(strings.ToLower(host), ".localhost") {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}

// parseHTTPURL parses an absolute http(s) URL without credentials or a
// fragment; http is only accepted for loopback hosts (or AllowInsecure).
func (s *Service) parseHTTPURL(field, raw string) (*url.URL, error) {
	if len(raw) > maxURLLen || strings.ContainsFunc(raw, func(r rune) bool { return r <= 0x20 || r == 0x7f }) {
		return nil, invalidf("%s must be an absolute URL of at most %d characters", field, maxURLLen)
	}
	u, err := url.Parse(raw)
	if err != nil || !u.IsAbs() || u.Host == "" || u.Opaque != "" {
		return nil, invalidf("%s must be an absolute URL", field)
	}
	if u.User != nil {
		return nil, invalidf("%s must not contain credentials", field)
	}
	if u.Fragment != "" || strings.Contains(raw, "#") {
		return nil, invalidf("%s must not contain a fragment", field)
	}
	switch strings.ToLower(u.Scheme) {
	case "https":
	case "http":
		if !s.opts.AllowInsecure && !isLoopbackHost(u.Hostname()) {
			return nil, invalidf("%s must use https (http is only allowed for localhost)", field)
		}
	default:
		return nil, invalidf("%s must be an http(s) URL", field)
	}
	return u, nil
}

// checkEndpoint validates an issuer or provider endpoint URL.
func (s *Service) checkEndpoint(field, raw string) error {
	u, err := s.parseHTTPURL(field, raw)
	if err != nil {
		return err
	}
	if field == "issuer" && u.RawQuery != "" {
		return invalidf("issuer must not contain a query")
	}
	return nil
}

func (s *Service) checkRedirectURL(raw string) error {
	if _, err := s.parseHTTPURL("redirect_url", raw); err != nil {
		return err
	}
	return nil
}
