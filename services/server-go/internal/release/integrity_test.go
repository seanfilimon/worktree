package release

// v0.7 trust (§15.8): asset checksums, verified releases, SBOM.

import (
	"bytes"
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

func hexSHA256(b []byte) string {
	s := sha256.Sum256(b)
	return hex.EncodeToString(s[:])
}

func TestAssetSHA256AndChecksums(t *testing.T) {
	f := newFixture(t)
	_, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v1.0"})
	must(t, err)
	a, err := f.svc.PutAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", "b.bin", "", bytes.NewReader([]byte("bravo")))
	must(t, err)
	if a.SHA256 != hexSHA256([]byte("bravo")) {
		t.Fatalf("sha256 = %s", a.SHA256)
	}
	_, err = f.svc.PutAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", "a.txt", "", bytes.NewReader([]byte("alpha")))
	must(t, err)

	// An asset uploaded before v0.7 (no SHA256 recorded) is hashed on read.
	old := []byte("legacy bytes")
	h := hash.Sum(old)
	must(t, f.objs.Put(f.ctx, h, old))
	must(t, meta.UpdateDocT(f.ctx, f.store, Coll, Key(f.tree.ID, "v1.0"), func(r *domain.Release) (*domain.Release, error) {
		r.Assets = append(r.Assets, domain.ReleaseAsset{Name: "c.old", Size: int64(len(old)), Hash: h})
		return r, nil
	}))

	sums, err := f.svc.Checksums(f.ctx, f.tree.ID, "v1.0", false)
	must(t, err)
	want := hexSHA256([]byte("alpha")) + "  a.txt\n" +
		hexSHA256([]byte("bravo")) + "  b.bin\n" +
		hexSHA256(old) + "  c.old\n"
	if sums != want {
		t.Fatalf("checksums =\n%s\nwant\n%s", sums, want)
	}
	if _, err := f.svc.Checksums(f.ctx, f.tree.ID, "nope", true); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing release: %v", err)
	}
}

func TestReleaseVerifiedFollowsTag(t *testing.T) {
	f := newFixture(t)
	must(t, f.store.PutTag(f.ctx, &domain.Tag{TreeID: f.tree.ID, Name: "v2.0", Target: "snap-2", CreatedAt: t0,
		Signature: []byte{1}, SigningKeyID: "k1", Verified: true, Signer: "alice"}))
	r, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v2.0"})
	must(t, err)
	if !r.Verified || r.SigningKeyID != "k1" || r.Signer != "alice" {
		t.Fatalf("created = %+v", r)
	}
	plain, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v1.0"})
	must(t, err)
	if plain.Verified {
		t.Fatal("unsigned tag release verified")
	}
	got, err := f.svc.Get(f.ctx, f.tree.ID, "v2.0", false)
	must(t, err)
	if !got.Verified {
		t.Fatal("get: not verified")
	}
	list, err := f.svc.List(f.ctx, f.tree.ID, false)
	must(t, err)
	for _, r := range list {
		if r.Verified != (r.Tag == "v2.0") {
			t.Fatalf("list %s verified=%v", r.Tag, r.Verified)
		}
	}
	// The stored document never carries the computed fields.
	stored, err := meta.GetDocT[domain.Release](f.ctx, f.store, Coll, Key(f.tree.ID, "v2.0"))
	must(t, err)
	if stored.Verified || stored.Signer != "" {
		t.Fatalf("stored = %+v", stored)
	}
	// A tag recreated elsewhere no longer vouches for the release.
	must(t, f.store.DeleteTag(f.ctx, f.tree.ID, "v2.0"))
	must(t, f.store.PutTag(f.ctx, &domain.Tag{TreeID: f.tree.ID, Name: "v2.0", Target: "snap-1", CreatedAt: t0, Verified: true}))
	got, err = f.svc.Get(f.ctx, f.tree.ID, "v2.0", false)
	must(t, err)
	if got.Verified {
		t.Fatal("moved tag still verifies the release")
	}
}

func TestCreateCheckTagRefusesNewTag(t *testing.T) {
	f := newFixture(t)
	denied := &domain.PushDenied{Kind: "signature_required", Reason: "must be signed"}
	_, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v9", SnapshotID: "snap-2",
		CheckTag: func(*domain.Tag) error { return denied }})
	if !errors.Is(err, denied) && !errors.As(err, new(*domain.PushDenied)) {
		t.Fatalf("err = %v", err)
	}
	if _, err := f.store.GetTag(f.ctx, f.tree.ID, "v9"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("tag created: %v", err)
	}
	// Existing tags are not re-checked.
	_, err = f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v1.0",
		CheckTag: func(*domain.Tag) error { return denied }})
	must(t, err)
}

func putFile(t *testing.T, f *fixture, path string, data []byte) *syncpb.ManifestEntry {
	t.Helper()
	h := hash.Sum(data)
	must(t, f.objs.Put(f.ctx, h, data))
	return &syncpb.ManifestEntry{Path: path, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data))}
}

func TestGenerateAndAttachSBOM(t *testing.T) {
	f := newFixture(t)
	_, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v1.0"})
	must(t, err)
	readme := []byte("# web\n")
	lib := []byte("package lib\n")
	vendor := []byte("vendored\n")
	entries := []*syncpb.ManifestEntry{
		putFile(t, f, "src/lib.go", lib),
		putFile(t, f, "README.md", readme),
		putFile(t, f, "vendor/x.txt", vendor),
	}
	rules := []domain.LicenseRule{
		{TreeID: f.tree.ID, PathGlob: "**", SPDX: "MIT"},
		{TreeID: f.tree.ID, PathGlob: "vendor/**", SPDX: "LicenseRef-Acme OR Apache-2.0"},
	}
	data, err := f.svc.GenerateSBOM(f.ctx, SBOMInput{
		Tenant: "acme", Tree: "web", Tag: "v1.0", SnapshotID: "snap-1",
		Namespace: "https://wt.example.com/spdx/", Creator: "alice", Tool: "W0rkTree-0.7.0",
		Entries: entries, Omitted: 2, Rules: rules,
	})
	must(t, err)

	var doc map[string]any
	must(t, json.Unmarshal(data, &doc))
	if doc["spdxVersion"] != "SPDX-2.3" || doc["dataLicense"] != "CC0-1.0" || doc["SPDXID"] != "SPDXRef-DOCUMENT" {
		t.Fatalf("header = %v", doc)
	}
	if doc["name"] != "acme/web@v1.0" || !strings.HasPrefix(doc["documentNamespace"].(string), "https://wt.example.com/spdx/acme/web/v1.0/") {
		t.Fatalf("name/namespace = %v %v", doc["name"], doc["documentNamespace"])
	}
	ci := doc["creationInfo"].(map[string]any)
	if ci["created"] != "2026-05-06T07:08:09Z" || len(ci["creators"].([]any)) != 2 {
		t.Fatalf("creationInfo = %v", ci)
	}
	files := doc["files"].([]any)
	if len(files) != 3 {
		t.Fatalf("files = %v", files)
	}
	first := files[0].(map[string]any)
	if first["fileName"] != "./README.md" || first["licenseConcluded"] != "MIT" {
		t.Fatalf("first file = %v", first)
	}
	sums := map[string]string{}
	for _, c := range first["checksums"].([]any) {
		c := c.(map[string]any)
		sums[c["algorithm"].(string)] = c["checksumValue"].(string)
	}
	s1 := sha1.Sum(readme)
	if sums["SHA1"] != hex.EncodeToString(s1[:]) || sums["SHA256"] != hexSHA256(readme) || sums["BLAKE3"] != hash.Sum(readme).Hex() {
		t.Fatalf("checksums = %v", sums)
	}
	if v := files[2].(map[string]any); v["licenseConcluded"] != "LicenseRef-Acme OR Apache-2.0" {
		t.Fatalf("vendor = %v", v)
	}
	extracted := doc["hasExtractedLicensingInfos"].([]any)
	if len(extracted) != 1 || extracted[0].(map[string]any)["licenseId"] != "LicenseRef-Acme" {
		t.Fatalf("extracted = %v", extracted)
	}
	pkg := doc["packages"].([]any)[0].(map[string]any)
	if pkg["versionInfo"] != "v1.0" || pkg["filesAnalyzed"] != true || !strings.Contains(pkg["comment"].(string), "2 path(s)") {
		t.Fatalf("package = %v", pkg)
	}
	code := pkg["packageVerificationCode"].(map[string]any)["packageVerificationCodeValue"].(string)
	if len(code) != 40 {
		t.Fatalf("verification code = %q", code)
	}
	if rels := doc["relationships"].([]any); len(rels) != 4 {
		t.Fatalf("relationships = %v", rels)
	}

	asset, err := f.svc.AttachSBOM(f.ctx, f.p, f.tenant, f.tree, "v1.0", data)
	must(t, err)
	if asset.Name != SBOMAssetName || asset.ContentType != SBOMContentType || asset.SHA256 != hexSHA256(data) {
		t.Fatalf("asset = %+v", asset)
	}

	// No rules: NOASSERTION everywhere, no extracted licenses.
	data, err = f.svc.GenerateSBOM(f.ctx, SBOMInput{Tenant: "acme", Tree: "web", Tag: "v1.0", Entries: entries[:1]})
	must(t, err)
	if strings.Contains(string(data), "hasExtractedLicensingInfos") || !strings.Contains(string(data), `"licenseInfoFromFiles": [
        "NOASSERTION"`) {
		t.Fatalf("no-rule sbom:\n%s", data)
	}
	// Missing content fails.
	missing := &syncpb.ManifestEntry{Path: "gone", BlobHash: &syncpb.ContentHash{Value: make([]byte, 32)}}
	if _, err := f.svc.GenerateSBOM(f.ctx, SBOMInput{Entries: []*syncpb.ManifestEntry{missing}}); err == nil {
		t.Fatal("missing blob accepted")
	}
}
