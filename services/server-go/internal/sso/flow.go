package sso

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"net/url"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// pendingLogin is a started login awaiting its callback. It is bound to
// the configuration it was started with.
type pendingLogin struct {
	verifier      string
	nonce         string
	redirectAfter string
	issuer        string
	clientID      string
	redirectURL   string
	expires       time.Time
}

const maxCodeLen = 4096

var (
	errBadState = &Error{Msg: "the sign-in request is invalid or has expired; please try again", Kind: domain.ErrUnauthenticated}
	errCfgMoved = &Error{Msg: "the single sign-on configuration changed during sign-in; please try again", Kind: domain.ErrUnauthenticated}
)

// randomToken returns n random bytes, base64url-encoded without padding.
func randomToken(n int) (string, error) {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("sso: reading random bytes: %w", err)
	}
	return base64.RawURLEncoding.EncodeToString(b), nil
}

// usable reports whether c describes a provider logins can be started with.
func usable(c *domain.SSOConfig) bool {
	return c.Enabled && c.Issuer != "" && c.ClientID != "" && c.RedirectURL != ""
}

// AuthURL starts a login and returns the provider's authorization URL.
// redirectAfter must be an admin-panel path (see SafeRedirect).
func (s *Service) AuthURL(ctx context.Context, redirectAfter string) (string, error) {
	ra, ok := SafeRedirect(redirectAfter)
	if !ok {
		return "", invalidf("redirect must be a path under /admin/")
	}
	cfg, err := s.loadConfig(ctx)
	if err != nil {
		return "", err
	}
	if !usable(cfg) {
		return "", ErrNotEnabled
	}
	md, err := s.discover(ctx, cfg.Issuer)
	if err != nil {
		return "", err
	}
	state, err := randomToken(32)
	if err != nil {
		return "", err
	}
	nonce, err := randomToken(32)
	if err != nil {
		return "", err
	}
	verifier, err := randomToken(32) // 43 characters (RFC 7636 §4.1)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256([]byte(verifier))
	challenge := base64.RawURLEncoding.EncodeToString(sum[:])

	u, err := url.Parse(md.AuthorizationEndpoint)
	if err != nil {
		return "", providerErr(err, "the identity provider's authorization endpoint is invalid")
	}
	q := u.Query()
	q.Set("response_type", "code")
	q.Set("client_id", cfg.ClientID)
	q.Set("redirect_uri", cfg.RedirectURL)
	q.Set("scope", strings.Join(cfg.Scopes, " "))
	q.Set("state", state)
	q.Set("nonce", nonce)
	q.Set("code_challenge", challenge)
	q.Set("code_challenge_method", "S256")
	u.RawQuery = q.Encode()

	s.putPending(state, &pendingLogin{
		verifier:      verifier,
		nonce:         nonce,
		redirectAfter: ra,
		issuer:        cfg.Issuer,
		clientID:      cfg.ClientID,
		redirectURL:   cfg.RedirectURL,
		expires:       s.now().Add(s.opts.StateTTL),
	})
	return u.String(), nil
}

// putPending stores a pending login, pruning expired entries and evicting
// the oldest one when the table is full.
func (s *Service) putPending(state string, p *pendingLogin) {
	s.pendingMu.Lock()
	defer s.pendingMu.Unlock()
	now := s.now()
	if len(s.pending) >= s.opts.MaxPending {
		for k, v := range s.pending {
			if !now.Before(v.expires) {
				delete(s.pending, k)
			}
		}
	}
	for len(s.pending) >= s.opts.MaxPending {
		var oldest string
		var at time.Time
		for k, v := range s.pending {
			if oldest == "" || v.expires.Before(at) {
				oldest, at = k, v.expires
			}
		}
		delete(s.pending, oldest)
	}
	s.pending[state] = p
}

// takePending removes and returns the pending login for state (single
// use). Expired or unknown states yield nil.
func (s *Service) takePending(state string) *pendingLogin {
	s.pendingMu.Lock()
	defer s.pendingMu.Unlock()
	p := s.pending[state]
	if p == nil {
		return nil
	}
	delete(s.pending, state)
	if !s.now().Before(p.expires) {
		return nil
	}
	return p
}

// identity is what the provider told us about the user.
type identity struct {
	issuer        string
	subject       string
	email         string
	emailVerified bool
	name          string
	usernames     []string // username candidates in preference order
}

// Callback completes a login: it redeems the code, verifies the ID token,
// maps the identity to an account and issues a session.
func (s *Service) Callback(ctx context.Context, code, state string, meta domain.SessionMeta) (string, *domain.Account, string, error) {
	if state == "" || len(state) > 256 {
		return "", nil, "", errBadState
	}
	pl := s.takePending(state)
	if pl == nil {
		return "", nil, "", errBadState
	}
	if code == "" || len(code) > maxCodeLen {
		return "", nil, "", newErr(domain.ErrInvalid, "the identity provider did not return an authorization code")
	}
	cfg, err := s.loadConfig(ctx)
	if err != nil {
		return "", nil, "", err
	}
	if !usable(cfg) {
		return "", nil, "", ErrNotEnabled
	}
	if cfg.Issuer != pl.issuer || cfg.ClientID != pl.clientID || cfg.RedirectURL != pl.redirectURL {
		return "", nil, "", errCfgMoved
	}
	md, err := s.discover(ctx, cfg.Issuer)
	if err != nil {
		return "", nil, "", err
	}
	tr, err := s.exchangeCode(ctx, md, cfg, code, pl.verifier)
	if err != nil {
		return "", nil, "", err
	}
	c, err := s.verifyIDToken(ctx, tr.IDToken, idTokenCheck{
		issuer:   cfg.Issuer,
		clientID: cfg.ClientID,
		nonce:    pl.nonce,
		jwksURI:  md.JWKSURI,
		algs:     md.SigningAlgs,
	})
	if err != nil {
		var e *Error
		if errors.As(err, &e) && e.Cause != nil {
			s.log.WarnContext(ctx, "sso: ID token rejected", "issuer", cfg.Issuer, "reason", e.Cause.Error())
		}
		return "", nil, "", err
	}
	id := identityFromClaims(cfg, c)
	if id.email == "" && md.UserinfoEndpoint != "" && tr.AccessToken != "" &&
		(tr.TokenType == "" || strings.EqualFold(tr.TokenType, "bearer")) {
		s.mergeUserinfo(ctx, cfg, md.UserinfoEndpoint, tr.AccessToken, &id)
	}

	acct, link, err := s.mapAccount(ctx, cfg, &id)
	if err != nil {
		return "", nil, "", err
	}
	token, err := s.security.IssueSession(ctx, acct.ID, false, meta)
	if err != nil {
		return "", nil, "", fmt.Errorf("sso: issuing session: %w", err)
	}
	link.LastLoginAt = s.now().UTC()
	if id.email != "" {
		link.Email = id.email
	}
	if err := s.store.PutSSOIdentity(ctx, link); err != nil {
		s.log.WarnContext(ctx, "sso: recording last login failed", "account_id", acct.ID, "error", err)
	}
	return token, acct, pl.redirectAfter, nil
}

func identityFromClaims(cfg *domain.SSOConfig, c claims) identity {
	id := identity{
		issuer:        cfg.Issuer,
		subject:       c.str("sub"),
		email:         strings.TrimSpace(c.str("email")),
		emailVerified: c.boolish("email_verified"),
		name:          c.str("name"),
	}
	if id.email == "" {
		id.emailVerified = false
	}
	id.usernames = usernameCandidates(cfg, c, id.email)
	return id
}

func usernameCandidates(cfg *domain.SSOConfig, c claims, email string) []string {
	var out []string
	for _, claim := range []string{cfg.UsernameClaim, DefaultUsernameClaim} {
		if v := c.str(claim); v != "" {
			out = append(out, v)
		}
	}
	if local, _, ok := strings.Cut(email, "@"); ok && local != "" {
		out = append(out, local)
	}
	return out
}

// mergeUserinfo fills missing email information from the userinfo
// endpoint. Failures are logged; the fallback is best effort.
func (s *Service) mergeUserinfo(ctx context.Context, cfg *domain.SSOConfig, endpoint, accessToken string, id *identity) {
	ui, err := s.fetchUserinfo(ctx, endpoint, accessToken)
	if err != nil {
		s.log.WarnContext(ctx, "sso: userinfo request failed", "issuer", cfg.Issuer, "error", err)
		return
	}
	// OpenID Connect Core §5.3.2: the sub must match the ID token's.
	if ui.str("sub") != id.subject {
		s.log.WarnContext(ctx, "sso: userinfo subject mismatch; ignoring response", "issuer", cfg.Issuer)
		return
	}
	if email := strings.TrimSpace(ui.str("email")); email != "" {
		id.email = email
		id.emailVerified = ui.boolish("email_verified")
	}
	if id.name == "" {
		id.name = ui.str("name")
	}
	id.usernames = append(usernameCandidates(cfg, ui, id.email), id.usernames...)
}

func emailDomain(email string) string {
	at := strings.LastIndexByte(email, '@')
	if at < 0 {
		return ""
	}
	return strings.ToLower(email[at+1:])
}

func forbidden(format string, args ...any) error {
	return newErr(domain.ErrForbidden, format, args...)
}

// mapAccount resolves the account of id, linking or creating one as the
// configuration allows, and returns it with its identity link.
func (s *Service) mapAccount(ctx context.Context, cfg *domain.SSOConfig, id *identity) (*domain.Account, *domain.SSOIdentity, error) {
	s.mapMu.Lock()
	defer s.mapMu.Unlock()

	if len(cfg.AllowedDomains) > 0 {
		if id.email == "" || !id.emailVerified {
			return nil, nil, forbidden("a verified email address is required to sign in")
		}
		if !containsStr(cfg.AllowedDomains, emailDomain(id.email)) {
			return nil, nil, forbidden("accounts from the email domain %q may not sign in", emailDomain(id.email))
		}
	}
	now := s.now().UTC()

	// 1. An existing link.
	link, err := s.store.GetSSOIdentity(ctx, id.issuer, id.subject)
	switch {
	case err == nil:
		acct, err := s.store.GetAccount(ctx, link.AccountID)
		if errors.Is(err, domain.ErrNotFound) {
			return nil, nil, forbidden("the account linked to this identity no longer exists")
		} else if err != nil {
			return nil, nil, fmt.Errorf("sso: loading linked account: %w", err)
		}
		if acct.Disabled {
			return nil, nil, forbidden("this account is disabled")
		}
		return acct, link, nil
	case !errors.Is(err, domain.ErrNotFound):
		return nil, nil, fmt.Errorf("sso: looking up identity: %w", err)
	}

	accts, err := s.store.ListAccounts(ctx)
	if err != nil {
		return nil, nil, fmt.Errorf("sso: listing accounts: %w", err)
	}
	var byEmail []domain.Account
	if id.email != "" {
		for i := range accts {
			if strings.EqualFold(accts[i].Email, id.email) {
				byEmail = append(byEmail, accts[i])
			}
		}
	}

	// 2. Link an existing account by verified email.
	if id.emailVerified && len(byEmail) > 0 {
		if len(byEmail) > 1 {
			return nil, nil, newErr(domain.ErrConflict, "several accounts use this email address; ask an administrator to resolve it")
		}
		acct := byEmail[0]
		if acct.Disabled {
			return nil, nil, forbidden("this account is disabled")
		}
		links, err := s.store.ListAccountSSOIdentities(ctx, acct.ID)
		if err != nil {
			return nil, nil, fmt.Errorf("sso: listing identity links: %w", err)
		}
		for _, l := range links {
			if l.Issuer == id.issuer && l.Subject != id.subject {
				return nil, nil, newErr(domain.ErrConflict, "the account with this email is already linked to a different identity")
			}
		}
		link := &domain.SSOIdentity{Issuer: id.issuer, Subject: id.subject, AccountID: acct.ID, Email: id.email, CreatedAt: now, LastLoginAt: now}
		if err := s.store.PutSSOIdentity(ctx, link); err != nil {
			return nil, nil, fmt.Errorf("sso: linking identity: %w", err)
		}
		s.log.InfoContext(ctx, "sso: linked identity to existing account by verified email",
			"issuer", id.issuer, "account_id", acct.ID, "username", acct.Username)
		return &acct, link, nil
	}

	// 3. Create a new account.
	if !cfg.AutoCreate {
		return nil, nil, forbidden("no account is linked to this identity; ask an administrator for access")
	}
	if id.email == "" {
		return nil, nil, forbidden("the identity provider did not supply an email address, which is required to create an account")
	}
	if err := iam.ValidateEmail(id.email); err != nil {
		return nil, nil, forbidden("the identity provider supplied an invalid email address")
	}
	if len(byEmail) > 0 {
		// Unverified email that collides with an existing account: never
		// link (that would allow account takeover) and never duplicate.
		return nil, nil, newErr(domain.ErrConflict, "an account with this email address already exists, but the identity provider has not verified the address")
	}
	acct, err := s.createAccount(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	link = &domain.SSOIdentity{Issuer: id.issuer, Subject: id.subject, AccountID: acct.ID, Email: id.email, CreatedAt: now, LastLoginAt: now}
	if err := s.store.PutSSOIdentity(ctx, link); err != nil {
		return nil, nil, fmt.Errorf("sso: linking new account: %w", err)
	}
	s.log.InfoContext(ctx, "sso: created account for new identity",
		"issuer", id.issuer, "account_id", acct.ID, "username", acct.Username)
	return acct, link, nil
}

// createAccount creates an account with a username derived from the
// identity (de-duplicated with numeric, then random suffixes) and a random
// password nobody knows.
func (s *Service) createAccount(ctx context.Context, id *identity) (*domain.Account, error) {
	base := ""
	for _, c := range id.usernames {
		if b := SanitizeUsername(c); b != "" {
			base = b
			break
		}
	}
	if base == "" {
		base = "user"
	}
	password, err := randomToken(autoPasswordEntropy)
	if err != nil {
		return nil, err
	}
	for attempt := 0; attempt < maxUsernameAttempts; attempt++ {
		name, err := usernameVariant(base, attempt)
		if err != nil {
			return nil, err
		}
		if iam.ValidateUsername(name) != nil {
			continue // reserved
		}
		acct, err := s.accounts.CreateAccount(ctx, name, id.email, password, false)
		switch {
		case err == nil:
			s.setDisplayName(ctx, acct, id.name)
			return acct, nil
		case errors.Is(err, domain.ErrAlreadyExists), errors.Is(err, domain.ErrInvalid):
			continue
		default:
			return nil, fmt.Errorf("sso: creating account: %w", err)
		}
	}
	return nil, newErr(domain.ErrConflict, "could not find a free username for this identity; ask an administrator to create the account")
}

// setDisplayName stores the provider's display name, best effort.
func (s *Service) setDisplayName(ctx context.Context, acct *domain.Account, name string) {
	name = strings.TrimSpace(name)
	if name == "" || utf8.RuneCountInString(name) > maxNameLen || !utf8.ValidString(name) ||
		strings.ContainsFunc(name, unicode.IsControl) || name == acct.DisplayName {
		return
	}
	upd := *acct
	upd.DisplayName = name
	if err := s.store.UpdateAccount(ctx, &upd); err != nil {
		s.log.WarnContext(ctx, "sso: setting display name failed", "account_id", acct.ID, "error", err)
		return
	}
	acct.DisplayName = name
}

// usernameVariant returns the attempt-th candidate for base: base,
// base-2 ... base-10, then base-<random hex>, each within the length limit.
func usernameVariant(base string, attempt int) (string, error) {
	suffix := ""
	switch {
	case attempt == 0:
	case attempt < 10:
		suffix = "-" + strconv.Itoa(attempt+1)
	default:
		var b [3]byte
		if _, err := rand.Read(b[:]); err != nil {
			return "", fmt.Errorf("sso: reading random bytes: %w", err)
		}
		suffix = "-" + hex.EncodeToString(b[:])
	}
	if len(base)+len(suffix) > usernameMaxLen {
		base = strings.TrimRight(base[:usernameMaxLen-len(suffix)], "-")
	}
	return base + suffix, nil
}

// SanitizeUsername maps an arbitrary claim value onto the username rules
// (2-39 characters of a-z, 0-9 and '-', starting and ending with a letter
// or digit). Separators become '-', other characters are dropped. It
// returns "" when fewer than two characters remain.
func SanitizeUsername(s string) string {
	var b strings.Builder
	dash := false
	for _, r := range strings.ToLower(s) {
		switch {
		case r >= 'a' && r <= 'z', r >= '0' && r <= '9':
			b.WriteRune(r)
			dash = false
		case r == '-' || r == '_' || r == '.' || r == '+' || r == ' ' || r == '@':
			if !dash && b.Len() > 0 {
				b.WriteByte('-')
				dash = true
			}
		}
		if b.Len() >= usernameMaxLen {
			break
		}
	}
	out := strings.Trim(b.String(), "-")
	if len(out) > usernameMaxLen {
		out = strings.TrimRight(out[:usernameMaxLen], "-")
	}
	if len(out) < 2 {
		return ""
	}
	return out
}
