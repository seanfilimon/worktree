package treeconfig

import (
	"encoding/json"
	"path"
	"regexp"
	"slices"
	"sort"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// Tree-relative paths of the config-as-code files (§16.3).
const (
	Dir             = ".wt-tree"
	ConfigFile      = Dir + "/config.toml"
	BranchesFile    = Dir + "/branches.toml"
	ProtectionsFile = Dir + "/protections.toml"
	AccessDir       = Dir + "/access/"
	PoliciesFile    = AccessDir + "policies.toml"
	RolesFile       = AccessDir + "roles.toml"
)

// Limits (the same in the Rust validator).
const (
	MaxFileSize       = 256 << 10
	MaxBranches       = 500
	MaxProtections    = 100
	MaxDescription    = 1000 // characters
	MaxPushers        = 100
	MaxRequiredChecks = 50
	MaxContextLen     = 255
	MaxApprovals      = 100
	maxRefLen         = 255
	maxPatternLen     = 512
)

// IsConfigPath reports whether a tree-relative path is one of the
// config-as-code files the server validates and applies.
func IsConfigPath(p string) bool {
	switch p {
	case ConfigFile, BranchesFile, ProtectionsFile:
		return true
	}
	return strings.HasPrefix(p, AccessDir) && len(p) > len(AccessDir)
}

// isAccessPath reports whether p is one of the access document files
// (access/policies.toml, or the refused access/roles.toml). Other files
// below access/ (notes, READMEs) neither trigger nor change an apply.
func isAccessPath(p string) bool { return p == PoliciesFile || p == RolesFile }

// BranchDecl is one [[branch]] entry of branches.toml.
type BranchDecl struct {
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
	Owner       string `json:"owner,omitempty"`
}

// Parsed is the validated content of a set of .wt-tree files.
type Parsed struct {
	// Branches / Protections are nil when the file is absent.
	Branches    []BranchDecl              `json:"branches"`
	Protections []domain.BranchProtection `json:"protections"`
	// Access is the canonical JSON access document compiled from
	// access/policies.toml (nil when absent).
	Access []byte `json:"-"`
	// Diagnostics holds every error and warning, sorted.
	Diagnostics []Diagnostic `json:"diagnostics"`

	hasBranches, hasProtections, hasAccess bool
	// positions for server-side checks.
	branchPos     map[string]Pos // name key position by branch name
	ownerPos      map[string]Pos // owner key position by branch name
	protectionPos map[string]Pos // pattern position
	policyPos     []Pos          // [[policy]] header positions
	statementPos  [][]Pos        // [[policy.statement]] header positions
}

// HasErrors reports whether the files have at least one error.
func (p *Parsed) HasErrors() bool { return len(Errors(p.Diagnostics)) > 0 }

// Validate parses and validates the config-as-code files of files (tree-
// relative path → content; files that are not config paths are ignored).
// It performs every check that needs no server state; the server adds
// reference checks (owners) and the access document checks.
func Validate(files map[string][]byte) *Parsed {
	out := &Parsed{branchPos: map[string]Pos{}, ownerPos: map[string]Pos{}, protectionPos: map[string]Pos{}}
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	for _, p := range paths {
		if !IsConfigPath(p) {
			continue
		}
		src := files[p]
		d := &diags{file: p}
		if len(src) > MaxFileSize {
			d.add(ELimit, Pos{1, 1}, "the file is larger than %d bytes", MaxFileSize)
			out.Diagnostics = append(out.Diagnostics, d.list...)
			continue
		}
		switch {
		case p == ConfigFile:
			validateConfig(src, d)
		case p == BranchesFile:
			out.hasBranches = true
			out.Branches = validateBranches(src, d, out.branchPos, out.ownerPos)
		case p == ProtectionsFile:
			out.hasProtections = true
			out.Protections = validateProtections(src, d, out.protectionPos)
		case p == PoliciesFile:
			out.hasAccess = true
			out.Access, out.policyPos, out.statementPos = compilePolicies(src, d)
		case p == RolesFile:
			d.add(EAccessTreeRoles, Pos{1, 1}, "tree-level access files cannot define roles; define them in .wt/access/roles.toml")
		}
		out.Diagnostics = append(out.Diagnostics, d.list...)
	}
	sortDiagnostics(out.Diagnostics)
	if out.Diagnostics == nil {
		out.Diagnostics = []Diagnostic{}
	}
	return out
}

// parseFile parses src, reporting a syntax error with code.
func parseFile(src []byte, d *diags, code string) *Table {
	t, err := ParseTOML(src)
	if err != nil {
		se, _ := err.(*SyntaxError)
		if se == nil {
			se = &SyntaxError{Pos: Pos{1, 1}, Msg: err.Error()}
		}
		d.add(code, se.Pos, "%s", se.Msg)
		return nil
	}
	return t
}

// ---- typed accessors (shared shape with the Rust validator) ----

type reader struct {
	d         *diags
	structure string // code for type errors
}

func (r reader) unknownKeys(t *Table, allowed []string, where string) {
	for _, k := range t.Keys {
		if !slices.Contains(allowed, k) {
			r.d.add(r.structure, t.M[k].KeyPos, "unknown key '%s'%s (expected one of: %s)", k, where, strings.Join(allowed, ", "))
		}
	}
}

func (r reader) str(t *Table, key string) (string, *Value) {
	v := t.Get(key)
	if v == nil {
		return "", nil
	}
	if v.Kind != KindString {
		r.d.add(r.structure, v.KeyPos, "%s must be a string, found %s", key, v.Kind.typeName())
		return "", nil
	}
	return strings.TrimSpace(v.Str), v
}

func (r reader) boolean(t *Table, key string) (bool, *Value) {
	v := t.Get(key)
	if v == nil {
		return false, nil
	}
	if v.Kind != KindBool {
		r.d.add(r.structure, v.KeyPos, "%s must be true or false, found %s", key, v.Kind.typeName())
		return false, nil
	}
	return v.Bool, v
}

func (r reader) integer(t *Table, key string) (int64, *Value) {
	v := t.Get(key)
	if v == nil {
		return 0, nil
	}
	if v.Kind != KindInteger {
		r.d.add(r.structure, v.KeyPos, "%s must be an integer, found %s", key, v.Kind.typeName())
		return 0, nil
	}
	return v.Int, v
}

// strings reads an array of strings (a single string is accepted).
func (r reader) strings(t *Table, key string) ([]string, []*Value) {
	v := t.Get(key)
	if v == nil {
		return nil, nil
	}
	switch v.Kind {
	case KindString:
		return []string{strings.TrimSpace(v.Str)}, []*Value{v}
	case KindArray:
		out, vals := []string{}, []*Value{}
		for _, e := range v.Arr {
			if e.Kind != KindString {
				r.d.add(r.structure, e.Pos, "%s entries must be strings, found %s", key, e.Kind.typeName())
				continue
			}
			out = append(out, strings.TrimSpace(e.Str))
			vals = append(vals, e)
		}
		return out, vals
	}
	r.d.add(r.structure, v.KeyPos, "%s must be an array of strings, found %s", key, v.Kind.typeName())
	return nil, nil
}

// tables reads an array of tables ([[key]] or key = [{...}]).
func (r reader) tables(t *Table, key string) []*Value {
	v := t.Get(key)
	if v == nil {
		return nil
	}
	if v.Kind != KindArray {
		r.d.add(r.structure, v.KeyPos, "%s must be an array of tables ([[%s]]), found %s", key, key, v.Kind.typeName())
		return nil
	}
	var out []*Value
	for _, e := range v.Arr {
		if e.Kind != KindTable {
			r.d.add(r.structure, e.Pos, "%s entries must be tables, found %s", key, e.Kind.typeName())
			continue
		}
		out = append(out, e)
	}
	return out
}

func charLen(s string) int { return utf8.RuneCountInString(s) }

// ---- config.toml ----

// configSchema lists the keys read from the tree-level config.toml
// sections and their kinds ("bool", "uint", "string", "size", "strings",
// "patterns", "enum:...").
var configSchema = map[string]map[string]string{
	"auto_snapshot": {
		"enabled": "bool", "inactivity_secs": "uint", "inactivity_timeout_secs": "uint",
		"max_changed_files": "uint", "max_changed_bytes": "size", "message_format": "string",
		"on_branch_switch": "enum:refuse,snapshot,stash",
	},
	"secret_scanning": {"enabled": "bool", "block_on_match": "bool", "exclude": "strings", "patterns": "patterns"},
	"hooks":           {"enabled": "bool", "timeout_secs": "uint"},
}

// legacyConfigKeys are top-level identity keys of pre-v0.7 markers
// (migrated to local.toml by the client; ignored here).
var legacyConfigKeys = []string{"name", "id"}

var sizeRe = regexp.MustCompile(`(?i)^\s*\d+(\.\d+)?\s*(b|kb|kib|mb|mib|gb|gib|tb|tib|k|m|g|t)?\s*$`)

func validateConfig(src []byte, d *diags) {
	t := parseFile(src, d, ESyntax)
	if t == nil {
		return
	}
	sections := make([]string, 0, len(configSchema))
	for s := range configSchema {
		sections = append(sections, s)
	}
	sort.Strings(sections)
	for _, k := range t.Keys {
		v := t.M[k]
		schema, known := configSchema[k]
		switch {
		case slices.Contains(legacyConfigKeys, k) && v.Kind != KindTable:
			continue
		case !known && v.Kind == KindTable:
			d.add(WSection, v.KeyPos, "[%s] is not read at tree level (allowed: %s)", k, strings.Join(sections, ", "))
			continue
		case !known:
			d.add(WUnknownKey, v.KeyPos, "unknown key '%s' (ignored)", k)
			continue
		case v.Kind != KindTable:
			d.add(EStructure, v.KeyPos, "%s must be a table ([%s]), found %s", k, k, v.Kind.typeName())
			continue
		}
		for _, key := range v.Tbl.Keys {
			kv := v.Tbl.M[key]
			kind, ok := schema[key]
			if !ok {
				d.add(WUnknownKey, kv.KeyPos, "unknown key '%s.%s' (ignored)", k, key)
				continue
			}
			checkConfigValue(d, k+"."+key, kind, kv)
		}
	}
}

func checkConfigValue(d *diags, key, kind string, v *Value) {
	bad := func(want string) {
		d.add(EStructure, v.KeyPos, "%s must be %s, found %s", key, want, v.Kind.typeName())
	}
	switch {
	case kind == "bool":
		if v.Kind != KindBool {
			bad("true or false")
		}
	case kind == "uint":
		if v.Kind != KindInteger {
			bad("an integer")
		} else if v.Int < 0 {
			d.add(EValue, v.KeyPos, "%s must not be negative", key)
		}
	case kind == "string":
		if v.Kind != KindString {
			bad("a string")
		}
	case kind == "size":
		switch v.Kind {
		case KindInteger:
			if v.Int < 0 {
				d.add(EValue, v.KeyPos, "%s must not be negative", key)
			}
		case KindString:
			if !sizeRe.MatchString(v.Str) {
				d.add(EValue, v.KeyPos, "%s: %q is not a size (e.g. 50MB)", key, v.Str)
			}
		default:
			bad("a size (integer or string such as \"50MB\")")
		}
	case strings.HasPrefix(kind, "enum:"):
		allowed := strings.Split(strings.TrimPrefix(kind, "enum:"), ",")
		if v.Kind != KindString {
			bad("a string")
		} else if !slices.Contains(allowed, v.Str) {
			d.add(EValue, v.KeyPos, "%s must be one of %s, got %q", key, strings.Join(allowed, ", "), v.Str)
		}
	case kind == "strings":
		if v.Kind != KindArray {
			bad("an array of strings")
			return
		}
		for _, e := range v.Arr {
			if e.Kind != KindString {
				d.add(EStructure, e.Pos, "%s entries must be strings, found %s", key, e.Kind.typeName())
			}
		}
	case kind == "patterns":
		if v.Kind != KindArray {
			bad("an array of tables")
			return
		}
		for _, e := range v.Arr {
			if e.Kind != KindTable {
				d.add(EStructure, e.Pos, "%s entries must be tables, found %s", key, e.Kind.typeName())
				continue
			}
			r := reader{d: d, structure: EStructure}
			r.unknownKeys(e.Tbl, []string{"name", "regex"}, " in "+key)
			for _, f := range []string{"name", "regex"} {
				if s, fv := r.str(e.Tbl, f); e.Tbl.Get(f) == nil || (fv != nil && s == "") {
					d.add(EMissing, e.Pos, "%s entries need a non-empty %s", key, f)
				}
			}
		}
	}
}

// ---- branches.toml ----

var (
	userOwnerRe = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{0,38}$`)
	teamOwnerRe = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{0,62}$`)
)

// checkBranchName mirrors the server's ref-name rules (syncsvc).
func checkBranchName(name string) string {
	switch {
	case name == "":
		return "branch name is empty"
	case len(name) > maxRefLen:
		return "branch name is longer than 255 bytes"
	case strings.ContainsFunc(name, func(r rune) bool {
		return unicode.IsSpace(r) || unicode.IsControl(r) || strings.ContainsRune(`~^:?*[\`, r)
	}):
		return "branch name contains whitespace, a control character or one of ~^:?*[\\"
	}
	for _, c := range strings.Split(name, "/") {
		if c == "" || c == "." || c == ".." {
			return "branch name has an empty or relative path component"
		}
	}
	return ""
}

// CheckOwner validates the syntax of a branch owner ("user:NAME" or
// "team:SLUG"); "" means valid.
func CheckOwner(owner string) string {
	kind, name, ok := strings.Cut(owner, ":")
	switch {
	case !ok:
		return "owner must be user:NAME or team:SLUG"
	case kind == "user" && userOwnerRe.MatchString(name):
		return ""
	case kind == "team" && teamOwnerRe.MatchString(name):
		return ""
	case kind == "user" || kind == "team":
		return "owner " + strings.TrimSpace(owner) + " has an invalid name"
	}
	return "owner must be user:NAME or team:SLUG"
}

func validateBranches(src []byte, d *diags, namePos, ownerPos map[string]Pos) []BranchDecl {
	out := []BranchDecl{}
	t := parseFile(src, d, ESyntax)
	if t == nil {
		return out
	}
	r := reader{d: d, structure: EStructure}
	r.unknownKeys(t, []string{"branch"}, "")
	entries := r.tables(t, "branch")
	if len(entries) > MaxBranches {
		d.add(ELimit, Pos{1, 1}, "at most %d branches may be declared", MaxBranches)
	}
	seen := map[string]bool{}
	for _, e := range entries {
		tb := e.Tbl
		r.unknownKeys(tb, []string{"name", "description", "owner"}, " in [[branch]]")
		name, nv := r.str(tb, "name")
		switch {
		case nv == nil && tb.Get("name") == nil:
			d.add(EMissing, e.Pos, "[[branch]] needs a name")
			continue
		case nv == nil:
			continue
		}
		if msg := checkBranchName(name); msg != "" {
			d.add(EBranchName, nv.KeyPos, "%s", msg)
			continue
		}
		if seen[name] {
			d.add(EDuplicate, nv.KeyPos, "branch %s is declared more than once", name)
			continue
		}
		seen[name] = true
		namePos[name] = nv.KeyPos
		decl := BranchDecl{Name: name}
		if desc, dv := r.str(tb, "description"); dv != nil {
			if charLen(desc) > MaxDescription {
				d.add(ELimit, dv.KeyPos, "description must be at most %d characters", MaxDescription)
			}
			decl.Description = desc
		}
		if owner, ov := r.str(tb, "owner"); ov != nil && owner != "" {
			if msg := CheckOwner(owner); msg != "" {
				d.add(EOwner, ov.KeyPos, "%s", msg)
			}
			decl.Owner = owner
			ownerPos[name] = ov.KeyPos
		}
		out = append(out, decl)
	}
	return out
}

// ---- protections.toml ----

var protectionKeys = []string{
	"pattern", "no_direct_push", "required_approvals", "allowed_pushers", "no_delete",
	"require_ci_pass", "required_checks", "require_signed", "require_linear", "block_on_todos",
	"merge_methods", "no_force_push", "require_owner_approval",
}

// checkPattern mirrors review.ValidatePattern and the REST API's glob
// check; "" means valid.
func checkPattern(p string) string {
	if p == "" {
		return "pattern is empty"
	}
	if len(p) > maxPatternLen {
		return "pattern is longer than 512 bytes"
	}
	if strings.ContainsFunc(p, unicode.IsControl) {
		return "pattern contains a control character"
	}
	if review.ValidatePattern(p) != nil {
		return "pattern " + p + " is malformed (empty segment or bad glob)"
	}
	if _, err := path.Match(p, ""); err != nil {
		return "pattern " + p + " is not a valid glob"
	}
	return ""
}

func validateProtections(src []byte, d *diags, patternPos map[string]Pos) []domain.BranchProtection {
	out := []domain.BranchProtection{}
	t := parseFile(src, d, ESyntax)
	if t == nil {
		return out
	}
	r := reader{d: d, structure: EStructure}
	r.unknownKeys(t, []string{"protection"}, "")
	entries := r.tables(t, "protection")
	if len(entries) > MaxProtections {
		d.add(ELimit, Pos{1, 1}, "at most %d protections may be declared", MaxProtections)
	}
	seen := map[string]bool{}
	for _, e := range entries {
		tb := e.Tbl
		r.unknownKeys(tb, protectionKeys, " in [[protection]]")
		pattern, pv := r.str(tb, "pattern")
		switch {
		case pv == nil && tb.Get("pattern") == nil:
			d.add(EMissing, e.Pos, "[[protection]] needs a pattern")
			continue
		case pv == nil:
			continue
		}
		if msg := checkPattern(pattern); msg != "" {
			d.add(EPattern, pv.KeyPos, "%s", msg)
			continue
		}
		if seen[pattern] {
			d.add(EDuplicate, pv.KeyPos, "pattern %s is declared more than once", pattern)
			continue
		}
		seen[pattern] = true
		patternPos[pattern] = pv.KeyPos
		p := domain.BranchProtection{Pattern: pattern, AllowedPushers: []string{}}
		p.NoDirectPush, _ = r.boolean(tb, "no_direct_push")
		p.NoDelete, _ = r.boolean(tb, "no_delete")
		p.RequireCIPass, _ = r.boolean(tb, "require_ci_pass")
		p.RequireSigned, _ = r.boolean(tb, "require_signed")
		p.RequireLinear, _ = r.boolean(tb, "require_linear")
		p.BlockOnTodos, _ = r.boolean(tb, "block_on_todos")
		p.NoForcePush, _ = r.boolean(tb, "no_force_push")
		p.RequireOwnerApproval, _ = r.boolean(tb, "require_owner_approval")
		if n, nv := r.integer(tb, "required_approvals"); nv != nil {
			if n < 0 || n > MaxApprovals {
				d.add(EValue, nv.KeyPos, "required_approvals must be between 0 and %d", MaxApprovals)
			} else {
				p.RequiredApprovals = int(n)
			}
		}
		pushers, pvals := r.strings(tb, "allowed_pushers")
		if len(pushers) > MaxPushers {
			d.add(ELimit, tb.Get("allowed_pushers").KeyPos, "allowed_pushers lists at most %d users", MaxPushers)
		}
		for i, u := range pushers {
			if u == "" || len(u) > 100 || strings.ContainsFunc(u, unicode.IsSpace) {
				d.add(EValue, pvals[i].Pos, "allowed_pushers entries must be usernames")
				continue
			}
			if !slices.Contains(p.AllowedPushers, u) {
				p.AllowedPushers = append(p.AllowedPushers, u)
			}
		}
		checks, cvals := r.strings(tb, "required_checks")
		if len(checks) > MaxRequiredChecks {
			d.add(ELimit, tb.Get("required_checks").KeyPos, "required_checks lists at most %d contexts", MaxRequiredChecks)
		}
		for i, c := range checks {
			if c == "" || len(c) > MaxContextLen || strings.ContainsFunc(c, unicode.IsControl) {
				d.add(EValue, cvals[i].Pos, "required_checks entries must be 1-%d bytes of printable text", MaxContextLen)
				continue
			}
			if !slices.Contains(p.RequiredChecks, c) {
				p.RequiredChecks = append(p.RequiredChecks, c)
			}
		}
		methods, mvals := r.strings(tb, "merge_methods")
		var ok []string
		for i, m := range methods {
			if !review.ValidMergeMethod(m) {
				d.add(EValue, mvals[i].Pos, "merge method %q is not one of %s", m, strings.Join(review.AllMergeMethods, ", "))
				continue
			}
			ok = append(ok, m)
		}
		if len(ok) > 0 {
			p.MergeMethods, _ = review.NormalizeMergeMethods(ok)
		}
		out = append(out, p)
	}
	return out
}

// ---- access/policies.toml → canonical access document ----

// accessAliases are the permission aliases of the declarative compiler.
var accessAliases = map[string]string{
	"sync:push":       "branch:push",
	"snapshot:create": "tree:write",
	"branch:merge":    "mr:merge",
	"tree:delete":     "tree:admin",
}

type accessDoc struct {
	Roles    []domain.CustomRole `json:"roles"`
	Policies []accessPolicy      `json:"policies"`
}

type accessPolicy struct {
	Name        string            `json:"name"`
	Description string            `json:"description"`
	Enabled     bool              `json:"enabled"`
	Statements  []accessStatement `json:"statements"`
}

type accessStatement struct {
	Sid        string           `json:"sid,omitempty"`
	Effect     string           `json:"effect"`
	Actions    []string         `json:"actions"`
	Resources  []string         `json:"resources"`
	Principals []string         `json:"principals"`
	Conditions accessConditions `json:"conditions"`
}

type accessConditions struct {
	IPRanges   []string           `json:"ip_ranges,omitempty"`
	RequireMFA bool               `json:"require_mfa,omitempty"`
	NotBefore  string             `json:"not_before,omitempty"`
	NotAfter   string             `json:"not_after,omitempty"`
	TimeWindow *domain.TimeWindow `json:"time_window,omitempty"`
}

// compilePolicies parses a tree-level policies.toml into the canonical JSON
// document accesscfg applies. Structural problems use the E20xx codes of
// the Rust compiler; semantic checks are left to accesscfg (the server
// rejects them with EAccess). It returns nil JSON when there are errors.
func compilePolicies(src []byte, d *diags) ([]byte, []Pos, [][]Pos) {
	t := parseFile(src, d, EAccessSyntax)
	if t == nil {
		return nil, nil, nil
	}
	before := len(Errors(d.list))
	r := reader{d: d, structure: EAccessStructure}
	r.unknownKeys(t, []string{"policy"}, "")
	doc := accessDoc{Roles: []domain.CustomRole{}, Policies: []accessPolicy{}}
	var policyPos []Pos
	var stmtPos [][]Pos
	for _, e := range r.tables(t, "policy") {
		tb := e.Tbl
		r.unknownKeys(tb, []string{"name", "description", "enabled", "statement"}, " in [[policy]]")
		name, _ := r.str(tb, "name")
		desc, _ := r.str(tb, "description")
		enabled := true
		if b, bv := r.boolean(tb, "enabled"); bv != nil {
			enabled = b
		}
		pol := accessPolicy{Name: name, Description: desc, Enabled: enabled, Statements: []accessStatement{}}
		var sp []Pos
		for _, se := range r.tables(tb, "statement") {
			pol.Statements = append(pol.Statements, compileStatement(r, se.Tbl))
			sp = append(sp, se.Pos)
		}
		doc.Policies = append(doc.Policies, pol)
		policyPos = append(policyPos, e.Pos)
		stmtPos = append(stmtPos, sp)
	}
	if len(Errors(d.list)) > before {
		return nil, policyPos, stmtPos
	}
	raw, err := json.Marshal(doc)
	if err != nil {
		d.add(EAccessStructure, Pos{1, 1}, "cannot encode the access document: %v", err)
		return nil, policyPos, stmtPos
	}
	return raw, policyPos, stmtPos
}

func compileStatement(r reader, t *Table) accessStatement {
	r.unknownKeys(t, []string{"sid", "effect", "principals", "actions", "resources", "conditions"}, " in [[policy.statement]]")
	st := accessStatement{Actions: []string{}, Resources: []string{}, Principals: []string{}}
	st.Sid, _ = r.str(t, "sid")
	effect, _ := r.str(t, "effect")
	st.Effect = strings.ToLower(effect)
	actions, avals := r.strings(t, "actions")
	for i, a := range actions {
		if canon, ok := accessAliases[a]; ok {
			r.d.add(WAccessAlias, avals[i].Pos, "'%s' is an alias; translated to '%s'", a, canon)
			a = canon
		}
		st.Actions = append(st.Actions, a)
	}
	if res, _ := r.strings(t, "resources"); res != nil {
		st.Resources = res
	}
	if pr, _ := r.strings(t, "principals"); pr != nil {
		st.Principals = pr
	}
	if c := t.Get("conditions"); c != nil {
		if c.Kind != KindTable {
			r.d.add(EAccessStructure, c.KeyPos, "conditions must be a table, found %s", c.Kind.typeName())
		} else {
			st.Conditions = compileConditions(r, c.Tbl)
		}
	}
	return st
}

func compileConditions(r reader, t *Table) accessConditions {
	r.unknownKeys(t, []string{"mfa", "require_mfa", "ip_ranges", "not_before", "not_after", "time_window"}, " in conditions")
	var c accessConditions
	mfa, _ := r.boolean(t, "mfa")
	req, _ := r.boolean(t, "require_mfa")
	c.RequireMFA = mfa || req
	c.IPRanges, _ = r.strings(t, "ip_ranges")
	timeOf := func(key string) string {
		v := t.Get(key)
		if v == nil {
			return ""
		}
		var s string
		switch v.Kind {
		case KindString:
			s = strings.TrimSpace(v.Str)
		case KindDatetime:
			s = v.Str
			if len(s) > 10 && s[10] == ' ' {
				s = s[:10] + "T" + s[11:]
			}
		default:
			r.d.add(EAccessStructure, v.KeyPos, "%s must be an RFC 3339 date-time, found %s", key, v.Kind.typeName())
			return ""
		}
		if _, err := time.Parse(time.RFC3339, s); err != nil {
			r.d.add("E2011", v.KeyPos, "\"%s\" is not an RFC 3339 date-time with a time zone", s)
			return ""
		}
		return s
	}
	c.NotBefore = timeOf("not_before")
	c.NotAfter = timeOf("not_after")
	if w := t.Get("time_window"); w != nil {
		if w.Kind != KindTable {
			r.d.add(EAccessStructure, w.KeyPos, "time_window must be a table, found %s", w.Kind.typeName())
		} else {
			r.unknownKeys(w.Tbl, []string{"days", "start", "end", "timezone"}, " in time_window")
			tw := &domain.TimeWindow{Days: []string{}}
			if days, _ := r.strings(w.Tbl, "days"); days != nil {
				tw.Days = days
			}
			tw.Start, _ = r.str(w.Tbl, "start")
			tw.End, _ = r.str(w.Tbl, "end")
			tw.Timezone, _ = r.str(w.Tbl, "timezone")
			c.TimeWindow = tw
		}
	}
	return c
}

// accessErrorRe extracts the policy / statement indexes of an accesscfg
// error message ("policies[0] (deny-x): statements[1].actions: ...").
var accessErrorRe = regexp.MustCompile(`^policies\[(\d+)\](?: \([^)]*\))?:(?: statements\[(\d+)\])?`)

// locateAccessError positions an accesscfg error at the [[policy]] or
// [[policy.statement]] header it refers to (1:1 otherwise).
func (p *Parsed) locateAccessError(msg string) Pos {
	m := accessErrorRe.FindStringSubmatch(msg)
	if m == nil {
		return Pos{1, 1}
	}
	i := atoi(m[1])
	if i < 0 || i >= len(p.policyPos) {
		return Pos{1, 1}
	}
	if m[2] != "" {
		if j := atoi(m[2]); j >= 0 && j < len(p.statementPos[i]) {
			return p.statementPos[i][j]
		}
	}
	return p.policyPos[i]
}

func atoi(s string) int {
	n := 0
	for _, c := range s {
		if c < '0' || c > '9' || n > 1<<20 {
			return -1
		}
		n = n*10 + int(c-'0')
	}
	return n
}
