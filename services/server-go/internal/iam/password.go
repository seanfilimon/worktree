package iam

import (
	"crypto/rand"
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"fmt"
	"strconv"
	"strings"

	"golang.org/x/crypto/argon2"
)

// PasswordParams are the argon2id cost parameters used for new hashes.
// Zero fields take the defaults from DefaultPasswordParams.
type PasswordParams struct {
	Memory  uint32 // KiB
	Time    uint32 // iterations
	Threads uint8  // parallelism
	SaltLen uint32 // bytes
	KeyLen  uint32 // bytes
}

// DefaultPasswordParams follows RFC 9106 §4 (second recommended option):
// 64 MiB, 3 passes, 4 lanes, 128-bit salt, 256-bit key.
var DefaultPasswordParams = PasswordParams{
	Memory:  64 * 1024,
	Time:    3,
	Threads: 4,
	SaltLen: 16,
	KeyLen:  32,
}

// Bounds applied when decoding stored hashes, so a corrupted or malicious
// hash cannot make verification consume unbounded resources.
const (
	maxHashMemory  = 1 << 20 // 1 GiB in KiB
	maxHashTime    = 64
	minSaltLen     = 8
	maxSaltLen     = 64
	minKeyLen      = 16
	maxKeyLen      = 128
	argon2idPrefix = "$argon2id$"
)

// ErrMalformedHash is returned by VerifyPassword for strings that are not
// valid argon2id PHC hashes.
var ErrMalformedHash = errors.New("iam: malformed password hash")

func (p PasswordParams) withDefaults() PasswordParams {
	d := DefaultPasswordParams
	if p.Memory != 0 {
		d.Memory = p.Memory
	}
	if p.Time != 0 {
		d.Time = p.Time
	}
	if p.Threads != 0 {
		d.Threads = p.Threads
	}
	if p.SaltLen != 0 {
		d.SaltLen = p.SaltLen
	}
	if p.KeyLen != 0 {
		d.KeyLen = p.KeyLen
	}
	// argon2 requires at least 8 KiB per lane.
	if minMem := 8 * uint32(d.Threads); d.Memory < minMem {
		d.Memory = minMem
	}
	return d
}

func (p PasswordParams) validate() error {
	switch {
	case p.Memory < 8*uint32(p.Threads) || p.Memory > maxHashMemory:
		return fmt.Errorf("%w: memory out of range", ErrMalformedHash)
	case p.Time < 1 || p.Time > maxHashTime:
		return fmt.Errorf("%w: time out of range", ErrMalformedHash)
	case p.Threads < 1:
		return fmt.Errorf("%w: parallelism out of range", ErrMalformedHash)
	case p.SaltLen < minSaltLen || p.SaltLen > maxSaltLen:
		return fmt.Errorf("%w: salt length out of range", ErrMalformedHash)
	case p.KeyLen < minKeyLen || p.KeyLen > maxKeyLen:
		return fmt.Errorf("%w: key length out of range", ErrMalformedHash)
	}
	return nil
}

// HashPassword derives an argon2id hash of password with a fresh random
// salt and returns it in PHC string format. It does not validate the
// password policy; see ValidatePassword.
func HashPassword(password string, params PasswordParams) (string, error) {
	p := params.withDefaults()
	if err := p.validate(); err != nil {
		return "", fmt.Errorf("iam: invalid password params: %w", err)
	}
	salt := make([]byte, p.SaltLen)
	if _, err := rand.Read(salt); err != nil {
		return "", fmt.Errorf("iam: generating salt: %w", err)
	}
	key := argon2.IDKey([]byte(password), salt, p.Time, p.Memory, p.Threads, p.KeyLen)
	return encodePHC(p, salt, key), nil
}

func encodePHC(p PasswordParams, salt, key []byte) string {
	return fmt.Sprintf("$argon2id$v=%d$m=%d,t=%d,p=%d$%s$%s",
		argon2.Version, p.Memory, p.Time, p.Threads,
		base64.RawStdEncoding.EncodeToString(salt),
		base64.RawStdEncoding.EncodeToString(key))
}

// decodePHC parses an argon2id PHC string.
func decodePHC(encoded string) (PasswordParams, []byte, []byte, error) {
	var p PasswordParams
	if !strings.HasPrefix(encoded, argon2idPrefix) {
		return p, nil, nil, ErrMalformedHash
	}
	parts := strings.Split(encoded, "$")
	// "", "argon2id", "v=19", "m=..,t=..,p=..", salt, key
	if len(parts) != 6 {
		return p, nil, nil, ErrMalformedHash
	}
	if parts[2] != "v="+strconv.Itoa(argon2.Version) {
		return p, nil, nil, fmt.Errorf("%w: unsupported version", ErrMalformedHash)
	}
	fields := strings.Split(parts[3], ",")
	if len(fields) != 3 {
		return p, nil, nil, ErrMalformedHash
	}
	for i, want := range []string{"m", "t", "p"} {
		k, v, ok := strings.Cut(fields[i], "=")
		if !ok || k != want {
			return p, nil, nil, ErrMalformedHash
		}
		bits := 32
		if want == "p" {
			bits = 8
		}
		n, err := strconv.ParseUint(v, 10, bits)
		if err != nil {
			return p, nil, nil, ErrMalformedHash
		}
		switch want {
		case "m":
			p.Memory = uint32(n)
		case "t":
			p.Time = uint32(n)
		case "p":
			p.Threads = uint8(n)
		}
	}
	salt, err := base64.RawStdEncoding.Strict().DecodeString(parts[4])
	if err != nil {
		return p, nil, nil, ErrMalformedHash
	}
	key, err := base64.RawStdEncoding.Strict().DecodeString(parts[5])
	if err != nil {
		return p, nil, nil, ErrMalformedHash
	}
	p.SaltLen = uint32(len(salt))
	p.KeyLen = uint32(len(key))
	if err := p.validate(); err != nil {
		return p, nil, nil, err
	}
	return p, salt, key, nil
}

// VerifyPassword reports whether password matches the PHC-encoded argon2id
// hash. The derived key is compared in constant time. A malformed hash
// returns an error wrapping ErrMalformedHash.
func VerifyPassword(password, encoded string) (bool, error) {
	p, salt, key, err := decodePHC(encoded)
	if err != nil {
		return false, err
	}
	got := argon2.IDKey([]byte(password), salt, p.Time, p.Memory, p.Threads, p.KeyLen)
	return subtle.ConstantTimeCompare(got, key) == 1, nil
}

// needsRehash reports whether encoded was produced with parameters other
// than want (so it should be upgraded after a successful verification).
func needsRehash(encoded string, want PasswordParams) bool {
	p, _, _, err := decodePHC(encoded)
	if err != nil {
		return true
	}
	return p != want
}
