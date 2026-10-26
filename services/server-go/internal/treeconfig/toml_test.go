package treeconfig

import (
	"errors"
	"math"
	"strings"
	"testing"
)

func mustParse(t *testing.T, src string) *Table {
	t.Helper()
	tb, err := ParseTOML([]byte(src))
	if err != nil {
		t.Fatalf("ParseTOML: %v\n%s", err, src)
	}
	return tb
}

func TestParseTOMLValues(t *testing.T) {
	tb := mustParse(t, `# comment
title = "TOML \"x\" \u00e9"
lit = 'C:\path'
n = 1_000
hex = 0xff
oct = 0o17
bin = 0b101
neg = -42
f = 6.5e-1
inf = -inf
yes = true
no = false
dt = 1979-05-27T07:32:00Z
sp = 1979-05-27 07:32:00-07:00
lt = 07:32:00
arr = [ 1, 2,
  3, # trailing comment
]
nested = [[1], ["a"]]
inline = { a = 1, b.c = "x" }
ml = """
line1
  line2 \
  joined"""
mll = '''
raw\n'''
"quoted key" = 1
a.b.c = 2

[table]
k = "v"

[table.sub]
x = 1

[[arr_t]]
i = 1
[[arr_t]]
i = 2
[arr_t.child]
y = 3
`)
	str := func(k string) string { return tb.Get(k).Str }
	if str("title") != `TOML "x" é` || str("lit") != `C:\path` {
		t.Errorf("strings: %q %q", str("title"), str("lit"))
	}
	for k, want := range map[string]int64{"n": 1000, "hex": 255, "oct": 15, "bin": 5, "neg": -42} {
		if v := tb.Get(k); v.Kind != KindInteger || v.Int != want {
			t.Errorf("%s = %+v, want %d", k, v, want)
		}
	}
	if v := tb.Get("f"); v.Kind != KindFloat || v.Float != 0.65 {
		t.Errorf("f = %+v", v)
	}
	if v := tb.Get("inf"); !math.IsInf(v.Float, -1) {
		t.Errorf("inf = %+v", v)
	}
	if !tb.Get("yes").Bool || tb.Get("no").Bool || tb.Get("no").Kind != KindBool {
		t.Error("booleans")
	}
	for _, k := range []string{"dt", "sp", "lt"} {
		if tb.Get(k).Kind != KindDatetime {
			t.Errorf("%s is %v", k, tb.Get(k).Kind)
		}
	}
	if a := tb.Get("arr"); len(a.Arr) != 3 || a.Arr[2].Int != 3 {
		t.Errorf("arr = %+v", a)
	}
	if n := tb.Get("nested"); len(n.Arr) != 2 || n.Arr[1].Arr[0].Str != "a" {
		t.Errorf("nested = %+v", n)
	}
	in := tb.Get("inline").Tbl
	if in.Get("a").Int != 1 || in.Get("b").Tbl.Get("c").Str != "x" {
		t.Errorf("inline = %+v", in)
	}
	if str("ml") != "line1\n  line2 joined" {
		t.Errorf("ml = %q", str("ml"))
	}
	if str("mll") != `raw\n` {
		t.Errorf("mll = %q", str("mll"))
	}
	if tb.Get("quoted key").Int != 1 || tb.Get("a").Tbl.Get("b").Tbl.Get("c").Int != 2 {
		t.Error("keys")
	}
	if tb.Get("table").Tbl.Get("sub").Tbl.Get("x").Int != 1 {
		t.Error("sub table")
	}
	at := tb.Get("arr_t")
	if len(at.Arr) != 2 || at.Arr[1].Tbl.Get("child").Tbl.Get("y").Int != 3 {
		t.Errorf("array of tables = %+v", at)
	}
}

func TestParseTOMLPositions(t *testing.T) {
	tb := mustParse(t, "a = 1\n\n[[b]]\n  name = \"é\"\nlist = [\"x\", \"y\"]\n")
	if p := tb.Get("a").KeyPos; p != (Pos{1, 1}) {
		t.Errorf("a key at %v", p)
	}
	if p := tb.Get("a").Pos; p != (Pos{1, 5}) {
		t.Errorf("a value at %v", p)
	}
	b := tb.Get("b").Arr[0]
	if b.Pos != (Pos{3, 1}) {
		t.Errorf("[[b]] at %v", b.Pos)
	}
	if p := b.Tbl.Get("name").KeyPos; p != (Pos{4, 3}) {
		t.Errorf("name at %v", p)
	}
	if p := b.Tbl.Get("list").Arr[1].Pos; p != (Pos{5, 14}) {
		t.Errorf("list[1] at %v", p)
	}
	// Columns count characters, not bytes.
	tb = mustParse(t, "k = \"é\" # ü\nx = [\"ü\", 1]\n")
	if p := tb.Get("x").Arr[1].Pos; p != (Pos{2, 11}) {
		t.Errorf("x[1] at %v", p)
	}
	// CRLF line endings.
	tb = mustParse(t, "a = 1\r\nb = 2\r\n")
	if p := tb.Get("b").KeyPos; p != (Pos{2, 1}) {
		t.Errorf("b at %v", p)
	}
}

func TestParseTOMLErrors(t *testing.T) {
	cases := map[string]string{
		"a = ":                       "expected a value",
		"a = 1\na = 2":               "duplicate key",
		"[t]\n[t]":                   "defined more than once",
		"a = \"x":                    "unterminated string",
		"a = [1, 2":                  "unterminated array",
		"a = {x = 1":                 "unterminated inline table",
		"a = 1 b = 2":                "expected a newline",
		"= 1":                        "expected a key",
		"a = 01":                     "invalid value",
		"a = 1\n[a]":                 "duplicate key",
		"a = { x = 1 }\n[a.y]":       "already defined",
		"[a\nb = 1":                  "close the table header",
		"a = \"\\q\"":                "invalid escape",
		"a = tru":                    "invalid value",
		"x = [1]\n[[x]]":             "conflicts",
		"a = 99999999999999999999":   "out of range",
		"a = \"\"\"never closed\n":   "unterminated multi-line string",
		"a.b = 1\n[a]\n":             "defined more than once",
		"a = 1979-05-27T07:32:00Zzz": "invalid value",
	}
	for src, want := range cases {
		_, err := ParseTOML([]byte(src))
		var se *SyntaxError
		if !errors.As(err, &se) {
			t.Errorf("%q: err = %v, want a syntax error", src, err)
			continue
		}
		if !strings.Contains(se.Msg, want) {
			t.Errorf("%q: %q does not mention %q", src, se.Msg, want)
		}
		if se.Pos.Line < 1 || se.Pos.Col < 1 {
			t.Errorf("%q: position %v", src, se.Pos)
		}
	}
	if _, err := ParseTOML([]byte{0xff, 0xfe}); err == nil {
		t.Error("invalid UTF-8 accepted")
	}
	// The position of a duplicate key is the second key.
	_, err := ParseTOML([]byte("a = 1\n  a = 2\n"))
	if se := err.(*SyntaxError); se.Pos != (Pos{2, 3}) {
		t.Errorf("duplicate at %v", se.Pos)
	}
}
