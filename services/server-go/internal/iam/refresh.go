package iam

// Refresh tokens (docs/ENGINEERING_PLAN.md §16.6, workstream device-auth).
//
// Interactive logins (password, password + MFA, device authorization)
// return a pair: a short-lived access token (a session, Options.AccessTTL,
// default 1 h) and a refresh token (Options.RefreshTTL, default 30 days).
// A refresh token is single use: POST /api/v1/auth/refresh exchanges it for
// a new pair (rotation) and the previous access session is deleted.
//
// Every pair descends from one login and forms a family, stored as one
// document in the refresh_tokens collection (key: family id). The raw
// token embeds the family id ("wtrt_<family>.<secret>") so lookups need no
// index; the document keeps the hash of the current token and of every
// token it replaced. Presenting a replaced token again is reuse: the family
// is revoked (its access session deleted, further refreshes fail), because
// either the legitimate client or a thief holds a copy. A benign race —
// two processes sharing a credential file refreshing at once — is told
// apart by presenting the immediately previous token within
// Options.RefreshReuseGrace of its rotation: that request is rejected
// without revoking anything, and the client re-reads the rotated token.

import (
	"context"
	"crypto/subtle"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

const (
	// DefaultAccessTTL is the lifetime of access sessions issued with a
	// refresh token when Options.AccessTTL is zero.
	DefaultAccessTTL = time.Hour
	// DefaultRefreshTTL is the lifetime of a refresh token when
	// Options.RefreshTTL is zero. Each rotation starts a new period.
	DefaultRefreshTTL = 30 * 24 * time.Hour
	// DefaultRefreshReuseGrace is the window in which re-presenting the
	// previous refresh token is treated as a client race rather than reuse.
	DefaultRefreshReuseGrace = 10 * time.Second

	// RefreshTokenPrefix starts every raw refresh token.
	RefreshTokenPrefix = "wtrt_"
	// CollRefreshTokens holds one refreshFamily per login (key: family id).
	CollRefreshTokens = "refresh_tokens"

	// maxRefreshHistory bounds the replaced-token hashes kept per family
	// (older ones are forgotten: presenting them is merely invalid).
	maxRefreshHistory = 1000
	// uuidLen is the length of a textual UUID (family ids).
	uuidLen = 36
)

var (
	errInvalidRefresh = fmt.Errorf("invalid or expired refresh token: %w", domain.ErrUnauthenticated)
	// ErrRefreshReused reports that a rotated refresh token was presented
	// again; the whole family has been revoked.
	ErrRefreshReused = fmt.Errorf("refresh token reuse detected; the session was revoked, sign in again: %w", domain.ErrUnauthenticated)
)

// TokenPair is an access session plus the refresh token that renews it.
type TokenPair struct {
	AccessToken      string
	RefreshToken     string
	Session          *domain.APIToken // the access session (TokenHash set)
	AccessExpiresAt  time.Time
	RefreshExpiresAt time.Time
	Account          *domain.Account
}

// refreshFamily is the refresh_tokens document of one login.
type refreshFamily struct {
	ID            string        `json:"id"`
	AccountID     string        `json:"account_id"`
	SessionID     string        `json:"session_id"` // current access session
	Name          string        `json:"name"`       // access session name ("session", "device: ...")
	MFA           bool          `json:"mfa,omitempty"`
	Scopes        []string      `json:"scopes,omitempty"`
	TokenHash     string        `json:"token_hash"` // current refresh token
	IssuedAt      time.Time     `json:"issued_at"`
	ExpiresAt     time.Time     `json:"expires_at"`
	Used          []usedRefresh `json:"used,omitempty"` // replaced tokens, oldest first
	Rotations     int           `json:"rotations"`
	CreatedAt     time.Time     `json:"created_at"`
	RevokedAt     *time.Time    `json:"revoked_at,omitempty"`
	RevokedReason string        `json:"revoked_reason,omitempty"`
}

type usedRefresh struct {
	Hash      string    `json:"hash"`
	RotatedAt time.Time `json:"rotated_at"`
}

func (s *Service) accessTTL() time.Duration {
	if s.opts.AccessTTL > 0 {
		return s.opts.AccessTTL
	}
	return DefaultAccessTTL
}

func (s *Service) refreshTTL() time.Duration {
	if s.opts.RefreshTTL > 0 {
		return s.opts.RefreshTTL
	}
	return DefaultRefreshTTL
}

func (s *Service) refreshGrace() time.Duration {
	switch {
	case s.opts.RefreshReuseGrace > 0:
		return s.opts.RefreshReuseGrace
	case s.opts.RefreshReuseGrace < 0:
		return 0
	}
	return DefaultRefreshReuseGrace
}

// AccessTTL is the lifetime of access sessions issued with refresh tokens.
func (s *Service) AccessTTL() time.Duration { return s.accessTTL() }

// RefreshTTL is the lifetime of refresh tokens.
func (s *Service) RefreshTTL() time.Duration { return s.refreshTTL() }

// newRefreshSecret returns a raw refresh token of family famID and its hash.
func newRefreshSecret(famID string) (raw, hashHex string, err error) {
	return newSecret(RefreshTokenPrefix + famID + ".")
}

// refreshFamilyID returns the family id embedded in a well-formed raw
// refresh token.
func refreshFamilyID(raw string) (string, bool) {
	if len(raw) != len(RefreshTokenPrefix)+uuidLen+1+secretBody || !strings.HasPrefix(raw, RefreshTokenPrefix) {
		return "", false
	}
	id := raw[len(RefreshTokenPrefix) : len(RefreshTokenPrefix)+uuidLen]
	if _, err := uuid.Parse(id); err != nil {
		return "", false
	}
	if !wellFormedSecret(RefreshTokenPrefix+id+".", raw) {
		return "", false
	}
	return id, true
}

func equalHash(a, b string) bool { return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1 }

// createSession stores an access session with a caller-chosen id.
func (s *Service) createSession(ctx context.Context, id string, tmpl domain.APIToken, now time.Time) (string, *domain.APIToken, error) {
	raw, h, err := newToken()
	if err != nil {
		return "", nil, err
	}
	tok := tmpl
	tok.ID = id
	tok.Kind = domain.TokenSession
	tok.TokenHash = h
	tok.CreatedAt = now
	tok.LastUsedAt = nil
	exp := now.Add(s.accessTTL())
	tok.ExpiresAt = &exp
	tok.Scopes = nil
	if len(tmpl.Scopes) > 0 {
		tok.Scopes = append([]string(nil), tmpl.Scopes...)
	}
	if err := s.store.CreateToken(ctx, &tok); err != nil {
		return "", nil, fmt.Errorf("iam: storing token: %w", err)
	}
	out := tok
	return raw, &out, nil
}

// issuePair creates an access session (AccessTTL) for acct and a new
// refresh token family renewing it.
func (s *Service) issuePair(ctx context.Context, acct *domain.Account, mfa bool, name string, scopes []string, m domain.SessionMeta) (*TokenPair, error) {
	now := s.now().UTC()
	famID := uuid.NewString()
	refreshRaw, refreshHash, err := newRefreshSecret(famID)
	if err != nil {
		return nil, err
	}
	accessRaw, sess, err := s.createSession(ctx, uuid.NewString(), domain.APIToken{
		AccountID: acct.ID,
		Name:      name,
		MFA:       mfa,
		Scopes:    scopes,
		LastIP:    m.RemoteIP,
		UserAgent: m.UserAgent,
	}, now)
	if err != nil {
		return nil, err
	}
	fam := refreshFamily{
		ID:        famID,
		AccountID: acct.ID,
		SessionID: sess.ID,
		Name:      name,
		MFA:       mfa,
		Scopes:    sess.Scopes,
		TokenHash: refreshHash,
		IssuedAt:  now,
		ExpiresAt: now.Add(s.refreshTTL()),
		CreatedAt: now,
	}
	if err := s.store.CreateDoc(ctx, CollRefreshTokens, famID, &fam); err != nil {
		_ = s.deleteToken(ctx, sess.ID)
		return nil, fmt.Errorf("iam: storing refresh token: %w", err)
	}
	out := *acct
	return &TokenPair{
		AccessToken:      accessRaw,
		RefreshToken:     refreshRaw,
		Session:          sess,
		AccessExpiresAt:  *sess.ExpiresAt,
		RefreshExpiresAt: fam.ExpiresAt,
		Account:          &out,
	}, nil
}

// UpgradeSession exchanges a session token just issued by a login
// (LoginWithMeta, CompleteMFALogin, Login) for a token pair: a new access
// session with the same owner, name, scopes, MFA flag and client, valid for
// AccessTTL, plus a refresh token. The original session is deleted. Only
// session tokens qualify (ErrInvalid otherwise); unknown tokens fail with
// ErrUnauthenticated, disabled accounts with ErrForbidden.
func (s *Service) UpgradeSession(ctx context.Context, rawSession string, m domain.SessionMeta) (*TokenPair, error) {
	if !wellFormedToken(rawSession) {
		return nil, fmt.Errorf("iam: %w", errInvalidToken)
	}
	tok, err := s.store.GetTokenByHash(ctx, HashToken(rawSession))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidToken)
		}
		return nil, fmt.Errorf("iam: looking up token: %w", err)
	}
	if TokenKind(tok) != domain.TokenSession || tok.AccountID == "" {
		return nil, fmt.Errorf("iam: only sessions can be renewed with refresh tokens: %w", domain.ErrInvalid)
	}
	if tok.ExpiresAt != nil && !s.now().Before(*tok.ExpiresAt) {
		return nil, fmt.Errorf("iam: %w", errInvalidToken)
	}
	acct, err := s.activeAccount(ctx, tok.AccountID)
	if err != nil {
		return nil, err
	}
	m = s.requestMeta(ctx, m)
	if m.RemoteIP == "" {
		m.RemoteIP = tok.LastIP
	}
	if m.UserAgent == "" {
		m.UserAgent = tok.UserAgent
	}
	name := tok.Name
	if name == "" {
		name = sessionTokenName
	}
	pair, err := s.issuePair(ctx, acct, tok.MFA, name, tok.Scopes, m)
	if err != nil {
		return nil, err
	}
	if err := s.deleteToken(ctx, tok.ID); err != nil {
		return nil, err
	}
	return pair, nil
}

type refreshOutcome int

const (
	refreshInvalid refreshOutcome = iota
	refreshRotated
	refreshRace
	refreshReused
)

// Refresh rotates a refresh token: it returns a new access session and a
// new refresh token, deletes the previous access session and invalidates
// the presented token. Errors wrap ErrUnauthenticated: unknown, malformed,
// expired or revoked tokens and disabled accounts yield an invalid-token
// error; a token that was already rotated yields ErrRefreshReused after
// revoking its family (see the package comment above).
func (s *Service) Refresh(ctx context.Context, raw string, m domain.SessionMeta) (*TokenPair, error) {
	raw = strings.TrimSpace(raw)
	famID, ok := refreshFamilyID(raw)
	if !ok {
		return nil, fmt.Errorf("iam: %w", errInvalidRefresh)
	}
	var cur refreshFamily
	if err := s.store.GetDoc(ctx, CollRefreshTokens, famID, &cur); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidRefresh)
		}
		return nil, fmt.Errorf("iam: looking up refresh token: %w", err)
	}
	acct, err := s.store.GetAccount(ctx, cur.AccountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidRefresh)
		}
		return nil, fmt.Errorf("iam: looking up account: %w", err)
	}
	if acct.Disabled {
		return nil, fmt.Errorf("iam: account is disabled: %w", domain.ErrUnauthenticated)
	}

	h := HashToken(raw)
	now := s.now().UTC()
	newRaw, newHash, err := newRefreshSecret(famID)
	if err != nil {
		return nil, err
	}
	newSessionID := uuid.NewString()
	grace := s.refreshGrace()
	var (
		outcome    refreshOutcome
		fam        refreshFamily
		oldSession string
	)
	err = meta.UpdateDocT(ctx, s.store, CollRefreshTokens, famID, func(c *refreshFamily) (*refreshFamily, error) {
		outcome = refreshInvalid
		if c == nil || c.RevokedAt != nil {
			return nil, errInvalidRefresh
		}
		if equalHash(h, c.TokenHash) {
			if !now.Before(c.ExpiresAt) {
				return nil, errInvalidRefresh
			}
			next := *c
			next.Used = append(append([]usedRefresh(nil), c.Used...), usedRefresh{Hash: c.TokenHash, RotatedAt: now})
			if len(next.Used) > maxRefreshHistory {
				next.Used = next.Used[len(next.Used)-maxRefreshHistory:]
			}
			next.TokenHash = newHash
			next.IssuedAt = now
			next.ExpiresAt = now.Add(s.refreshTTL())
			next.Rotations++
			oldSession = c.SessionID
			next.SessionID = newSessionID
			outcome, fam = refreshRotated, next
			return &next, nil
		}
		for i, u := range c.Used {
			if !equalHash(h, u.Hash) {
				continue
			}
			if i == len(c.Used)-1 && now.Sub(u.RotatedAt) < grace {
				outcome = refreshRace
				return nil, errInvalidRefresh
			}
			next := *c
			at := now
			next.RevokedAt = &at
			next.RevokedReason = "reuse"
			outcome, fam = refreshReused, next
			return &next, nil
		}
		return nil, errInvalidRefresh
	})
	switch {
	case outcome == refreshReused && err == nil:
		_ = s.deleteToken(ctx, fam.SessionID)
		return nil, fmt.Errorf("iam: %w", ErrRefreshReused)
	case errors.Is(err, errInvalidRefresh):
		return nil, fmt.Errorf("iam: %w", errInvalidRefresh)
	case err != nil:
		return nil, fmt.Errorf("iam: rotating refresh token: %w", err)
	}

	m = s.requestMeta(ctx, m)
	accessRaw, sess, err := s.createSession(ctx, newSessionID, domain.APIToken{
		AccountID: fam.AccountID,
		Name:      fam.Name,
		MFA:       fam.MFA,
		Scopes:    fam.Scopes,
		LastIP:    m.RemoteIP,
		UserAgent: m.UserAgent,
	}, now)
	if err != nil {
		return nil, err
	}
	if oldSession != "" {
		if err := s.deleteToken(ctx, oldSession); err != nil {
			return nil, err
		}
	}
	out := *acct
	return &TokenPair{
		AccessToken:      accessRaw,
		RefreshToken:     newRaw,
		Session:          sess,
		AccessExpiresAt:  *sess.ExpiresAt,
		RefreshExpiresAt: fam.ExpiresAt,
		Account:          &out,
	}, nil
}

// revokeRefreshFamilies deletes every refresh family matching match and its
// current access session, and prunes expired families along the way. It
// returns how many families were revoked.
func (s *Service) revokeRefreshFamilies(ctx context.Context, match func(*refreshFamily) bool) (int, error) {
	fams, err := meta.ListDocsT[refreshFamily](ctx, s.store, CollRefreshTokens, "")
	if err != nil {
		return 0, fmt.Errorf("iam: listing refresh tokens: %w", err)
	}
	now := s.now()
	n := 0
	for i := range fams {
		f := &fams[i]
		expired := !now.Before(f.ExpiresAt)
		if !expired && !match(f) {
			continue
		}
		if err := s.store.DeleteDoc(ctx, CollRefreshTokens, f.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return n, fmt.Errorf("iam: deleting refresh token: %w", err)
		}
		if expired && !match(f) {
			continue
		}
		if f.SessionID != "" {
			if err := s.deleteToken(ctx, f.SessionID); err != nil {
				return n, err
			}
		}
		if f.RevokedAt == nil {
			n++
		}
	}
	return n, nil
}

// revokeSessionRefresh revokes the refresh family renewing session
// sessionID, if any (signing out a session ends its login).
func (s *Service) revokeSessionRefresh(ctx context.Context, sessionID string) error {
	_, err := s.revokeRefreshFamilies(ctx, func(f *refreshFamily) bool { return f.SessionID == sessionID })
	return err
}

// revokeAccountRefresh revokes every refresh family of the account except
// the one renewing session keepSessionID ("" = none kept).
func (s *Service) revokeAccountRefresh(ctx context.Context, accountID, keepSessionID string) error {
	_, err := s.revokeRefreshFamilies(ctx, func(f *refreshFamily) bool {
		return f.AccountID == accountID && (keepSessionID == "" || f.SessionID != keepSessionID)
	})
	return err
}
