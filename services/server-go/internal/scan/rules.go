package scan

import (
	"bytes"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"regexp"
	"sort"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/zeebo/blake3"
)

// The secret scanning engine. It is a port of the client scanner
// (crates/worktree-sdk/src/scan/secrets.rs) driven by the same rule file:
// rules.json is a byte-identical copy of
// crates/worktree-protocol/src/scan_rules.json (TestRulesFileMatchesProtocol)
// and both scanners reproduce scan_rule_cases.json exactly
// (TestParityCases). Patterns use the common subset of Rust regex and RE2.
//
// Known divergences, all outside realistic secrets: Go matches negated
// classes and counted repetitions per UTF-8 rune where Rust (Unicode off)
// counts bytes, `\s` excludes \v in Go, and a truncated multi-byte sequence
// becomes one U+FFFD per byte in Go excerpts.

//go:embed rules.json
var rulesJSON []byte

// FingerprintPrefix and FingerprintHexLen define finding fingerprints:
// the first FingerprintHexLen hex digits of BLAKE3(prefix NUL rule NUL
// value) (worktree_protocol::scan_rules::fingerprint).
const (
	FingerprintPrefix = "wt-secret-v1"
	FingerprintHexLen = 24
)

const (
	maxExcerptLen       = 200
	excerptContext      = 60
	minPartialRedactLen = 12
	mask                = "****"
	binarySniffLen      = 8000
)

// RuleSet is the decoded rule file.
type RuleSet struct {
	Version            int      `json:"version"`
	AllowMarker        string   `json:"allow_marker"`
	MaxScanLineLen     int      `json:"max_scan_line_len"`
	PlaceholderMarkers []string `json:"placeholder_markers"`
	Rules              []Rule   `json:"rules"`
}

// Rule is one secret detection rule.
type Rule struct {
	ID                  string   `json:"id"`
	Description         string   `json:"description"`
	Pattern             string   `json:"pattern"`
	Keywords            []string `json:"keywords"`
	MinRun              int      `json:"min_run"`
	MinEntropy          float64  `json:"min_entropy"`
	NeedsLetterAndDigit bool     `json:"needs_letter_and_digit"`
	SkipPlaceholders    bool     `json:"skip_placeholders"`
	MaskRestOfLine      bool     `json:"mask_rest_of_line"`
	ValidateTail        string   `json:"validate_tail"`

	re       *regexp.Regexp
	keywords [][]byte
}

// Match is one secret found in a file. Excerpt is the line with every
// secret on it redacted; the secret itself is never kept.
type Match struct {
	Rule        string `json:"rule"`
	Path        string `json:"path"`
	Line        int    `json:"line"` // 1-based
	Excerpt     string `json:"excerpt"`
	Fingerprint string `json:"fingerprint"`
}

var rules = mustLoadRules(rulesJSON)

func mustLoadRules(data []byte) *RuleSet {
	set, err := loadRules(data)
	if err != nil {
		panic("scan: " + err.Error())
	}
	return set
}

func loadRules(data []byte) (*RuleSet, error) {
	var set RuleSet
	if err := json.Unmarshal(data, &set); err != nil {
		return nil, fmt.Errorf("rules.json: %w", err)
	}
	for i := range set.Rules {
		r := &set.Rules[i]
		re, err := regexp.Compile("(?m)" + r.Pattern)
		if err != nil {
			return nil, fmt.Errorf("rule %s: %w", r.ID, err)
		}
		r.re = re
		switch r.ValidateTail {
		case "", "key_body":
		default:
			return nil, fmt.Errorf("rule %s: unknown validate_tail %q", r.ID, r.ValidateTail)
		}
		for _, k := range r.Keywords {
			r.keywords = append(r.keywords, []byte(strings.ToLower(k)))
		}
	}
	return &set, nil
}

// Rules returns the built-in rules in priority order.
func Rules() []Rule { return rules.Rules }

// Fingerprint returns the fingerprint of a secret value found by rule.
func Fingerprint(rule string, value []byte) string {
	h := blake3.New()
	_, _ = h.Write([]byte(FingerprintPrefix))
	_, _ = h.Write([]byte{0})
	_, _ = h.Write([]byte(rule))
	_, _ = h.Write([]byte{0})
	_, _ = h.Write(value)
	return hex.EncodeToString(h.Sum(nil))[:FingerprintHexLen]
}

// IsBinary reports whether data has a NUL byte in its first 8000 bytes.
func IsBinary(data []byte) bool {
	return bytes.IndexByte(data[:min(len(data), binarySniffLen)], 0) >= 0
}

type candidate struct {
	rule        int
	start, end  int
	fingerprint string
}

// ScanBytes scans one file's content with the built-in rules (binary
// content yields nothing). Matches are in file order.
func ScanBytes(path string, data []byte) []Match {
	return rules.scan(path, data)
}

func (set *RuleSet) scan(path string, data []byte) []Match {
	if len(data) == 0 || IsBinary(data) {
		return nil
	}
	var lower []byte
	var cands []candidate
	lineCache := map[string][][2]int{}
	for idx := range set.Rules {
		r := &set.Rules[idx]
		var locs [][]int
		if len(r.keywords) == 0 {
			locs = r.re.FindAllSubmatchIndex(data, -1)
		} else {
			// Only lines containing a keyword can match (every match holds
			// one, and no match crosses a line break), and Go's regexp is
			// much faster on short inputs: run the rule line by line.
			if lower == nil {
				lower = asciiLower(data)
			}
			key := strings.Join(r.Keywords, "\x00")
			lines, ok := lineCache[key]
			if !ok {
				lines = keywordLines(lower, r.keywords)
				lineCache[key] = lines
			}
			for _, ln := range lines {
				if r.MinRun > 0 && !hasRun(data[ln[0]:ln[1]], r.MinRun) {
					continue
				}
				for _, loc := range r.re.FindAllSubmatchIndex(data[ln[0]:ln[1]], -1) {
					for k := range loc {
						if loc[k] >= 0 {
							loc[k] += ln[0]
						}
					}
					locs = append(locs, loc)
				}
			}
		}
		for _, loc := range locs {
			start, end := loc[0], loc[1]
			for g := 1; 2*g+1 < len(loc); g++ {
				if loc[2*g] >= 0 {
					start, end = loc[2*g], loc[2*g+1]
					break
				}
			}
			value := data[start:end]
			if !set.accepts(r, value) {
				continue
			}
			fp := ""
			if r.ValidateTail == "key_body" {
				body, ok := keyBody(data[loc[1]:])
				if !ok {
					continue
				}
				v := append(append(append([]byte(nil), value...), '\n'), body...)
				fp = Fingerprint(r.ID, v)
			} else {
				fp = Fingerprint(r.ID, value)
			}
			cands = append(cands, candidate{rule: idx, start: start, end: end, fingerprint: fp})
		}
	}
	if len(cands) == 0 {
		return nil
	}
	sort.SliceStable(cands, func(i, j int) bool {
		if cands[i].start != cands[j].start {
			return cands[i].start < cands[j].start
		}
		return cands[i].rule < cands[j].rule
	})

	var out []Match
	lineNo, countedTo := 1, 0
	marker := []byte(set.AllowMarker)
	for i := 0; i < len(cands); {
		start := cands[i].start
		lineNo += bytes.Count(data[countedTo:start], []byte{'\n'})
		countedTo = start
		lineStart := bytes.LastIndexByte(data[:start], '\n') + 1
		lineEnd := len(data)
		if p := bytes.IndexByte(data[start:], '\n'); p >= 0 {
			lineEnd = start + p
		}
		groupEnd := len(cands)
		for j := i; j < len(cands); j++ {
			if cands[j].start >= lineEnd {
				groupEnd = j
				break
			}
		}
		line := data[lineStart:lineEnd]
		if !bytes.Contains(line, marker) {
			var local []candidate
			for _, c := range cands[i:groupEnd] {
				if c.end <= lineEnd && c.end-lineStart <= set.MaxScanLineLen {
					local = append(local, candidate{rule: c.rule, start: c.start - lineStart, end: c.end - lineStart, fingerprint: c.fingerprint})
				}
			}
			out = set.emitLine(path, lineNo, line, local, out)
		}
		i = groupEnd
	}
	return out
}

// emitLine deduplicates one line's candidates (earlier rules win overlaps)
// and appends matches sharing one redacted excerpt.
func (set *RuleSet) emitLine(path string, lineNo int, line []byte, local []candidate, out []Match) []Match {
	if len(local) == 0 {
		return out
	}
	sort.SliceStable(local, func(i, j int) bool {
		if local[i].rule != local[j].rule {
			return local[i].rule < local[j].rule
		}
		return local[i].start < local[j].start
	})
	accepted := make([]candidate, 0, len(local))
	for _, c := range local {
		ok := true
		for _, a := range accepted {
			if !(c.end <= a.start || c.start >= a.end) {
				ok = false
				break
			}
		}
		if ok {
			accepted = append(accepted, c)
		}
	}
	sort.SliceStable(accepted, func(i, j int) bool { return accepted[i].start < accepted[j].start })
	redacted, positions := set.redactLine(line, accepted)
	for k, c := range accepted {
		out = append(out, Match{
			Rule:        set.Rules[c.rule].ID,
			Path:        path,
			Line:        lineNo,
			Excerpt:     excerpt(redacted, positions[k]),
			Fingerprint: c.fingerprint,
		})
	}
	return out
}

// redactLine renders line with every span redacted and returns, per span,
// its byte offset in the result (-1 when swallowed by a masked tail).
func (set *RuleSet) redactLine(line []byte, spans []candidate) (string, []int) {
	line = bytes.TrimSuffix(line, []byte{'\r'})
	var b strings.Builder
	positions := make([]int, 0, len(spans))
	cursor := 0
	for _, sp := range spans {
		if sp.start < cursor {
			positions = append(positions, -1)
			continue
		}
		end := min(sp.end, len(line))
		b.WriteString(lossy(line[cursor:sp.start]))
		positions = append(positions, b.Len())
		value := line[sp.start:end]
		if set.Rules[sp.rule].MaskRestOfLine {
			b.WriteString(lossy(value))
			if bytes.IndexFunc(line[end:], func(r rune) bool { return !isASCIISpace(r) }) >= 0 {
				b.WriteString(mask)
			}
			cursor = len(line)
		} else {
			b.WriteString(redact(value))
			cursor = end
		}
	}
	if cursor < len(line) {
		b.WriteString(lossy(line[cursor:]))
	}
	return b.String(), positions
}

// isASCIISpace matches Rust's u8::is_ascii_whitespace (space, \t, \n, \f,
// \r). Non-ASCII runes count as content.
func isASCIISpace(r rune) bool {
	switch r {
	case ' ', '\t', '\n', '\f', '\r':
		return true
	}
	return false
}

// redact keeps the first and last four characters of a secret.
func redact(value []byte) string {
	chars := []rune(lossy(value))
	if len(chars) < minPartialRedactLen {
		return mask
	}
	return string(chars[:4]) + mask + string(chars[len(chars)-4:])
}

// excerpt trims the redacted line and windows long lines around pos.
func excerpt(redacted string, pos int) string {
	trimmed := strings.TrimSpace(redacted)
	if len(trimmed) <= maxExcerptLen {
		return trimmed
	}
	lead := len(redacted) - len(strings.TrimLeftFunc(redacted, unicode.IsSpace))
	p := 0
	if pos >= 0 {
		p = max(pos-lead, 0)
	}
	p = min(p, len(trimmed))
	from := floorCharBoundary(trimmed, max(p-excerptContext, 0))
	to := floorCharBoundary(trimmed, min(p+excerptContext*2, len(trimmed)))
	var b strings.Builder
	if from > 0 {
		b.WriteString("…")
	}
	b.WriteString(strings.TrimSpace(trimmed[from:to]))
	if to < len(trimmed) {
		b.WriteString("…")
	}
	return b.String()
}

func floorCharBoundary(s string, i int) int {
	for i > 0 && i < len(s) && !utf8.RuneStart(s[i]) {
		i--
	}
	return i
}

// lossy converts bytes to a string, replacing each invalid byte with
// U+FFFD (Rust's String::from_utf8_lossy for single invalid bytes).
func lossy(b []byte) string {
	if utf8.Valid(b) {
		return string(b)
	}
	var sb strings.Builder
	for len(b) > 0 {
		r, n := utf8.DecodeRune(b)
		if r == utf8.RuneError && n == 1 {
			sb.WriteRune(utf8.RuneError)
		} else {
			sb.Write(b[:n])
		}
		b = b[n:]
	}
	return sb.String()
}

func (set *RuleSet) accepts(r *Rule, value []byte) bool {
	if r.SkipPlaceholders && set.isPlaceholder(value) {
		return false
	}
	if r.NeedsLetterAndDigit && !(hasDigit(value) && hasLetter(value)) {
		return false
	}
	return r.MinEntropy <= 0 || shannonEntropy(value) >= r.MinEntropy
}

func (set *RuleSet) isPlaceholder(value []byte) bool {
	lower := asciiLower(value)
	for _, m := range set.PlaceholderMarkers {
		if bytes.Contains(lower, []byte(m)) {
			return true
		}
	}
	return false
}

func hasDigit(b []byte) bool {
	for _, c := range b {
		if c >= '0' && c <= '9' {
			return true
		}
	}
	return false
}

func hasLetter(b []byte) bool {
	for _, c := range b {
		if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') {
			return true
		}
	}
	return false
}

// shannonEntropy is the Shannon entropy of value in bits per byte.
func shannonEntropy(value []byte) float64 {
	if len(value) == 0 {
		return 0
	}
	var counts [256]int
	for _, c := range value {
		counts[c]++
	}
	n := float64(len(value))
	var e float64
	for _, c := range counts {
		if c > 0 {
			p := float64(c) / n
			e -= p * math.Log2(p)
		}
	}
	return e
}

func asciiLower(b []byte) []byte {
	out := make([]byte, len(b))
	for i, c := range b {
		if c >= 'A' && c <= 'Z' {
			c += 'a' - 'A'
		}
		out[i] = c
	}
	return out
}

// hasRun reports whether line holds n consecutive bytes other than space,
// \t, \r, \n and \f (Rule.MinRun).
func hasRun(line []byte, n int) bool {
	run := 0
	for _, c := range line {
		switch c {
		case ' ', '\t', '\r', '\n', '\f':
			run = 0
		default:
			run++
			if run >= n {
				return true
			}
		}
	}
	return false
}

// keywordLines returns the lines of lower (as [start, end) including the
// line break) that contain one of the keywords, in order.
func keywordLines(lower []byte, keywords [][]byte) [][2]int {
	starts := map[int]int{}
	for _, kw := range keywords {
		for pos := 0; pos < len(lower); {
			i := bytes.Index(lower[pos:], kw)
			if i < 0 {
				break
			}
			i += pos
			ls := bytes.LastIndexByte(lower[:i], '\n') + 1
			le := len(lower)
			if j := bytes.IndexByte(lower[i:], '\n'); j >= 0 {
				le = i + j + 1
			}
			starts[ls] = le
			pos = le
		}
	}
	out := make([][2]int, 0, len(starts))
	for s, e := range starts {
		out = append(out, [2]int{s, e})
	}
	sort.Slice(out, func(i, j int) bool { return out[i][0] < out[j][0] })
	return out
}

// keyBody checks that a PEM / PGP header is followed by key material — on
// the next lines or after an escaped \n — and returns its first line.
func keyBody(tail []byte) ([]byte, bool) {
	rest := tail
	for i := 0; i < 4; i++ {
		var ok bool
		if rest, ok = stripLineBreak(rest); !ok {
			return nil, false
		}
		n := nextLineBreak(rest)
		line := trimQuotes(rest[:n])
		if len(line) == 0 || bytes.IndexByte(line, ':') >= 0 {
			rest = rest[n:]
			continue
		}
		if len(line) < 16 {
			return nil, false
		}
		for _, c := range line {
			if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '+' || c == '/' || c == '=') {
				return nil, false
			}
		}
		return line, true
	}
	return nil, false
}

func stripLineBreak(s []byte) ([]byte, bool) {
	s = bytes.TrimPrefix(s, []byte("\r"))
	if r, ok := bytes.CutPrefix(s, []byte("\n")); ok {
		return r, true
	}
	s = bytes.TrimPrefix(s, []byte(`\r`))
	return bytes.CutPrefix(s, []byte(`\n`))
}

func nextLineBreak(s []byte) int {
	for i := 0; i < len(s); i++ {
		switch s[i] {
		case '\n', '\r':
			return i
		case '\\':
			if i+1 < len(s) && (s[i+1] == 'n' || s[i+1] == 'r') {
				return i
			}
		}
	}
	return len(s)
}

func trimQuotes(s []byte) []byte {
	isTrim := func(c byte) bool {
		switch c {
		case ' ', '\t', '\n', '\f', '\r', '"', '\'', ',', '+':
			return true
		}
		return false
	}
	start := 0
	for start < len(s) && isTrim(s[start]) {
		start++
	}
	end := len(s)
	for end > start && isTrim(s[end-1]) {
		end--
	}
	return s[start:end]
}
