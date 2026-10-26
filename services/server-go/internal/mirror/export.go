package mirror

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// ---- identities (parity with crates/worktree-git/src/ident.rs) -------------

// Signature fallbacks when a snapshot author has no name or email.
const (
	FallbackName  = "unknown"
	FallbackEmail = "unknown@localhost"
)

// Author is a snapshot author split into Git signature parts.
type Author struct {
	Name  string
	Email string
}

// ParseAuthor splits a snapshot author string like the Rust exporter:
// "Jane Doe <jane@example.com>" -> (Jane Doe, jane@example.com);
// "jane@example.com" -> (jane, jane@example.com); "jane" -> (jane, "").
// Angle brackets and control characters are removed.
func ParseAuthor(raw string) Author {
	raw = strings.TrimSpace(raw)
	if open := strings.LastIndex(raw, "<"); open >= 0 && strings.HasSuffix(raw, ">") {
		return Author{Name: sanitize(raw[:open]), Email: sanitize(raw[open+1 : len(raw)-1])}
	}
	if strings.Contains(raw, "@") && strings.IndexFunc(raw, unicode.IsSpace) < 0 {
		email := sanitize(raw)
		name, _, _ := strings.Cut(email, "@")
		return Author{Name: name, Email: email}
	}
	return Author{Name: sanitize(raw)}
}

func sanitize(part string) string {
	var b strings.Builder
	for _, r := range part {
		if r == '<' || r == '>' || unicode.IsControl(r) {
			continue
		}
		b.WriteRune(r)
	}
	return strings.TrimSpace(b.String())
}

// signature renders a fast-import identity: "Name <email> <unix> +0000".
func signature(author string, at time.Time) string {
	a := ParseAuthor(author)
	if a.Name == "" {
		a.Name = FallbackName
	}
	if a.Email == "" {
		a.Email = FallbackEmail
	}
	return fmt.Sprintf("%s <%s> %d +0000", a.Name, a.Email, at.Unix())
}

// MessageToGit trims trailing whitespace and, when non-empty, ends the
// message with one newline (the convention `git commit` follows).
func MessageToGit(m string) string {
	m = strings.TrimRightFunc(m, unicode.IsSpace)
	if m == "" {
		return ""
	}
	return m + "\n"
}

// ---- refs and paths --------------------------------------------------------

// ValidRefName reports whether name (a full ref like refs/heads/x) obeys
// git check-ref-format.
func ValidRefName(name string) bool {
	if name == "" || name == "@" || strings.HasSuffix(name, "/") || strings.HasSuffix(name, ".") ||
		strings.Contains(name, "..") || strings.Contains(name, "//") || strings.Contains(name, "@{") {
		return false
	}
	for _, r := range name {
		if r < 0x20 || r == 0x7f || strings.ContainsRune(" ~^:?*[\\", r) {
			return false
		}
	}
	for _, comp := range strings.Split(name, "/") {
		if comp == "" || strings.HasPrefix(comp, ".") || strings.HasSuffix(comp, ".lock") {
			return false
		}
	}
	return true
}

// validPath reports whether a manifest path can be written to a Git tree.
func validPath(p string) bool {
	if p == "" || strings.ContainsRune(p, 0) {
		return false
	}
	for _, c := range strings.Split(p, "/") {
		if c == "" || c == "." || c == ".." || strings.EqualFold(c, ".git") {
			return false
		}
	}
	return true
}

// quotePath C-quotes a path for a fast-import command.
func quotePath(p string) string {
	var b strings.Builder
	b.WriteByte('"')
	for i := 0; i < len(p); i++ {
		c := p[i]
		switch c {
		case '"':
			b.WriteString(`\"`)
		case '\\':
			b.WriteString(`\\`)
		case '\n':
			b.WriteString(`\n`)
		case '\t':
			b.WriteString(`\t`)
		default:
			if c < 0x20 || c == 0x7f {
				fmt.Fprintf(&b, `\%03o`, c)
			} else {
				b.WriteByte(c)
			}
		}
	}
	b.WriteByte('"')
	return b.String()
}

// ---- license filter ------------------------------------------------------

// filter decides which paths of a snapshot are exported.
type filter struct {
	rules []domain.LicenseRule // general, unexpired rules
}

func newFilter(rules []domain.LicenseRule, now time.Time) filter {
	active := make([]domain.LicenseRule, 0, len(rules))
	for _, r := range rules {
		if r.Grantee == "" && (r.ExpiresAt == nil || r.ExpiresAt.After(now)) {
			active = append(active, r)
		}
	}
	sort.Slice(active, func(i, j int) bool { return active[i].PathGlob < active[j].PathGlob })
	return filter{rules: active}
}

// fingerprint identifies the filter: commits built under a different one
// differ, so the history is exported again when it changes.
func (f filter) fingerprint() string {
	type key struct{ Glob, SPDX, Grant string }
	keys := make([]key, len(f.rules))
	for i, r := range f.rules {
		keys[i] = key{r.PathGlob, r.SPDX, r.Grant}
	}
	raw, _ := json.Marshal(keys)
	sum := sha256.Sum256(append([]byte("mirror-filter-v1\x00"), raw...))
	return hex.EncodeToString(sum[:16])
}

// redistributable reports whether a non-member may export p (a path no
// rule matches is unrestricted).
func (f filter) redistributable(p string) bool {
	rule, ok := license.Resolve(f.rules, p)
	return !ok || license.Permits(rule, license.OpExport)
}

// file is one file of an exported tree.
type file struct {
	entry *syncpb.ManifestEntry // nil for the generated notice
	h     hash.Hash
	exec  bool
	data  []byte // generated content
}

func (f file) mode() string {
	if f.exec {
		return "100755"
	}
	return "100644"
}

// ---- export --------------------------------------------------------------

// importRef is the scratch ref fast-import builds commits on.
const importRef = "refs/wt-mirror/import"

// exporter writes a fast-import stream for the snapshots missing from a
// mirror's map.
type exporter struct {
	ctx     context.Context
	s       *Service
	filter  filter
	commits map[string]string // snapshot id -> existing commit id

	w        *bufio.Writer
	marks    map[string]int    // snapshot id -> commit mark (this session)
	blobs    map[hash.Hash]int // content hash -> blob mark (this session)
	nextMark int
	snaps    map[string]*domain.Snapshot
	files    map[hash.Hash]map[string]file // manifest hash -> files (cache)

	excluded map[string]bool // paths left out by license rules
	skipped  map[string]bool // paths that cannot be represented in Git
	written  []string        // snapshot ids written, in order
}

func newExporter(ctx context.Context, s *Service, f filter, commits map[string]string) *exporter {
	return &exporter{
		ctx: ctx, s: s, filter: f, commits: commits,
		marks: map[string]int{}, blobs: map[hash.Hash]int{}, snaps: map[string]*domain.Snapshot{},
		files: map[hash.Hash]map[string]file{}, excluded: map[string]bool{}, skipped: map[string]bool{},
	}
}

func (e *exporter) snapshot(id string) (*domain.Snapshot, error) {
	if s, ok := e.snaps[id]; ok {
		return s, nil
	}
	s, err := e.s.d.Store.GetSnapshot(e.ctx, id)
	if err != nil {
		return nil, err
	}
	e.snaps[id] = s
	return s, nil
}

// known reports whether id already has a commit (from the map or this
// session).
func (e *exporter) known(id string) bool {
	if _, ok := e.marks[id]; ok {
		return true
	}
	_, ok := e.commits[id]
	return ok
}

// plan returns the snapshots reachable from tips that have no commit yet,
// parents before children. Parents that do not exist are dropped (as the
// Rust exporter does); a missing tip is an error.
func (e *exporter) plan(tips []string) ([]string, error) {
	var order []string
	state := map[string]uint8{} // 1 = visiting, 2 = done
	type frame struct {
		id   string
		next int
	}
	for _, tip := range tips {
		if e.known(tip) || state[tip] == 2 {
			continue
		}
		if _, err := e.snapshot(tip); err != nil {
			return nil, fmt.Errorf("snapshot %s: %w", tip, err)
		}
		stack := []frame{{id: tip}}
		state[tip] = 1
		for len(stack) > 0 {
			if err := e.ctx.Err(); err != nil {
				return nil, err
			}
			top := &stack[len(stack)-1]
			s := e.snaps[top.id]
			if top.next < len(s.Parents) {
				p := s.Parents[top.next]
				top.next++
				if p == "" || e.known(p) || state[p] != 0 {
					continue
				}
				if _, err := e.snapshot(p); err != nil {
					if errors.Is(err, domain.ErrNotFound) {
						continue
					}
					return nil, err
				}
				state[p] = 1
				stack = append(stack, frame{id: p})
				continue
			}
			state[top.id] = 2
			order = append(order, top.id)
			stack = stack[:len(stack)-1]
		}
	}
	return order, nil
}

// treeFiles returns the exported files of a manifest: license-excluded and
// invalid paths removed, a generated LICENSES/NOTICE added.
func (e *exporter) treeFiles(mh hash.Hash) (map[string]file, error) {
	if fs, ok := e.files[mh]; ok {
		return fs, nil
	}
	raw, err := e.s.d.Objects.Get(e.ctx, mh)
	if err != nil {
		return nil, fmt.Errorf("manifest %s: %v", mh.Hex(), err)
	}
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("manifest %s: %w", mh.Hex(), err)
	}
	out := make(map[string]file, len(m.GetEntries())+1)
	var included []string
	excluded := 0
	for _, ent := range m.GetEntries() {
		p := ent.GetPath()
		switch {
		case p == license.NoticePath:
			continue // regenerated below
		case !validPath(p):
			e.skipped[p] = true
			continue
		case !e.filter.redistributable(p):
			e.excluded[p] = true
			excluded++
			continue
		}
		h, err := content.EntryHash(ent)
		if err != nil {
			return nil, err
		}
		out[p] = file{entry: ent, h: h, exec: ent.GetExecutable()}
		included = append(included, p)
	}
	footer := ""
	switch {
	case excluded == 1:
		footer = "\n1 path was excluded: its license does not permit redistribution.\n"
	case excluded > 1:
		footer = fmt.Sprintf("\n%d paths were excluded: their license does not permit redistribution.\n", excluded)
	}
	if notice := license.BuildNotice(e.filter.rules, included, nil, footer); notice != nil {
		out[license.NoticePath] = file{h: hash.Sum(notice), data: notice}
	}
	if len(e.files) >= 64 {
		clear(e.files)
	}
	e.files[mh] = out
	return out, nil
}

// parentRef is the fast-import reference of a parent commit.
func (e *exporter) parentRef(id string) (string, bool) {
	if m, ok := e.marks[id]; ok {
		return ":" + strconv.Itoa(m), true
	}
	if c, ok := e.commits[id]; ok {
		return c, true
	}
	return "", false
}

func (e *exporter) data(b []byte) error {
	if _, err := fmt.Fprintf(e.w, "data %d\n", len(b)); err != nil {
		return err
	}
	if _, err := e.w.Write(b); err != nil {
		return err
	}
	_, err := e.w.WriteString("\n")
	return err
}

// blob writes the content of f (once per session) and returns its mark.
func (e *exporter) blob(f file) (int, error) {
	if m, ok := e.blobs[f.h]; ok {
		return m, nil
	}
	data := f.data
	if f.entry != nil {
		var err error
		if data, err = content.ReadEntry(e.ctx, e.s.d.Objects, f.entry); err != nil {
			return 0, fmt.Errorf("%s: %w", f.entry.GetPath(), err)
		}
	}
	e.nextMark++
	m := e.nextMark
	if _, err := fmt.Fprintf(e.w, "blob\nmark :%d\n", m); err != nil {
		return 0, err
	}
	if err := e.data(data); err != nil {
		return 0, err
	}
	e.blobs[f.h] = m
	return m, nil
}

// commit writes the commit of snapshot id.
func (e *exporter) commit(id string) error {
	s := e.snaps[id]
	files, err := e.treeFiles(s.ManifestHash)
	if err != nil {
		return err
	}
	var parents []string
	var base map[string]file
	for _, p := range s.Parents {
		ref, ok := e.parentRef(p)
		if !ok || slices.Contains(parents, ref) {
			continue
		}
		if len(parents) == 0 {
			ps, err := e.snapshot(p)
			if err != nil {
				return err
			}
			if base, err = e.treeFiles(ps.ManifestHash); err != nil {
				return err
			}
		}
		parents = append(parents, ref)
	}

	// Changes against the first parent's tree: deletions, then writes.
	var deletes, writes []string
	for p := range base {
		if _, ok := files[p]; !ok {
			deletes = append(deletes, p)
		}
	}
	for p, f := range files {
		if old, ok := base[p]; !ok || old.h != f.h || old.exec != f.exec {
			writes = append(writes, p)
		}
	}
	sort.Strings(deletes)
	sort.Strings(writes)
	blobMarks := make(map[string]int, len(writes))
	for _, p := range writes {
		m, err := e.blob(files[p])
		if err != nil {
			return err
		}
		blobMarks[p] = m
	}

	e.nextMark++
	mark := e.nextMark
	sig := signature(s.Author, s.CreatedAt)
	if _, err := fmt.Fprintf(e.w, "reset %s\ncommit %s\nmark :%d\nauthor %s\ncommitter %s\n", importRef, importRef, mark, sig, sig); err != nil {
		return err
	}
	if err := e.data([]byte(MessageToGit(s.Message))); err != nil {
		return err
	}
	for i, ref := range parents {
		kw := "merge"
		if i == 0 {
			kw = "from"
		}
		if _, err := fmt.Fprintf(e.w, "%s %s\n", kw, ref); err != nil {
			return err
		}
	}
	for _, p := range deletes {
		if _, err := fmt.Fprintf(e.w, "D %s\n", quotePath(p)); err != nil {
			return err
		}
	}
	for _, p := range writes {
		if _, err := fmt.Fprintf(e.w, "M %s :%d %s\n", files[p].mode(), blobMarks[p], quotePath(p)); err != nil {
			return err
		}
	}
	if _, err := e.w.WriteString("\n"); err != nil {
		return err
	}
	e.marks[id] = mark
	e.written = append(e.written, id)
	return nil
}

// annotatedTag writes an annotated tag object for t (its target must have
// a commit).
func (e *exporter) annotatedTag(t domain.Tag) error {
	ref, ok := e.parentRef(t.Target)
	if !ok {
		return fmt.Errorf("tag %s: target %s has no commit", t.Name, t.Target)
	}
	if _, err := fmt.Fprintf(e.w, "tag %s\nfrom %s\ntagger %s\n", t.Name, ref, signature(t.Tagger, t.CreatedAt)); err != nil {
		return err
	}
	return e.data([]byte(MessageToGit(t.Message)))
}

// stream writes the whole fast-import input: every missing commit of
// tips, then the annotated tags.
func (e *exporter) stream(w io.Writer, order []string, tags []domain.Tag) error {
	e.w = bufio.NewWriterSize(w, 256<<10)
	for _, id := range order {
		if err := e.ctx.Err(); err != nil {
			return err
		}
		if err := e.commit(id); err != nil {
			return err
		}
	}
	for _, t := range tags {
		if t.Message == "" {
			continue
		}
		if err := e.annotatedTag(t); err != nil {
			return err
		}
	}
	if _, err := e.w.WriteString("done\n"); err != nil {
		return err
	}
	return e.w.Flush()
}

// parseMarks reads a fast-import marks file (":<mark> <sha>" lines).
func parseMarks(raw string) map[int]string {
	out := map[int]string{}
	for _, line := range strings.Split(raw, "\n") {
		mark, sha, ok := strings.Cut(strings.TrimSpace(line), " ")
		if !ok || !strings.HasPrefix(mark, ":") {
			continue
		}
		n, err := strconv.Atoi(mark[1:])
		if err == nil {
			out[n] = sha
		}
	}
	return out
}
