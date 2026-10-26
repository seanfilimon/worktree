package treeconfig

import (
	"fmt"
	"sort"
	"strings"
)

// Diagnostic codes. They are shared with the Rust validator
// (crates/worktree-protocol/src/config/as_code.rs, module `codes`) and
// documented in specs/dot-wt-tree/ConfigAsCode.md. Errors are E30xx,
// warnings W31xx; the access files keep the E20xx/W21xx codes of the
// declarative access compiler.
const (
	ESyntax     = "E3001" // the file is not valid TOML
	EStructure  = "E3002" // unknown key in branches/protections, or a value of the wrong type
	EMissing    = "E3003" // a required field is missing or empty
	EBranchName = "E3004" // invalid branch name
	EPattern    = "E3005" // invalid protection pattern
	EDuplicate  = "E3006" // two entries for the same branch / pattern
	EValue      = "E3007" // a value out of range or not allowed (approvals, merge method, check, pusher)
	EOwner      = "E3008" // owner is not user:NAME / team:SLUG, or (server) does not exist
	ELimit      = "E3009" // too many entries or a text that is too long
	EPermission = "E3010" // (server) the pusher may not change this configuration
	EAccess     = "E3011" // (server) the access document was refused by the server

	WManagedByAPI = "W3101" // (server) a protection pattern is managed through the API; the entry is ignored
	WNoSuchBranch = "W3102" // (server) a declared branch does not exist yet (applied when it is created)
	WSection      = "W3103" // a config.toml section that is not read at tree level
	WUnknownKey   = "W3104" // an unknown key in config.toml (ignored)

	// Access files (crates/worktree-protocol/src/access/declarative.rs).
	EAccessSyntax    = "E2001"
	EAccessStructure = "E2002"
	EAccessMissing   = "E2003"
	EAccessTreeRoles = "E2013"
	WAccessAlias     = "W2101"
)

// Severity of a diagnostic.
const (
	SeverityError   = "error"
	SeverityWarning = "warning"
)

// Diagnostic is one problem found in a .wt-tree file. Line and Col are
// 1-based (0 when the problem has no position, e.g. a server-side
// refusal of a whole document).
type Diagnostic struct {
	Code     string `json:"code"`
	Severity string `json:"severity"`
	File     string `json:"file"`
	Line     int    `json:"line"`
	Col      int    `json:"col"`
	Message  string `json:"message"`
}

// String renders "file:line:col: CODE message" (the CLI's format).
func (d Diagnostic) String() string {
	loc := d.File
	if d.Line > 0 {
		loc += fmt.Sprintf(":%d:%d", d.Line, d.Col)
	}
	return loc + ": " + d.Code + " " + d.Message
}

// IsError reports whether d is an error (not a warning).
func (d Diagnostic) IsError() bool { return d.Severity == SeverityError }

func newDiag(code, file string, pos Pos, format string, args ...any) Diagnostic {
	sev := SeverityError
	if strings.HasPrefix(code, "W") {
		sev = SeverityWarning
	}
	return Diagnostic{Code: code, Severity: sev, File: file, Line: pos.Line, Col: pos.Col, Message: fmt.Sprintf(format, args...)}
}

// diags collects diagnostics for one file.
type diags struct {
	file string
	list []Diagnostic
}

func (d *diags) add(code string, pos Pos, format string, args ...any) {
	d.list = append(d.list, newDiag(code, d.file, pos, format, args...))
}

// sortDiagnostics orders by file, line, column, code.
func sortDiagnostics(ds []Diagnostic) {
	sort.SliceStable(ds, func(i, j int) bool {
		a, b := ds[i], ds[j]
		if a.File != b.File {
			return a.File < b.File
		}
		if a.Line != b.Line {
			return a.Line < b.Line
		}
		if a.Col != b.Col {
			return a.Col < b.Col
		}
		return a.Code < b.Code
	})
}

// Errors returns the errors of ds.
func Errors(ds []Diagnostic) []Diagnostic {
	var out []Diagnostic
	for _, d := range ds {
		if d.IsError() {
			out = append(out, d)
		}
	}
	return out
}

// Warnings returns the warnings of ds.
func Warnings(ds []Diagnostic) []Diagnostic {
	var out []Diagnostic
	for _, d := range ds {
		if !d.IsError() {
			out = append(out, d)
		}
	}
	return out
}

// maxReported bounds the diagnostics quoted in a push rejection.
const maxReported = 10

// summarize renders errors for a push rejection message.
func summarize(errs []Diagnostic) string {
	parts := make([]string, 0, min(len(errs), maxReported)+1)
	for i, d := range errs {
		if i == maxReported {
			parts = append(parts, fmt.Sprintf("... and %d more", len(errs)-maxReported))
			break
		}
		parts = append(parts, d.String())
	}
	return strings.Join(parts, "; ")
}
