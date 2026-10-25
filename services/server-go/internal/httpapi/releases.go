package httpapi

import (
	"bytes"
	"errors"
	"fmt"
	"mime"
	"net/http"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/release"
)

// registerReleaseRoutes registers the v0.5 routes for releases and release
// assets (docs/ENGINEERING_PLAN.md §13.5). Reads need tree:read (drafts
// are visible only with release:create), creating, editing and uploading
// release:create, deleting releases and assets release:delete. Tags
// containing '/' are passed percent-encoded (%2F) in the path. Without
// Deps.Releases they answer 501.
func (a *api) registerReleaseRoutes(tree string) {
	a.handle("GET "+tree+"/releases", a.handleListReleases)
	a.handle("POST "+tree+"/releases", a.handleCreateRelease)
	a.handle("GET "+tree+"/releases/{tag}", a.handleGetRelease)
	a.handle("PATCH "+tree+"/releases/{tag}", a.handlePatchRelease)
	a.handle("DELETE "+tree+"/releases/{tag}", a.handleDeleteRelease)
	a.handle("PUT "+tree+"/releases/{tag}/assets/{name}", a.handlePutReleaseAsset)
	a.handle("GET "+tree+"/releases/{tag}/assets/{name}", a.handleGetReleaseAsset)
	a.handle("DELETE "+tree+"/releases/{tag}/assets/{name}", a.handleDeleteReleaseAsset)
}

func (a *api) releases() (*release.Service, error) {
	if a.d.Releases == nil {
		return nil, errIAMNotImplemented("releases")
	}
	return a.d.Releases, nil
}

// canSeeDrafts reports whether the caller may see draft releases of tree.
func (a *api) canSeeDrafts(r *http.Request, tree *domain.Tree) bool {
	return a.d.Authz.Authorize(r.Context(), principal(r), domain.ActReleaseCreate, tree.TenantID, tree) == nil
}

type releaseList struct {
	Releases []domain.Release `json:"releases"`
}

// handleListReleases answers GET {tree}/releases -> {"releases":[Release]}
// (newest first).
func (a *api) handleListReleases(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.releases()
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	list, err := svc.List(r.Context(), tree.ID, a.canSeeDrafts(r, tree))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, releaseList{Releases: nonNil(list)})
	return nil
}

type createReleaseRequest struct {
	Tag        string `json:"tag"`
	Name       string `json:"name"`
	Notes      string `json:"notes"`
	Draft      bool   `json:"draft"`
	Prerelease bool   `json:"prerelease"`
	Snapshot   string `json:"snapshot"`
}

// handleCreateRelease answers POST {tree}/releases
// {tag, name?, notes?, draft?, prerelease?, snapshot?}. snapshot is a
// revision of the tree; the tag is created there when it does not exist.
func (a *api) handleCreateRelease(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.releases()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActReleaseCreate)
	if err != nil {
		return err
	}
	var req createReleaseRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	ctx := r.Context()
	if strings.TrimSpace(req.Tag) == "" {
		return badRequestf("tag is required")
	}
	if _, err := a.d.Store.GetTag(ctx, tree.ID, req.Tag); err != nil {
		if !errors.Is(err, domain.ErrNotFound) {
			return err
		}
		// A new tag: apply the ref-name rules.
		if err := validateRefNameManage("tag", req.Tag); err != nil {
			return err
		}
	}
	in := release.CreateInput{Tag: req.Tag, Name: req.Name, Notes: req.Notes, Draft: req.Draft, Prerelease: req.Prerelease}
	if req.Snapshot != "" {
		s, err := a.snapshotForRev(ctx, tree, req.Snapshot)
		if err != nil {
			return err
		}
		in.SnapshotID = s.ID
	}
	rel, err := svc.Create(ctx, p, t, tree, in)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusCreated, rel)
	return nil
}

// handleGetRelease answers GET {tree}/releases/{tag}.
func (a *api) handleGetRelease(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.releases()
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	rel, err := svc.Get(r.Context(), tree.ID, r.PathValue("tag"), a.canSeeDrafts(r, tree))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, rel)
	return nil
}

// handlePatchRelease answers PATCH {tree}/releases/{tag}
// {name?, notes?, draft?, prerelease?}.
func (a *api) handlePatchRelease(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.releases()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActReleaseCreate)
	if err != nil {
		return err
	}
	var patch release.Patch
	if err := decodeJSON(w, r, &patch); err != nil {
		return err
	}
	rel, err := svc.Update(r.Context(), p, t, tree, r.PathValue("tag"), patch)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, rel)
	return nil
}

// handleDeleteRelease answers DELETE {tree}/releases/{tag} (the tag stays).
func (a *api) handleDeleteRelease(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.releases()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActReleaseDelete)
	if err != nil {
		return err
	}
	if err := svc.Delete(r.Context(), p, t, tree, r.PathValue("tag")); err != nil {
		return err
	}
	w.WriteHeader(http.StatusNoContent)
	return nil
}

func errAssetTooLarge() error {
	return &apiError{status: http.StatusRequestEntityTooLarge, code: "payload_too_large",
		msg: fmt.Sprintf("release assets are limited to %d bytes", release.MaxAssetSize)}
}

// handlePutReleaseAsset answers PUT {tree}/releases/{tag}/assets/{name}
// with the raw asset bytes as the body (at most 64 MiB; Content-Type is
// recorded as the asset's media type) -> 201 ReleaseAsset.
func (a *api) handlePutReleaseAsset(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.releases()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActReleaseCreate)
	if err != nil {
		return err
	}
	if r.ContentLength > release.MaxAssetSize {
		return errAssetTooLarge()
	}
	body := http.MaxBytesReader(w, r.Body, release.MaxAssetSize)
	asset, err := svc.PutAsset(r.Context(), p, t, tree, r.PathValue("tag"), r.PathValue("name"), r.Header.Get("Content-Type"), body)
	if err != nil {
		var mbe *http.MaxBytesError
		var tle *release.TooLargeError
		if errors.As(err, &mbe) || errors.As(err, &tle) {
			return errAssetTooLarge()
		}
		return err
	}
	writeJSON(w, http.StatusCreated, asset)
	return nil
}

// handleGetReleaseAsset answers GET {tree}/releases/{tag}/assets/{name}
// with the asset bytes (Content-Type, Content-Disposition: attachment,
// ETag; Range requests are honoured).
func (a *api) handleGetReleaseAsset(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.releases()
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	asset, data, err := svc.Asset(r.Context(), tree.ID, r.PathValue("tag"), r.PathValue("name"), a.canSeeDrafts(r, tree))
	if err != nil {
		return err
	}
	h := w.Header()
	ct := asset.ContentType
	if ct == "" {
		ct = "application/octet-stream"
	}
	h.Set("Content-Type", ct)
	h.Set("X-Content-Type-Options", "nosniff")
	h.Set("Content-Disposition", mime.FormatMediaType("attachment", map[string]string{"filename": asset.Name}))
	h.Set("ETag", `"`+asset.Hash.Hex()+`"`)
	h.Set("Cache-Control", "private, no-cache")
	http.ServeContent(w, r, "", asset.UploadedAt, bytes.NewReader(data))
	return nil
}

// handleDeleteReleaseAsset answers DELETE {tree}/releases/{tag}/assets/{name}.
func (a *api) handleDeleteReleaseAsset(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.releases()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActReleaseDelete)
	if err != nil {
		return err
	}
	if err := svc.DeleteAsset(r.Context(), p, t, tree, r.PathValue("tag"), r.PathValue("name")); err != nil {
		return err
	}
	w.WriteHeader(http.StatusNoContent)
	return nil
}
