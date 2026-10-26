package codesearch

import (
	"path"
	"regexp"
	"strings"
	"unicode/utf8"
)

// Symbol extraction (docs/ENGINEERING_PLAN.md §15.9).
//
// A line-based, regular-expression extractor finds definitions in Go,
// Rust, TypeScript/JavaScript, Python, Java and C/C++ files: functions,
// methods, types/classes/structs/interfaces/traits/enums and constants,
// each with its 1-based line, the enclosing type for methods (container)
// and the defining line as signature. Brace languages track the nesting
// depth (strings and comments are skipped) so that only declarations at
// the top level, directly inside a type body (methods) or inside a
// namespace/module block are reported, never local definitions in
// function bodies; Python uses indentation. The extractor is heuristic: it
// does not parse, so unusual formatting can hide or invent a symbol.

// SymbolKind is the kind of a symbol.
type SymbolKind string

// Symbol kinds. The order of SymbolKinds is part of the on-disk format.
const (
	KindFunction  SymbolKind = "function"
	KindMethod    SymbolKind = "method"
	KindClass     SymbolKind = "class"
	KindStruct    SymbolKind = "struct"
	KindInterface SymbolKind = "interface"
	KindTrait     SymbolKind = "trait"
	KindEnum      SymbolKind = "enum"
	KindType      SymbolKind = "type"
	KindConst     SymbolKind = "const"
)

// SymbolKinds lists every kind (the index is the on-disk code).
var SymbolKinds = []SymbolKind{KindFunction, KindMethod, KindClass, KindStruct, KindInterface, KindTrait, KindEnum, KindType, KindConst}

// ValidKind reports whether k is a known symbol kind.
func ValidKind(k SymbolKind) bool {
	for _, s := range SymbolKinds {
		if s == k {
			return true
		}
	}
	return false
}

// Limits of the extractor.
const (
	maxSymbolsPerFile = 5000
	maxSymbolLine     = 2000 // longer lines (minified code) are not scanned
	maxSignature      = 200
)

// Language returns the language of a file from its extension ("" when no
// extractor handles it).
func Language(p string) string {
	switch strings.ToLower(path.Ext(p)) {
	case ".go":
		return "go"
	case ".rs":
		return "rust"
	case ".ts", ".tsx", ".mts", ".cts":
		return "typescript"
	case ".js", ".jsx", ".mjs", ".cjs":
		return "javascript"
	case ".py", ".pyi":
		return "python"
	case ".java":
		return "java"
	case ".c", ".h":
		return "c"
	case ".cc", ".cpp", ".cxx", ".c++", ".hpp", ".hh", ".hxx", ".h++", ".ipp":
		return "cpp"
	}
	return ""
}

// Symbol is one definition found by ExtractSymbols.
type Symbol struct {
	Name      string
	Kind      SymbolKind
	Line      int // 1-based
	Container string
	Signature string
}

// ExtractSymbols returns the definitions in content (by path's language).
func ExtractSymbols(p string, content []byte) []Symbol {
	var ex func(lines []string) []Symbol
	switch Language(p) {
	case "go":
		ex = extractGo
	case "rust":
		ex = extractRust
	case "typescript", "javascript":
		ex = extractTS
	case "python":
		ex = extractPython
	case "java":
		ex = extractJava
	case "c", "cpp":
		ex = extractC
	default:
		return nil
	}
	raw := splitLines(content)
	lines := make([]string, len(raw))
	for i, l := range raw {
		if len(l) <= maxSymbolLine {
			lines[i] = string(l)
		}
	}
	out := ex(lines)
	if len(out) > maxSymbolsPerFile {
		out = out[:maxSymbolsPerFile]
	}
	return out
}

// signature is the trimmed defining line without a trailing "{", cut to
// maxSignature bytes at a character boundary.
func signature(line string) string {
	s := strings.TrimSpace(line)
	s = strings.TrimSpace(strings.TrimSuffix(s, "{"))
	if len(s) > maxSignature {
		cut := maxSignature
		for cut > 0 && !utf8.RuneStart(s[cut]) {
			cut--
		}
		s = s[:cut]
	}
	return s
}

type symList struct{ out []Symbol }

func (l *symList) add(name string, kind SymbolKind, i int, container, line string) {
	if name == "" || len(l.out) >= maxSymbolsPerFile {
		return
	}
	l.out = append(l.out, Symbol{Name: name, Kind: kind, Line: i + 1, Container: container, Signature: signature(line)})
}

// ---- brace depth scanning (Rust, TS/JS, Java, C/C++) ----

// braceScanner counts "{" and "}" outside strings, character literals and
// comments, carrying block comments and backtick strings across lines.
type braceScanner struct {
	inBlock    bool // inside /* ... */
	inTemplate bool // inside `...` (JS template literal)
}

// delta returns the net brace change of line and whether the line opens a
// block ("{" seen) before closing it.
func (b *braceScanner) delta(line string) int {
	d := 0
	for i := 0; i < len(line); i++ {
		c := line[i]
		switch {
		case b.inBlock:
			if c == '*' && i+1 < len(line) && line[i+1] == '/' {
				b.inBlock = false
				i++
			}
		case b.inTemplate:
			if c == '\\' {
				i++
			} else if c == '`' {
				b.inTemplate = false
			}
		case c == '/' && i+1 < len(line) && line[i+1] == '/':
			return d
		case c == '/' && i+1 < len(line) && line[i+1] == '*':
			b.inBlock = true
			i++
		case c == '"':
			for i++; i < len(line) && line[i] != '"'; i++ {
				if line[i] == '\\' {
					i++
				}
			}
		case c == '`':
			b.inTemplate = true
		case c == '\'':
			// A character literal ('x', '\n', '{'); Rust lifetimes ('a)
			// have no closing quote two bytes later and are skipped.
			if i+2 < len(line) && line[i+1] == '\\' {
				if j := strings.IndexByte(line[i+2:], '\''); j >= 0 && j < 10 {
					i += 2 + j
				}
			} else if i+2 < len(line) && line[i+2] == '\'' {
				i += 2
			}
		case c == '{':
			d++
		case c == '}':
			d--
		}
	}
	return d
}

// scope is an open block of a brace language: a type body (named, its
// functions are methods) or a transparent namespace/module.
type scope struct {
	depth int // brace depth inside the block
	name  string
	typ   bool
	enum  bool // a Java enum body (constants look like calls)
}

// braceWalk iterates the lines of a brace language, calling fn with the
// innermost open scope whose body is exactly at the depth at the start of
// the line (nil at the top level), and ok = the line is at the top level
// or directly in that scope's body. fn returns a scope to open when the
// line starts a block (nil for none).
func braceWalk(lines []string, fn func(i int, line string, top *scope, ok bool) *scope) {
	var sc braceScanner
	var stack []scope
	depth := 0
	for i, line := range lines {
		for len(stack) > 0 && depth < stack[len(stack)-1].depth {
			stack = stack[:len(stack)-1]
		}
		var top *scope
		ok := depth == 0
		if len(stack) > 0 && stack[len(stack)-1].depth == depth {
			top = &stack[len(stack)-1]
			ok = true
		}
		wasBlock := sc.inBlock || sc.inTemplate
		var open *scope
		if !wasBlock && strings.TrimSpace(line) != "" {
			open = fn(i, line, top, ok)
		}
		d := sc.delta(line)
		if open != nil && d > 0 {
			open.depth = depth + 1
			stack = append(stack, *open)
		}
		depth += d
		if depth < 0 {
			depth = 0
		}
	}
}

// ---- Go ----

var (
	goFunc        = regexp.MustCompile(`^func\s+([A-Za-z_]\w*)\s*[\[(]`)
	goMethod      = regexp.MustCompile(`^func\s*\(\s*(?:\w+\s+)?\*?\s*([A-Za-z_]\w*)(?:\[[^\]]*\])?\s*\)\s*([A-Za-z_]\w*)\s*[\[(]`)
	goType        = regexp.MustCompile(`^type\s+([A-Za-z_]\w*)(?:\[[^\]]*\])?\s+(=\s*)?(\S+)`)
	goTypeInBlock = regexp.MustCompile(`^\s+([A-Za-z_]\w*)(?:\[[^\]]*\])?\s+(=\s*)?(\S+)`)
	goConst       = regexp.MustCompile(`^const\s+([A-Za-z_]\w*)\b`)
	goConstInBlk  = regexp.MustCompile(`^\s+([A-Za-z_]\w*)\b`)
	goBlockStart  = regexp.MustCompile(`^(type|const)\s*\(\s*(//.*)?$`)
)

func goTypeKind(rest string) SymbolKind {
	switch {
	case strings.HasPrefix(rest, "struct"):
		return KindStruct
	case strings.HasPrefix(rest, "interface"):
		return KindInterface
	}
	return KindType
}

func extractGo(lines []string) []Symbol {
	var l symList
	block := "" // "type" | "const" inside a grouped declaration
	braces := 0 // brace depth inside a grouped type declaration
	var sc braceScanner
	for i, line := range lines {
		if block != "" {
			if braces == 0 && strings.HasPrefix(line, ")") {
				block = ""
				continue
			}
			if braces == 0 && !strings.HasPrefix(strings.TrimSpace(line), "//") {
				switch block {
				case "type":
					if m := goTypeInBlock.FindStringSubmatch(line); m != nil {
						l.add(m[1], goTypeKind(m[3]), i, "", line)
					}
				case "const":
					if m := goConstInBlk.FindStringSubmatch(line); m != nil {
						l.add(m[1], KindConst, i, "", line)
					}
				}
			}
			braces = max(braces+sc.delta(line), 0)
			continue
		}
		if m := goBlockStart.FindStringSubmatch(line); m != nil {
			block, braces = m[1], 0
			continue
		}
		if m := goMethod.FindStringSubmatch(line); m != nil {
			l.add(m[2], KindMethod, i, m[1], line)
		} else if m := goFunc.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindFunction, i, "", line)
		} else if m := goType.FindStringSubmatch(line); m != nil {
			l.add(m[1], goTypeKind(m[3]), i, "", line)
		} else if m := goConst.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindConst, i, "", line)
		}
	}
	return l.out
}

// ---- Rust ----

const rsVis = `(?:pub(?:\s*\([^)]*\))?\s+)?`

var (
	rsFn     = regexp.MustCompile(`^\s*` + rsVis + `(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+(?:"[^"]*"\s+)?)?fn\s+([A-Za-z_]\w*)`)
	rsStruct = regexp.MustCompile(`^\s*` + rsVis + `(struct|union)\s+([A-Za-z_]\w*)`)
	rsEnum   = regexp.MustCompile(`^\s*` + rsVis + `enum\s+([A-Za-z_]\w*)`)
	rsTrait  = regexp.MustCompile(`^\s*` + rsVis + `(?:unsafe\s+)?(?:auto\s+)?trait\s+([A-Za-z_]\w*)`)
	rsType   = regexp.MustCompile(`^\s*` + rsVis + `type\s+([A-Za-z_]\w*)`)
	rsConst  = regexp.MustCompile(`^\s*` + rsVis + `(?:const|static(?:\s+mut)?)\s+([A-Za-z_]\w*)\s*:`)
	rsImpl   = regexp.MustCompile(`^\s*(?:unsafe\s+)?impl\b(?:\s*<[^{]*?>)?\s+(?:[^{]*?\s+for\s+)?(?:[\w]+::)*([A-Za-z_]\w*)`)
	rsMod    = regexp.MustCompile(`^\s*` + rsVis + `mod\s+[A-Za-z_]\w*\s*\{`)
)

func extractRust(lines []string) []Symbol {
	var l symList
	braceWalk(lines, func(i int, line string, top *scope, ok bool) *scope {
		if !ok {
			return nil
		}
		container, inType := "", false
		if top != nil && top.typ {
			container, inType = top.name, true
		}
		if m := rsImpl.FindStringSubmatch(line); m != nil && !inType {
			return &scope{name: m[1], typ: true}
		}
		if rsMod.MatchString(line) && !inType {
			return &scope{}
		}
		if m := rsFn.FindStringSubmatch(line); m != nil {
			kind := KindFunction
			if inType {
				kind = KindMethod
			}
			l.add(m[1], kind, i, container, line)
			return nil
		}
		if m := rsTrait.FindStringSubmatch(line); m != nil && !inType {
			l.add(m[1], KindTrait, i, "", line)
			return &scope{name: m[1], typ: true}
		}
		if m := rsStruct.FindStringSubmatch(line); m != nil && !inType {
			l.add(m[2], KindStruct, i, "", line)
		} else if m := rsEnum.FindStringSubmatch(line); m != nil && !inType {
			l.add(m[1], KindEnum, i, "", line)
		} else if m := rsType.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindType, i, container, line)
		} else if m := rsConst.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindConst, i, container, line)
		}
		return nil
	})
	return l.out
}

// ---- TypeScript / JavaScript ----

const tsID = `([A-Za-z_$][\w$]*)`

var (
	tsFunc      = regexp.MustCompile(`^\s*(?:export\s+)?(?:default\s+)?(?:declare\s+)?(?:async\s+)?function\b\s*\*?\s*` + tsID)
	tsClass     = regexp.MustCompile(`^\s*(?:export\s+)?(?:default\s+)?(?:declare\s+)?(?:abstract\s+)?class\s+` + tsID)
	tsInterface = regexp.MustCompile(`^\s*(?:export\s+)?(?:declare\s+)?interface\s+` + tsID)
	tsType      = regexp.MustCompile(`^\s*(?:export\s+)?(?:declare\s+)?type\s+` + tsID + `\s*(?:<[^=]*>)?\s*=`)
	tsEnum      = regexp.MustCompile(`^\s*(?:export\s+)?(?:declare\s+)?(?:const\s+)?enum\s+` + tsID)
	tsArrow     = regexp.MustCompile(`^\s*(?:export\s+)?(?:const|let|var)\s+` + tsID + `\s*(?::[^=]+)?=\s*(?:async\s+)?(?:function\b|\([^)]*\)\s*(?::[^=]+)?=>|[A-Za-z_$][\w$]*\s*=>)`)
	tsConst     = regexp.MustCompile(`^\s*(?:export\s+)?const\s+` + tsID + `\s*(?::[^=]+)?=`)
	tsMethod    = regexp.MustCompile(`^\s*(?:(?:public|private|protected|static|readonly|abstract|override|async|get|set|declare)\s+)*\*?\s*(#?[A-Za-z_$][\w$]*)\s*(?:<[^>]*>)?\s*\(`)
	tsPropFn    = regexp.MustCompile(`^\s*(?:(?:public|private|protected|static|readonly|override)\s+)*(#?[A-Za-z_$][\w$]*)\s*(?::[^=]+)?=\s*(?:async\s+)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*(?::[^=]+)?=>`)
	tsNamespace = regexp.MustCompile(`^\s*(?:export\s+)?(?:declare\s+)?(?:namespace|module)\s+[\w$.]+\s*\{`)
)

var tsKeywords = map[string]bool{"if": true, "for": true, "while": true, "switch": true, "catch": true, "return": true,
	"function": true, "with": true, "do": true, "else": true, "new": true, "typeof": true, "await": true, "super": true, "this": true}

func extractTS(lines []string) []Symbol {
	var l symList
	braceWalk(lines, func(i int, line string, top *scope, ok bool) *scope {
		if !ok {
			return nil
		}
		if top != nil && top.typ {
			if m := tsPropFn.FindStringSubmatch(line); m != nil {
				l.add(m[1], KindMethod, i, top.name, line)
			} else if m := tsMethod.FindStringSubmatch(line); m != nil && !tsKeywords[m[1]] {
				l.add(m[1], KindMethod, i, top.name, line)
			}
			return nil
		}
		if tsNamespace.MatchString(line) {
			return &scope{}
		}
		if m := tsClass.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindClass, i, "", line)
			return &scope{name: m[1], typ: true}
		}
		if m := tsInterface.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindInterface, i, "", line)
		} else if m := tsEnum.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindEnum, i, "", line)
		} else if m := tsType.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindType, i, "", line)
		} else if m := tsFunc.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindFunction, i, "", line)
		} else if m := tsArrow.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindFunction, i, "", line)
		} else if m := tsConst.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindConst, i, "", line)
		}
		return nil
	})
	return l.out
}

// ---- Python ----

var (
	pyClass = regexp.MustCompile(`^(\s*)class\s+([A-Za-z_]\w*)`)
	pyDef   = regexp.MustCompile(`^(\s*)(?:async\s+)?def\s+([A-Za-z_]\w*)`)
	pyConst = regexp.MustCompile(`^([A-Z][A-Z0-9_]*)\s*(?::[^=]*)?=[^=]`)
)

func pyIndent(s string) int {
	n := 0
	for _, c := range s {
		switch c {
		case ' ':
			n++
		case '\t':
			n += 8 - n%8
		default:
			return n
		}
	}
	return n
}

func extractPython(lines []string) []Symbol {
	var l symList
	type frame struct {
		indent int
		name   string
		class  bool
	}
	var stack []frame
	inDoc := "" // inside a triple-quoted string
	for i, line := range lines {
		trim := strings.TrimSpace(line)
		if inDoc != "" {
			if strings.Contains(trim, inDoc) {
				inDoc = ""
			}
			continue
		}
		if trim == "" || strings.HasPrefix(trim, "#") {
			continue
		}
		for _, q := range []string{`"""`, `'''`} {
			if strings.HasPrefix(trim, q) && !strings.Contains(trim[3:], q) {
				inDoc = q
			}
		}
		if inDoc != "" {
			continue
		}
		ind := pyIndent(line)
		for len(stack) > 0 && ind <= stack[len(stack)-1].indent {
			stack = stack[:len(stack)-1]
		}
		var top *frame
		if len(stack) > 0 {
			top = &stack[len(stack)-1]
		}
		if m := pyClass.FindStringSubmatch(line); m != nil {
			if top == nil || top.class {
				container := ""
				if top != nil {
					container = top.name
				}
				l.add(m[2], KindClass, i, container, line)
			}
			stack = append(stack, frame{indent: ind, name: m[2], class: true})
		} else if m := pyDef.FindStringSubmatch(line); m != nil {
			switch {
			case top == nil:
				l.add(m[2], KindFunction, i, "", line)
			case top.class:
				l.add(m[2], KindMethod, i, top.name, line)
			}
			stack = append(stack, frame{indent: ind, name: m[2]})
		} else if m := pyConst.FindStringSubmatch(line); m != nil && ind == 0 {
			l.add(m[1], KindConst, i, "", line)
		}
	}
	return l.out
}

// ---- Java ----

var (
	javaType   = regexp.MustCompile(`^\s*(?:@\w+(?:\([^)]*\))?\s+)*(?:(?:public|private|protected|static|final|abstract|sealed|non-sealed|strictfp)\s+)*(class|interface|enum|record|@interface)\s+([A-Za-z_$][\w$]*)`)
	javaMethod = regexp.MustCompile(`^\s*(?:@\w+(?:\([^)]*\))?\s+)*(?:(?:public|private|protected|static|final|abstract|synchronized|native|default|strictfp)\s+)*(?:<[^>]+>\s+)?(?:[\w$.]+(?:<[^()]*>)?(?:\[\])*\s+)?([A-Za-z_$][\w$]*)\s*\(`)
	javaConst  = regexp.MustCompile(`^\s*(?:(?:public|private|protected)\s+)?(?:static\s+final|final\s+static)\s+[\w$.<>\[\], ]+?\s+([A-Za-z_$][\w$]*)\s*=`)
	// javaEnumConst is an enum constant line ("RED(1),", "GREEN;").
	javaEnumConst = regexp.MustCompile(`^\s*[A-Z][A-Z0-9_]*\s*(?:\(|,|;|\{|$)`)
)

var javaKeywords = map[string]bool{"if": true, "for": true, "while": true, "switch": true, "catch": true, "return": true,
	"new": true, "throw": true, "else": true, "synchronized": true, "super": true, "this": true, "try": true, "do": true, "assert": true}

func javaKind(k string) SymbolKind {
	switch k {
	case "interface", "@interface":
		return KindInterface
	case "enum":
		return KindEnum
	}
	return KindClass
}

func extractJava(lines []string) []Symbol {
	var l symList
	braceWalk(lines, func(i int, line string, top *scope, ok bool) *scope {
		if !ok {
			return nil
		}
		container := ""
		if top != nil {
			container = top.name
		}
		if m := javaType.FindStringSubmatch(line); m != nil {
			l.add(m[2], javaKind(m[1]), i, container, line)
			return &scope{name: m[2], typ: true, enum: m[1] == "enum"}
		}
		if top == nil {
			return nil // only types live at the top level
		}
		if top.enum && javaEnumConst.MatchString(line) {
			return nil
		}
		if m := javaConst.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindConst, i, container, line)
		} else if m := javaMethod.FindStringSubmatch(line); m != nil && !javaKeywords[m[1]] {
			l.add(m[1], KindMethod, i, container, line)
		}
		return nil
	})
	return l.out
}

// ---- C / C++ ----

var (
	cStruct    = regexp.MustCompile(`^\s*(?:typedef\s+)?(?:struct|union)\s+([A-Za-z_]\w*)\s*(?:\{|$|:[^:])`)
	cClass     = regexp.MustCompile(`^\s*(?:template\s*<[^>]*>\s*)?class\s+(?:[A-Z_][A-Z0-9_]*\s+)?([A-Za-z_]\w*)\s*(?:final\s*)?(?:\{|$|:[^:])`)
	cEnum      = regexp.MustCompile(`^\s*(?:typedef\s+)?enum\s+(?:class\s+|struct\s+)?([A-Za-z_]\w*)\s*(?::\s*[\w:]+\s*)?(?:\{|$)`)
	cTypedef   = regexp.MustCompile(`^\s*typedef\s+[^{]*?\b([A-Za-z_]\w*)\s*(?:\[[^\]]*\])?\s*;\s*$`)
	cTypedefFn = regexp.MustCompile(`^\s*typedef\s+[^{]*?\(\s*\*\s*([A-Za-z_]\w*)\s*\)`)
	cClose     = regexp.MustCompile(`^\}\s*([A-Za-z_]\w*)\s*;`)
	cUsing     = regexp.MustCompile(`^\s*using\s+([A-Za-z_]\w*)\s*=`)
	cDefine    = regexp.MustCompile(`^\s*#\s*define\s+([A-Za-z_]\w*)(\()?`)
	cConst     = regexp.MustCompile(`^\s*(?:static\s+|inline\s+)*(?:constexpr|const)\s+[\w:<>]+(?:\s*[*&])?\s+([A-Za-z_]\w*)\s*(?:=|\{)`)
	cFunc      = regexp.MustCompile(`^\s*(?:template\s*<[^>]*>\s*)?[\w:<>,*&~\s]*?[\w>*&]\s*[\s*&]((?:[A-Za-z_]\w*::)*~?[A-Za-z_]\w*)\s*\(`)
	cCtor      = regexp.MustCompile(`^\s*((?:[A-Za-z_]\w*::)+~?[A-Za-z_]\w*)\s*\(`)
	cNamespace = regexp.MustCompile(`^\s*(?:inline\s+)?(?:namespace(?:\s+[\w:]+)?|extern\s+"C"(?:\+\+)?)\s*\{`)
)

var cKeywords = map[string]bool{"if": true, "for": true, "while": true, "switch": true, "return": true, "else": true,
	"do": true, "case": true, "sizeof": true, "typedef": true, "using": true, "new": true, "delete": true, "throw": true,
	"catch": true, "static_assert": true, "decltype": true, "alignof": true, "defined": true}

// forwardDecl reports a declaration without a body ("struct node;").
func forwardDecl(trim string) bool {
	return strings.HasSuffix(trim, ";") && !strings.Contains(trim, "{")
}

var cStmtStart = regexp.MustCompile(`^\s*(?:return|else|if|for|while|switch|case|do|goto|throw|delete|new)\b`)

func extractC(lines []string) []Symbol {
	var l symList
	braceWalk(lines, func(i int, line string, top *scope, ok bool) *scope {
		trim := strings.TrimSpace(line)
		if strings.HasPrefix(trim, "#") {
			if m := cDefine.FindStringSubmatch(line); m != nil && (top == nil || !top.typ) {
				kind := KindConst
				if m[2] != "" {
					kind = KindFunction
				}
				l.add(m[1], kind, i, "", line)
			}
			return nil
		}
		// "} name;" at column 0 closes a top-level "typedef struct {".
		if m := cClose.FindStringSubmatch(line); m != nil && top == nil {
			l.add(m[1], KindType, i, "", line)
			return nil
		}
		if !ok {
			return nil
		}
		container, inType := "", false
		if top != nil && top.typ {
			container, inType = top.name, true
		}
		if cNamespace.MatchString(line) {
			return &scope{}
		}
		if m := cClass.FindStringSubmatch(line); m != nil && !forwardDecl(trim) {
			l.add(m[1], KindClass, i, container, line)
			return &scope{name: m[1], typ: true}
		}
		if m := cStruct.FindStringSubmatch(line); m != nil && !forwardDecl(trim) {
			l.add(m[1], KindStruct, i, container, line)
			return &scope{name: m[1], typ: true}
		}
		if m := cEnum.FindStringSubmatch(line); m != nil && !forwardDecl(trim) {
			l.add(m[1], KindEnum, i, container, line)
			return nil
		}
		if m := cTypedefFn.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindType, i, container, line)
			return nil
		}
		if m := cTypedef.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindType, i, container, line)
			return nil
		}
		if m := cUsing.FindStringSubmatch(line); m != nil {
			l.add(m[1], KindType, i, container, line)
			return nil
		}
		if m := cConst.FindStringSubmatch(line); m != nil && !strings.Contains(line, "(") {
			l.add(m[1], KindConst, i, container, line)
			return nil
		}
		if cStmtStart.MatchString(line) {
			return nil
		}
		// An "=" before the parameter list is an initialisation, not a
		// function ("int x = f(1);"), except for operator overloads.
		if eq, paren := strings.IndexByte(trim, '='), strings.IndexByte(trim, '('); eq >= 0 && (paren < 0 || eq < paren) &&
			!strings.Contains(trim, "operator") {
			return nil
		}
		// A prototype ("int f(int);") is a declaration; outside type
		// bodies only definitions are reported.
		proto := strings.HasSuffix(trim, ";")
		if proto && !inType {
			return nil
		}
		name := ""
		if m := cFunc.FindStringSubmatch(line); m != nil {
			name = m[1]
		} else if m := cCtor.FindStringSubmatch(line); m != nil {
			name = m[1]
		} else if inType {
			// Constructors inside a class body: "Name(args)".
			if j := strings.IndexByte(trim, '('); j > 0 && strings.TrimSpace(trim[:j]) == container {
				name = container
			}
		}
		if name == "" {
			return nil
		}
		short := name
		if k := strings.LastIndex(name, "::"); k >= 0 {
			short = name[k+2:]
			if container == "" {
				container = name[:k]
			}
		}
		if cKeywords[strings.TrimPrefix(short, "~")] {
			return nil
		}
		kind := KindFunction
		if container != "" {
			kind = KindMethod
		}
		l.add(short, kind, i, container, line)
		return nil
	})
	return l.out
}
