package hash

import (
	"encoding/json"
	"errors"
	"strings"
	"testing"
)

// helloWorldHex is the BLAKE3 hash of []byte("hello world"), the
// canonical test vector for cross-language parity with the Rust
// ContentHash. If this constant changes, something is wrong with
// either the BLAKE3 implementation or the byte layout.
const helloWorldHex = "d74981efa70a0c880b8d8c1985d075dbcbf679b99a5f9914e5aaf96b831a9e24"

// emptyHex is the BLAKE3 hash of an empty input. Documented constant
// from the BLAKE3 reference.
const emptyHex = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"

func TestSum_KnownVectorHelloWorld(t *testing.T) {
	got := Sum([]byte("hello world")).Hex()
	if got != helloWorldHex {
		t.Fatalf("Sum(\"hello world\").Hex() = %q, want %q", got, helloWorldHex)
	}
}

func TestSum_KnownVectorEmpty(t *testing.T) {
	got := Sum(nil).Hex()
	if got != emptyHex {
		t.Fatalf("Sum(nil).Hex() = %q, want %q", got, emptyHex)
	}
	got2 := Sum([]byte{}).Hex()
	if got2 != emptyHex {
		t.Fatalf("Sum([]byte{}).Hex() = %q, want %q", got2, emptyHex)
	}
}

func TestSum_Deterministic(t *testing.T) {
	a := Sum([]byte("the quick brown fox"))
	b := Sum([]byte("the quick brown fox"))
	if a != b {
		t.Fatalf("Sum is not deterministic: %s != %s", a, b)
	}
}

func TestHex_LengthAndCase(t *testing.T) {
	h := Sum([]byte("anything"))
	s := h.Hex()
	if len(s) != HexSize {
		t.Fatalf("Hex() length = %d, want %d", len(s), HexSize)
	}
	if strings.ToLower(s) != s {
		t.Fatalf("Hex() is not all-lowercase: %q", s)
	}
}

func TestString_AliasesHex(t *testing.T) {
	h := Sum([]byte("xyz"))
	if h.String() != h.Hex() {
		t.Fatalf("String() %q != Hex() %q", h.String(), h.Hex())
	}
}

func TestFromHex_Roundtrip(t *testing.T) {
	original := Sum([]byte("roundtrip"))
	parsed, err := FromHex(original.Hex())
	if err != nil {
		t.Fatalf("FromHex(%q) returned err = %v", original.Hex(), err)
	}
	if parsed != original {
		t.Fatalf("FromHex roundtrip mismatch: %s != %s", parsed, original)
	}
}

func TestFromHex_EmptyStringRejected(t *testing.T) {
	_, err := FromHex("")
	if !errors.Is(err, ErrInvalidHex) {
		t.Fatalf("FromHex(\"\") err = %v, want errors.Is ErrInvalidHex", err)
	}
}

func TestFromHex_WrongLengthRejected(t *testing.T) {
	_, err := FromHex("abcd")
	if !errors.Is(err, ErrInvalidHex) {
		t.Fatalf("FromHex(\"abcd\") err = %v, want errors.Is ErrInvalidHex", err)
	}
}

func TestFromHex_UppercaseRejected(t *testing.T) {
	upper := strings.ToUpper(helloWorldHex)
	_, err := FromHex(upper)
	if !errors.Is(err, ErrInvalidHex) {
		t.Fatalf("FromHex(uppercase) err = %v, want errors.Is ErrInvalidHex", err)
	}
}

func TestFromHex_NonHexByteRejected(t *testing.T) {
	bad := strings.Repeat("z", HexSize)
	_, err := FromHex(bad)
	if !errors.Is(err, ErrInvalidHex) {
		t.Fatalf("FromHex(all-z) err = %v, want errors.Is ErrInvalidHex", err)
	}
}

func TestMarshalText_UnmarshalText_JSONRoundtrip(t *testing.T) {
	type wrapper struct {
		ID Hash `json:"id"`
	}
	original := wrapper{ID: Sum([]byte("json"))}
	encoded, err := json.Marshal(original)
	if err != nil {
		t.Fatalf("json.Marshal err = %v", err)
	}
	var decoded wrapper
	if err := json.Unmarshal(encoded, &decoded); err != nil {
		t.Fatalf("json.Unmarshal err = %v", err)
	}
	if decoded.ID != original.ID {
		t.Fatalf("JSON roundtrip mismatch: %s != %s", decoded.ID, original.ID)
	}
	// And the encoded JSON should embed the lowercase hex form directly.
	want := `{"id":"` + original.ID.Hex() + `"}`
	if string(encoded) != want {
		t.Fatalf("encoded JSON = %s, want %s", encoded, want)
	}
}

func TestUnmarshalText_InvalidHexRejected(t *testing.T) {
	var h Hash
	if err := h.UnmarshalText([]byte("nope")); !errors.Is(err, ErrInvalidHex) {
		t.Fatalf("UnmarshalText err = %v, want errors.Is ErrInvalidHex", err)
	}
}

func TestIsZero(t *testing.T) {
	var zero Hash
	if !zero.IsZero() {
		t.Fatal("zero Hash should report IsZero() == true")
	}
	if Sum(nil).IsZero() {
		t.Fatal("Sum(nil) should not be the zero Hash (BLAKE3 of empty is not all-zero)")
	}
}

func TestEqual(t *testing.T) {
	a := Sum([]byte("a"))
	b := Sum([]byte("a"))
	c := Sum([]byte("b"))
	if !a.Equal(b) {
		t.Fatal("Equal: identical hashes should compare equal")
	}
	if a.Equal(c) {
		t.Fatal("Equal: different hashes should not compare equal")
	}
}

func TestSize_Constant(t *testing.T) {
	if Size != 32 {
		t.Fatalf("Size = %d, want 32 (BLAKE3 output size)", Size)
	}
	if HexSize != 2*Size {
		t.Fatalf("HexSize = %d, want %d (= 2*Size)", HexSize, 2*Size)
	}
}
