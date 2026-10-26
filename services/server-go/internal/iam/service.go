package iam

import (
	"context"
	"errors"
	"fmt"
	"runtime"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// DefaultSessionTTL is the lifetime of Login session tokens when
// Options.SessionTTL is zero.
const DefaultSessionTTL = 30 * 24 * time.Hour

const (
	sessionTokenName   = "session"
	personalTenantPlan = "free"
	// touchInterval bounds how often a token's LastUsedAt is persisted.
	touchInterval       = time.Minute
	touchSweepThreshold = 4096
	maxConcurrentHashes = 16
)

// Options configures the IAM service.
type Options struct {
	// SessionTTL is the lifetime of tokens issued by Login (default 30 days).
	SessionTTL time.Duration
	// AllowRegistration enables self-service Register.
	AllowRegistration bool

	// DisableRateLimit turns off login failure rate limiting (for tests).
	DisableRateLimit bool
	// MaxLoginFailures is the number of failed logins per (username, client
	// IP) tolerated within LoginFailureWindow (default 10).
	MaxLoginFailures int
	// LoginFailureWindow is the rate limiting window (default 5 minutes).
	LoginFailureWindow time.Duration

	// MaxMFAFailures is the number of failed second-factor attempts
	// (login codes, TOTP confirmation, recovery-code regeneration and
	// password re-entry for disabling MFA) tolerated per account within
	// MFAFailureWindow (default 10). Unlike login rate limiting it cannot
	// be disabled.
	MaxMFAFailures int
	// MFAFailureWindow is the MFA rate limiting window (default 15 minutes).
	MFAFailureWindow time.Duration

	// Password sets the argon2id cost for new hashes; zero fields use
	// DefaultPasswordParams. Tests may lower it to run faster.
	Password PasswordParams

	// Clock returns the current time (default time.Now). For tests.
	Clock func() time.Time

	// device-auth (§16.6): token pairs and the device authorization grant
	// (refresh.go, device.go). Zero values use the Default* constants.
	AccessTTL         time.Duration // access sessions issued with refresh tokens (1 h)
	RefreshTTL        time.Duration // refresh tokens (30 days)
	RefreshReuseGrace time.Duration // previous-token race window (10 s; negative = none)
	// PublicURL is the server's external base URL; device verification
	// links point at <PublicURL>/admin/device. "" = derived per request.
	PublicURL string
}

// Service implements domain.Authenticator, domain.Authorizer and
// domain.AccountSecurity on top of a domain.MetaStore. It is safe for
// concurrent use.
type Service struct {
	store      domain.MetaStore
	opts       Options
	params     PasswordParams
	now        func() time.Time
	limiter    *loginLimiter // nil when rate limiting is disabled
	mfaLimiter *loginLimiter // per-account second-factor failures; always set
	pending    *mfaPending   // first-step MFA login tokens
	hashSem    chan struct{} // bounds concurrent argon2 work (memory)

	// mfaMu serializes read-verify-write cycles on MFA state so a TOTP step
	// or recovery code cannot be used twice by concurrent requests.
	mfaMu sync.Mutex
	// invMu serializes invitation acceptance (single use).
	invMu sync.Mutex
	// resetMu serializes password-reset redemption (single use).
	resetMu sync.Mutex

	// regMu serializes account creation so the existence checks and the
	// multi-record creation in register are atomic within this process.
	regMu sync.Mutex

	dummyOnce sync.Once
	dummyHash string
	dummyErr  error

	touchMu sync.Mutex
	touched map[string]time.Time // token id -> last persisted use

	device deviceLimits // device-auth (§16.6): rate limits of device.go (zero value ready)
}

var (
	_ domain.Authenticator   = (*Service)(nil)
	_ domain.Authorizer      = (*Service)(nil)
	_ domain.AccountSecurity = (*Service)(nil)
)

var (
	errBadCredentials = fmt.Errorf("invalid username or password: %w", domain.ErrUnauthenticated)
	errInvalidToken   = fmt.Errorf("invalid or expired token: %w", domain.ErrUnauthenticated)
)

// New returns an IAM service backed by store.
func New(store domain.MetaStore, opts Options) *Service {
	if opts.SessionTTL <= 0 {
		opts.SessionTTL = DefaultSessionTTL
	}
	if opts.MaxLoginFailures <= 0 {
		opts.MaxLoginFailures = DefaultMaxLoginFailures
	}
	if opts.LoginFailureWindow <= 0 {
		opts.LoginFailureWindow = DefaultLoginFailureWindow
	}
	if opts.MaxMFAFailures <= 0 {
		opts.MaxMFAFailures = DefaultMaxMFAFailures
	}
	if opts.MFAFailureWindow <= 0 {
		opts.MFAFailureWindow = DefaultMFAFailureWindow
	}
	s := &Service{
		store:      store,
		opts:       opts,
		params:     opts.Password.withDefaults(),
		now:        opts.Clock,
		mfaLimiter: newLoginLimiter(opts.MaxMFAFailures, opts.MFAFailureWindow),
		pending:    newMFAPending(),
		touched:    make(map[string]time.Time),
	}
	if s.now == nil {
		s.now = time.Now
	}
	if !opts.DisableRateLimit {
		s.limiter = newLoginLimiter(opts.MaxLoginFailures, opts.LoginFailureWindow)
	}
	n := runtime.GOMAXPROCS(0) / int(s.params.Threads)
	s.hashSem = make(chan struct{}, min(max(n, 1), maxConcurrentHashes))
	return s
}

// ---- password hashing (bounded concurrency) ----

func (s *Service) acquireHash(ctx context.Context) error {
	select {
	case s.hashSem <- struct{}{}:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (s *Service) releaseHash() { <-s.hashSem }

func (s *Service) hashPassword(ctx context.Context, password string) (string, error) {
	if err := s.acquireHash(ctx); err != nil {
		return "", err
	}
	defer s.releaseHash()
	return HashPassword(password, s.params)
}

func (s *Service) verifyPassword(ctx context.Context, password, encoded string) (bool, error) {
	if err := s.acquireHash(ctx); err != nil {
		return false, err
	}
	defer s.releaseHash()
	return VerifyPassword(password, encoded)
}

// dummy returns a hash with the service's parameters, verified against when
// the username is unknown so both failure paths cost the same.
func (s *Service) dummy() (string, error) {
	s.dummyOnce.Do(func() {
		s.dummyHash, s.dummyErr = HashPassword("\x00iam-dummy-password\x00", s.params)
	})
	return s.dummyHash, s.dummyErr
}

// ---- Authenticator ----

// Register creates an account plus its personal tenant (slug = username,
// kind user, plan free) with the account as owner. It fails with
// ErrForbidden when registration is disabled, ErrInvalid for bad input and
// ErrAlreadyExists when the username, tenant slug or email is taken.
func (s *Service) Register(ctx context.Context, username, email, password string) (*domain.Account, error) {
	if !s.opts.AllowRegistration {
		return nil, fmt.Errorf("iam: registration is disabled: %w", domain.ErrForbidden)
	}
	return s.register(ctx, username, email, password, false)
}

func (s *Service) register(ctx context.Context, username, email, password string, asAdmin bool) (*domain.Account, error) {
	username = NormalizeUsername(username)
	email = strings.TrimSpace(email)
	if err := validateUsername(username, asAdmin); err != nil {
		return nil, fmt.Errorf("iam: %w", err)
	}
	if err := ValidateEmail(email); err != nil {
		return nil, fmt.Errorf("iam: %w", err)
	}
	if err := ValidatePassword(password); err != nil {
		return nil, fmt.Errorf("iam: %w", err)
	}
	// Hash before taking the lock: it is the expensive part.
	pwHash, err := s.hashPassword(ctx, password)
	if err != nil {
		return nil, err
	}

	s.regMu.Lock()
	defer s.regMu.Unlock()

	if err := s.checkAvailable(ctx, username, email); err != nil {
		return nil, err
	}
	now := s.now().UTC()
	tenant := &domain.Tenant{
		ID:        uuid.NewString(),
		Slug:      username,
		Name:      username,
		Kind:      domain.TenantUser,
		Plan:      personalTenantPlan,
		CreatedAt: now,
	}
	// The tenant is created first: its slug uniqueness also guards against
	// a concurrently created organization of the same name. MetaStore has
	// no tenant deletion, so a failure after this point (store error)
	// leaves an empty personal tenant behind.
	if err := s.store.CreateTenant(ctx, tenant); err != nil {
		return nil, fmt.Errorf("iam: creating personal tenant: %w", err)
	}
	acct := &domain.Account{
		ID:           uuid.NewString(),
		Username:     username,
		Email:        email,
		DisplayName:  username,
		PasswordHash: pwHash,
		TenantID:     tenant.ID,
		IsAdmin:      asAdmin,
		CreatedAt:    now,
	}
	if err := s.store.CreateAccount(ctx, acct); err != nil {
		return nil, fmt.Errorf("iam: creating account: %w", err)
	}
	if err := s.store.PutMembership(ctx, &domain.Membership{
		TenantID:  tenant.ID,
		AccountID: acct.ID,
		Role:      domain.RoleOwner,
		CreatedAt: now,
	}); err != nil {
		return nil, fmt.Errorf("iam: creating owner membership: %w", err)
	}
	out := *acct
	return &out, nil
}

// checkAvailable returns ErrAlreadyExists when username (as an account or
// tenant slug) or email (case-insensitively) is already in use.
func (s *Service) checkAvailable(ctx context.Context, username, email string) error {
	if _, err := s.store.GetAccountByUsername(ctx, username); err == nil {
		return fmt.Errorf("iam: username %q is taken: %w", username, domain.ErrAlreadyExists)
	} else if !errors.Is(err, domain.ErrNotFound) {
		return fmt.Errorf("iam: looking up account: %w", err)
	}
	if _, err := s.store.GetTenantBySlug(ctx, username); err == nil {
		return fmt.Errorf("iam: tenant %q already exists: %w", username, domain.ErrAlreadyExists)
	} else if !errors.Is(err, domain.ErrNotFound) {
		return fmt.Errorf("iam: looking up tenant: %w", err)
	}
	// tenant-lifecycle (§16.4): slugs of renamed or purged tenants stay
	// reserved for 90 days.
	if reserved, err := slugReservedAt(ctx, s.store, username, "", s.now().UTC()); err != nil {
		return fmt.Errorf("iam: looking up slug reservations: %w", err)
	} else if reserved {
		return fmt.Errorf("iam: username %q is reserved by a renamed or deleted tenant: %w", username, domain.ErrAlreadyExists)
	}
	accts, err := s.store.ListAccounts(ctx)
	if err != nil {
		return fmt.Errorf("iam: listing accounts: %w", err)
	}
	for i := range accts {
		if strings.EqualFold(accts[i].Email, email) {
			return fmt.Errorf("iam: email is already registered: %w", domain.ErrAlreadyExists)
		}
	}
	return nil
}

// Login verifies the password and issues a new session token. Unknown
// usernames and wrong passwords yield the same error (wrapping
// ErrUnauthenticated) after the same amount of work. Repeated failures for
// the same username and client address yield ErrForbidden until the rate
// limit window passes. Disabled accounts yield ErrForbidden. Accounts with
// a second factor enabled cannot use this single-step login: after a
// correct password it fails with ErrForbidden ("mfa required"); use
// LoginWithMeta and CompleteMFALogin instead.
func (s *Service) Login(ctx context.Context, username, password string) (string, *domain.Account, error) {
	meta := s.requestMeta(ctx, domain.SessionMeta{})
	acct, err := s.passwordLogin(ctx, username, password, meta.RemoteIP)
	if err != nil {
		return "", nil, err
	}
	enabled, err := s.mfaEnabled(ctx, acct.ID)
	if err != nil {
		return "", nil, err
	}
	if enabled {
		return "", nil, fmt.Errorf("iam: mfa required: complete the login with a second factor: %w", domain.ErrForbidden)
	}
	raw, _, err := s.issueSession(ctx, acct.ID, false, meta)
	if err != nil {
		return "", nil, err
	}
	return raw, acct, nil
}

// passwordLogin is the shared first login step: it rate limits per
// (username, ip), verifies the password with constant work, rejects
// disabled accounts, upgrades outdated hashes and prunes expired tokens. It
// returns a copy of the account.
func (s *Service) passwordLogin(ctx context.Context, username, password, ip string) (*domain.Account, error) {
	username = NormalizeUsername(username)
	key := username + "\x00" + ip
	now := s.now()
	if s.limiter.blocked(key, now) {
		return nil, fmt.Errorf("iam: too many attempts, try again later: %w", domain.ErrForbidden)
	}

	var acct *domain.Account
	if validateUsername(username, true) == nil {
		a, err := s.store.GetAccountByUsername(ctx, username)
		switch {
		case err == nil:
			acct = a
		case !errors.Is(err, domain.ErrNotFound):
			return nil, fmt.Errorf("iam: looking up account: %w", err)
		}
	}
	candidate := password
	if len(candidate) > MaxPasswordLen {
		// Never matches a stored hash (passwords are capped at set time);
		// avoid hashing attacker-sized input.
		candidate, acct = "", nil
	}
	// Without a usable stored hash, verify against a dummy so the failure
	// costs the same as a wrong password; the result is always rejected.
	usable := acct != nil && acct.PasswordHash != ""
	encoded := ""
	if usable {
		encoded = acct.PasswordHash
	} else {
		d, err := s.dummy()
		if err != nil {
			return nil, err
		}
		encoded = d
	}
	ok, err := s.verifyPassword(ctx, candidate, encoded)
	if ctxErr := ctx.Err(); ctxErr != nil {
		return nil, ctxErr
	}
	if !usable || !ok || err != nil {
		s.limiter.fail(key, now)
		return nil, fmt.Errorf("iam: %w", errBadCredentials)
	}
	if acct.Disabled {
		return nil, fmt.Errorf("iam: account is disabled: %w", domain.ErrForbidden)
	}
	s.limiter.reset(key)

	if needsRehash(acct.PasswordHash, s.params) {
		s.upgradeHash(ctx, acct.ID, password)
	}
	s.pruneExpiredTokens(ctx, acct.ID, now)
	out := *acct
	return &out, nil
}

// upgradeHash re-hashes password with the current parameters. Best effort:
// failures leave the old (still valid) hash in place.
func (s *Service) upgradeHash(ctx context.Context, accountID, password string) {
	h, err := s.hashPassword(ctx, password)
	if err != nil {
		return
	}
	a, err := s.store.GetAccount(ctx, accountID)
	if err != nil {
		return
	}
	a.PasswordHash = h
	_ = s.store.UpdateAccount(ctx, a)
}

// pruneExpiredTokens deletes the account's expired tokens. Best effort.
func (s *Service) pruneExpiredTokens(ctx context.Context, accountID string, now time.Time) {
	toks, err := s.store.ListTokens(ctx, accountID)
	if err != nil {
		return
	}
	for _, t := range toks {
		if t.ExpiresAt != nil && !now.Before(*t.ExpiresAt) {
			_ = s.store.DeleteToken(ctx, t.ID)
		}
	}
}

// Authenticate resolves a raw bearer token. It returns an error wrapping
// ErrUnauthenticated for unknown, malformed or expired tokens, for tokens
// of disabled or deleted accounts and for keys of disabled or deleted
// service accounts. The principal carries the principal kind, the token's
// scopes, its MFA flag (sessions only) and its id. Service-account keys
// resolve to a service-account principal (see serviceAccountPrincipal).
func (s *Service) Authenticate(ctx context.Context, token string) (*domain.Principal, error) {
	if !wellFormedToken(token) {
		return nil, fmt.Errorf("iam: %w", errInvalidToken)
	}
	tok, err := s.store.GetTokenByHash(ctx, HashToken(token))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidToken)
		}
		return nil, fmt.Errorf("iam: looking up token: %w", err)
	}
	now := s.now()
	if tok.ExpiresAt != nil && !now.Before(*tok.ExpiresAt) {
		return nil, fmt.Errorf("iam: %w", errInvalidToken)
	}
	var p *domain.Principal
	if tok.ServiceAccountID != "" || tok.Kind == domain.TokenService {
		p, err = s.serviceAccountPrincipal(ctx, tok)
	} else {
		p, err = s.userPrincipal(ctx, tok)
	}
	if err != nil {
		return nil, err
	}
	p.Scopes = append([]string(nil), tok.Scopes...)
	if len(p.Scopes) == 0 {
		p.Scopes = nil
	}
	p.TokenID = tok.ID
	s.touch(ctx, tok, now)
	return p, nil
}

// userPrincipal resolves a personal token (session or api) to its account.
func (s *Service) userPrincipal(ctx context.Context, tok *domain.APIToken) (*domain.Principal, error) {
	if tok.AccountID == "" {
		return nil, fmt.Errorf("iam: %w", errInvalidToken)
	}
	acct, err := s.store.GetAccount(ctx, tok.AccountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidToken)
		}
		return nil, fmt.Errorf("iam: looking up account: %w", err)
	}
	if acct.Disabled {
		return nil, fmt.Errorf("iam: account is disabled: %w", domain.ErrUnauthenticated)
	}
	return &domain.Principal{
		AccountID: acct.ID,
		Username:  acct.Username,
		TenantID:  acct.TenantID,
		IsAdmin:   acct.IsAdmin,
		Kind:      domain.PrincipalUser,
		MFA:       tok.MFA && TokenKind(tok) == domain.TokenSession,
	}, nil
}

// touch records token use, at most once per touchInterval per token.
// Failures are ignored: usage tracking must never fail a request.
func (s *Service) touch(ctx context.Context, tok *domain.APIToken, now time.Time) {
	recent := func(last time.Time) bool {
		d := now.Sub(last)
		return d >= 0 && d < touchInterval
	}
	if tok.LastUsedAt != nil && recent(*tok.LastUsedAt) {
		return
	}
	s.touchMu.Lock()
	if last, ok := s.touched[tok.ID]; ok && recent(last) {
		s.touchMu.Unlock()
		return
	}
	if len(s.touched) >= touchSweepThreshold {
		for id, last := range s.touched {
			if !recent(last) {
				delete(s.touched, id)
			}
		}
	}
	s.touched[tok.ID] = now
	s.touchMu.Unlock()
	if mt, ok := s.store.(TokenMetaToucher); ok {
		meta := s.requestMeta(ctx, domain.SessionMeta{})
		// Keep the recorded client when the request carries none.
		if meta.RemoteIP == "" {
			meta.RemoteIP = tok.LastIP
		}
		if meta.UserAgent == "" {
			meta.UserAgent = tok.UserAgent
		}
		_ = mt.TouchTokenMeta(ctx, tok.ID, now.UTC(), meta.RemoteIP, meta.UserAgent)
		return
	}
	_ = s.store.TouchToken(ctx, tok.ID, now.UTC())
}

// CreateToken issues a named, unrestricted personal API token (Kind api)
// for accountID. ttl 0 means the token never expires; negative ttl is
// invalid. CreateScopedToken issues tokens limited to some actions.
func (s *Service) CreateToken(ctx context.Context, accountID, name string, ttl time.Duration) (string, *domain.APIToken, error) {
	return s.CreateScopedToken(ctx, accountID, name, ttl, nil)
}

// issue stores a new token built from tmpl (owner, name, kind, scopes, MFA
// flag and client metadata) with a fresh secret, id and timestamps.
// ttl <= 0 means no expiry.
func (s *Service) issue(ctx context.Context, tmpl *domain.APIToken, ttl time.Duration) (string, *domain.APIToken, error) {
	raw, h, err := newToken()
	if err != nil {
		return "", nil, err
	}
	now := s.now().UTC()
	tok := *tmpl
	tok.ID = uuid.NewString()
	tok.TokenHash = h
	tok.CreatedAt = now
	tok.ExpiresAt = nil
	tok.LastUsedAt = nil
	tok.Scopes = nil
	if len(tmpl.Scopes) > 0 {
		tok.Scopes = append([]string(nil), tmpl.Scopes...)
	}
	if ttl > 0 {
		exp := now.Add(ttl)
		tok.ExpiresAt = &exp
	}
	if err := s.store.CreateToken(ctx, &tok); err != nil {
		return "", nil, fmt.Errorf("iam: storing token: %w", err)
	}
	out := tok
	out.Scopes = append([]string(nil), tok.Scopes...)
	if len(out.Scopes) == 0 {
		out.Scopes = nil
	}
	return raw, &out, nil
}

// RevokeToken deletes token tokenID on behalf of accountID. Accounts may
// revoke only their own tokens; server admins may revoke any token. A token
// the caller may not revoke is reported as ErrNotFound so token ids of other
// accounts cannot be probed.
func (s *Service) RevokeToken(ctx context.Context, accountID, tokenID string) error {
	actor, err := s.store.GetAccount(ctx, accountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return fmt.Errorf("iam: unknown account: %w", domain.ErrUnauthenticated)
		}
		return fmt.Errorf("iam: looking up account: %w", err)
	}
	toks, err := s.store.ListTokens(ctx, actor.ID)
	if err != nil {
		return fmt.Errorf("iam: listing tokens: %w", err)
	}
	own := false
	for _, t := range toks {
		if t.ID == tokenID {
			own = true
			break
		}
	}
	if !own && !(actor.IsAdmin && !actor.Disabled) {
		return fmt.Errorf("iam: token %q: %w", tokenID, domain.ErrNotFound)
	}
	if err := s.store.DeleteToken(ctx, tokenID); err != nil {
		return fmt.Errorf("iam: deleting token: %w", err)
	}
	s.touchMu.Lock()
	delete(s.touched, tokenID)
	s.touchMu.Unlock()
	// device-auth (§16.6): signing out a session also ends its refresh token.
	return s.revokeSessionRefresh(ctx, tokenID)
}

// SetPassword validates and stores a new password for accountID. Existing
// tokens remain valid.
func (s *Service) SetPassword(ctx context.Context, accountID, password string) error {
	if err := ValidatePassword(password); err != nil {
		return fmt.Errorf("iam: %w", err)
	}
	h, err := s.hashPassword(ctx, password)
	if err != nil {
		return err
	}
	acct, err := s.store.GetAccount(ctx, accountID)
	if err != nil {
		return fmt.Errorf("iam: looking up account: %w", err)
	}
	acct.PasswordHash = h
	if err := s.store.UpdateAccount(ctx, acct); err != nil {
		return fmt.Errorf("iam: updating account: %w", err)
	}
	return nil
}
