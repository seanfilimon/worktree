package scan

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// protocolDir is crates/worktree-protocol/src relative to this package.
var protocolDir = filepath.Join("..", "..", "..", "..", "crates", "worktree-protocol", "src")

// readProtocolFile reads a file shared with the Rust scanner, skipping the
// test when the repository layout is not available (e.g. a build of the
// server module alone).
func readProtocolFile(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(protocolDir, name))
	if os.IsNotExist(err) {
		t.Skipf("%s not found (server module built outside the repository)", name)
	}
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func TestRulesFileMatchesProtocol(t *testing.T) {
	want := readProtocolFile(t, "scan_rules.json")
	norm := func(b []byte) []byte { return bytes.ReplaceAll(b, []byte("\r\n"), []byte("\n")) }
	if !bytes.Equal(norm(want), norm(rulesJSON)) {
		t.Fatal("internal/scan/rules.json differs from crates/worktree-protocol/src/scan_rules.json; copy the protocol file")
	}
}

func TestRulesLoad(t *testing.T) {
	if len(Rules()) < 15 {
		t.Fatalf("%d rules", len(Rules()))
	}
	if rules.AllowMarker != "wt:allow-secret" || rules.MaxScanLineLen != 8192 {
		t.Fatalf("allow marker %q, line cap %d", rules.AllowMarker, rules.MaxScanLineLen)
	}
	if _, err := loadRules([]byte(`{"rules":[{"id":"x","pattern":"("}]}`)); err == nil {
		t.Fatal("invalid pattern accepted")
	}
	if _, err := loadRules([]byte(`{"rules":[{"id":"x","pattern":"a","validate_tail":"nope"}]}`)); err == nil {
		t.Fatal("unknown validate_tail accepted")
	}
	if _, err := loadRules([]byte(`{`)); err == nil {
		t.Fatal("invalid JSON accepted")
	}
}

// part is one element of a parity case input.
func assemble(t *testing.T, raw []json.RawMessage) []byte {
	t.Helper()
	var out []byte
	for _, p := range raw {
		var s string
		if json.Unmarshal(p, &s) == nil {
			out = append(out, s...)
			continue
		}
		var obj struct {
			Repeat *string `json:"repeat"`
			Count  int     `json:"count"`
			Hex    *string `json:"hex"`
		}
		if err := json.Unmarshal(p, &obj); err != nil {
			t.Fatalf("bad part %s: %v", p, err)
		}
		switch {
		case obj.Repeat != nil:
			out = append(out, strings.Repeat(*obj.Repeat, obj.Count)...)
		case obj.Hex != nil:
			b, err := hex.DecodeString(*obj.Hex)
			if err != nil {
				t.Fatal(err)
			}
			out = append(out, b...)
		default:
			t.Fatalf("bad part %s", p)
		}
	}
	return out
}

type parityCase struct {
	Name  string            `json:"name"`
	Path  string            `json:"path"`
	Parts []json.RawMessage `json:"parts"`
	Want  []struct {
		Rule         string            `json:"rule"`
		Line         int               `json:"line"`
		Excerpt      string            `json:"excerpt"`
		ExcerptParts []json.RawMessage `json:"excerpt_parts"`
		Fingerprint  string            `json:"fingerprint"`
	} `json:"want"`
}

func loadCases(t *testing.T) []parityCase {
	var doc struct {
		Cases []parityCase `json:"cases"`
	}
	if err := json.Unmarshal(readProtocolFile(t, "scan_rule_cases.json"), &doc); err != nil {
		t.Fatal(err)
	}
	if len(doc.Cases) < 10 {
		t.Fatalf("%d cases", len(doc.Cases))
	}
	return doc.Cases
}

// TestParityCases runs the cases the Rust scanner's shared_parity_cases
// test checks: both scanners must report the same findings.
func TestParityCases(t *testing.T) {
	for _, c := range loadCases(t) {
		t.Run(c.Name, func(t *testing.T) {
			got := ScanBytes(c.Path, assemble(t, c.Parts))
			if len(got) != len(c.Want) {
				t.Fatalf("got %d findings %+v, want %d", len(got), got, len(c.Want))
			}
			for i, w := range c.Want {
				g := got[i]
				ex := w.Excerpt
				if w.ExcerptParts != nil {
					ex = string(assemble(t, w.ExcerptParts))
				}
				if g.Rule != w.Rule || g.Line != w.Line || g.Excerpt != ex || g.Path != c.Path || len(g.Fingerprint) != FingerprintHexLen {
					t.Errorf("finding %d = %+v, want rule %s line %d excerpt %q", i, g, w.Rule, w.Line, ex)
				}
				if w.Fingerprint != "" && g.Fingerprint != w.Fingerprint {
					t.Errorf("finding %d fingerprint = %s, want %s", i, g.Fingerprint, w.Fingerprint)
				}
			}
		})
	}
}

func TestSharedFilesTripNoRule(t *testing.T) {
	if m := ScanBytes("rules.json", rulesJSON); len(m) != 0 {
		t.Fatalf("rules.json: %+v", m)
	}
	if m := ScanBytes("cases.json", readProtocolFile(t, "scan_rule_cases.json")); len(m) != 0 {
		t.Fatalf("scan_rule_cases.json: %+v", m)
	}
}

func TestFingerprint(t *testing.T) {
	a := Fingerprint("github-token", []byte("value"))
	if len(a) != FingerprintHexLen || a != Fingerprint("github-token", []byte("value")) {
		t.Fatalf("unstable fingerprint %q", a)
	}
	if a == Fingerprint("jwt", []byte("value")) || a == Fingerprint("github-token", []byte("other")) {
		t.Fatal("fingerprints collide")
	}
}

func TestEngineHelpers(t *testing.T) {
	if redact([]byte("AKIAIOSFODNN7EXAMPLE")) != "AKIA****MPLE" || redact([]byte("short")) != "****" {
		t.Fatal("redact")
	}
	if shannonEntropy(nil) != 0 || shannonEntropy([]byte("aaaa")) != 0 {
		t.Fatal("entropy of constant input")
	}
	if e := shannonEntropy([]byte("abcd")); e < 1.999 || e > 2.001 {
		t.Fatalf("entropy(abcd) = %v", e)
	}
	if !IsBinary([]byte("a\x00b")) || IsBinary([]byte("text")) {
		t.Fatal("IsBinary")
	}
	if lossy([]byte("caf\xe9")) != "caf�" {
		t.Fatalf("lossy = %q", lossy([]byte("caf\xe9")))
	}
	if ScanBytes("empty", nil) != nil {
		t.Fatal("empty input")
	}
}
