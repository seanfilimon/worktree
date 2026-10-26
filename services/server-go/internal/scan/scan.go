// Package scan scans pushed content for secrets and audits license
// compliance (docs/ENGINEERING_PLAN.md §16.7,
// crates/worktree-protocol/specs/security/SecretScanning.md).
//
// # Secret scanning
//
// Service is a domain.PushChecker. For every push it scans the text files
// the new snapshots add or change (each new snapshot against its first
// parent; blobs over the size cap, chunked files and binaries are skipped)
// with the rule set shared with the client scanner (rules.go). What happens
// with findings depends on the tree's mode (Settings.Mode):
//
//	off    nothing is scanned
//	warn   (default) the push goes through; findings are stored, event
//	       secret.detected is published and "secret.detected" audited
//	block  the push is rejected (*domain.PushDenied{Kind: "secret_detected"},
//	       on the wire an AccessDenied whose reason lists the findings as
//	       path:line [rule] redacted excerpt and fingerprint) unless the
//	       pusher holds branch:bypass_protection on the tree (the push
//	       goes through, recorded as "bypassed")
//
// Allowlist entries (collection scan_allow, key tree id) name fingerprints
// of known false positives; matching findings are ignored in every mode.
//
// # Documents
//
//	scan_settings   key tree id                       Settings
//	scan_findings   DocKey(tree id, finding id)       Finding
//	scan_allow      key tree id                       AllowList
//
// The finding id is derived from (fingerprint, path), so a secret pushed
// again at the same path updates its finding (Count, LastSeenAt) instead of
// adding one. tree.deleted removes the tree's documents.
package scan

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"sort"
	"strings"
	"time"

	"github.com/zeebo/blake3"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Document collections.
const (
	CollSettings = "scan_settings"
	CollFindings = "scan_findings"
	CollAllow    = "scan_allow"
)

// Scanning modes (Settings.Mode).
const (
	ModeOff   = "off"
	ModeWarn  = "warn"
	ModeBlock = "block"
)

// Finding statuses and the actions taken on the push that last saw it.
const (
	StatusOpen     = "open"
	StatusResolved = "resolved"

	ActionWarned   = "warned"
	ActionBlocked  = "blocked"
	ActionBypassed = "bypassed"
)

// Resolutions accepted by Resolve.
var Resolutions = []string{"revoked", "false_positive", "test_data", "wont_fix"}

// KindSecretDetected is the PushDenied.Kind of a blocked push.
const KindSecretDetected = "secret_detected"

// Limits.
const (
	DefaultMaxFileBytes = 1 << 20  // files larger than this are not scanned
	MaxMaxFileBytes     = 16 << 20 // upper bound of Settings.MaxFileBytes
	MaxPushScanBytes    = 64 << 20 // bytes scanned per push at most
	MaxExclude          = 100      // exclude globs per tree
	MaxAllowEntries     = 1000     // allowlist entries per tree
	MaxReasonLen        = 500
	MaxNoteLen          = 1000
	maxDiffSnapshots    = 256 // above this many new snapshots only tip vs old tip is scanned
	maxRecorded         = 100 // findings stored per push
	maxListedInDenial   = 10
	maxEventFindings    = 20
)

// Deps are the collaborators of the scan package. Store and Objects are
// required; the others may be nil.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Service is the scan component. It implements domain.PushChecker.
type Service struct {
	d   Deps
	now func() time.Time
}

var _ domain.PushChecker = (*Service)(nil)

// New returns a Service.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d, now: func() time.Time { return time.Now().UTC() }}
}

// Settings is the secret scanning configuration of a tree.
type Settings struct {
	TreeID string `json:"tree_id"`
	// Mode is off, warn (default) or block.
	Mode string `json:"mode"`
	// Exclude lists path globs (license rule glob syntax) never scanned.
	Exclude []string `json:"exclude"`
	// MaxFileBytes: larger files are skipped (0 = 1 MiB; at most 16 MiB).
	MaxFileBytes int64      `json:"max_file_bytes"`
	UpdatedBy    string     `json:"updated_by,omitempty"`
	UpdatedAt    *time.Time `json:"updated_at,omitempty"`
}

// SettingsPatch is the body of PUT {tree}/compliance/settings; absent
// fields are unchanged.
type SettingsPatch struct {
	Mode         *string   `json:"mode,omitempty"`
	Exclude      *[]string `json:"exclude,omitempty"`
	MaxFileBytes *int64    `json:"max_file_bytes,omitempty"`
}

// Finding is a secret found in a push (one per fingerprint and path).
type Finding struct {
	ID          string `json:"id"`
	TreeID      string `json:"tree_id"`
	Path        string `json:"path"`
	Line        int    `json:"line"`
	Rule        string `json:"rule"`
	Excerpt     string `json:"excerpt"` // redacted
	Fingerprint string `json:"fingerprint"`
	// Branch and SnapshotID locate the latest push that contained it.
	Branch     string `json:"branch"`
	SnapshotID string `json:"snapshot_id"`
	// Status is open or resolved; Action what the latest push did (warned,
	// blocked, bypassed).
	Status      string     `json:"status"`
	Action      string     `json:"action"`
	Count       int        `json:"count"` // pushes that contained it
	PushedBy    string     `json:"pushed_by"`
	FirstSeenAt time.Time  `json:"first_seen_at"`
	LastSeenAt  time.Time  `json:"last_seen_at"`
	Resolution  string     `json:"resolution,omitempty"`
	Note        string     `json:"note,omitempty"`
	ResolvedBy  string     `json:"resolved_by,omitempty"`
	ResolvedAt  *time.Time `json:"resolved_at,omitempty"`
}

// FindingList is the body of GET {tree}/compliance/findings.
type FindingList struct {
	Findings []Finding `json:"findings"`
	Open     int       `json:"open"`
	Resolved int       `json:"resolved"`
}

// ResolveRequest is the body of POST {tree}/compliance/findings/{id}/resolve.
type ResolveRequest struct {
	// Resolution: revoked, false_positive, test_data or wont_fix.
	Resolution string `json:"resolution,omitempty"`
	Note       string `json:"note,omitempty"`
	// Reopen sets the finding back to open (Resolution is ignored).
	Reopen bool `json:"reopen,omitempty"`
}

// AllowEntry lets the secret with Fingerprint through every push.
type AllowEntry struct {
	Fingerprint string    `json:"fingerprint"`
	Reason      string    `json:"reason"`
	CreatedBy   string    `json:"created_by"`
	CreatedAt   time.Time `json:"created_at"`
}

// AllowList is the scan_allow document of a tree.
type AllowList struct {
	TreeID  string       `json:"tree_id"`
	Entries []AllowEntry `json:"entries"`
}

// AllowRequest is the body of POST {tree}/compliance/allowlist.
type AllowRequest struct {
	Fingerprint string `json:"fingerprint,omitempty"`
	Reason      string `json:"reason,omitempty"`
}

// ---- settings ----

// GetSettings returns the tree's settings (defaults when never set).
func (s *Service) GetSettings(ctx context.Context, treeID string) (*Settings, error) {
	var st Settings
	err := s.d.Store.GetDoc(ctx, CollSettings, treeID, &st)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		st = Settings{}
	case err != nil:
		return nil, err
	}
	st.TreeID = treeID
	if st.Mode == "" {
		st.Mode = ModeWarn
	}
	if st.Exclude == nil {
		st.Exclude = []string{}
	}
	return &st, nil
}

// effectiveMaxFile is the size cap in force.
func (st *Settings) effectiveMaxFile() int64 {
	if st.MaxFileBytes <= 0 {
		return DefaultMaxFileBytes
	}
	return min(st.MaxFileBytes, MaxMaxFileBytes)
}

// excluded reports whether p matches an exclude glob.
func (st *Settings) excluded(p string) bool {
	for _, g := range st.Exclude {
		if license.Match(g, p) {
			return true
		}
	}
	return false
}

// UpdateSettings applies a patch. Errors wrap domain.ErrInvalid.
func (s *Service) UpdateSettings(ctx context.Context, p *domain.Principal, tree *domain.Tree, patch SettingsPatch) (*Settings, error) {
	st, err := s.GetSettings(ctx, tree.ID)
	if err != nil {
		return nil, err
	}
	if patch.Mode != nil {
		switch *patch.Mode {
		case ModeOff, ModeWarn, ModeBlock:
			st.Mode = *patch.Mode
		default:
			return nil, fmt.Errorf("%w: mode must be off, warn or block", domain.ErrInvalid)
		}
	}
	if patch.Exclude != nil {
		ex := make([]string, 0, len(*patch.Exclude))
		seen := map[string]bool{}
		for _, g := range *patch.Exclude {
			g = strings.TrimSpace(g)
			if g == "" || seen[g] {
				continue
			}
			if err := license.Validate(domain.LicenseRule{PathGlob: g, SPDX: "MIT"}); err != nil {
				return nil, fmt.Errorf("%w: exclude %q: invalid glob", domain.ErrInvalid, g)
			}
			seen[g] = true
			ex = append(ex, g)
		}
		if len(ex) > MaxExclude {
			return nil, fmt.Errorf("%w: at most %d exclude globs", domain.ErrInvalid, MaxExclude)
		}
		st.Exclude = ex
	}
	if patch.MaxFileBytes != nil {
		if *patch.MaxFileBytes < 0 || *patch.MaxFileBytes > MaxMaxFileBytes {
			return nil, fmt.Errorf("%w: max_file_bytes must be between 0 (default 1 MiB) and %d", domain.ErrInvalid, MaxMaxFileBytes)
		}
		st.MaxFileBytes = *patch.MaxFileBytes
	}
	now := s.now()
	st.UpdatedAt = &now
	if p != nil {
		st.UpdatedBy = p.Username
	}
	if err := s.d.Store.PutDoc(ctx, CollSettings, tree.ID, st); err != nil {
		return nil, err
	}
	return st, nil
}

// ---- findings ----

// findingID is the stable id of a finding of fingerprint at path.
func findingID(fingerprint, path string) string {
	sum := blake3.Sum256([]byte(fingerprint + "\x00" + path))
	return hex.EncodeToString(sum[:8])
}

// ListFindings returns the tree's findings, most recently seen first.
// status is "open", "resolved" or "" (all); limit <= 0 means all.
func (s *Service) ListFindings(ctx context.Context, treeID, status string, limit int) (*FindingList, error) {
	all, err := s.findings(ctx, treeID)
	if err != nil {
		return nil, err
	}
	res := &FindingList{Findings: []Finding{}}
	for _, f := range all {
		switch f.Status {
		case StatusResolved:
			res.Resolved++
		default:
			res.Open++
		}
		if status != "" && f.Status != status {
			continue
		}
		if limit > 0 && len(res.Findings) >= limit {
			continue
		}
		res.Findings = append(res.Findings, f)
	}
	return res, nil
}

// findings returns every finding of a tree, most recently seen first.
func (s *Service) findings(ctx context.Context, treeID string) ([]Finding, error) {
	raws, err := s.d.Store.ListDocs(ctx, CollFindings, meta.DocPrefix(treeID))
	if err != nil {
		return nil, err
	}
	out := make([]Finding, 0, len(raws))
	for _, raw := range raws {
		var f Finding
		if err := json.Unmarshal(raw, &f); err != nil {
			continue
		}
		out = append(out, f)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if !out[i].LastSeenAt.Equal(out[j].LastSeenAt) {
			return out[i].LastSeenAt.After(out[j].LastSeenAt)
		}
		if out[i].Path != out[j].Path {
			return out[i].Path < out[j].Path
		}
		return out[i].Line < out[j].Line
	})
	return out, nil
}

// GetFinding returns one finding (ErrNotFound when unknown).
func (s *Service) GetFinding(ctx context.Context, treeID, id string) (*Finding, error) {
	var f Finding
	if err := s.d.Store.GetDoc(ctx, CollFindings, domain.DocKey(treeID, id), &f); err != nil {
		return nil, err
	}
	return &f, nil
}

// Resolve marks a finding resolved (or reopens it).
func (s *Service) Resolve(ctx context.Context, p *domain.Principal, tree *domain.Tree, id string, req ResolveRequest) (*Finding, error) {
	if !req.Reopen {
		ok := false
		for _, r := range Resolutions {
			ok = ok || r == req.Resolution
		}
		if !ok {
			return nil, fmt.Errorf("%w: resolution must be one of %s", domain.ErrInvalid, strings.Join(Resolutions, ", "))
		}
	}
	if len(req.Note) > MaxNoteLen {
		return nil, fmt.Errorf("%w: note is longer than %d bytes", domain.ErrInvalid, MaxNoteLen)
	}
	var out Finding
	err := s.d.Store.UpdateDoc(ctx, CollFindings, domain.DocKey(tree.ID, id), func(raw []byte) ([]byte, error) {
		if raw == nil {
			return nil, domain.ErrNotFound
		}
		if err := json.Unmarshal(raw, &out); err != nil {
			return nil, err
		}
		if req.Reopen {
			out.Status, out.Resolution, out.ResolvedBy, out.ResolvedAt = StatusOpen, "", "", nil
			out.Note = req.Note
		} else {
			now := s.now()
			out.Status, out.Resolution, out.Note, out.ResolvedAt = StatusResolved, req.Resolution, req.Note, &now
			if p != nil {
				out.ResolvedBy = p.Username
			}
		}
		return json.Marshal(out)
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ---- allowlist ----

// GetAllowList returns the tree's allowlist (empty when none).
func (s *Service) GetAllowList(ctx context.Context, treeID string) (*AllowList, error) {
	var al AllowList
	err := s.d.Store.GetDoc(ctx, CollAllow, treeID, &al)
	if err != nil && !errors.Is(err, domain.ErrNotFound) {
		return nil, err
	}
	al.TreeID = treeID
	if al.Entries == nil {
		al.Entries = []AllowEntry{}
	}
	return &al, nil
}

// validFingerprint reports whether fp has fingerprint syntax.
func validFingerprint(fp string) bool {
	if len(fp) != FingerprintHexLen {
		return false
	}
	_, err := hex.DecodeString(fp)
	return err == nil && strings.ToLower(fp) == fp
}

// Allow adds (or replaces the reason of) an allowlist entry and resolves
// the open findings with that fingerprint as false positives.
func (s *Service) Allow(ctx context.Context, p *domain.Principal, tree *domain.Tree, req AllowRequest) (*AllowEntry, error) {
	fp := strings.ToLower(strings.TrimSpace(req.Fingerprint))
	reason := strings.TrimSpace(req.Reason)
	switch {
	case !validFingerprint(fp):
		return nil, fmt.Errorf("%w: fingerprint must be %d hex digits (shown with each finding)", domain.ErrInvalid, FingerprintHexLen)
	case reason == "":
		return nil, fmt.Errorf("%w: reason is required", domain.ErrInvalid)
	case len(reason) > MaxReasonLen:
		return nil, fmt.Errorf("%w: reason is longer than %d bytes", domain.ErrInvalid, MaxReasonLen)
	}
	entry := AllowEntry{Fingerprint: fp, Reason: reason, CreatedAt: s.now()}
	if p != nil {
		entry.CreatedBy = p.Username
	}
	err := s.d.Store.UpdateDoc(ctx, CollAllow, tree.ID, func(raw []byte) ([]byte, error) {
		al := AllowList{TreeID: tree.ID}
		if raw != nil {
			if err := json.Unmarshal(raw, &al); err != nil {
				return nil, err
			}
		}
		replaced := false
		for i := range al.Entries {
			if al.Entries[i].Fingerprint == fp {
				al.Entries[i] = entry
				replaced = true
			}
		}
		if !replaced {
			if len(al.Entries) >= MaxAllowEntries {
				return nil, fmt.Errorf("%w: the allowlist is full (%d entries)", domain.ErrInvalid, MaxAllowEntries)
			}
			al.Entries = append(al.Entries, entry)
		}
		return json.Marshal(al)
	})
	if err != nil {
		return nil, err
	}
	// Open findings of this secret are false positives now.
	if all, err := s.findings(ctx, tree.ID); err == nil {
		for _, f := range all {
			if f.Fingerprint == fp && f.Status == StatusOpen {
				_, _ = s.Resolve(ctx, p, tree, f.ID, ResolveRequest{Resolution: "false_positive", Note: "allowlisted: " + reason})
			}
		}
	}
	return &entry, nil
}

// Disallow removes an allowlist entry (ErrNotFound when absent).
func (s *Service) Disallow(ctx context.Context, tree *domain.Tree, fingerprint string) error {
	fp := strings.ToLower(strings.TrimSpace(fingerprint))
	return s.d.Store.UpdateDoc(ctx, CollAllow, tree.ID, func(raw []byte) ([]byte, error) {
		if raw == nil {
			return nil, domain.ErrNotFound
		}
		var al AllowList
		if err := json.Unmarshal(raw, &al); err != nil {
			return nil, err
		}
		kept := al.Entries[:0]
		for _, e := range al.Entries {
			if e.Fingerprint != fp {
				kept = append(kept, e)
			}
		}
		if len(kept) == len(al.Entries) {
			return nil, domain.ErrNotFound
		}
		al.Entries = kept
		return json.Marshal(al)
	})
}

// allowSet returns the allowlisted fingerprints of a tree.
func (s *Service) allowSet(ctx context.Context, treeID string) map[string]bool {
	al, err := s.GetAllowList(ctx, treeID)
	if err != nil {
		s.d.Logger.WarnContext(ctx, "scan: reading the allowlist failed", "tree", treeID, "err", err)
		return nil
	}
	set := make(map[string]bool, len(al.Entries))
	for _, e := range al.Entries {
		set[e.Fingerprint] = true
	}
	return set
}

// ---- lifecycle ----

// Start subscribes to tree.deleted and removes the deleted tree's documents
// until ctx ends. It is a no-op without a bus.
func (s *Service) Start(ctx context.Context) {
	if s.d.Bus == nil {
		return
	}
	ch, cancel := s.d.Bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTreeDeleted}})
	go func() {
		defer cancel()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-ch:
				if !ok {
					return
				}
				s.DeleteTree(context.WithoutCancel(ctx), e.TreeID)
			}
		}
	}()
}

// DeleteTree removes every scan document of a tree.
func (s *Service) DeleteTree(ctx context.Context, treeID string) {
	if treeID == "" {
		return
	}
	if _, err := s.d.Store.DeleteDocs(ctx, CollFindings, meta.DocPrefix(treeID)); err != nil {
		s.d.Logger.Warn("scan: delete findings of deleted tree", slog.String("tree_id", treeID), slog.Any("error", err))
	}
	for _, coll := range []string{CollSettings, CollAllow} {
		if err := s.d.Store.DeleteDoc(ctx, coll, treeID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			s.d.Logger.Warn("scan: delete document of deleted tree", slog.String("collection", coll), slog.String("tree_id", treeID), slog.Any("error", err))
		}
	}
}
