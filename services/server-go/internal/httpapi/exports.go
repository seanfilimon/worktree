package httpapi

import (
	"archive/tar"
	"archive/zip"
	"compress/gzip"
	"context"
	"errors"
	"fmt"
	"io"
	"mime"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// noticePath is the generated license notice inside an archive (shared
// with `wt archive` / `wt git export`, crates/worktree-git/src/license.rs).
const noticePath = "LICENSES/NOTICE"

// registerExportRoutes registers the license-aware archive export (§13.8):
//
//	GET {tree}/archive/{snapshot}?format=tar.gz|zip
func (a *api) registerExportRoutes(tree string) {
	a.handle("GET "+tree+"/archive/{snapshot}", a.handleArchive)
}

// archiveFile is one file to write into an archive.
type archiveFile struct {
	entry *syncpb.ManifestEntry
	data  []byte // set for generated files
}

// handleArchive streams an archive of a snapshot ({snapshot} is a snapshot
// id or a branch name). Paths the caller may not read — license rules
// (License.CheckRead) and tree:read path denies (Access) — are omitted, and
// a generated LICENSES/NOTICE lists the SPDX id of every included path
// covered by a license rule. Audited as tree.export.
func (a *api) handleArchive(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	format := r.URL.Query().Get("format")
	switch format {
	case "":
		format = "tar.gz"
	case "tar.gz", "tgz":
		format = "tar.gz"
	case "zip":
	default:
		return badRequestf("format must be tar.gz or zip, got %q", format)
	}
	ctx := r.Context()
	s, err := a.archiveSnapshot(ctx, tree, r.PathValue("snapshot"))
	if err != nil {
		return err
	}
	m, err := a.loadManifest(ctx, s)
	if err != nil {
		return err
	}
	entries := make([]*syncpb.ManifestEntry, 0, len(m.GetEntries()))
	for _, e := range m.GetEntries() {
		if e.GetPath() != noticePath {
			entries = append(entries, e)
		}
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].GetPath() < entries[j].GetPath() })

	readable, err := a.exportablePaths(r, t, tree, entries)
	if err != nil {
		return err
	}
	files := make([]archiveFile, 0, len(entries)+1)
	omitted := 0
	for i, e := range entries {
		if readable[i] {
			files = append(files, archiveFile{entry: e})
		} else {
			omitted++
		}
	}
	rules, err := a.d.Store.ListLicenseRules(ctx, tree.ID)
	if err != nil && !errors.Is(err, domain.ErrNotFound) {
		return err
	}
	if notice := buildNotice(activeLicenseRules(rules, time.Now()), files, omitted,
		[]string{"Tree: " + t.Slug + "/" + tree.Name, "Snapshot: " + s.ID}); notice != nil {
		files = append(files, archiveFile{entry: &syncpb.ManifestEntry{Path: noticePath, Size: uint64(len(notice))}, data: notice})
		sort.Slice(files, func(i, j int) bool { return files[i].entry.GetPath() < files[j].entry.GetPath() })
	}

	short := s.ID
	if len(short) > 12 {
		short = short[:12]
	}
	name := tree.Name + "-" + short + "." + format
	hdr := w.Header()
	if format == "zip" {
		hdr.Set("Content-Type", "application/zip")
	} else {
		hdr.Set("Content-Type", "application/gzip")
	}
	if cd := mime.FormatMediaType("attachment", map[string]string{"filename": name}); cd != "" {
		hdr.Set("Content-Disposition", cd)
	}
	hdr.Set("X-Content-Type-Options", "nosniff")
	hdr.Set("X-Archive-Omitted", strconv.Itoa(omitted))
	a.audit(r, "tree.export", t.ID, "tree", tree.ID, map[string]string{
		"snapshot": s.ID,
		"format":   format,
		"files":    strconv.Itoa(len(files)),
		"omitted":  strconv.Itoa(omitted),
	})
	w.WriteHeader(http.StatusOK)

	read := func(f archiveFile) ([]byte, error) {
		if f.data != nil {
			return f.data, nil
		}
		return content.ReadEntry(ctx, a.d.Objects, f.entry)
	}
	if format == "zip" {
		err = writeZipArchive(w, files, s.CreatedAt, read)
	} else {
		err = writeTarGzArchive(w, files, s.CreatedAt, read)
	}
	if err != nil {
		// The status is already sent: abort the connection so the client
		// sees a truncated download rather than a valid-looking archive.
		a.log.ErrorContext(ctx, "archive export failed", "tree", tree.ID, "snapshot", s.ID, "error", err)
		panic(http.ErrAbortHandler)
	}
	return nil
}

// archiveSnapshot resolves {snapshot}: a snapshot id of the tree, else the
// tip of a branch of that name.
func (a *api) archiveSnapshot(ctx context.Context, tree *domain.Tree, ref string) (*domain.Snapshot, error) {
	s, err := a.treeSnapshot(ctx, tree, ref)
	if err == nil {
		return s, nil
	}
	b, berr := a.d.Store.GetBranch(ctx, tree.ID, ref)
	if berr != nil || b.Tip == "" {
		return nil, err
	}
	return a.treeSnapshot(ctx, tree, b.Tip)
}

// exportablePaths reports, per entry, whether the caller may read it: the
// license checker must allow it and no tree:read policy may deny the path.
// Errors other than a license denial fail the request (fail closed).
func (a *api) exportablePaths(r *http.Request, t *domain.Tenant, tree *domain.Tree, entries []*syncpb.ManifestEntry) ([]bool, error) {
	ctx := r.Context()
	p := principal(r)
	out := make([]bool, len(entries))
	paths := make([]string, len(entries))
	for i, e := range entries {
		paths[i] = e.GetPath()
		out[i] = true
	}
	if a.d.License != nil && len(paths) > 0 {
		err := a.d.License.CheckRead(ctx, p, tree, paths)
		var denied *domain.PushDenied
		switch {
		case err == nil:
		case errors.As(err, &denied):
			// Some path is restricted: find out which ones.
			for i, path := range paths {
				err := a.d.License.CheckRead(ctx, p, tree, []string{path})
				switch {
				case err == nil:
				case errors.As(err, &denied):
					out[i] = false
				default:
					return nil, err
				}
			}
		default:
			return nil, err
		}
	}
	if a.d.Access != nil {
		env := domain.RequestEnv{RemoteIP: remoteHost(r), Now: time.Now()}
		for i, path := range paths {
			if !out[i] {
				continue
			}
			d, err := a.d.Access.Evaluate(ctx, p, domain.ActTreeRead, domain.Resource{TenantID: t.ID, Tree: tree, Path: path}, env)
			if err != nil {
				return nil, err
			}
			if !d.Allowed {
				out[i] = false
			}
		}
	}
	return out, nil
}

// activeLicenseRules keeps general rules (no grantee) that have not
// expired: grantee-specific rules describe one tenant's rights and do not
// change a file's license.
func activeLicenseRules(rules []domain.LicenseRule, now time.Time) []domain.LicenseRule {
	out := make([]domain.LicenseRule, 0, len(rules))
	for _, r := range rules {
		if r.Grantee == "" && (r.ExpiresAt == nil || r.ExpiresAt.After(now)) {
			out = append(out, r)
		}
	}
	return out
}

// buildNotice renders LICENSES/NOTICE (same layout as the CLI): the SPDX id
// of every included path covered by a rule, grouped by SPDX id. It returns
// nil when no included path is covered and nothing was omitted.
func buildNotice(rules []domain.LicenseRule, files []archiveFile, omitted int, header []string) []byte {
	bySPDX := map[string][]string{}
	for _, f := range files {
		if rule, ok := license.Resolve(rules, f.entry.GetPath()); ok {
			bySPDX[rule.SPDX] = append(bySPDX[rule.SPDX], f.entry.GetPath())
		}
	}
	if len(bySPDX) == 0 && omitted == 0 {
		return nil
	}
	var b strings.Builder
	b.WriteString("W0rkTree license notice\n=======================\n\n")
	for _, line := range header {
		b.WriteString(line + "\n")
	}
	if len(header) > 0 {
		b.WriteString("\n")
	}
	b.WriteString("SPDX license identifiers of the files in this export, from the tree's\n" +
		"license rules. Files that are not listed are not covered by a rule.\n")
	ids := make([]string, 0, len(bySPDX))
	for id := range bySPDX {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	for _, id := range ids {
		b.WriteString("\n" + id + "\n")
		paths := bySPDX[id]
		sort.Strings(paths)
		for _, p := range paths {
			b.WriteString("    " + p + "\n")
		}
	}
	switch {
	case omitted == 1:
		b.WriteString("\n1 path was omitted: it is not readable by the requester.\n")
	case omitted > 1:
		fmt.Fprintf(&b, "\n%d paths were omitted: they are not readable by the requester.\n", omitted)
	}
	return []byte(b.String())
}

func fileMode(e *syncpb.ManifestEntry) int64 {
	if e.GetExecutable() {
		return 0o755
	}
	return 0o644
}

func writeTarGzArchive(w io.Writer, files []archiveFile, mtime time.Time, read func(archiveFile) ([]byte, error)) error {
	gz := gzip.NewWriter(w)
	tw := tar.NewWriter(gz)
	for _, f := range files {
		data, err := read(f)
		if err != nil {
			return fmt.Errorf("%s: %w", f.entry.GetPath(), err)
		}
		h := &tar.Header{
			Typeflag: tar.TypeReg,
			Name:     f.entry.GetPath(),
			Mode:     fileMode(f.entry),
			Size:     int64(len(data)),
			ModTime:  mtime.UTC(),
		}
		if err := tw.WriteHeader(h); err != nil {
			return err
		}
		if _, err := tw.Write(data); err != nil {
			return err
		}
	}
	if err := tw.Close(); err != nil {
		return err
	}
	return gz.Close()
}

func writeZipArchive(w io.Writer, files []archiveFile, mtime time.Time, read func(archiveFile) ([]byte, error)) error {
	zw := zip.NewWriter(w)
	for _, f := range files {
		data, err := read(f)
		if err != nil {
			return fmt.Errorf("%s: %w", f.entry.GetPath(), err)
		}
		h := &zip.FileHeader{Name: f.entry.GetPath(), Method: zip.Deflate, Modified: mtime.UTC()}
		h.SetMode(0o644)
		if f.entry.GetExecutable() {
			h.SetMode(0o755)
		}
		fw, err := zw.CreateHeader(h)
		if err != nil {
			return err
		}
		if _, err := fw.Write(data); err != nil {
			return err
		}
	}
	return zw.Close()
}
