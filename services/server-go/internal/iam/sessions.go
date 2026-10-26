package iam

import (
	"context"
	"errors"
	"fmt"
	"net"
	"sort"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// maxUserAgentLen bounds the stored user agent (bytes).
const maxUserAgentLen = 256

// TokenMetaToucher is optionally implemented by a domain.MetaStore that can
// persist the client address and user agent of a token's latest use along
// with LastUsedAt. When the store implements it, Authenticate records them
// (throttled like TouchToken); otherwise only TouchToken is called and the
// values recorded when the token was issued are kept.
type TokenMetaToucher interface {
	TouchTokenMeta(ctx context.Context, id string, at time.Time, lastIP, userAgent string) error
}

// TokenKind returns the effective kind of t. Tokens created before v0.4
// have no Kind: those named "session" are sessions, others API tokens.
func TokenKind(t *domain.APIToken) string {
	switch {
	case t == nil:
		return ""
	case t.Kind != "":
		return t.Kind
	case t.ServiceAccountID != "":
		return domain.TokenService
	case t.Name == sessionTokenName:
		return domain.TokenSession
	default:
		return domain.TokenAPI
	}
}

// activeAccount loads accountID, failing with ErrNotFound when it does not
// exist and ErrForbidden when it is disabled.
func (s *Service) activeAccount(ctx context.Context, accountID string) (*domain.Account, error) {
	acct, err := s.store.GetAccount(ctx, accountID)
	if err != nil {
		return nil, fmt.Errorf("iam: looking up account: %w", err)
	}
	if acct.Disabled {
		return nil, fmt.Errorf("iam: account is disabled: %w", domain.ErrForbidden)
	}
	return acct, nil
}

// requestMeta completes meta with the client address and user agent
// recorded in ctx (WithRemoteAddr / gRPC peer, WithUserAgent) and
// normalizes both: the address becomes a bare IP ("" if unparsable) and the
// user agent is stripped of control characters and truncated.
func (s *Service) requestMeta(ctx context.Context, meta domain.SessionMeta) domain.SessionMeta {
	if meta.RemoteIP == "" {
		meta.RemoteIP = clientIP(ctx)
	}
	if meta.UserAgent == "" {
		meta.UserAgent = UserAgentFrom(ctx)
	}
	meta.RemoteIP = normalizeIP(meta.RemoteIP)
	meta.UserAgent = sanitizeUserAgent(meta.UserAgent)
	return meta
}

// normalizeIP returns the bare IP of addr ("host" or "host:port"), or ""
// when addr is not an IP address.
func normalizeIP(addr string) string {
	addr = strings.TrimSpace(addr)
	if host, _, err := net.SplitHostPort(addr); err == nil {
		addr = host
	}
	addr = strings.TrimSuffix(strings.TrimPrefix(addr, "["), "]")
	if i := strings.IndexByte(addr, '%'); i >= 0 { // IPv6 zone
		addr = addr[:i]
	}
	ip := net.ParseIP(addr)
	if ip == nil {
		return ""
	}
	return ip.String()
}

// sanitizeUserAgent drops control characters and invalid UTF-8 and caps the
// length at maxUserAgentLen bytes (on a rune boundary).
func sanitizeUserAgent(ua string) string {
	ua = strings.ToValidUTF8(ua, "")
	ua = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return -1
		}
		return r
	}, ua)
	ua = strings.TrimSpace(ua)
	if len(ua) > maxUserAgentLen {
		cut := maxUserAgentLen
		for cut > 0 && !utf8.RuneStart(ua[cut]) {
			cut--
		}
		ua = ua[:cut]
	}
	return ua
}

// issueSession creates a session token (Kind session, Options.SessionTTL)
// for accountID with the given MFA flag and client metadata.
func (s *Service) issueSession(ctx context.Context, accountID string, mfa bool, meta domain.SessionMeta) (string, *domain.APIToken, error) {
	return s.issue(ctx, &domain.APIToken{
		AccountID: accountID,
		Name:      sessionTokenName,
		Kind:      domain.TokenSession,
		MFA:       mfa,
		LastIP:    meta.RemoteIP,
		UserAgent: meta.UserAgent,
	}, s.opts.SessionTTL)
}

// mfaEnabled reports whether the account has a confirmed second factor.
// Store failures are returned (never treated as "no MFA").
func (s *Service) mfaEnabled(ctx context.Context, accountID string) (bool, error) {
	st, err := s.store.GetMFA(ctx, accountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return false, nil
		}
		return false, fmt.Errorf("iam: looking up mfa state: %w", err)
	}
	return st.TOTPEnabled, nil
}

// LoginWithMeta is the first login step. It verifies the password exactly
// like Login (same rate limiting and errors), then:
//
//   - without a second factor, issues a session (Kind session) recording
//     meta (falling back to the address and user agent in ctx) and returns
//     it in Token;
//   - with TOTP enabled, returns MFARequired and an MFAToken ("wtm_…",
//     valid for MFATokenTTL, at most MaxMFATokenAttempts code attempts)
//     to pass to CompleteMFALogin. No session exists yet.
//
// Account is set in both cases.
func (s *Service) LoginWithMeta(ctx context.Context, username, password string, meta domain.SessionMeta) (*domain.LoginResult, error) {
	meta = s.requestMeta(ctx, meta)
	acct, err := s.passwordLogin(ctx, username, password, meta.RemoteIP)
	if err != nil {
		return nil, err
	}
	enabled, err := s.mfaEnabled(ctx, acct.ID)
	if err != nil {
		return nil, err
	}
	if enabled {
		tok, err := s.pending.create(acct.ID, s.now())
		if err != nil {
			return nil, err
		}
		return &domain.LoginResult{Account: acct, MFARequired: true, MFAToken: tok}, nil
	}
	raw, _, err := s.issueSession(ctx, acct.ID, false, meta)
	if err != nil {
		return nil, err
	}
	return &domain.LoginResult{Token: raw, Account: acct}, nil
}

// CompleteMFALogin is the second login step: it exchanges an MFA token
// from LoginWithMeta plus a current TOTP code or an unused recovery code
// for a session with MFA set. Errors:
//
//   - unknown, expired, exhausted or concurrently used MFA token, or a
//     wrong code: ErrUnauthenticated (a wrong code counts against both the
//     token's attempts and the account's MFA rate limit);
//   - account MFA rate limit exhausted or account disabled: ErrForbidden.
//
// The MFA token is single use: it is consumed by a successful exchange.
func (s *Service) CompleteMFALogin(ctx context.Context, mfaToken, code string, meta domain.SessionMeta) (string, *domain.Account, error) {
	meta = s.requestMeta(ctx, meta)
	now := s.now()
	accountID, err := s.pending.claim(mfaToken, now)
	if err != nil {
		return "", nil, err
	}
	success := false
	defer func() {
		if success {
			s.pending.consume(mfaToken)
		} else {
			s.pending.release(mfaToken)
		}
	}()

	if s.mfaLimiter.blocked(accountID, now) {
		return "", nil, fmt.Errorf("iam: too many second-factor attempts, try again later: %w", domain.ErrForbidden)
	}
	acct, err := s.store.GetAccount(ctx, accountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			s.pending.consume(mfaToken)
			return "", nil, fmt.Errorf("iam: %w", errInvalidMFAToken)
		}
		return "", nil, fmt.Errorf("iam: looking up account: %w", err)
	}
	if acct.Disabled {
		s.pending.consume(mfaToken)
		return "", nil, fmt.Errorf("iam: account is disabled: %w", domain.ErrForbidden)
	}
	ok, err := s.verifySecondFactor(ctx, accountID, code, now, true)
	if errors.Is(err, errMFANotEnabled) {
		// MFA was disabled between the two steps: start over.
		s.pending.consume(mfaToken)
		return "", nil, fmt.Errorf("iam: %w", errInvalidMFAToken)
	}
	if err != nil {
		return "", nil, err
	}
	if !ok {
		s.mfaLimiter.fail(accountID, now)
		s.pending.fail(mfaToken)
		return "", nil, fmt.Errorf("iam: invalid authentication code: %w", domain.ErrUnauthenticated)
	}
	s.mfaLimiter.reset(accountID)
	raw, _, err := s.issueSession(ctx, accountID, true, meta)
	if err != nil {
		return "", nil, err
	}
	success = true
	out := *acct
	return raw, &out, nil
}

// IssueSession creates a session token for accountID directly, without a
// password (used by SSO after the identity provider authenticated the
// user). mfa records whether a second factor was verified. The account
// must exist (ErrNotFound) and be enabled (ErrForbidden).
func (s *Service) IssueSession(ctx context.Context, accountID string, mfa bool, meta domain.SessionMeta) (string, error) {
	acct, err := s.activeAccount(ctx, accountID)
	if err != nil {
		return "", err
	}
	raw, _, err := s.issueSession(ctx, acct.ID, mfa, s.requestMeta(ctx, meta))
	if err != nil {
		return "", err
	}
	return raw, nil
}

// ListSessions returns the account's unexpired sessions (tokens of Kind
// session, including pre-v0.4 tokens named "session"), newest first.
// TokenHash is set so callers can flag the current session (compare with
// TokenHashFrom); it is never serialized.
func (s *Service) ListSessions(ctx context.Context, accountID string) ([]domain.APIToken, error) {
	toks, err := s.store.ListTokens(ctx, accountID)
	if err != nil {
		return nil, fmt.Errorf("iam: listing tokens: %w", err)
	}
	now := s.now()
	out := make([]domain.APIToken, 0, len(toks))
	for _, t := range toks {
		if accountID == "" || t.AccountID != accountID || TokenKind(&t) != domain.TokenSession {
			continue
		}
		if t.ExpiresAt != nil && !now.Before(*t.ExpiresAt) {
			continue
		}
		out = append(out, t)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if !out[i].CreatedAt.Equal(out[j].CreatedAt) {
			return out[i].CreatedAt.After(out[j].CreatedAt)
		}
		return out[i].ID > out[j].ID
	})
	return out, nil
}

// RevokeOtherSessions deletes every session of the account except the one
// whose token hash is keepTokenHash (normally TokenHashFrom(ctx); "" revokes
// all sessions). API tokens are kept. Pending MFA logins of the account are
// cancelled too. It returns the number of sessions revoked.
func (s *Service) RevokeOtherSessions(ctx context.Context, accountID, keepTokenHash string) (int, error) {
	if accountID == "" {
		return 0, fmt.Errorf("iam: account id is required: %w", domain.ErrInvalid)
	}
	toks, err := s.store.ListTokens(ctx, accountID)
	if err != nil {
		return 0, fmt.Errorf("iam: listing tokens: %w", err)
	}
	n := 0
	keepID := ""
	for _, t := range toks {
		if t.AccountID != accountID || TokenKind(&t) != domain.TokenSession {
			continue
		}
		if keepTokenHash != "" && t.TokenHash == keepTokenHash {
			keepID = t.ID
			continue
		}
		if err := s.deleteToken(ctx, t.ID); err != nil {
			return n, err
		}
		n++
	}
	s.pending.dropAccount(accountID)
	// device-auth (§16.6): the other logins' refresh tokens end too.
	if err := s.revokeAccountRefresh(ctx, accountID, keepID); err != nil {
		return n, err
	}
	return n, nil
}

// deleteToken removes a token (already-deleted is not an error) and forgets
// its touch throttle entry.
func (s *Service) deleteToken(ctx context.Context, id string) error {
	if err := s.store.DeleteToken(ctx, id); err != nil && !errors.Is(err, domain.ErrNotFound) {
		return fmt.Errorf("iam: deleting token: %w", err)
	}
	s.touchMu.Lock()
	delete(s.touched, id)
	s.touchMu.Unlock()
	return nil
}

// revokeAllTokens deletes every token of the account (sessions and API
// tokens) and cancels its pending MFA logins.
func (s *Service) revokeAllTokens(ctx context.Context, accountID string) error {
	toks, err := s.store.ListTokens(ctx, accountID)
	if err != nil {
		return fmt.Errorf("iam: listing tokens: %w", err)
	}
	for _, t := range toks {
		if t.AccountID != accountID {
			continue
		}
		if err := s.deleteToken(ctx, t.ID); err != nil {
			return err
		}
	}
	s.pending.dropAccount(accountID)
	// device-auth (§16.6): and every refresh token.
	return s.revokeAccountRefresh(ctx, accountID, "")
}
