package codesearch

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// symStrings renders symbols as "kind container.name:line".
func symStrings(syms []Symbol) []string {
	out := []string{}
	for _, s := range syms {
		name := s.Name
		if s.Container != "" {
			name = s.Container + "." + name
		}
		out = append(out, fmt.Sprintf("%s %s:%d", s.Kind, name, s.Line))
	}
	return out
}

func TestExtractSymbolsByLanguage(t *testing.T) {
	cases := []struct {
		path, src string
		want      []string
	}{
		{"main.go", `package main

import "fmt"

const Version = "1.0"

const (
	// A comment
	ModeA = iota
	ModeB
)

type (
	Point struct {
		X, Y int
	}
	ID = string
)

type Server struct {
	addr string
}

type Handler interface {
	Serve()
}

type Celsius float64

func NewServer(addr string) *Server {
	inner := func() {}
	_ = inner
	return &Server{addr: addr}
}

func (s *Server) Start(ctx context.Context) error {
	return nil
}

func (List[T]) Len() int { return 0 }

func Map[T any](xs []T) []T { return xs }
`, []string{"const Version:5", "const ModeA:9", "const ModeB:10", "struct Point:14", "type ID:17", "struct Server:20",
			"interface Handler:24", "type Celsius:28", "function NewServer:30", "method Server.Start:36", "method List.Len:40", "function Map:42"}},

		{"lib.rs", `use std::fmt;

pub const MAX: usize = 10;
static mut COUNTER: u32 = 0;

pub struct Config {
    name: String,
}

pub(crate) enum Mode { A, B }

pub trait Render {
    fn render(&self) -> String;
}

type Result<T> = std::result::Result<T, Error>;

impl Config {
    pub fn new(name: &str) -> Self {
        let s = "{ not a block";
        fn helper() {}
        Config { name: name.into() }
    }
    const DEFAULT: u8 = 1;
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

pub async fn run() {}

mod tests {
    fn it_works() {}
}
`, []string{"const MAX:3", "const COUNTER:4", "struct Config:6", "enum Mode:10", "trait Render:12", "method Render.render:13",
			"type Result:16", "method Config.new:19", "const Config.DEFAULT:24", "method Config.fmt:28", "function run:33", "function it_works:36"}},

		{"app.ts", `import x from "y";

export const API_URL = "https://example.com";
export const handler = async (req: Request) => {
  return 1;
};
export default function App() {
  const local = () => 1;
}
export interface Props {
  name: string;
}
export type Id = string | number;
export enum Color { Red, Green }
export abstract class Store<T> {
  private items: T[] = [];
  constructor(private name: string) {}
  get size(): number {
    if (this.items.length) {
      return 1;
    }
    return 0;
  }
  async load(id: Id): Promise<T> {
    return this.items[0];
  }
  reset = () => {
    this.items = [];
  };
}
function helper(a, b) { return a + b }
`, []string{"const API_URL:3", "function handler:4", "function App:7", "interface Props:10", "type Id:13", "enum Color:14",
			"class Store:15", "method Store.constructor:17", "method Store.size:18", "method Store.load:24", "method Store.reset:27",
			"function helper:31"}},

		{"svc.py", `"""Module docstring.

def not_a_function():
"""
import os

MAX_SIZE = 10
lower = 1

class Service(Base):
    """Docs."""
    LIMIT = 5

    def __init__(self):
        def inner():
            pass

    async def fetch(self, url):
        pass

    class Meta:
        def option(self):
            pass

def main():
    pass
`, []string{"const MAX_SIZE:7", "class Service:10", "method Service.__init__:14", "method Service.fetch:18",
			"class Service.Meta:21", "method Meta.option:22", "function main:25"}},

		{"Store.java", `package x;

public final class Store<T> implements Iterable<T> {
    public static final int MAX = 10;
    private final Map<String, Integer> counts = new HashMap<>();
    int size = compute(1);

    public Store(int cap) {
        if (cap > 0) {
            init(cap);
        }
    }

    @Override
    public Iterator<T> iterator() {
        return null;
    }

    static <K> List<K> keys(Map<K, ?> m) { return null; }

    abstract void close();

    enum Kind {
        RED("r"),
        GREEN("g");
        String code() { return ""; }
    }

    interface Listener {
        void onEvent(String e);
    }
}
`, []string{"class Store:3", "const Store.MAX:4", "method Store.Store:8", "method Store.iterator:15", "method Store.keys:19",
			"method Store.close:21", "enum Store.Kind:23", "method Kind.code:26", "interface Store.Listener:29", "method Listener.onEvent:30"}},

		{"util.c", `#include <stdio.h>
#define BUF_SIZE 64
#define MAX(a, b) ((a) > (b) ? (a) : (b))

typedef unsigned long size_type;
typedef int (*cmp_fn)(const void *, const void *);

struct node {
    int value;
};

typedef struct {
    int x;
} point_t;

enum color { RED, GREEN };

static const int LIMIT = 3;

int add(int a, int b);

static int add(int a, int b) {
    int c = add(1, 2);
    return a + b;
}

void *xalloc(size_t n)
{
    return malloc(n);
}
`, []string{"const BUF_SIZE:2", "function MAX:3", "type size_type:5", "type cmp_fn:6", "struct node:8", "type point_t:14",
			"enum color:16", "const LIMIT:18", "function add:22", "function xalloc:27"}},

		{"widget.cpp", `namespace ui {

class Widget : public Base {
public:
    Widget(int w);
    ~Widget();
    int width() const { return w_; }
    void draw() override;
private:
    int w_ = 0;
};

using WidgetPtr = std::shared_ptr<Widget>;

Widget::Widget(int w) : w_(w) {}

void Widget::draw() {
    render(this);
}

template <typename T>
T clamp(T v, T lo, T hi) {
    return v;
}

}  // namespace ui

TEST(WidgetTest, Draws) {
}
`, []string{"class Widget:3", "method Widget.Widget:5", "method Widget.width:7", "method Widget.draw:8",
			"type WidgetPtr:13", "method Widget.Widget:15", "method Widget.draw:17", "function clamp:22"}},
	}
	for _, c := range cases {
		t.Run(c.path, func(t *testing.T) {
			got := symStrings(ExtractSymbols(c.path, []byte(c.src)))
			if !reflect.DeepEqual(got, c.want) {
				t.Fatalf("symbols of %s:\n got  %q\n want %q", c.path, got, c.want)
			}
		})
	}
	if s := ExtractSymbols("README.md", []byte("func main() {}\n")); s != nil {
		t.Fatalf("markdown symbols: %v", s)
	}
}

func TestExtractSymbolsDetails(t *testing.T) {
	syms := ExtractSymbols("a.go", []byte("func (s *Server) Start() error {\n\treturn nil\n}\n"))
	eq(t, syms, []Symbol{{Name: "Start", Kind: KindMethod, Line: 1, Container: "Server", Signature: "func (s *Server) Start() error"}})
	long := "func F(" + strings.Repeat("é", 300) + ") {}\n"
	syms = ExtractSymbols("a.go", []byte(long))
	if len(syms) != 1 || len(syms[0].Signature) > maxSignature || strings.ContainsRune(syms[0].Signature, '\uFFFD') {
		t.Fatalf("signature not clipped at a rune boundary: %q", syms[0].Signature)
	}
	eq(t, Language("x/Y.TSX"), "typescript")
	eq(t, Language("a.hpp"), "cpp")
	eq(t, Language("Makefile"), "")
	kinds, err := ParseKinds("function, Method,,const")
	must(t, err)
	eq(t, kinds, []SymbolKind{KindFunction, KindMethod, KindConst})
	if _, err := ParseKinds("function,widget"); !errors.Is(err, ErrInvalidQuery) {
		t.Fatalf("bad kind: %v", err)
	}
}

// ---- symbol search ----

func symLocs(r *SymbolResult) []string {
	out := []string{}
	for _, s := range r.Symbols {
		out = append(out, fmt.Sprintf("%s:%s:%d %s %s %s", s.Tree, s.Path, s.Line, s.Kind, s.Name, s.Match))
	}
	return out
}

func (w *world) symbols(t *testing.T, q SymbolQuery, targets ...Target) *SymbolResult {
	t.Helper()
	res, err := w.ix.SearchSymbols(context.Background(), q, targets, nil)
	must(t, err)
	return res
}

var symbolSample = map[string]string{
	"pkg/parse.go":   "package pkg\n\nfunc Parse(s string) {}\n\nfunc ParseConfig() {}\n\nfunc MustParse() {}\n\ntype Parser struct{}\n\nfunc (p *Parser) parse() {}\n",
	"src/lib.rs":     "pub fn parse() {}\npub struct ParseError;\n",
	"web/app.ts":     "export function reparse() {}\nexport const PARSE_LIMIT = 3;\n",
	"docs/notes.txt": "func Parse() is mentioned here\n",
}

func TestSymbolSearchRanking(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	snap := w.push(t, web, "main", symbolSample)
	tg := Target{TenantSlug: "acme", Tree: web}

	res := w.symbols(t, SymbolQuery{Query: "parse"}, tg)
	eq(t, symLocs(res), []string{
		"web:pkg/parse.go:11 method parse exact", // exact, query's case
		"web:src/lib.rs:1 function parse exact",
		"web:pkg/parse.go:3 function Parse exact", // exact, other case
		"web:pkg/parse.go:9 struct Parser prefix",
		"web:src/lib.rs:2 struct ParseError prefix", // then shorter names first
		"web:web/app.ts:2 const PARSE_LIMIT prefix",
		"web:pkg/parse.go:5 function ParseConfig prefix",
		"web:web/app.ts:1 function reparse substring",
		"web:pkg/parse.go:7 function MustParse substring",
	})
	h := res.Symbols[0]
	eq(t, []any{h.TenantSlug, h.Branch, h.SnapshotID, h.Container, h.Signature, h.Language},
		[]any{"acme", "main", snap, "Parser", "func (p *Parser) parse() {}", "go"})
	eq(t, []any{res.Truncated, res.SearchedTrees}, []any{false, 1})

	// Kind filter, path filter, limit.
	res = w.symbols(t, SymbolQuery{Query: "parse", Kinds: []SymbolKind{KindStruct, KindConst}}, tg)
	eq(t, symLocs(res), []string{"web:pkg/parse.go:9 struct Parser prefix", "web:src/lib.rs:2 struct ParseError prefix", "web:web/app.ts:2 const PARSE_LIMIT prefix"})
	res = w.symbols(t, SymbolQuery{Query: "parse", Path: "*.rs"}, tg)
	eq(t, len(res.Symbols), 2)
	res = w.symbols(t, SymbolQuery{Query: "parse", Limit: 2}, tg)
	eq(t, []any{len(res.Symbols), res.Truncated}, []any{2, true})
	eq(t, res.Symbols[1].Path, "src/lib.rs")

	// Errors.
	for _, q := range []SymbolQuery{{}, {Query: "  "}, {Query: strings.Repeat("x", MaxSymbolQuery+1)}, {Query: "a", Path: "[x"},
		{Query: "a", Kinds: []SymbolKind{"widget"}}, {Query: "a", Branch: "dev", AllBranches: true}} {
		if _, err := w.ix.SearchSymbols(context.Background(), q, []Target{tg}, nil); !errors.Is(err, ErrInvalidQuery) {
			t.Errorf("%+v: err = %v", q, err)
		}
	}
}

func TestSymbolSearchReadFilter(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	files := map[string]string{}
	for i := 0; i < 2*filterBatch+5; i++ {
		files[fmt.Sprintf("f%03d.go", i)] = fmt.Sprintf("package f\n\nfunc Handle%03d() {}\n", i)
	}
	files["secret/key.go"] = "package secret\n\nfunc Handle() {}\n"
	w.push(t, web, "main", files)
	calls := 0
	filter := func(_ context.Context, _ *domain.Tree, paths []string) (func(string) bool, error) {
		calls++
		if len(paths) > filterBatch {
			t.Errorf("batch of %d paths", len(paths))
		}
		return func(p string) bool { return strings.HasPrefix(p, "secret/") || p == "f000.go" }, nil
	}
	tg := Target{TenantSlug: "acme", Tree: web}
	res, err := w.ix.SearchSymbols(context.Background(), SymbolQuery{Query: "handle", Limit: 3}, []Target{tg}, filter)
	must(t, err)
	// The exact match is restricted; restricted hits do not count.
	eq(t, symLocs(res), []string{"web:f001.go:3 function Handle001 prefix", "web:f002.go:3 function Handle002 prefix", "web:f003.go:3 function Handle003 prefix"})
	eq(t, res.Truncated, true)
	eq(t, calls, 1)
	res, err = w.ix.SearchSymbols(context.Background(), SymbolQuery{Query: "handle", Limit: MaxLimit}, []Target{tg}, filter)
	must(t, err)
	eq(t, []any{len(res.Symbols), res.Truncated}, []any{2*filterBatch + 4, false})
	boom := errors.New("boom")
	_, err = w.ix.SearchSymbols(context.Background(), SymbolQuery{Query: "handle"}, []Target{tg},
		func(context.Context, *domain.Tree, []string) (func(string) bool, error) { return nil, boom })
	if !errors.Is(err, boom) {
		t.Fatalf("err = %v", err)
	}
}

func TestSymbolSearchAcrossTreesAndBranches(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	api := w.tree(t, "acme", "api")
	w.push(t, web, "main", map[string]string{"a.go": "package a\n\nfunc Run() {}\n"})
	w.push(t, api, "main", map[string]string{"b.go": "package b\n\nfunc Run() {}\n\nfunc RunAll() {}\n"})
	w.push(t, api, "dev", map[string]string{"b.go": "package b\n\nfunc Run() {}\n\nfunc RunAll() {}\n", "c.go": "package b\n\nfunc RunDev() {}\n"})
	targets := []Target{{TenantSlug: "acme", Tree: web}, {TenantSlug: "acme", Tree: api}}
	res := w.symbols(t, SymbolQuery{Query: "run"}, targets...)
	eq(t, symLocs(res), []string{"api:b.go:3 function Run exact", "web:a.go:3 function Run exact", "api:b.go:5 function RunAll prefix"})
	eq(t, res.SearchedTrees, 2)
	// All branches: dev's unchanged b.go is not reported twice.
	res = w.symbols(t, SymbolQuery{Query: "run", AllBranches: true}, targets...)
	eq(t, symLocs(res), []string{"api:b.go:3 function Run exact", "web:a.go:3 function Run exact", "api:b.go:5 function RunAll prefix", "api:c.go:3 function RunDev prefix"})
	eq(t, res.Symbols[3].Branch, "dev")
	res = w.symbols(t, SymbolQuery{Query: "rundev", Branch: "dev"}, targets...)
	eq(t, symLocs(res), []string{"api:c.go:3 function RunDev exact"})
	eq(t, res.SearchedTrees, 2) // web has no dev branch: searched, nothing found
}
