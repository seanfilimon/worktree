package iam

// OAuth 2.0 device authorization grant (RFC 8628) for the CLI
// (docs/ENGINEERING_PLAN.md §16.6, workstream device-auth).
//
// A device (wt login --device) asks for a device code and a short user
// code, shows the user code and a verification link, and polls. The user
// opens the link while signed in to the admin panel, confirms the code and
// approves or denies. The next poll after approval returns a token pair
// (access session + refresh token, see refresh.go) bound to the approving
// account. Device and user codes are single use and expire after
// DeviceCodeTTL.
//
// Storage (collection device_codes):
//
//	DocKey("u", USER-CODE)          -> deviceDoc   (the request)
//	DocKey("d", sha256(device code)) -> deviceIndex (device code -> user code)

import (
	"context"
	"crypto/rand"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"
	"unicode"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

const (
	// CollDeviceCodes holds pending device authorizations.
	CollDeviceCodes = "device_codes"
	// DeviceCodePrefix starts every raw device code.
	DeviceCodePrefix = "wtd_"
	// DeviceCodeTTL is how long a device authorization may be completed.
	DeviceCodeTTL = 10 * time.Minute
	// DeviceInterval is the minimum polling interval announced to clients.
	DeviceInterval = 5 * time.Second
	// deviceSlowDown is added to a client's interval when it polls too fast.
	deviceSlowDown = 5 * time.Second
	// devicePollTolerance forgives polls that arrive slightly early (jitter).
	devicePollTolerance = time.Second

	// Device statuses.
	DeviceStatusPending  = "pending"
	DeviceStatusApproved = "approved"
	DeviceStatusDenied   = "denied"

	// DeviceSessionPrefix starts the name of sessions created by the
	// device flow ("device: <client name>").
	DeviceSessionPrefix = "device: "

	// userCodeAlphabet has no vowels (no accidental words) and no
	// look-alike characters; 20^8 ≈ 2.6e10 codes (RFC 8628 §6.1).
	userCodeAlphabet = "BCDFGHJKLMNPQRSTVWXZ"
	userCodeLen      = 8

	maxDeviceClientName = 100
	maxDeviceClientID   = 64
	maxPendingDevices   = 10000

	// Rate limits: device authorizations started per client address, and
	// failed user-code lookups per account, per window.
	maxDeviceStartsPerIP   = 30
	maxDeviceCodeFailures  = 10
	deviceRateWindow       = 10 * time.Minute
	deviceSweepMinInterval = 30 * time.Second
)

// ErrRateLimited is wrapped by errors that a rate limit rejected; the HTTP
// layer answers 429 rate_limited.
var ErrRateLimited = errors.New("rate limited")

var (
	errUnknownUserCode   = fmt.Errorf("unknown or expired device code: %w", domain.ErrNotFound)
	errDeviceMFARequired = fmt.Errorf("approving a device needs your two-factor code: %w", domain.ErrForbidden)
)

// DeviceError is a token-endpoint error of RFC 8628 §3.5 (and RFC 6749
// §5.2). Code is the OAuth error code the HTTP layer reports.
type DeviceError struct {
	Code    string
	Message string
}

func (e *DeviceError) Error() string { return e.Code + ": " + e.Message }

// Token endpoint errors of the device flow.
var (
	ErrAuthorizationPending = &DeviceError{"authorization_pending", "the user has not approved this device yet"}
	ErrSlowDown             = &DeviceError{"slow_down", "polling too fast; wait the announced interval (now increased by 5 seconds)"}
	ErrAccessDenied         = &DeviceError{"access_denied", "the user denied this device"}
	ErrExpiredToken         = &DeviceError{"expired_token", "the device code expired; start again"}
	ErrInvalidGrant         = &DeviceError{"invalid_grant", "unknown or already used device code"}
)

// DeviceAuthorization is the answer to a device authorization request.
type DeviceAuthorization struct {
	DeviceCode string
	UserCode   string // XXXX-XXXX
	ExpiresIn  time.Duration
	Interval   time.Duration
}

// DeviceRequest describes a pending device authorization to the user who
// may approve it (GET /api/v1/auth/device/{user_code}).
type DeviceRequest struct {
	UserCode string `json:"user_code"`
	// ClientID and ClientName are what the device said about itself.
	ClientID   string   `json:"client_id,omitempty"`
	ClientName string   `json:"client_name"`
	Scopes     []string `json:"scopes"` // empty = full account access
	// Status is pending, approved or denied.
	Status    string    `json:"status"`
	RemoteIP  string    `json:"remote_ip,omitempty"`  // address of the device
	UserAgent string    `json:"user_agent,omitempty"` // user agent of the device
	CreatedAt time.Time `json:"created_at"`
	ExpiresAt time.Time `json:"expires_at"`
	// MFARequired reports that approving needs a TOTP or recovery code
	// because the caller's session was not created with a second factor.
	MFARequired bool `json:"mfa_required"`
}

// deviceDoc is the device_codes document of one request.
type deviceDoc struct {
	UserCode   string     `json:"user_code"`
	DeviceHash string     `json:"device_hash"`
	ClientID   string     `json:"client_id,omitempty"`
	ClientName string     `json:"client_name"`
	Scopes     []string   `json:"scopes,omitempty"`
	Status     string     `json:"status"`
	RemoteIP   string     `json:"remote_ip,omitempty"`
	UserAgent  string     `json:"user_agent,omitempty"`
	CreatedAt  time.Time  `json:"created_at"`
	ExpiresAt  time.Time  `json:"expires_at"`
	IntervalS  int        `json:"interval"` // seconds
	LastPollAt *time.Time `json:"last_poll_at,omitempty"`
	AccountID  string     `json:"account_id,omitempty"` // approver / denier
	MFA        bool       `json:"mfa,omitempty"`        // approval verified a second factor
	DecidedAt  *time.Time `json:"decided_at,omitempty"`
}

type deviceIndex struct {
	UserCode string `json:"user_code"`
}

// deviceLimits holds the in-memory rate limiters of the device flow. The
// zero value is ready to use.
type deviceLimits struct {
	once      sync.Once
	starts    *loginLimiter // per client IP
	failures  *loginLimiter // per account
	mu        sync.Mutex
	lastSweep time.Time
}

func (d *deviceLimits) init() *deviceLimits {
	d.once.Do(func() {
		d.starts = newLoginLimiter(maxDeviceStartsPerIP, deviceRateWindow)
		d.failures = newLoginLimiter(maxDeviceCodeFailures, deviceRateWindow)
	})
	return d
}

func userKey(code string) string                { return domain.DocKey("u", code) }
func deviceKey(hash string) string              { return domain.DocKey("d", hash) }
func (d *deviceDoc) expired(now time.Time) bool { return !now.Before(d.ExpiresAt) }

// PublicURL is Options.PublicURL without a trailing slash ("" when unset).
func (s *Service) PublicURL() string { return strings.TrimRight(s.opts.PublicURL, "/") }

// NormalizeUserCode accepts a user code in any case, with or without the
// hyphen and spaces, and returns the canonical "XXXX-XXXX" form.
func NormalizeUserCode(code string) (string, bool) {
	var b strings.Builder
	for _, r := range code {
		switch {
		case r == '-' || unicode.IsSpace(r):
			continue
		case b.Len() >= userCodeLen:
			return "", false
		}
		r = unicode.ToUpper(r)
		if !strings.ContainsRune(userCodeAlphabet, r) {
			return "", false
		}
		b.WriteRune(r)
	}
	if b.Len() != userCodeLen {
		return "", false
	}
	s := b.String()
	return s[:4] + "-" + s[4:], true
}

func newUserCode() (string, error) {
	buf := make([]byte, 32)
	out := make([]byte, 0, userCodeLen)
	// Rejection sampling: bytes >= 240 would bias the modulo.
	for len(out) < userCodeLen {
		if _, err := rand.Read(buf); err != nil {
			return "", fmt.Errorf("iam: generating user code: %w", err)
		}
		for _, c := range buf {
			if c < 240 && len(out) < userCodeLen {
				out = append(out, userCodeAlphabet[int(c)%len(userCodeAlphabet)])
			}
		}
	}
	return string(out[:4]) + "-" + string(out[4:]), nil
}

// cleanClientText strips control characters and caps the length.
func cleanClientText(s string, max int) string {
	s = sanitizeUserAgent(s)
	if len(s) > max {
		cut := max
		for cut > 0 && (s[cut]&0xC0) == 0x80 {
			cut--
		}
		s = strings.TrimSpace(s[:cut])
	}
	return s
}

func validClientID(id string) bool {
	if len(id) > maxDeviceClientID {
		return false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		if !('a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' || '0' <= c && c <= '9' || c == '-' || c == '_' || c == '.') {
			return false
		}
	}
	return true
}

// StartDeviceAuthorization begins a device authorization (RFC 8628 §3.1)
// for a client describing itself with clientID (optional, [A-Za-z0-9._-]
// up to 64) and clientName (shown to the approving user; default
// "Device"). scopes restricts the resulting tokens like scoped API tokens
// (empty = full access). Each client address may start
// maxDeviceStartsPerIP authorizations per 10 minutes (ErrRateLimited).
func (s *Service) StartDeviceAuthorization(ctx context.Context, clientID, clientName string, scopes []string, m domain.SessionMeta) (*DeviceAuthorization, error) {
	m = s.requestMeta(ctx, m)
	clientID = strings.TrimSpace(clientID)
	if !validClientID(clientID) {
		return nil, fmt.Errorf("iam: client_id must be up to %d letters, digits, '.', '-' or '_': %w", maxDeviceClientID, domain.ErrInvalid)
	}
	clientName = cleanClientText(clientName, maxDeviceClientName)
	if clientName == "" {
		clientName = "Device"
	}
	scopes, err := ValidateScopes(scopes)
	if err != nil {
		return nil, fmt.Errorf("iam: %w", err)
	}
	lim := s.device.init()
	now := s.now().UTC()
	key := "start\x00" + m.RemoteIP
	if lim.starts.blocked(key, now) {
		return nil, fmt.Errorf("iam: too many device authorizations from this address, try again later: %w", ErrRateLimited)
	}
	lim.starts.fail(key, now)
	pending, err := s.sweepDeviceCodes(ctx, now, false)
	if err != nil {
		return nil, err
	}
	if pending >= maxPendingDevices {
		if pending, err = s.sweepDeviceCodes(ctx, now, true); err != nil {
			return nil, err
		}
		if pending >= maxPendingDevices {
			return nil, fmt.Errorf("iam: too many pending device authorizations, try again later: %w", ErrRateLimited)
		}
	}

	deviceRaw, deviceHash, err := newSecret(DeviceCodePrefix)
	if err != nil {
		return nil, err
	}
	doc := deviceDoc{
		DeviceHash: deviceHash,
		ClientID:   clientID,
		ClientName: clientName,
		Scopes:     scopes,
		Status:     DeviceStatusPending,
		RemoteIP:   m.RemoteIP,
		UserAgent:  m.UserAgent,
		CreatedAt:  now,
		ExpiresAt:  now.Add(DeviceCodeTTL),
		IntervalS:  int(DeviceInterval / time.Second),
	}
	for attempt := 0; ; attempt++ {
		code, err := newUserCode()
		if err != nil {
			return nil, err
		}
		doc.UserCode = code
		err = s.store.CreateDoc(ctx, CollDeviceCodes, userKey(code), &doc)
		if err == nil {
			break
		}
		if !errors.Is(err, domain.ErrAlreadyExists) || attempt >= 8 {
			return nil, fmt.Errorf("iam: storing device code: %w", err)
		}
	}
	if err := s.store.CreateDoc(ctx, CollDeviceCodes, deviceKey(deviceHash), &deviceIndex{UserCode: doc.UserCode}); err != nil {
		_ = s.store.DeleteDoc(ctx, CollDeviceCodes, userKey(doc.UserCode))
		return nil, fmt.Errorf("iam: storing device code: %w", err)
	}
	return &DeviceAuthorization{
		DeviceCode: deviceRaw,
		UserCode:   doc.UserCode,
		ExpiresIn:  DeviceCodeTTL,
		Interval:   DeviceInterval,
	}, nil
}

// sweepDeviceCodes deletes expired device authorizations (at most once per
// deviceSweepMinInterval unless force) and returns how many remain; -1
// when the sweep was skipped and the count is unknown (treated as low).
func (s *Service) sweepDeviceCodes(ctx context.Context, now time.Time, force bool) (int, error) {
	lim := s.device.init()
	lim.mu.Lock()
	if !force && now.Sub(lim.lastSweep) < deviceSweepMinInterval && now.Sub(lim.lastSweep) >= 0 {
		lim.mu.Unlock()
		return -1, nil
	}
	lim.lastSweep = now
	lim.mu.Unlock()
	docs, err := meta.ListDocsT[deviceDoc](ctx, s.store, CollDeviceCodes, domain.DocKey("u", ""))
	if err != nil {
		return 0, fmt.Errorf("iam: listing device codes: %w", err)
	}
	left := 0
	for i := range docs {
		d := &docs[i]
		if !d.expired(now) {
			left++
			continue
		}
		s.deleteDevice(ctx, d)
	}
	return left, nil
}

// deleteDevice removes a device authorization and its index (best effort).
func (s *Service) deleteDevice(ctx context.Context, d *deviceDoc) {
	_ = s.store.DeleteDoc(ctx, CollDeviceCodes, userKey(d.UserCode))
	if d.DeviceHash != "" {
		_ = s.store.DeleteDoc(ctx, CollDeviceCodes, deviceKey(d.DeviceHash))
	}
}

// deviceCaller checks the approving principal: a signed-in, enabled user
// account within its failed-lookup budget.
func (s *Service) deviceCaller(ctx context.Context, p *domain.Principal) (*domain.Account, error) {
	if p == nil || p.AccountID == "" || p.IsServiceAccount() {
		return nil, fmt.Errorf("iam: sign in with a user account to approve devices: %w", domain.ErrForbidden)
	}
	acct, err := s.activeAccount(ctx, p.AccountID)
	if err != nil {
		return nil, err
	}
	if s.device.init().failures.blocked(acct.ID, s.now()) {
		return nil, fmt.Errorf("iam: too many unknown device codes, try again later: %w", ErrRateLimited)
	}
	return acct, nil
}

// loadDevice returns the pending-or-decided request for userCode; unknown,
// malformed and expired codes count against the account's failure budget.
func (s *Service) loadDevice(ctx context.Context, accountID, userCode string) (*deviceDoc, string, error) {
	now := s.now()
	code, ok := NormalizeUserCode(userCode)
	if !ok {
		s.device.init().failures.fail(accountID, now)
		return nil, "", fmt.Errorf("iam: %w", errUnknownUserCode)
	}
	var d deviceDoc
	if err := s.store.GetDoc(ctx, CollDeviceCodes, userKey(code), &d); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			s.device.init().failures.fail(accountID, now)
			return nil, "", fmt.Errorf("iam: %w", errUnknownUserCode)
		}
		return nil, "", fmt.Errorf("iam: looking up device code: %w", err)
	}
	if d.expired(now) {
		return nil, "", fmt.Errorf("iam: %w", errUnknownUserCode)
	}
	return &d, code, nil
}

func (d *deviceDoc) view(mfaRequired bool) *DeviceRequest {
	scopes := append([]string{}, d.Scopes...)
	return &DeviceRequest{
		UserCode:    d.UserCode,
		ClientID:    d.ClientID,
		ClientName:  d.ClientName,
		Scopes:      scopes,
		Status:      d.Status,
		RemoteIP:    d.RemoteIP,
		UserAgent:   d.UserAgent,
		CreatedAt:   d.CreatedAt,
		ExpiresAt:   d.ExpiresAt,
		MFARequired: mfaRequired,
	}
}

// deviceNeedsMFA reports whether approving needs a second-factor code: the
// account has MFA enabled and the caller's session did not verify it.
func (s *Service) deviceNeedsMFA(ctx context.Context, p *domain.Principal) (enabled, needed bool, err error) {
	enabled, err = s.mfaEnabled(ctx, p.AccountID)
	if err != nil {
		return false, false, err
	}
	return enabled, enabled && !p.MFA, nil
}

// DeviceRequest returns the device authorization with user code userCode
// for the signed-in user p to review. Unknown or expired codes fail with
// ErrNotFound; repeated failures with ErrRateLimited.
func (s *Service) DeviceRequest(ctx context.Context, userCode string, p *domain.Principal) (*DeviceRequest, error) {
	acct, err := s.deviceCaller(ctx, p)
	if err != nil {
		return nil, err
	}
	d, _, err := s.loadDevice(ctx, acct.ID, userCode)
	if err != nil {
		return nil, err
	}
	_, needed, err := s.deviceNeedsMFA(ctx, p)
	if err != nil {
		return nil, err
	}
	return d.view(needed), nil
}

// ApproveDevice approves a pending device authorization on behalf of p.
// When p's account has MFA enabled and p's session was not created with a
// second factor, mfaCode must be a current TOTP or unused recovery code
// (ErrForbidden otherwise; wrong codes count against the account's MFA rate
// limit). The device's session records the second factor when the account
// has one. Requests that are no longer pending fail with ErrConflict.
func (s *Service) ApproveDevice(ctx context.Context, userCode string, p *domain.Principal, mfaCode string) (*DeviceRequest, error) {
	return s.decideDevice(ctx, userCode, p, mfaCode, DeviceStatusApproved)
}

// DenyDevice denies a pending device authorization on behalf of p; the
// device's next poll fails with access_denied.
func (s *Service) DenyDevice(ctx context.Context, userCode string, p *domain.Principal) (*DeviceRequest, error) {
	return s.decideDevice(ctx, userCode, p, "", DeviceStatusDenied)
}

func (s *Service) decideDevice(ctx context.Context, userCode string, p *domain.Principal, mfaCode, status string) (*DeviceRequest, error) {
	acct, err := s.deviceCaller(ctx, p)
	if err != nil {
		return nil, err
	}
	_, code, err := s.loadDevice(ctx, acct.ID, userCode)
	if err != nil {
		return nil, err
	}
	enabled, needed := false, false
	if status == DeviceStatusApproved {
		if enabled, needed, err = s.deviceNeedsMFA(ctx, p); err != nil {
			return nil, err
		}
	}
	if needed {
		if err := s.checkDeviceMFA(ctx, acct.ID, mfaCode); err != nil {
			return nil, err
		}
	}
	now := s.now().UTC()
	var out *deviceDoc
	err = meta.UpdateDocT(ctx, s.store, CollDeviceCodes, userKey(code), func(cur *deviceDoc) (*deviceDoc, error) {
		if cur == nil || cur.expired(now) {
			return nil, errUnknownUserCode
		}
		if cur.Status != DeviceStatusPending {
			return nil, fmt.Errorf("device code %s was already %s: %w", cur.UserCode, cur.Status, domain.ErrConflict)
		}
		next := *cur
		next.Status = status
		next.AccountID = acct.ID
		next.MFA = status == DeviceStatusApproved && enabled
		at := now
		next.DecidedAt = &at
		out = &next
		return &next, nil
	})
	if err != nil {
		return nil, fmt.Errorf("iam: %w", err)
	}
	return out.view(false), nil
}

// checkDeviceMFA verifies a second-factor code for a device approval.
func (s *Service) checkDeviceMFA(ctx context.Context, accountID, code string) error {
	code = strings.ReplaceAll(strings.TrimSpace(code), " ", "")
	if code == "" {
		return fmt.Errorf("iam: %w", errDeviceMFARequired)
	}
	now := s.now()
	if s.mfaLimiter.blocked(accountID, now) {
		return fmt.Errorf("iam: too many second-factor attempts, try again later: %w", domain.ErrForbidden)
	}
	ok, err := s.verifySecondFactor(ctx, accountID, code, now, true)
	if errors.Is(err, errMFANotEnabled) {
		return nil // disabled meanwhile: nothing to verify
	}
	if err != nil {
		return err
	}
	if !ok {
		s.mfaLimiter.fail(accountID, now)
		return fmt.Errorf("iam: invalid authentication code: %w", domain.ErrForbidden)
	}
	s.mfaLimiter.reset(accountID)
	return nil
}

type deviceOutcome int

const (
	devicePending deviceOutcome = iota
	deviceTooFast
	deviceDenied
	deviceExpired
	deviceIssue
)

// PollDeviceToken is the device's token request (RFC 8628 §3.4). It
// returns a token pair once the user approved; otherwise one of
// ErrAuthorizationPending, ErrSlowDown (polled before the interval passed;
// the interval grows by 5 seconds), ErrAccessDenied, ErrExpiredToken or
// ErrInvalidGrant (unknown or already redeemed code). The device code is
// single use: it is deleted once tokens are issued, denied or expired.
func (s *Service) PollDeviceToken(ctx context.Context, deviceCode string, m domain.SessionMeta) (*TokenPair, error) {
	deviceCode = strings.TrimSpace(deviceCode)
	if !wellFormedSecret(DeviceCodePrefix, deviceCode) {
		return nil, ErrInvalidGrant
	}
	h := HashToken(deviceCode)
	var idx deviceIndex
	if err := s.store.GetDoc(ctx, CollDeviceCodes, deviceKey(h), &idx); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, ErrInvalidGrant
		}
		return nil, fmt.Errorf("iam: looking up device code: %w", err)
	}
	now := s.now().UTC()
	var (
		outcome deviceOutcome
		doc     deviceDoc
	)
	err := meta.UpdateDocT(ctx, s.store, CollDeviceCodes, userKey(idx.UserCode), func(cur *deviceDoc) (*deviceDoc, error) {
		if cur == nil || !equalHash(cur.DeviceHash, h) {
			return nil, ErrInvalidGrant
		}
		doc = *cur
		if cur.expired(now) {
			outcome = deviceExpired
			return nil, nil // delete
		}
		next := *cur
		interval := time.Duration(cur.IntervalS) * time.Second
		at := now
		tooFast := cur.LastPollAt != nil && now.Sub(*cur.LastPollAt) < interval-devicePollTolerance
		next.LastPollAt = &at
		switch {
		case cur.Status == DeviceStatusDenied:
			outcome = deviceDenied
			return nil, nil
		case cur.Status == DeviceStatusApproved:
			outcome = deviceIssue
			return nil, nil // single use: redeemed now
		case tooFast:
			next.IntervalS += int(deviceSlowDown / time.Second)
			outcome = deviceTooFast
		default:
			outcome = devicePending
		}
		return &next, nil
	})
	if err != nil {
		var de *DeviceError
		if errors.As(err, &de) {
			return nil, de
		}
		return nil, fmt.Errorf("iam: polling device code: %w", err)
	}
	switch outcome {
	case devicePending:
		return nil, ErrAuthorizationPending
	case deviceTooFast:
		return nil, ErrSlowDown
	case deviceDenied:
		_ = s.store.DeleteDoc(ctx, CollDeviceCodes, deviceKey(h))
		return nil, ErrAccessDenied
	case deviceExpired:
		_ = s.store.DeleteDoc(ctx, CollDeviceCodes, deviceKey(h))
		return nil, ErrExpiredToken
	}
	_ = s.store.DeleteDoc(ctx, CollDeviceCodes, deviceKey(h))
	acct, err := s.store.GetAccount(ctx, doc.AccountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, ErrAccessDenied
		}
		return nil, fmt.Errorf("iam: looking up account: %w", err)
	}
	if acct.Disabled {
		return nil, ErrAccessDenied
	}
	m = s.requestMeta(ctx, m)
	if m.UserAgent == "" {
		m.UserAgent = doc.UserAgent
	}
	return s.issuePair(ctx, acct, doc.MFA, DeviceSessionPrefix+doc.ClientName, doc.Scopes, m)
}
