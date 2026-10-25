package sso

import (
	"bytes"
	"context"
	"crypto"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/big"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ---- discovery ----

// providerMetadata is the subset of the OpenID Provider Metadata
// (OpenID Connect Discovery 1.0 §3) the relying party uses.
type providerMetadata struct {
	Issuer                string   `json:"issuer"`
	AuthorizationEndpoint string   `json:"authorization_endpoint"`
	TokenEndpoint         string   `json:"token_endpoint"`
	UserinfoEndpoint      string   `json:"userinfo_endpoint"`
	JWKSURI               string   `json:"jwks_uri"`
	TokenAuthMethods      []string `json:"token_endpoint_auth_methods_supported"`
	SigningAlgs           []string `json:"id_token_signing_alg_values_supported"`
}

type discoveryEntry struct {
	md        *providerMetadata
	fetchedAt time.Time
}

// providerErr marks failures talking to the identity provider.
func providerErr(cause error, format string, args ...any) error {
	return &Error{Msg: fmt.Sprintf(format, args...), Kind: domain.ErrUnauthenticated, Cause: cause}
}

// discover returns the provider metadata of issuer, from cache when fresh.
// A stale entry is served when refreshing fails.
func (s *Service) discover(ctx context.Context, issuer string) (*providerMetadata, error) {
	s.discMu.Lock()
	defer s.discMu.Unlock()
	now := s.now()
	e := s.disc[issuer]
	if e != nil && now.Sub(e.fetchedAt) < s.opts.DiscoveryTTL {
		return e.md, nil
	}
	md, err := s.fetchDiscovery(ctx, issuer)
	if err != nil {
		if e != nil {
			s.log.WarnContext(ctx, "sso: discovery refresh failed; using cached metadata", "issuer", issuer, "error", err)
			return e.md, nil
		}
		s.log.WarnContext(ctx, "sso: discovery failed", "issuer", issuer, "error", err)
		return nil, providerErr(err, "the identity provider could not be reached")
	}
	s.disc[issuer] = &discoveryEntry{md: md, fetchedAt: now}
	return md, nil
}

func (s *Service) fetchDiscovery(ctx context.Context, issuer string) (*providerMetadata, error) {
	wellKnown := strings.TrimSuffix(issuer, "/") + "/.well-known/openid-configuration"
	var md providerMetadata
	if err := s.getJSON(ctx, wellKnown, "", &md); err != nil {
		return nil, err
	}
	// OpenID Connect Discovery §4.3: the issuer must match exactly.
	if md.Issuer != issuer {
		return nil, fmt.Errorf("discovery issuer %q does not match configured issuer %q", md.Issuer, issuer)
	}
	for _, ep := range []struct{ name, v string }{
		{"authorization_endpoint", md.AuthorizationEndpoint},
		{"token_endpoint", md.TokenEndpoint},
		{"jwks_uri", md.JWKSURI},
	} {
		if ep.v == "" {
			return nil, fmt.Errorf("discovery document lacks %s", ep.name)
		}
		if err := s.checkEndpoint(ep.name, ep.v); err != nil {
			return nil, fmt.Errorf("discovery %s: %w", ep.name, err)
		}
	}
	if md.UserinfoEndpoint != "" {
		if err := s.checkEndpoint("userinfo_endpoint", md.UserinfoEndpoint); err != nil {
			md.UserinfoEndpoint = "" // unusable; the fallback is optional
		}
	}
	return &md, nil
}

// getJSON GETs u (with an optional bearer token) and decodes a 200 JSON
// response of at most maxResponseBodyBytes.
func (s *Service) getJSON(ctx context.Context, u, bearer string, dst any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")
	if bearer != "" {
		req.Header.Set("Authorization", "Bearer "+bearer)
	}
	resp, err := s.client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	body, err := readLimited(resp.Body)
	if err != nil {
		return err
	}
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("GET %s: HTTP %d", u, resp.StatusCode)
	}
	if err := json.Unmarshal(body, dst); err != nil {
		return fmt.Errorf("GET %s: decoding response: %w", u, err)
	}
	return nil
}

func readLimited(r io.Reader) ([]byte, error) {
	body, err := io.ReadAll(io.LimitReader(r, maxResponseBodyBytes+1))
	if err != nil {
		return nil, err
	}
	if len(body) > maxResponseBodyBytes {
		return nil, fmt.Errorf("response exceeds %d bytes", maxResponseBodyBytes)
	}
	return body, nil
}

// ---- JWKS ----

type jwk struct {
	Kty string `json:"kty"`
	Kid string `json:"kid"`
	Use string `json:"use"`
	Alg string `json:"alg"`
	N   string `json:"n"`
	E   string `json:"e"`
	Crv string `json:"crv"`
	X   string `json:"x"`
	Y   string `json:"y"`
}

type verifyKey struct {
	kid string
	alg string // "" = any compatible alg
	key crypto.PublicKey
}

type jwksEntry struct {
	keys      []verifyKey
	fetchedAt time.Time
}

const (
	minRSABits = 2048
	maxRSABits = 8192
)

// keysFor returns the candidate verification keys for (kid, alg). An
// unknown kid triggers a refresh of the key set, rate-limited by
// JWKSMinFetch.
func (s *Service) keysFor(ctx context.Context, jwksURI, kid, alg string) ([]verifyKey, error) {
	s.jwksMu.Lock()
	defer s.jwksMu.Unlock()
	now := s.now()
	e := s.jwks[jwksURI]
	fresh := e != nil && now.Sub(e.fetchedAt) < s.opts.JWKSTTL
	if fresh {
		if ks := matchKeys(e.keys, kid, alg); len(ks) > 0 {
			return ks, nil
		}
		if now.Sub(e.fetchedAt) < s.opts.JWKSMinFetch {
			return nil, nil
		}
	}
	keys, err := s.fetchJWKS(ctx, jwksURI)
	if err != nil {
		s.log.WarnContext(ctx, "sso: fetching JWKS failed", "jwks_uri", jwksURI, "error", err)
		if e != nil {
			return matchKeys(e.keys, kid, alg), nil
		}
		return nil, providerErr(err, "the identity provider's signing keys could not be retrieved")
	}
	s.jwks[jwksURI] = &jwksEntry{keys: keys, fetchedAt: now}
	return matchKeys(keys, kid, alg), nil
}

func matchKeys(keys []verifyKey, kid, alg string) []verifyKey {
	var out []verifyKey
	for _, k := range keys {
		if kid != "" && k.kid != "" && k.kid != kid {
			continue
		}
		if k.alg != "" && k.alg != alg {
			continue
		}
		switch alg {
		case "RS256":
			if _, ok := k.key.(*rsa.PublicKey); !ok {
				continue
			}
		case "ES256":
			if _, ok := k.key.(*ecdsa.PublicKey); !ok {
				continue
			}
		default:
			continue
		}
		out = append(out, k)
	}
	return out
}

func (s *Service) fetchJWKS(ctx context.Context, jwksURI string) ([]verifyKey, error) {
	var set struct {
		Keys []jwk `json:"keys"`
	}
	if err := s.getJSON(ctx, jwksURI, "", &set); err != nil {
		return nil, err
	}
	var keys []verifyKey
	for _, k := range set.Keys {
		if k.Use != "" && k.Use != "sig" {
			continue
		}
		pub, err := parseJWK(k)
		if err != nil {
			s.log.DebugContext(ctx, "sso: skipping unusable JWK", "kid", k.Kid, "error", err)
			continue
		}
		keys = append(keys, verifyKey{kid: k.Kid, alg: k.Alg, key: pub})
	}
	return keys, nil
}

func b64(s string) ([]byte, error) { return base64.RawURLEncoding.DecodeString(s) }

func parseJWK(k jwk) (crypto.PublicKey, error) {
	switch k.Kty {
	case "RSA":
		if k.Alg != "" && k.Alg != "RS256" {
			return nil, fmt.Errorf("unsupported alg %q", k.Alg)
		}
		nb, err := b64(k.N)
		if err != nil || len(nb) == 0 {
			return nil, errors.New("bad modulus")
		}
		eb, err := b64(k.E)
		if err != nil || len(eb) == 0 || len(eb) > 4 {
			return nil, errors.New("bad exponent")
		}
		n := new(big.Int).SetBytes(nb)
		if bits := n.BitLen(); bits < minRSABits || bits > maxRSABits {
			return nil, fmt.Errorf("RSA key size %d not in [%d, %d]", bits, minRSABits, maxRSABits)
		}
		var e int
		for _, b := range eb {
			e = e<<8 | int(b)
		}
		if e < 3 || e%2 == 0 {
			return nil, errors.New("bad exponent")
		}
		return &rsa.PublicKey{N: n, E: e}, nil
	case "EC":
		if k.Crv != "P-256" || (k.Alg != "" && k.Alg != "ES256") {
			return nil, fmt.Errorf("unsupported curve %q / alg %q", k.Crv, k.Alg)
		}
		x, err1 := b64(k.X)
		y, err2 := b64(k.Y)
		if err1 != nil || err2 != nil || len(x) != 32 || len(y) != 32 {
			return nil, errors.New("bad coordinates")
		}
		raw := append(append([]byte{4}, x...), y...)
		pub, err := ecdsa.ParseUncompressedPublicKey(elliptic.P256(), raw)
		if err != nil {
			return nil, err
		}
		return pub, nil
	default:
		return nil, fmt.Errorf("unsupported kty %q", k.Kty)
	}
}

// ---- ID token ----

const maxIDTokenLen = 16 << 10

type jwtHeader struct {
	Alg  string          `json:"alg"`
	Kid  string          `json:"kid"`
	Typ  string          `json:"typ"`
	Crit json.RawMessage `json:"crit"`
}

// claims is a decoded JWT claim set.
type claims map[string]any

func (c claims) str(name string) string {
	v, _ := c[name].(string)
	return v
}

// num returns a NumericDate claim (seconds since the epoch).
func (c claims) num(name string) (int64, bool) {
	n, ok := c[name].(json.Number)
	if !ok {
		return 0, false
	}
	if i, err := n.Int64(); err == nil {
		return i, true
	}
	f, err := n.Float64()
	if err != nil || f < 0 || f > 1<<53 {
		return 0, false
	}
	return int64(f), true
}

// boolish accepts true and "true" (some providers encode booleans as strings).
func (c claims) boolish(name string) bool {
	switch v := c[name].(type) {
	case bool:
		return v
	case string:
		return strings.EqualFold(v, "true")
	}
	return false
}

func (c claims) audiences() ([]string, bool) {
	switch v := c["aud"].(type) {
	case string:
		return []string{v}, true
	case []any:
		out := make([]string, 0, len(v))
		for _, a := range v {
			s, ok := a.(string)
			if !ok {
				return nil, false
			}
			out = append(out, s)
		}
		return out, true
	}
	return nil, false
}

func decodeClaims(raw []byte) (claims, error) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var c claims
	if err := dec.Decode(&c); err != nil || c == nil {
		return nil, errors.New("malformed claims")
	}
	return c, nil
}

// idTokenCheck carries the expectations of an ID token.
type idTokenCheck struct {
	issuer   string
	clientID string
	nonce    string
	jwksURI  string
	algs     []string // provider-advertised algs ("" / nil = no restriction)
}

func invalidToken(cause error, format string, args ...any) error {
	return &Error{Msg: fmt.Sprintf(format, args...), Kind: domain.ErrUnauthenticated, Cause: cause}
}

// verifyIDToken checks the signature and the standard claims of raw
// (OpenID Connect Core §3.1.3.7) and returns its claims.
func (s *Service) verifyIDToken(ctx context.Context, raw string, want idTokenCheck) (claims, error) {
	bad := func(reason string) error {
		return invalidToken(errors.New(reason), "the identity provider returned an invalid ID token (%s)", reason)
	}
	if raw == "" {
		return nil, bad("missing")
	}
	if len(raw) > maxIDTokenLen {
		return nil, bad("too large")
	}
	parts := strings.Split(raw, ".")
	if len(parts) != 3 {
		return nil, bad("malformed")
	}
	hb, err := b64(parts[0])
	if err != nil {
		return nil, bad("malformed header")
	}
	var hdr jwtHeader
	if err := json.Unmarshal(hb, &hdr); err != nil {
		return nil, bad("malformed header")
	}
	if len(hdr.Crit) > 0 {
		return nil, bad("unsupported critical header")
	}
	if hdr.Alg != "RS256" && hdr.Alg != "ES256" {
		return nil, bad("unsupported signing algorithm")
	}
	if len(want.algs) > 0 && !containsStr(want.algs, hdr.Alg) {
		return nil, bad("signing algorithm not advertised by the provider")
	}
	sig, err := b64(parts[2])
	if err != nil || len(sig) == 0 {
		return nil, bad("malformed signature")
	}
	keys, err := s.keysFor(ctx, want.jwksURI, hdr.Kid, hdr.Alg)
	if err != nil {
		return nil, err
	}
	if len(keys) == 0 {
		return nil, bad("unknown signing key")
	}
	digest := sha256.Sum256([]byte(parts[0] + "." + parts[1]))
	verified := false
	for _, k := range keys {
		if verifySig(k.key, hdr.Alg, digest[:], sig) {
			verified = true
			break
		}
	}
	if !verified {
		return nil, bad("bad signature")
	}

	pb, err := b64(parts[1])
	if err != nil {
		return nil, bad("malformed payload")
	}
	c, err := decodeClaims(pb)
	if err != nil {
		return nil, bad("malformed payload")
	}
	if c.str("iss") != want.issuer {
		return nil, bad("wrong issuer")
	}
	aud, ok := c.audiences()
	if !ok || !containsStr(aud, want.clientID) {
		return nil, bad("wrong audience")
	}
	azp, hasAzp := c["azp"]
	if hasAzp || len(aud) > 1 {
		if s, _ := azp.(string); s != want.clientID {
			return nil, bad("wrong authorized party")
		}
	}
	now := s.now()
	skew := s.opts.ClockSkew
	exp, ok := c.num("exp")
	if !ok {
		return nil, bad("missing exp")
	}
	if !now.Before(time.Unix(exp, 0).Add(skew)) {
		return nil, bad("expired")
	}
	iat, ok := c.num("iat")
	if !ok {
		return nil, bad("missing iat")
	}
	if time.Unix(iat, 0).After(now.Add(skew)) {
		return nil, bad("issued in the future")
	}
	if _, present := c["nbf"]; present {
		nbf, ok := c.num("nbf")
		if !ok {
			return nil, bad("malformed nbf")
		}
		if time.Unix(nbf, 0).After(now.Add(skew)) {
			return nil, bad("not yet valid")
		}
	}
	nonce := c.str("nonce")
	if want.nonce == "" || subtle.ConstantTimeCompare([]byte(nonce), []byte(want.nonce)) != 1 {
		return nil, bad("nonce mismatch")
	}
	sub := c.str("sub")
	if sub == "" || len(sub) > 255 {
		return nil, bad("missing subject")
	}
	return c, nil
}

func verifySig(key crypto.PublicKey, alg string, digest, sig []byte) bool {
	switch alg {
	case "RS256":
		pub, ok := key.(*rsa.PublicKey)
		return ok && rsa.VerifyPKCS1v15(pub, crypto.SHA256, digest, sig) == nil
	case "ES256":
		pub, ok := key.(*ecdsa.PublicKey)
		if !ok || len(sig) != 64 {
			return false
		}
		r := new(big.Int).SetBytes(sig[:32])
		sv := new(big.Int).SetBytes(sig[32:])
		return ecdsa.Verify(pub, digest, r, sv)
	}
	return false
}

func containsStr(list []string, s string) bool {
	for _, v := range list {
		if v == s {
			return true
		}
	}
	return false
}

// ---- token endpoint ----

type tokenResponse struct {
	AccessToken      string `json:"access_token"`
	TokenType        string `json:"token_type"`
	IDToken          string `json:"id_token"`
	Error            string `json:"error"`
	ErrorDescription string `json:"error_description"`
}

// exchangeCode redeems an authorization code (RFC 6749 §4.1.3 + RFC 7636).
func (s *Service) exchangeCode(ctx context.Context, md *providerMetadata, cfg *domain.SSOConfig, code, verifier string) (*tokenResponse, error) {
	form := url.Values{
		"grant_type":    {"authorization_code"},
		"code":          {code},
		"redirect_uri":  {cfg.RedirectURL},
		"code_verifier": {verifier},
	}
	method := tokenAuthMethod(md.TokenAuthMethods, cfg.ClientSecret != "")
	switch method {
	case "client_secret_post":
		form.Set("client_id", cfg.ClientID)
		form.Set("client_secret", cfg.ClientSecret)
	case "none":
		form.Set("client_id", cfg.ClientID)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, md.TokenEndpoint, strings.NewReader(form.Encode()))
	if err != nil {
		return nil, providerErr(err, "the identity provider could not be reached")
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("Accept", "application/json")
	if method == "client_secret_basic" {
		// RFC 6749 §2.3.1: form-urlencode both parts before Basic encoding.
		req.SetBasicAuth(url.QueryEscape(cfg.ClientID), url.QueryEscape(cfg.ClientSecret))
	}
	resp, err := s.client.Do(req)
	if err != nil {
		s.log.WarnContext(ctx, "sso: token request failed", "error", err)
		return nil, providerErr(err, "the identity provider could not be reached")
	}
	defer resp.Body.Close()
	body, err := readLimited(resp.Body)
	if err != nil {
		return nil, providerErr(err, "the identity provider returned an invalid token response")
	}
	var tr tokenResponse
	jerr := json.Unmarshal(body, &tr)
	if resp.StatusCode != http.StatusOK {
		code := safeOAuthCode(tr.Error)
		s.log.WarnContext(ctx, "sso: token exchange rejected", "status", resp.StatusCode,
			"error", code, "description", truncate(tr.ErrorDescription, 200))
		if code == "" {
			code = fmt.Sprintf("HTTP %d", resp.StatusCode)
		}
		return nil, providerErr(fmt.Errorf("token endpoint: %s", code), "the identity provider rejected the sign-in (%s)", code)
	}
	if jerr != nil {
		return nil, providerErr(jerr, "the identity provider returned an invalid token response")
	}
	if tr.IDToken == "" {
		return nil, providerErr(errors.New("no id_token"), "the identity provider did not return an ID token")
	}
	return &tr, nil
}

// tokenAuthMethod picks the client authentication method for the token
// endpoint. The discovery default is client_secret_basic.
func tokenAuthMethod(supported []string, hasSecret bool) string {
	if !hasSecret {
		return "none"
	}
	if len(supported) == 0 || containsStr(supported, "client_secret_basic") {
		return "client_secret_basic"
	}
	if containsStr(supported, "client_secret_post") {
		return "client_secret_post"
	}
	return "client_secret_basic"
}

// safeOAuthCode keeps an OAuth error code only if it is a plain token.
func safeOAuthCode(s string) string {
	if s == "" || len(s) > 64 {
		return ""
	}
	for _, r := range s {
		if !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '_' || r == '-' || r == '.') {
			return ""
		}
	}
	return s
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}

// fetchUserinfo calls the userinfo endpoint (OpenID Connect Core §5.3).
func (s *Service) fetchUserinfo(ctx context.Context, endpoint, accessToken string) (claims, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Authorization", "Bearer "+accessToken)
	resp, err := s.client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	body, err := readLimited(resp.Body)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("userinfo: HTTP %d", resp.StatusCode)
	}
	if ct := resp.Header.Get("Content-Type"); strings.HasPrefix(ct, "application/jwt") {
		return nil, errors.New("userinfo: signed responses are not supported")
	}
	return decodeClaims(body)
}
