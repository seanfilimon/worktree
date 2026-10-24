// Package hash provides a BLAKE3 content-addressable hash type used as
// the key for the worktree object store.
//
// The 32-byte binary form and 64-char lowercase hex form match the Rust
// ContentHash in crates/worktree-protocol/src/core/hash.rs so hashes
// round-trip across the Go server <-> Rust bg boundary without
// re-encoding.
package hash

import (
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"fmt"

	"github.com/zeebo/blake3"
)

// Size is the length of a Hash in bytes.
const Size = 32

// HexSize is the length of a Hash's hex string representation.
const HexSize = 64

// Hash is a 32-byte BLAKE3 content-addressable hash.
type Hash [Size]byte

// Sum returns the BLAKE3 hash of data.
func Sum(data []byte) Hash {
	return Hash(blake3.Sum256(data))
}

// Hex returns the 64-char lowercase hex encoding of h.
func (h Hash) Hex() string {
	return hex.EncodeToString(h[:])
}

// String returns h.Hex(). Implements fmt.Stringer.
func (h Hash) String() string {
	return h.Hex()
}

// IsZero reports whether h is the all-zero hash.
func (h Hash) IsZero() bool {
	var zero Hash
	return h == zero
}

// Equal reports whether h and other are byte-equal, in constant time.
func (h Hash) Equal(other Hash) bool {
	return subtle.ConstantTimeCompare(h[:], other[:]) == 1
}

// MarshalText implements encoding.TextMarshaler.
func (h Hash) MarshalText() ([]byte, error) {
	out := make([]byte, HexSize)
	hex.Encode(out, h[:])
	return out, nil
}

// UnmarshalText implements encoding.TextUnmarshaler.
func (h *Hash) UnmarshalText(text []byte) error {
	parsed, err := FromHex(string(text))
	if err != nil {
		return err
	}
	*h = parsed
	return nil
}

// FromHex parses a 64-char lowercase hex string into a Hash.
//
// Uppercase hex is rejected (returns ErrInvalidHex) to match the
// lowercase-only contract of the Rust ContentHash::to_hex output.
func FromHex(s string) (Hash, error) {
	if len(s) != HexSize {
		return Hash{}, fmt.Errorf("%w: expected %d hex chars, got %d", ErrInvalidHex, HexSize, len(s))
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
			return Hash{}, fmt.Errorf("%w: byte %d (%q) is not lowercase hex", ErrInvalidHex, i, c)
		}
	}
	var h Hash
	if _, err := hex.Decode(h[:], []byte(s)); err != nil {
		return Hash{}, fmt.Errorf("%w: %v", ErrInvalidHex, err)
	}
	return h, nil
}

// ErrInvalidHex is returned by FromHex (and UnmarshalText) when the
// input is not a 64-char lowercase hex string.
var ErrInvalidHex = errors.New("hash: invalid hex encoding")
