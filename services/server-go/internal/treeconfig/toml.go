package treeconfig

// A small TOML 1.0 parser that keeps the position of every key, value and
// table header, so that validation errors can be reported as
// file:line:col. The Go server has no TOML dependency (go.mod); the
// .wt-tree files are small, so a straightforward recursive-descent parser
// is enough. Columns count characters (not bytes) from 1, like the Rust
// validator (crates/worktree-protocol/src/config/as_code.rs).

import (
	"fmt"
	"math"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"
)

// Pos is a 1-based line and column (in characters).
type Pos struct {
	Line int `json:"line"`
	Col  int `json:"col"`
}

// Kind is the type of a TOML value.
type Kind int

const (
	KindString Kind = iota
	KindInteger
	KindFloat
	KindBool
	KindDatetime
	KindArray
	KindTable
)

// typeName is the article + name used in messages ("a string"); it matches
// the Rust validator's wording.
func (k Kind) typeName() string {
	switch k {
	case KindString:
		return "a string"
	case KindInteger:
		return "an integer"
	case KindFloat:
		return "a float"
	case KindBool:
		return "a boolean"
	case KindDatetime:
		return "a date-time"
	case KindArray:
		return "an array"
	default:
		return "a table"
	}
}

// Value is a parsed TOML value. Pos is where the value starts; KeyPos is
// where the key that defined it starts (for tables created by a header:
// the '[' of the header; for array elements: the element itself).
type Value struct {
	Kind   Kind
	Pos    Pos
	KeyPos Pos
	Str    string // strings; datetimes (as written)
	Int    int64
	Float  float64
	Bool   bool
	Arr    []*Value
	Tbl    *Table

	// tableArray marks arrays created by [[header]] (only those can be
	// extended by further [[header]] lines).
	tableArray bool
}

// Table is a TOML table with keys in definition order.
type Table struct {
	Keys []string
	M    map[string]*Value
	Pos  Pos // header '[' / inline '{' / first dotted key

	defined  bool // defined by a [header] (or is the root)
	dotted   bool // created by a dotted key
	inline   bool // inline tables are sealed
	implicit bool // created as a parent of a header ([a.b] creates a)
}

func newTable(pos Pos) *Table { return &Table{M: map[string]*Value{}, Pos: pos} }

// Get returns the value of key (nil when absent).
func (t *Table) Get(key string) *Value {
	if t == nil {
		return nil
	}
	return t.M[key]
}

func (t *Table) set(key string, v *Value) {
	if _, ok := t.M[key]; !ok {
		t.Keys = append(t.Keys, key)
	}
	t.M[key] = v
}

// SyntaxError is a TOML syntax error at Pos.
type SyntaxError struct {
	Pos Pos
	Msg string
}

func (e *SyntaxError) Error() string { return fmt.Sprintf("%d:%d: %s", e.Pos.Line, e.Pos.Col, e.Msg) }

// ParseTOML parses src. Errors are *SyntaxError.
func ParseTOML(src []byte) (*Table, error) {
	if !utf8.Valid(src) {
		return nil, &SyntaxError{Pos: Pos{1, 1}, Msg: "the file is not valid UTF-8"}
	}
	p := &parser{src: string(src), line: 1, lineStart: 0}
	root := newTable(Pos{1, 1})
	root.defined = true
	p.root, p.cur = root, root
	if err := p.document(); err != nil {
		return nil, err
	}
	return root, nil
}

type parser struct {
	src       string
	i         int
	line      int
	lineStart int // byte offset of the current line
	root, cur *Table
}

func (p *parser) pos() Pos { return p.posAt(p.i) }

// posAt computes the position of byte offset off on the current line (or
// later on it).
func (p *parser) posAt(off int) Pos {
	return Pos{Line: p.line, Col: utf8.RuneCountInString(p.src[p.lineStart:off]) + 1}
}

func (p *parser) errAt(pos Pos, format string, args ...any) error {
	return &SyntaxError{Pos: pos, Msg: fmt.Sprintf(format, args...)}
}

func (p *parser) errf(format string, args ...any) error { return p.errAt(p.pos(), format, args...) }

func (p *parser) eof() bool { return p.i >= len(p.src) }

func (p *parser) peek() byte {
	if p.eof() {
		return 0
	}
	return p.src[p.i]
}

func (p *parser) at(s string) bool { return strings.HasPrefix(p.src[p.i:], s) }

// newline consumes "\n" or "\r\n" and advances the line counter.
func (p *parser) newline() bool {
	switch {
	case p.at("\r\n"):
		p.i += 2
	case p.at("\n"):
		p.i++
	default:
		return false
	}
	p.line++
	p.lineStart = p.i
	return true
}

func (p *parser) skipWS() {
	for !p.eof() && (p.src[p.i] == ' ' || p.src[p.i] == '\t') {
		p.i++
	}
}

// skipComment consumes a comment up to (not including) the newline.
func (p *parser) skipComment() error {
	if p.peek() != '#' {
		return nil
	}
	for !p.eof() && p.src[p.i] != '\n' {
		c := p.src[p.i]
		if c == '\r' && p.at("\r\n") {
			break
		}
		if (c < 0x20 && c != '\t') || c == 0x7f {
			return p.errf("control character in comment")
		}
		p.i++
	}
	return nil
}

// endOfLine requires optional whitespace and comment, then a newline or EOF.
func (p *parser) endOfLine() error {
	p.skipWS()
	if err := p.skipComment(); err != nil {
		return err
	}
	if p.eof() || p.newline() {
		return nil
	}
	return p.errf("expected a newline or end of file, found %s", p.describe())
}

func (p *parser) describe() string {
	if p.eof() {
		return "end of file"
	}
	r, _ := utf8.DecodeRuneInString(p.src[p.i:])
	return strconv.QuoteRune(r)
}

func (p *parser) document() error {
	for {
		p.skipWS()
		if p.eof() {
			return nil
		}
		switch c := p.peek(); {
		case c == '#':
			if err := p.endOfLine(); err != nil {
				return err
			}
		case c == '\n' || c == '\r':
			if !p.newline() {
				return p.errf("unexpected carriage return")
			}
		case c == '[':
			if err := p.header(); err != nil {
				return err
			}
		default:
			if err := p.keyval(p.cur); err != nil {
				return err
			}
			if err := p.endOfLine(); err != nil {
				return err
			}
		}
	}
}

type keyPart struct {
	name string
	pos  Pos
}

func (p *parser) key() ([]keyPart, error) {
	var parts []keyPart
	for {
		p.skipWS()
		pos := p.pos()
		var name string
		switch c := p.peek(); {
		case c == '"':
			s, err := p.basicString()
			if err != nil {
				return nil, err
			}
			name = s
		case c == '\'':
			s, err := p.literalString()
			if err != nil {
				return nil, err
			}
			name = s
		case isBare(c):
			start := p.i
			for !p.eof() && isBare(p.src[p.i]) {
				p.i++
			}
			name = p.src[start:p.i]
		default:
			return nil, p.errf("expected a key, found %s", p.describe())
		}
		parts = append(parts, keyPart{name: name, pos: pos})
		p.skipWS()
		if p.peek() != '.' {
			return parts, nil
		}
		p.i++
	}
}

func isBare(c byte) bool {
	return c >= 'A' && c <= 'Z' || c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '_' || c == '-'
}

func joinKey(parts []keyPart) string {
	names := make([]string, len(parts))
	for i, k := range parts {
		names[i] = k.name
	}
	return strings.Join(names, ".")
}

// header parses [a.b] or [[a.b]].
func (p *parser) header() error {
	start := p.pos()
	array := p.at("[[")
	if array {
		p.i += 2
	} else {
		p.i++
	}
	parts, err := p.key()
	if err != nil {
		return err
	}
	p.skipWS()
	closeTok := "]"
	if array {
		closeTok = "]]"
	}
	if !p.at(closeTok) {
		return p.errf("expected %q to close the table header", closeTok)
	}
	p.i += len(closeTok)

	// Walk to the parent of the last key, creating implicit tables.
	t := p.root
	for _, k := range parts[:len(parts)-1] {
		v := t.Get(k.name)
		switch {
		case v == nil:
			nt := newTable(start)
			nt.implicit = true
			t.set(k.name, &Value{Kind: KindTable, Pos: start, KeyPos: start, Tbl: nt})
			t = nt
		case v.Kind == KindTable && !v.Tbl.inline:
			t = v.Tbl
		case v.Kind == KindArray && v.tableArray && len(v.Arr) > 0:
			t = v.Arr[len(v.Arr)-1].Tbl
		default:
			return p.errAt(k.pos, "key %q is already defined as %s", k.name, v.Kind.typeName())
		}
	}
	last := parts[len(parts)-1]
	name := joinKey(parts)
	existing := t.Get(last.name)
	if array {
		nt := newTable(start)
		nt.defined = true
		elem := &Value{Kind: KindTable, Pos: start, KeyPos: start, Tbl: nt}
		switch {
		case existing == nil:
			t.set(last.name, &Value{Kind: KindArray, Pos: start, KeyPos: start, Arr: []*Value{elem}, tableArray: true})
		case existing.Kind == KindArray && existing.tableArray:
			existing.Arr = append(existing.Arr, elem)
		default:
			return p.errAt(last.pos, "duplicate key %q: [[%s]] conflicts with an earlier definition", last.name, name)
		}
		p.cur = nt
		return p.endOfLine()
	}
	switch {
	case existing == nil:
		nt := newTable(start)
		nt.defined = true
		t.set(last.name, &Value{Kind: KindTable, Pos: start, KeyPos: start, Tbl: nt})
		p.cur = nt
	case existing.Kind == KindTable && existing.Tbl.implicit && !existing.Tbl.defined && !existing.Tbl.dotted && !existing.Tbl.inline:
		existing.Tbl.defined = true
		existing.Tbl.implicit = false
		existing.Tbl.Pos = start
		existing.Pos, existing.KeyPos = start, start
		p.cur = existing.Tbl
	default:
		return p.errAt(last.pos, "duplicate key %q: table [%s] is defined more than once", last.name, name)
	}
	return p.endOfLine()
}

// keyval parses `key = value` into t.
func (p *parser) keyval(t *Table) error {
	parts, err := p.key()
	if err != nil {
		return err
	}
	p.skipWS()
	if p.peek() != '=' {
		return p.errf("expected '=' after the key %q", joinKey(parts))
	}
	p.i++
	p.skipWS()
	v, err := p.value()
	if err != nil {
		return err
	}
	for _, k := range parts[:len(parts)-1] {
		cur := t.Get(k.name)
		switch {
		case cur == nil:
			nt := newTable(k.pos)
			nt.dotted = true
			t.set(k.name, &Value{Kind: KindTable, Pos: k.pos, KeyPos: k.pos, Tbl: nt})
			t = nt
		case cur.Kind == KindTable && cur.Tbl.dotted && !cur.Tbl.inline:
			t = cur.Tbl
		default:
			return p.errAt(k.pos, "duplicate key %q", k.name)
		}
	}
	last := parts[len(parts)-1]
	if t.Get(last.name) != nil {
		return p.errAt(last.pos, "duplicate key %q", last.name)
	}
	v.KeyPos = last.pos
	t.set(last.name, v)
	return nil
}

func (p *parser) value() (*Value, error) {
	pos := p.pos()
	switch c := p.peek(); {
	case c == '"' || c == '\'':
		var s string
		var err error
		switch {
		case p.at(`"""`):
			s, err = p.mlBasicString()
		case p.at("'''"):
			s, err = p.mlLiteralString()
		case c == '"':
			s, err = p.basicString()
		default:
			s, err = p.literalString()
		}
		if err != nil {
			return nil, err
		}
		return &Value{Kind: KindString, Pos: pos, Str: s}, nil
	case c == '[':
		return p.array()
	case c == '{':
		return p.inlineTable()
	case p.at("true") && !p.bareFollows(4):
		p.i += 4
		return &Value{Kind: KindBool, Pos: pos, Bool: true}, nil
	case p.at("false") && !p.bareFollows(5):
		p.i += 5
		return &Value{Kind: KindBool, Pos: pos, Bool: false}, nil
	case c == '+' || c == '-' || c >= '0' && c <= '9' || c == 'i' || c == 'n':
		return p.scalar()
	case p.eof() || c == '\n' || c == '\r' || c == '#':
		return nil, p.errf("expected a value")
	default:
		return nil, p.errf("invalid value starting with %s", p.describe())
	}
}

func (p *parser) bareFollows(n int) bool {
	j := p.i + n
	return j < len(p.src) && isBare(p.src[j])
}

var (
	reDate      = regexp.MustCompile(`^\d{4}-\d{2}-\d{2}$`)
	reTimeStart = regexp.MustCompile(`^\d{2}:\d{2}`)
	reDatetime  = regexp.MustCompile(`^(\d{4}-\d{2}-\d{2})([Tt ](\d{2}:\d{2}:\d{2}(\.\d+)?)([Zz]|[+-]\d{2}:\d{2})?)?$`)
	reLocalTime = regexp.MustCompile(`^\d{2}:\d{2}:\d{2}(\.\d+)?$`)
	reDecInt    = regexp.MustCompile(`^[+-]?(0|[1-9](_?\d)*)$`)
	reHexInt    = regexp.MustCompile(`^0x[0-9A-Fa-f](_?[0-9A-Fa-f])*$`)
	reOctInt    = regexp.MustCompile(`^0o[0-7](_?[0-7])*$`)
	reBinInt    = regexp.MustCompile(`^0b[01](_?[01])*$`)
	reFloat     = regexp.MustCompile(`^[+-]?(0|[1-9](_?\d)*)((\.\d(_?\d)*)([eE][+-]?\d(_?\d)*)?|[eE][+-]?\d(_?\d)*)$`)
)

// scalar parses numbers and date-times.
func (p *parser) scalar() (*Value, error) {
	pos := p.pos()
	start := p.i
	for !p.eof() {
		c := p.src[p.i]
		if isBare(c) || c == '+' || c == '.' || c == ':' {
			p.i++
			continue
		}
		// "1979-05-27 07:32:00": a space between date and time.
		if c == ' ' && reDate.MatchString(p.src[start:p.i]) && reTimeStart.MatchString(p.src[p.i+1:]) {
			p.i++
			continue
		}
		break
	}
	tok := p.src[start:p.i]
	switch {
	case tok == "inf" || tok == "+inf" || tok == "-inf":
		f := math.Inf(1)
		if tok[0] == '-' {
			f = math.Inf(-1)
		}
		return &Value{Kind: KindFloat, Pos: pos, Float: f}, nil
	case tok == "nan" || tok == "+nan" || tok == "-nan":
		return &Value{Kind: KindFloat, Pos: pos, Float: math.NaN()}, nil
	case reDecInt.MatchString(tok):
		n, err := strconv.ParseInt(strings.ReplaceAll(tok, "_", ""), 10, 64)
		if err != nil {
			return nil, p.errAt(pos, "integer %s is out of range", tok)
		}
		return &Value{Kind: KindInteger, Pos: pos, Int: n}, nil
	case reHexInt.MatchString(tok), reOctInt.MatchString(tok), reBinInt.MatchString(tok):
		base := map[byte]int{'x': 16, 'o': 8, 'b': 2}[tok[1]]
		n, err := strconv.ParseInt(strings.ReplaceAll(tok[2:], "_", ""), base, 64)
		if err != nil {
			return nil, p.errAt(pos, "integer %s is out of range", tok)
		}
		return &Value{Kind: KindInteger, Pos: pos, Int: n}, nil
	case reFloat.MatchString(tok):
		f, err := strconv.ParseFloat(strings.ReplaceAll(tok, "_", ""), 64)
		if err != nil {
			return nil, p.errAt(pos, "invalid float %s", tok)
		}
		return &Value{Kind: KindFloat, Pos: pos, Float: f}, nil
	case reDatetime.MatchString(tok), reLocalTime.MatchString(tok):
		return &Value{Kind: KindDatetime, Pos: pos, Str: tok}, nil
	}
	return nil, p.errAt(pos, "invalid value %q", tok)
}

// skipArrayWS skips whitespace, newlines and comments inside arrays.
func (p *parser) skipArrayWS() error {
	for {
		p.skipWS()
		if err := p.skipComment(); err != nil {
			return err
		}
		if !p.newline() {
			if p.peek() == '\r' {
				return p.errf("unexpected carriage return")
			}
			return nil
		}
	}
}

func (p *parser) array() (*Value, error) {
	pos := p.pos()
	p.i++ // [
	v := &Value{Kind: KindArray, Pos: pos, Arr: []*Value{}}
	for {
		if err := p.skipArrayWS(); err != nil {
			return nil, err
		}
		if p.peek() == ']' {
			p.i++
			return v, nil
		}
		if p.eof() {
			return nil, p.errAt(pos, "unterminated array")
		}
		elem, err := p.value()
		if err != nil {
			return nil, err
		}
		elem.KeyPos = elem.Pos
		v.Arr = append(v.Arr, elem)
		if err := p.skipArrayWS(); err != nil {
			return nil, err
		}
		switch p.peek() {
		case ',':
			p.i++
		case ']':
			p.i++
			return v, nil
		default:
			if p.eof() {
				return nil, p.errAt(pos, "unterminated array")
			}
			return nil, p.errf("expected ',' or ']' in the array, found %s", p.describe())
		}
	}
}

func (p *parser) inlineTable() (*Value, error) {
	pos := p.pos()
	p.i++ // {
	t := newTable(pos)
	t.inline = true
	v := &Value{Kind: KindTable, Pos: pos, Tbl: t}
	p.skipWS()
	if p.peek() == '}' {
		p.i++
		return v, nil
	}
	for {
		if err := p.keyval(t); err != nil {
			return nil, err
		}
		p.skipWS()
		switch p.peek() {
		case ',':
			p.i++
			p.skipWS()
		case '}':
			p.i++
			sealInline(t)
			return v, nil
		default:
			if p.eof() || p.peek() == '\n' || p.peek() == '\r' {
				return nil, p.errAt(pos, "unterminated inline table (inline tables must be on one line)")
			}
			return nil, p.errf("expected ',' or '}' in the inline table, found %s", p.describe())
		}
	}
}

// sealInline marks the dotted sub-tables of an inline table as inline too.
func sealInline(t *Table) {
	t.inline = true
	for _, k := range t.Keys {
		if v := t.M[k]; v.Kind == KindTable {
			sealInline(v.Tbl)
		}
	}
}

func (p *parser) basicString() (string, error) {
	start := p.pos()
	p.i++ // "
	var b strings.Builder
	for {
		if p.eof() || p.peek() == '\n' || p.peek() == '\r' {
			return "", p.errAt(start, "unterminated string")
		}
		c := p.src[p.i]
		switch {
		case c == '"':
			p.i++
			return b.String(), nil
		case c == '\\':
			if err := p.escape(&b); err != nil {
				return "", err
			}
		case c < 0x20 && c != '\t' || c == 0x7f:
			return "", p.errf("control character in string")
		default:
			r, n := utf8.DecodeRuneInString(p.src[p.i:])
			b.WriteRune(r)
			p.i += n
		}
	}
}

func (p *parser) escape(b *strings.Builder) error {
	pos := p.pos()
	p.i++ // backslash
	if p.eof() {
		return p.errAt(pos, "unterminated escape sequence")
	}
	c := p.src[p.i]
	p.i++
	switch c {
	case 'b':
		b.WriteByte('\b')
	case 't':
		b.WriteByte('\t')
	case 'n':
		b.WriteByte('\n')
	case 'f':
		b.WriteByte('\f')
	case 'r':
		b.WriteByte('\r')
	case '"':
		b.WriteByte('"')
	case '\\':
		b.WriteByte('\\')
	case 'u', 'U':
		n := 4
		if c == 'U' {
			n = 8
		}
		if p.i+n > len(p.src) {
			return p.errAt(pos, "invalid unicode escape")
		}
		code, err := strconv.ParseUint(p.src[p.i:p.i+n], 16, 32)
		if err != nil || !utf8.ValidRune(rune(code)) {
			return p.errAt(pos, "invalid unicode escape")
		}
		p.i += n
		b.WriteRune(rune(code))
	default:
		return p.errAt(pos, "invalid escape sequence \\%c", c)
	}
	return nil
}

func (p *parser) literalString() (string, error) {
	start := p.pos()
	p.i++ // '
	from := p.i
	for {
		if p.eof() || p.peek() == '\n' || p.peek() == '\r' {
			return "", p.errAt(start, "unterminated string")
		}
		c := p.src[p.i]
		if c == '\'' {
			s := p.src[from:p.i]
			p.i++
			return s, nil
		}
		if c < 0x20 && c != '\t' || c == 0x7f {
			return "", p.errf("control character in string")
		}
		p.i++
	}
}

func (p *parser) mlBasicString() (string, error) {
	start := p.pos()
	p.i += 3
	p.newline() // a newline right after the delimiter is trimmed
	var b strings.Builder
	for {
		if p.eof() {
			return "", p.errAt(start, "unterminated multi-line string")
		}
		if p.at(`"""`) {
			// Up to two quotes may precede the closing delimiter.
			extra := 0
			for extra < 2 && p.i+3+extra < len(p.src) && p.src[p.i+3+extra] == '"' {
				extra++
			}
			b.WriteString(strings.Repeat(`"`, extra))
			p.i += 3 + extra
			return b.String(), nil
		}
		c := p.src[p.i]
		switch {
		case c == '\\':
			// Line-ending backslash: trim whitespace and newlines.
			j := p.i + 1
			for j < len(p.src) && (p.src[j] == ' ' || p.src[j] == '\t') {
				j++
			}
			if j < len(p.src) && (p.src[j] == '\n' || strings.HasPrefix(p.src[j:], "\r\n")) {
				p.i = j
				for {
					if p.newline() {
						continue
					}
					if !p.eof() && (p.src[p.i] == ' ' || p.src[p.i] == '\t') {
						p.i++
						continue
					}
					break
				}
				continue
			}
			if err := p.escape(&b); err != nil {
				return "", err
			}
		case c == '\n' || c == '\r':
			if !p.newline() {
				return "", p.errf("unexpected carriage return")
			}
			b.WriteByte('\n')
		case c < 0x20 && c != '\t' || c == 0x7f:
			return "", p.errf("control character in string")
		default:
			r, n := utf8.DecodeRuneInString(p.src[p.i:])
			b.WriteRune(r)
			p.i += n
		}
	}
}

func (p *parser) mlLiteralString() (string, error) {
	start := p.pos()
	p.i += 3
	p.newline()
	var b strings.Builder
	for {
		if p.eof() {
			return "", p.errAt(start, "unterminated multi-line string")
		}
		if p.at("'''") {
			extra := 0
			for extra < 2 && p.i+3+extra < len(p.src) && p.src[p.i+3+extra] == '\'' {
				extra++
			}
			b.WriteString(strings.Repeat("'", extra))
			p.i += 3 + extra
			return b.String(), nil
		}
		c := p.src[p.i]
		switch {
		case c == '\n' || c == '\r':
			if !p.newline() {
				return "", p.errf("unexpected carriage return")
			}
			b.WriteByte('\n')
		case c < 0x20 && c != '\t' || c == 0x7f:
			return "", p.errf("control character in string")
		default:
			r, n := utf8.DecodeRuneInString(p.src[p.i:])
			b.WriteRune(r)
			p.i += n
		}
	}
}
