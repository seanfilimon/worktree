package scan

import (
	"bufio"
	"bytes"
	"context"
	_ "embed"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
)

// License audit (GET {tree}/compliance/licenses): which license governs
// every file of a snapshot, which files no rule covers, which proprietary
// files other tenants can reach through grants, and a strict check of every
// rule's SPDX expression against the embedded SPDX License List (spdx.txt).

//go:embed spdx.txt
var spdxTxt []byte

// spdxList is the parsed SPDX License List.
type spdxList struct {
	version    string
	licenses   map[string]string // lower-case id -> canonical id
	deprecated map[string]string
	exceptions map[string]string
}

var spdx = parseSPDXList(spdxTxt)

func parseSPDXList(data []byte) *spdxList {
	l := &spdxList{licenses: map[string]string{}, deprecated: map[string]string{}, exceptions: map[string]string{}}
	sc := bufio.NewScanner(bytes.NewReader(data))
	for sc.Scan() {
		line := strings.TrimSpace(sc.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		kind, id, ok := strings.Cut(line, " ")
		if !ok {
			continue
		}
		switch kind {
		case "version":
			l.version = id
		case "L":
			l.licenses[strings.ToLower(id)] = id
		case "D":
			l.deprecated[strings.ToLower(id)] = id
		case "E":
			l.exceptions[strings.ToLower(id)] = id
		}
	}
	return l
}

// SPDXListVersion is the version of the embedded SPDX License List.
func SPDXListVersion() string { return spdx.version }

// StrictSPDX checks an SPDX expression strictly: it must parse
// (license.ValidateSPDX) and every license identifier must be on the SPDX
// License List (or be a LicenseRef-/DocumentRef- reference, or W0rkTree's
// "Proprietary" keyword), written in its canonical case and not deprecated;
// exceptions after WITH must be listed exceptions. It returns the problems
// found (none = valid).
func StrictSPDX(expr string) []string {
	if err := license.ValidateSPDX(expr); err != nil {
		return []string{err.Error()}
	}
	var problems []string
	toks := strings.FieldsFunc(expr, func(r rune) bool {
		return r == ' ' || r == '\t' || r == '\n' || r == '\r' || r == '(' || r == ')'
	})
	afterWith := false
	for _, tok := range toks {
		switch tok {
		case "AND", "OR":
			continue
		case "WITH":
			afterWith = true
			continue
		}
		if afterWith {
			afterWith = false
			canon, ok := spdx.exceptions[strings.ToLower(tok)]
			switch {
			case !ok:
				problems = append(problems, fmt.Sprintf("%q is not an exception on the SPDX License List %s", tok, spdx.version))
			case canon != tok:
				problems = append(problems, fmt.Sprintf("%q should be written %q", tok, canon))
			}
			continue
		}
		if strings.HasPrefix(tok, "LicenseRef-") || strings.HasPrefix(tok, "DocumentRef-") || strings.EqualFold(tok, "Proprietary") {
			continue
		}
		id := strings.TrimSuffix(tok, "+")
		lower := strings.ToLower(id)
		if canon, ok := spdx.licenses[lower]; ok {
			if canon != id {
				problems = append(problems, fmt.Sprintf("%q should be written %q", id, canon))
			}
			continue
		}
		if canon, ok := spdx.deprecated[strings.ToLower(tok)]; ok {
			problems = append(problems, fmt.Sprintf("%q is deprecated on the SPDX License List (use the -only / -or-later form or a WITH exception)", canon))
			continue
		}
		if canon, ok := spdx.deprecated[lower]; ok {
			problems = append(problems, fmt.Sprintf("%q is deprecated on the SPDX License List", canon))
			continue
		}
		problems = append(problems, fmt.Sprintf("%q is not on the SPDX License List %s (use LicenseRef-... for custom licenses)", id, spdx.version))
	}
	return problems
}

// Audit limits.
const (
	maxAuditList = 1000 // entries per path list
)

// LicenseUsage is how many files one SPDX expression governs.
type LicenseUsage struct {
	SPDX        string   `json:"spdx"`
	Files       int      `json:"files"`
	Proprietary bool     `json:"proprietary"`
	Globs       []string `json:"globs"` // the rules assigning it
}

// ExposedPath is a proprietary file another tenant may read.
type ExposedPath struct {
	Path string `json:"path"`
	SPDX string `json:"spdx"`
	// Grantee is the tenant slug the granting rule names ("" = every
	// tenant that does not own the tree).
	Grantee   string     `json:"grantee"`
	Grant     string     `json:"grant"`
	Glob      string     `json:"glob"`
	ExpiresAt *time.Time `json:"expires_at,omitempty"`
}

// RuleCheck is the strict check of one license rule.
type RuleCheck struct {
	PathGlob  string     `json:"path_glob"`
	SPDX      string     `json:"spdx"`
	Grant     string     `json:"grant"`
	Grantee   string     `json:"grantee"`
	ExpiresAt *time.Time `json:"expires_at,omitempty"`
	Expired   bool       `json:"expired"`
	Files     int        `json:"files"` // files the rule governs
	Valid     bool       `json:"valid"`
	Problems  []string   `json:"problems"`
}

// LicenseAudit is the body of GET {tree}/compliance/licenses.
type LicenseAudit struct {
	TreeID          string         `json:"tree_id"`
	Ref             string         `json:"ref"`
	SnapshotID      string         `json:"snapshot_id"`
	Files           int            `json:"files"`
	Licenses        []LicenseUsage `json:"licenses"`
	Unlicensed      []string       `json:"unlicensed"`
	UnlicensedCount int            `json:"unlicensed_count"`
	Exposed         []ExposedPath  `json:"exposed"`
	ExposedCount    int            `json:"exposed_count"`
	Rules           []RuleCheck    `json:"rules"`
	InvalidRules    int            `json:"invalid_rules"`
	SPDXListVersion string         `json:"spdx_list_version"`
	// Truncated: a path list was cut at 1000 entries (the counts are exact).
	Truncated bool `json:"truncated"`
}

// ResolveRef resolves "" (the default branch), a branch, a tag or a
// snapshot id of tree to a snapshot id. ErrNotFound when unknown.
func (s *Service) ResolveRef(ctx context.Context, tree *domain.Tree, ref string) (string, error) {
	name := ref
	if name == "" {
		name = tree.DefaultBranch
		if name == "" {
			name = "main"
		}
	}
	if b, err := s.d.Store.GetBranch(ctx, tree.ID, name); err == nil {
		if b.Tip == "" {
			return "", fmt.Errorf("%w: branch %s is empty", domain.ErrNotFound, name)
		}
		return b.Tip, nil
	} else if !errors.Is(err, domain.ErrNotFound) {
		return "", err
	}
	if ref == "" {
		return "", fmt.Errorf("%w: the tree has no default branch %s yet", domain.ErrNotFound, name)
	}
	if t, err := s.d.Store.GetTag(ctx, tree.ID, ref); err == nil {
		return t.Target, nil
	} else if !errors.Is(err, domain.ErrNotFound) {
		return "", err
	}
	snap, err := s.d.Store.GetSnapshot(ctx, ref)
	if err != nil || snap.TreeID != tree.ID {
		return "", fmt.Errorf("%w: no branch, tag or snapshot %q in this tree", domain.ErrNotFound, ref)
	}
	return snap.ID, nil
}

// AuditLicenses builds the license audit of the snapshot ref resolves to.
func (s *Service) AuditLicenses(ctx context.Context, tree *domain.Tree, ref string) (*LicenseAudit, error) {
	id, err := s.ResolveRef(ctx, tree, ref)
	if err != nil {
		return nil, err
	}
	snap, err := s.d.Store.GetSnapshot(ctx, id)
	if err != nil {
		return nil, err
	}
	m, err := (&manifests{s: s}).get(ctx, snap.ManifestHash)
	if err != nil {
		return nil, err
	}
	rules, err := s.d.Store.ListLicenseRules(ctx, tree.ID)
	if err != nil && !errors.Is(err, domain.ErrNotFound) {
		return nil, err
	}
	now := s.now()
	var general []domain.LicenseRule
	grantees := map[string][]domain.LicenseRule{}
	for _, r := range rules {
		if r.ExpiresAt != nil && !now.Before(*r.ExpiresAt) {
			continue
		}
		if r.Grantee == "" {
			general = append(general, r)
		} else {
			grantees[r.Grantee] = append(grantees[r.Grantee], r)
		}
	}
	granteeNames := make([]string, 0, len(grantees))
	for g := range grantees {
		granteeNames = append(granteeNames, g)
	}
	sort.Strings(granteeNames)

	a := &LicenseAudit{
		TreeID: tree.ID, Ref: ref, SnapshotID: id, Files: len(m.GetEntries()),
		Licenses: []LicenseUsage{}, Unlicensed: []string{}, Exposed: []ExposedPath{}, Rules: []RuleCheck{},
		SPDXListVersion: spdx.version,
	}
	if a.Ref == "" {
		a.Ref = tree.DefaultBranch
	}
	usage := map[string]*LicenseUsage{}
	ruleFiles := map[string]int{} // DocKey(glob, grantee) -> files
	expose := func(e ExposedPath) {
		a.ExposedCount++
		if len(a.Exposed) < maxAuditList {
			a.Exposed = append(a.Exposed, e)
		} else {
			a.Truncated = true
		}
	}
	for _, e := range m.GetEntries() {
		p := e.GetPath()
		r, ok := license.Resolve(general, p)
		if !ok {
			a.UnlicensedCount++
			if len(a.Unlicensed) < maxAuditList {
				a.Unlicensed = append(a.Unlicensed, p)
			} else {
				a.Truncated = true
			}
		} else {
			ruleFiles[domain.DocKey(r.PathGlob, "")]++
			u := usage[r.SPDX]
			if u == nil {
				u = &LicenseUsage{SPDX: r.SPDX, Proprietary: license.IsProprietary(r.SPDX), Globs: []string{}}
				usage[r.SPDX] = u
			}
			u.Files++
			if !containsString(u.Globs, r.PathGlob) {
				u.Globs = append(u.Globs, r.PathGlob)
			}
		}
		proprietary := ok && license.IsProprietary(r.SPDX)
		if proprietary && license.Permits(r, license.OpRead) {
			expose(ExposedPath{Path: p, SPDX: r.SPDX, Grant: r.Grant, Glob: r.PathGlob, ExpiresAt: r.ExpiresAt})
		}
		for _, g := range granteeNames {
			gr, gok := license.Resolve(grantees[g], p)
			if !gok {
				continue
			}
			ruleFiles[domain.DocKey(gr.PathGlob, g)]++
			if proprietary && license.Permits(gr, license.OpRead) {
				expose(ExposedPath{Path: p, SPDX: r.SPDX, Grantee: g, Grant: gr.Grant, Glob: gr.PathGlob, ExpiresAt: gr.ExpiresAt})
			}
		}
	}
	for _, u := range usage {
		sort.Strings(u.Globs)
		a.Licenses = append(a.Licenses, *u)
	}
	sort.Slice(a.Licenses, func(i, j int) bool {
		if a.Licenses[i].Files != a.Licenses[j].Files {
			return a.Licenses[i].Files > a.Licenses[j].Files
		}
		return a.Licenses[i].SPDX < a.Licenses[j].SPDX
	})
	for _, r := range rules {
		rc := RuleCheck{PathGlob: r.PathGlob, SPDX: r.SPDX, Grant: r.Grant, Grantee: r.Grantee, ExpiresAt: r.ExpiresAt,
			Expired: r.ExpiresAt != nil && !now.Before(*r.ExpiresAt), Files: ruleFiles[domain.DocKey(r.PathGlob, r.Grantee)]}
		rc.Problems = StrictSPDX(r.SPDX)
		if rc.Problems == nil {
			rc.Problems = []string{}
		}
		rc.Valid = len(rc.Problems) == 0
		if !rc.Valid {
			a.InvalidRules++
		}
		a.Rules = append(a.Rules, rc)
	}
	sort.Slice(a.Rules, func(i, j int) bool {
		if a.Rules[i].PathGlob != a.Rules[j].PathGlob {
			return a.Rules[i].PathGlob < a.Rules[j].PathGlob
		}
		return a.Rules[i].Grantee < a.Rules[j].Grantee
	})
	return a, nil
}

func containsString(xs []string, s string) bool {
	for _, x := range xs {
		if x == s {
			return true
		}
	}
	return false
}
