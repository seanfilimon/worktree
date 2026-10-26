package httpapi

// v0.7 trust (§15.8): signed REST tags, RequireSigned, verified releases,
// checksums and SBOM endpoints.

import (
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"log/slog"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/release"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
	"github.com/seanfilimon/worktree/services/server-go/internal/signing"
)

func newTrustFixture(t *testing.T) (*fixture, *depsStore) {
	t.Helper()
	var ds *depsStore
	f := newFixture(t, func(d *Deps) {
		ds = &depsStore{manageStore: &manageStore{fakeStore: d.Store.(*fakeStore)}, DocStore: meta.NewMemory()}
		d.Store = ds
		log := slog.New(slog.NewTextHandler(io.Discard, nil))
		d.Releases = release.New(release.Deps{Store: ds, Objects: d.Objects, History: d.History, Authz: d.Authz, Bus: d.Bus, Audit: d.Audit, Logger: log})
		d.Signing = signing.New(signing.Deps{Store: ds, Audit: d.Audit, Logger: log})
	})
	return f, ds
}

func TestTrustSignedTagsAndReleases(t *testing.T) {
	f, ds := newTrustFixture(t)
	ctx := context.Background()
	do := func(method, path, as string, body any, want int) *httptest.ResponseRecorder {
		t.Helper()
		rec := f.do(t, method, path, as, body)
		if rec.Code != want {
			t.Fatalf("%s %s as %q: %d, want %d; %s", method, path, as, rec.Code, want, rec.Body.String())
		}
		return rec
	}
	seed := make([]byte, ed25519.SeedSize)
	seed[0] = 42
	priv := ed25519.NewKeyFromSeed(seed)
	key := decode[domain.SigningKey](t, do("POST", "/api/v1/auth/keys", "alice",
		map[string]any{"name": "laptop", "public_key": []byte(priv.Public().(ed25519.PublicKey))}, 201))

	created := time.Date(2026, 6, 1, 2, 3, 4, 5006, time.UTC)
	signedBody := func(name, message string) map[string]any {
		tag := &domain.Tag{Name: name, Target: f.s2.ID, Message: message, SignedTagger: "Alice <alice@example.com>", CreatedAt: created}
		payload, err := signing.TagPayload(tag)
		must(t, err)
		return map[string]any{
			"name": name, "target": f.s2.ID, "message": message, "tagger": tag.SignedTagger,
			"created_at": created.Format(time.RFC3339Nano), "signing_key_id": key.ID,
			"signature": ed25519.Sign(priv, payload),
		}
	}

	// A signed REST tag is verified and keeps the signed fields.
	tag := decode[domain.Tag](t, do("POST", depsWebAPI+"/tags", "alice", signedBody("v2.0.0", "Two"), 201))
	if !tag.Verified || tag.Signer != "alice" || tag.Tagger != "alice" || tag.SignedTagger != "Alice <alice@example.com>" ||
		!tag.CreatedAt.Equal(created) || tag.SigningKeyID != key.ID || len(tag.Signature) != ed25519.SignatureSize {
		t.Fatalf("tag = %+v", tag)
	}
	// Tampering is refused (403 signature_required) and audited.
	bad := signedBody("v2.0.1", "One")
	bad["message"] = "changed"
	if rec := do("POST", depsWebAPI+"/tags", "alice", bad, 403); errCode(rec) != signing.KindSignatureRequired {
		t.Fatalf("code = %s", errCode(rec))
	}
	hasAudit(t, f, signing.AuditTagSignatureInvalid)
	// Signed tags need the snapshot id and created_at.
	byBranch := signedBody("v2.0.2", "x")
	byBranch["target"] = "main"
	do("POST", depsWebAPI+"/tags", "alice", byBranch, 400)
	noTime := signedBody("v2.0.3", "x")
	delete(noTime, "created_at")
	do("POST", depsWebAPI+"/tags", "alice", noTime, 400)
	// Unsigned tags still work, unverified.
	if plain := decode[domain.Tag](t, do("POST", depsWebAPI+"/tags", "alice", map[string]any{"name": "nightly", "target": "main"}, 201)); plain.Verified {
		t.Fatalf("plain = %+v", plain)
	}

	// RequireSigned tag protections refuse unsigned REST tags and the tags
	// releases would create; signed tags pass.
	must(t, ds.PutDoc(ctx, review.TagProtectionCollection, domain.DocKey(f.web.ID, "rel-*"),
		&domain.TagProtection{TreeID: f.web.ID, Pattern: "rel-*", RequireSigned: true}))
	if rec := do("POST", depsWebAPI+"/tags", "alice", map[string]any{"name": "rel-1", "target": "main"}, 403); !strings.Contains(rec.Body.String(), "must be signed") {
		t.Fatalf("body = %s", rec.Body.String())
	}
	do("POST", depsWebAPI+"/releases", "alice", map[string]any{"tag": "rel-2", "snapshot": "main"}, 403)
	do("POST", depsWebAPI+"/tags", "alice", signedBody("rel-3", "signed"), 201)

	// Releases of verified tags are verified.
	rel := decode[domain.Release](t, do("POST", depsWebAPI+"/releases", "alice", map[string]any{"tag": "v2.0.0"}, 201))
	if !rel.Verified || rel.Signer != "alice" || rel.SigningKeyID != key.ID {
		t.Fatalf("release = %+v", rel)
	}
	if r := decode[domain.Release](t, do("GET", depsWebAPI+"/releases/v2.0.0", "carol", nil, 200)); !r.Verified {
		t.Fatalf("get = %+v", r)
	}
	if r := decode[domain.Release](t, do("POST", depsWebAPI+"/releases", "alice", map[string]any{"tag": "v1"}, 201)); r.Verified {
		t.Fatalf("unsigned release = %+v", r)
	}

	// Assets carry sha256; checksums list them.
	req := httptest.NewRequest("PUT", depsWebAPI+"/releases/v2.0.0/assets/app.zip", strings.NewReader("PK-zip-bytes"))
	req.Header.Set("Authorization", "Bearer alice")
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	asset := decode[domain.ReleaseAsset](t, rec)
	sum := sha256.Sum256([]byte("PK-zip-bytes"))
	if asset.SHA256 != hex.EncodeToString(sum[:]) {
		t.Fatalf("asset = %+v", asset)
	}
	sums := do("GET", depsWebAPI+"/releases/v2.0.0/checksums", "carol", nil, 200)
	if got := sums.Body.String(); got != asset.SHA256+"  app.zip\n" || !strings.HasPrefix(sums.Header().Get("Content-Type"), "text/plain") {
		t.Fatalf("checksums = %q (%s)", got, sums.Header().Get("Content-Type"))
	}
	do("GET", depsWebAPI+"/releases/nope/checksums", "carol", nil, 404)
	do("GET", depsWebAPI+"/releases/v2.0.0/checksums", "dave", nil, 403)

	// SBOM: release:create only; lists readable files of the snapshot.
	do("POST", depsWebAPI+"/releases/v2.0.0/sbom", "carol", nil, 403)
	do("POST", depsWebAPI+"/releases/nope/sbom", "alice", nil, 404)
	sbom := decode[domain.ReleaseAsset](t, do("POST", depsWebAPI+"/releases/v2.0.0/sbom", "alice", nil, 201))
	if sbom.Name != release.SBOMAssetName || sbom.ContentType != release.SBOMContentType || sbom.SHA256 == "" {
		t.Fatalf("sbom asset = %+v", sbom)
	}
	hasAudit(t, f, "release.asset.upload")
	body := do("GET", depsWebAPI+"/releases/v2.0.0/assets/sbom.spdx.json", "carol", nil, 200).Body.Bytes()
	var doc struct {
		SPDXVersion string `json:"spdxVersion"`
		Name        string `json:"name"`
		Packages    []struct {
			Comment string `json:"comment"`
		} `json:"packages"`
		Files []struct {
			FileName string `json:"fileName"`
		} `json:"files"`
	}
	must(t, json.Unmarshal(body, &doc))
	names := []string{}
	for _, fl := range doc.Files {
		names = append(names, fl.FileName)
	}
	joined := strings.Join(names, ",")
	if doc.SPDXVersion != "SPDX-2.3" || doc.Name != "acme/web@v2.0.0" || !strings.Contains(joined, "./README.md") ||
		strings.Contains(joined, "secret/") || !strings.Contains(doc.Packages[0].Comment, "1 path(s)") {
		t.Fatalf("sbom = %s", body)
	}
	if got := do("GET", depsWebAPI+"/releases/v2.0.0/checksums", "carol", nil, 200).Body.String(); !strings.Contains(got, "  sbom.spdx.json\n") {
		t.Fatalf("checksums after sbom = %q", got)
	}
}

func TestTrustWithoutSigningDropsSignatures(t *testing.T) {
	f := newDepsFixture(t)
	rec := f.do(t, "POST", depsWebAPI+"/tags", "alice", map[string]any{
		"name": "v5", "target": f.s2.ID, "signature": []byte{1, 2, 3}, "signing_key_id": "k",
		"created_at": time.Now().UTC().Format(time.RFC3339Nano),
	})
	if rec.Code != 201 {
		t.Fatalf("create: %d %s", rec.Code, rec.Body.String())
	}
	if tag := decode[domain.Tag](t, rec); tag.Verified || len(tag.Signature) != 0 || tag.SigningKeyID != "" {
		t.Fatalf("tag = %+v", tag)
	}
}
