package scan

import (
	"context"
	"encoding/json"
	"errors"
	"sort"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ScanRequest is the body of POST {tree}/compliance/scan.
type ScanRequest struct {
	// Ref is a branch, tag or snapshot id ("" = the default branch).
	Ref string `json:"ref,omitempty"`
}

// ScanMatch is one secret found by an on-demand scan.
type ScanMatch struct {
	Rule        string `json:"rule"`
	Path        string `json:"path"`
	Line        int    `json:"line"`
	Excerpt     string `json:"excerpt"`
	Fingerprint string `json:"fingerprint"`
	// Allowed: the fingerprint is on the tree's allowlist.
	Allowed bool `json:"allowed"`
}

// ScanResult is the body answering POST {tree}/compliance/scan.
type ScanResult struct {
	TreeID     string      `json:"tree_id"`
	Ref        string      `json:"ref"`
	SnapshotID string      `json:"snapshot_id"`
	Mode       string      `json:"mode"`
	Files      int         `json:"files"`   // files scanned
	Bytes      int64       `json:"bytes"`   // bytes scanned
	Skipped    int         `json:"skipped"` // too large, chunked or excluded
	Truncated  bool        `json:"truncated"`
	Matches    []ScanMatch `json:"matches"`
}

// ScanSnapshot scans every file of the snapshot ref resolves to with the
// tree's exclude globs and size cap (nothing is stored).
func (s *Service) ScanSnapshot(ctx context.Context, tree *domain.Tree, ref string) (*ScanResult, error) {
	id, err := s.ResolveRef(ctx, tree, ref)
	if err != nil {
		return nil, err
	}
	snap, err := s.d.Store.GetSnapshot(ctx, id)
	if err != nil {
		return nil, err
	}
	st, err := s.GetSettings(ctx, tree.ID)
	if err != nil {
		return nil, err
	}
	m, err := (&manifests{s: s}).get(ctx, snap.ManifestHash)
	if err != nil {
		return nil, err
	}
	c := &collected{}
	c.add(st, m, nil, id, map[string]bool{})
	allow := s.allowSet(ctx, tree.ID)
	res := &ScanResult{TreeID: tree.ID, Ref: ref, SnapshotID: id, Mode: st.Mode, Files: len(c.files), Bytes: c.bytes,
		Skipped: c.skipped, Truncated: c.truncated, Matches: []ScanMatch{}}
	if res.Ref == "" {
		res.Ref = tree.DefaultBranch
	}
	for _, h := range s.scanFiles(ctx, c.files) {
		res.Matches = append(res.Matches, ScanMatch{Rule: h.Rule, Path: h.Path, Line: h.Line, Excerpt: h.Excerpt,
			Fingerprint: h.Fingerprint, Allowed: allow[h.Fingerprint]})
	}
	return res, nil
}

// ModeCounts counts trees per scanning mode.
type ModeCounts struct {
	Off   int `json:"off"`
	Warn  int `json:"warn"`
	Block int `json:"block"`
}

// TreeCompliance is one tree of the server-wide summary.
type TreeCompliance struct {
	TreeID           string     `json:"tree_id"`
	Tenant           string     `json:"tenant"` // slug
	Tree             string     `json:"tree"`   // name
	Mode             string     `json:"mode"`
	OpenFindings     int        `json:"open_findings"`
	ResolvedFindings int        `json:"resolved_findings"`
	BlockedFindings  int        `json:"blocked_findings"` // open findings whose last push was rejected
	Allowlisted      int        `json:"allowlisted"`
	LicenseRules     int        `json:"license_rules"`
	InvalidRules     int        `json:"invalid_rules"` // rules failing the strict SPDX check
	LastFindingAt    *time.Time `json:"last_finding_at,omitempty"`
}

// RecentFinding is a finding with its tree's names.
type RecentFinding struct {
	Finding
	Tenant   string `json:"tenant"`
	TreeName string `json:"tree_name"`
}

// Summary is the body of GET /admin/compliance.
type Summary struct {
	Trees            int              `json:"trees"`
	Modes            ModeCounts       `json:"modes"`
	OpenFindings     int              `json:"open_findings"`
	ResolvedFindings int              `json:"resolved_findings"`
	Allowlisted      int              `json:"allowlisted"`
	LicenseRules     int              `json:"license_rules"`
	InvalidRules     int              `json:"invalid_rules"`
	SPDXListVersion  string           `json:"spdx_list_version"`
	RulesVersion     int              `json:"rules_version"`
	ScanRules        int              `json:"scan_rules"`
	TreeList         []TreeCompliance `json:"tree_list"`
	Recent           []RecentFinding  `json:"recent"` // latest open findings (at most 20)
}

// Summarize builds the server-wide compliance summary.
func (s *Service) Summarize(ctx context.Context) (*Summary, error) {
	trees, err := s.d.Store.ListTrees(ctx, "")
	if err != nil {
		return nil, err
	}
	raws, err := s.d.Store.ListDocs(ctx, CollFindings, "")
	if err != nil {
		return nil, err
	}
	byTree := map[string][]Finding{}
	for _, raw := range raws {
		var f Finding
		if json.Unmarshal(raw, &f) == nil {
			byTree[f.TreeID] = append(byTree[f.TreeID], f)
		}
	}
	sum := &Summary{Trees: len(trees), SPDXListVersion: spdx.version, RulesVersion: rules.Version, ScanRules: len(rules.Rules),
		TreeList: []TreeCompliance{}, Recent: []RecentFinding{}}
	slugs := map[string]string{}
	var recent []RecentFinding
	for i := range trees {
		t := &trees[i]
		slug, ok := slugs[t.TenantID]
		if !ok {
			if ten, err := s.d.Store.GetTenant(ctx, t.TenantID); err == nil {
				slug = ten.Slug
			}
			slugs[t.TenantID] = slug
		}
		st, err := s.GetSettings(ctx, t.ID)
		if err != nil {
			return nil, err
		}
		tc := TreeCompliance{TreeID: t.ID, Tenant: slug, Tree: t.Name, Mode: st.Mode}
		switch st.Mode {
		case ModeOff:
			sum.Modes.Off++
		case ModeBlock:
			sum.Modes.Block++
		default:
			sum.Modes.Warn++
		}
		for _, f := range byTree[t.ID] {
			if f.Status == StatusResolved {
				tc.ResolvedFindings++
				continue
			}
			tc.OpenFindings++
			if f.Action == ActionBlocked {
				tc.BlockedFindings++
			}
			if tc.LastFindingAt == nil || f.LastSeenAt.After(*tc.LastFindingAt) {
				at := f.LastSeenAt
				tc.LastFindingAt = &at
			}
			recent = append(recent, RecentFinding{Finding: f, Tenant: slug, TreeName: t.Name})
		}
		if al, err := s.GetAllowList(ctx, t.ID); err == nil {
			tc.Allowlisted = len(al.Entries)
		}
		lrules, err := s.d.Store.ListLicenseRules(ctx, t.ID)
		if err != nil && !errors.Is(err, domain.ErrNotFound) {
			return nil, err
		}
		tc.LicenseRules = len(lrules)
		for _, r := range lrules {
			if len(StrictSPDX(r.SPDX)) > 0 {
				tc.InvalidRules++
			}
		}
		sum.OpenFindings += tc.OpenFindings
		sum.ResolvedFindings += tc.ResolvedFindings
		sum.Allowlisted += tc.Allowlisted
		sum.LicenseRules += tc.LicenseRules
		sum.InvalidRules += tc.InvalidRules
		sum.TreeList = append(sum.TreeList, tc)
	}
	sort.SliceStable(sum.TreeList, func(i, j int) bool {
		a, b := sum.TreeList[i], sum.TreeList[j]
		if a.OpenFindings != b.OpenFindings {
			return a.OpenFindings > b.OpenFindings
		}
		if a.Tenant != b.Tenant {
			return a.Tenant < b.Tenant
		}
		return a.Tree < b.Tree
	})
	sort.SliceStable(recent, func(i, j int) bool { return recent[i].LastSeenAt.After(recent[j].LastSeenAt) })
	if len(recent) > 20 {
		recent = recent[:20]
	}
	sum.Recent = append(sum.Recent, recent...)
	return sum, nil
}
