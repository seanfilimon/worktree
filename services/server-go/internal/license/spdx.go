package license

import (
	"fmt"
	"strings"
)

// SPDX license expressions (SPDX 2.3 Annex D), validated syntactically:
//
//	expr     = and-expr *( "OR" and-expr )
//	and-expr = with-expr *( "AND" with-expr )
//	with     = primary [ "WITH" exception-id ]
//	primary  = "(" expr ")" | license-id [ "+" ] | license-ref
//	license-ref = [ "DocumentRef-" idstring ":" ] "LicenseRef-" idstring
//	idstring = 1*( ALPHA / DIGIT / "-" / "." )
//
// Operators are case-sensitive (upper case). Identifiers are not checked
// against the SPDX license list; "Proprietary" (any case) is accepted as
// the special keyword described in LicenseCompliance.md.

const (
	maxSPDXLen   = 1024
	maxSPDXDepth = 32
)

type exprKind int

const (
	exprLicense exprKind = iota
	exprWith
	exprAnd
	exprOr
)

// expr is a parsed SPDX license expression.
type expr struct {
	kind     exprKind
	id       string // exprLicense: identifier (without '+'); exprWith: exception
	orLater  bool   // exprLicense: trailing '+'
	children []*expr
	ref      bool // exprLicense: [DocumentRef-x:]LicenseRef-y form
}

type spdxParser struct {
	toks  []string
	pos   int
	depth int
}

func tokenizeSPDX(s string) []string {
	var toks []string
	var cur strings.Builder
	flush := func() {
		if cur.Len() > 0 {
			toks = append(toks, cur.String())
			cur.Reset()
		}
	}
	for _, r := range s {
		switch r {
		case ' ', '\t', '\n', '\r':
			flush()
		case '(', ')':
			flush()
			toks = append(toks, string(r))
		default:
			cur.WriteRune(r)
		}
	}
	flush()
	return toks
}

// parseSPDX parses a license expression.
func parseSPDX(s string) (*expr, error) {
	if strings.TrimSpace(s) == "" {
		return nil, fmt.Errorf("license expression is empty")
	}
	if len(s) > maxSPDXLen {
		return nil, fmt.Errorf("license expression longer than %d bytes", maxSPDXLen)
	}
	p := &spdxParser{toks: tokenizeSPDX(s)}
	e, err := p.parseOr()
	if err != nil {
		return nil, fmt.Errorf("license expression %q: %w", s, err)
	}
	if p.pos != len(p.toks) {
		return nil, fmt.Errorf("license expression %q: unexpected %q", s, p.toks[p.pos])
	}
	return e, nil
}

func (p *spdxParser) peek() string {
	if p.pos < len(p.toks) {
		return p.toks[p.pos]
	}
	return ""
}

func (p *spdxParser) parseOr() (*expr, error) {
	return p.parseBinary("OR", exprOr, p.parseAnd)
}

func (p *spdxParser) parseAnd() (*expr, error) {
	return p.parseBinary("AND", exprAnd, p.parseWith)
}

func (p *spdxParser) parseBinary(op string, kind exprKind, operand func() (*expr, error)) (*expr, error) {
	first, err := operand()
	if err != nil {
		return nil, err
	}
	children := []*expr{first}
	for p.peek() == op {
		p.pos++
		next, err := operand()
		if err != nil {
			return nil, err
		}
		children = append(children, next)
	}
	if len(children) == 1 {
		return first, nil
	}
	return &expr{kind: kind, children: children}, nil
}

func (p *spdxParser) parseWith() (*expr, error) {
	base, err := p.parsePrimary()
	if err != nil {
		return nil, err
	}
	if p.peek() != "WITH" {
		return base, nil
	}
	if base.kind != exprLicense {
		return nil, fmt.Errorf("WITH must follow a single license identifier")
	}
	p.pos++
	exc := p.peek()
	if exc == "" {
		return nil, fmt.Errorf("missing exception identifier after WITH")
	}
	if isOperator(exc) || exc == "(" || exc == ")" || !isIDString(exc) {
		return nil, fmt.Errorf("invalid exception identifier %q", exc)
	}
	p.pos++
	return &expr{kind: exprWith, id: exc, children: []*expr{base}}, nil
}

func (p *spdxParser) parsePrimary() (*expr, error) {
	tok := p.peek()
	switch {
	case tok == "":
		return nil, fmt.Errorf("unexpected end of expression")
	case tok == "(":
		p.depth++
		if p.depth > maxSPDXDepth {
			return nil, fmt.Errorf("parentheses nested deeper than %d", maxSPDXDepth)
		}
		p.pos++
		e, err := p.parseOr()
		if err != nil {
			return nil, err
		}
		if p.peek() != ")" {
			return nil, fmt.Errorf("missing ')'")
		}
		p.pos++
		p.depth--
		return e, nil
	case tok == ")" || isOperator(tok):
		return nil, fmt.Errorf("unexpected %q", tok)
	}
	p.pos++
	return parseLicenseID(tok)
}

func isOperator(tok string) bool { return tok == "AND" || tok == "OR" || tok == "WITH" }

func parseLicenseID(tok string) (*expr, error) {
	if ref, ok := licenseRef(tok); ok {
		if !isIDString(ref) {
			return nil, fmt.Errorf("invalid license reference %q", tok)
		}
		return &expr{kind: exprLicense, id: tok, ref: true}, nil
	}
	if strings.HasPrefix(tok, "DocumentRef-") {
		return nil, fmt.Errorf("invalid license reference %q (want DocumentRef-x:LicenseRef-y)", tok)
	}
	id, orLater := strings.CutSuffix(tok, "+")
	if !isIDString(id) {
		return nil, fmt.Errorf("invalid license identifier %q", tok)
	}
	switch strings.ToLower(id) {
	case "and", "or", "with":
		return nil, fmt.Errorf("operator %q must be upper case", tok)
	}
	return &expr{kind: exprLicense, id: id, orLater: orLater}, nil
}

// licenseRef extracts the idstring of a [DocumentRef-x:]LicenseRef-y token.
func licenseRef(tok string) (string, bool) {
	if doc, rest, ok := strings.Cut(tok, ":"); ok {
		d, isDoc := strings.CutPrefix(doc, "DocumentRef-")
		if !isDoc || !isIDString(d) {
			return "", false
		}
		tok = rest
	}
	return strings.CutPrefix(tok, "LicenseRef-")
}

func isIDString(s string) bool {
	if s == "" {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9', c == '-', c == '.':
		default:
			return false
		}
	}
	return true
}

// proprietary reports whether every way of satisfying e involves a
// proprietary license: OR is proprietary only if all alternatives are, AND
// if any operand is.
func (e *expr) proprietary() bool {
	switch e.kind {
	case exprLicense:
		return isProprietaryID(e.id)
	case exprWith:
		return e.children[0].proprietary()
	case exprAnd:
		for _, c := range e.children {
			if c.proprietary() {
				return true
			}
		}
		return false
	default: // exprOr
		for _, c := range e.children {
			if !c.proprietary() {
				return false
			}
		}
		return true
	}
}

// isProprietaryID matches "Proprietary" and "LicenseRef-Proprietary"
// (any case), and any LicenseRef whose name contains "proprietary"
// (e.g. "LicenseRef-Acme-Proprietary").
func isProprietaryID(id string) bool {
	l := strings.ToLower(id)
	if l == "proprietary" {
		return true
	}
	if _, ref, ok := strings.Cut(l, "licenseref-"); ok {
		return strings.Contains(ref, "proprietary")
	}
	return false
}

// ValidateSPDX checks that s is a syntactically valid SPDX license
// expression (see the grammar above).
func ValidateSPDX(s string) error {
	_, err := parseSPDX(s)
	return err
}

// IsProprietary reports whether the SPDX expression denotes proprietary
// code (no access for non-owning tenants without a grant). Expressions that
// fail to parse are treated as proprietary (fail closed).
func IsProprietary(spdx string) bool {
	e, err := parseSPDX(spdx)
	if err != nil {
		return true
	}
	return e.proprietary()
}
