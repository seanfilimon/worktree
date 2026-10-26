package release

// v0.7 trust (docs/ENGINEERING_PLAN.md §15.8): release integrity.
//
//   - Every asset records the hex SHA-256 of its bytes (ReleaseAsset.SHA256,
//     computed on upload; assets uploaded before v0.7 get it computed when
//     checksums are requested).
//   - Checksums renders a SHA256SUMS file ("<hex>  <name>\n", by name).
//   - A release is verified when its tag carries a verified signature
//     (Release.Verified, computed on every read, never stored).
//   - GenerateSBOM renders an SPDX 2.3 JSON document of the files of the
//     tag's snapshot (SHA1, SHA256 and BLAKE3 checksums, license rule SPDX
//     ids); AttachSBOM stores it as the asset "sbom.spdx.json".

import (
	"bytes"
	"context"
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"slices"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// SBOMAssetName is the asset name of a generated SBOM.
const SBOMAssetName = "sbom.spdx.json"

// SBOMContentType is the media type of SPDX JSON documents.
const SBOMContentType = "application/spdx+json"

// MaxSBOMFiles bounds the files listed in one SBOM.
const MaxSBOMFiles = 200_000

func sha256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// decorate sets the computed trust fields of releases of treeID from their
// tags (best effort: a missing tag leaves a release unverified).
func (s *Service) decorate(ctx context.Context, treeID string, rels []domain.Release) {
	for i := range rels {
		r := &rels[i]
		r.Verified, r.SigningKeyID, r.Signer = false, "", ""
		tag, err := s.d.Store.GetTag(ctx, treeID, r.Tag)
		if err != nil {
			continue
		}
		// A tag moved (force-recreated) away from the release's snapshot
		// does not vouch for it.
		if tag.Verified && tag.Target == r.SnapshotID {
			r.Verified, r.SigningKeyID, r.Signer = true, tag.SigningKeyID, tag.Signer
		}
	}
}

// Checksums returns the SHA256SUMS text of the assets of the release of
// tag ("<hex sha256>  <name>\n" lines sorted by name).
func (s *Service) Checksums(ctx context.Context, treeID, tag string, withDrafts bool) (string, error) {
	r, err := s.Get(ctx, treeID, tag, withDrafts)
	if err != nil {
		return "", err
	}
	assets := slices.Clone(r.Assets)
	slices.SortFunc(assets, func(a, b domain.ReleaseAsset) int { return strings.Compare(a.Name, b.Name) })
	var b strings.Builder
	for _, a := range assets {
		sum := a.SHA256
		if sum == "" {
			if s.d.Objects == nil {
				return "", errors.New("release: no object store configured")
			}
			data, err := s.d.Objects.Get(ctx, a.Hash)
			if err != nil {
				if errors.Is(err, objstore.ErrNotFound) {
					return "", errorf(domain.ErrNotFound, "content of asset %q is missing", a.Name)
				}
				return "", err
			}
			sum = sha256Hex(data)
		}
		fmt.Fprintf(&b, "%s  %s\n", sum, a.Name)
	}
	return b.String(), nil
}

// SBOMInput describes the snapshot an SBOM covers.
type SBOMInput struct {
	Tenant     string // tenant slug
	Tree       string // tree name
	Tag        string
	SnapshotID string
	// Namespace is the URI prefix of the document namespace (a unique
	// suffix is appended), e.g. "https://wt.example.com/spdx".
	Namespace string
	Creator   string // username of the requester
	Tool      string // e.g. "W0rkTree-0.7.0"
	// Entries are the manifest entries to list (already filtered to the
	// paths the requester may read); Omitted counts the others.
	Entries []*syncpb.ManifestEntry
	Omitted int
	// Rules are the tree's license rules in effect (general, unexpired).
	Rules []domain.LicenseRule
	// Now overrides the creation time (tests); zero = the service clock.
	Now time.Time
}

// SPDX 2.3 JSON shapes (the subset W0rkTree writes).
type (
	spdxDocument struct {
		SPDXVersion       string              `json:"spdxVersion"`
		DataLicense       string              `json:"dataLicense"`
		SPDXID            string              `json:"SPDXID"`
		Name              string              `json:"name"`
		DocumentNamespace string              `json:"documentNamespace"`
		CreationInfo      spdxCreationInfo    `json:"creationInfo"`
		Packages          []spdxPackage       `json:"packages"`
		Files             []spdxFile          `json:"files"`
		Relationships     []spdxRelationship  `json:"relationships"`
		ExtractedLicenses []spdxExtractedInfo `json:"hasExtractedLicensingInfos,omitempty"`
	}
	spdxCreationInfo struct {
		Created  string   `json:"created"`
		Creators []string `json:"creators"`
	}
	spdxPackage struct {
		Name                 string           `json:"name"`
		SPDXID               string           `json:"SPDXID"`
		VersionInfo          string           `json:"versionInfo"`
		DownloadLocation     string           `json:"downloadLocation"`
		FilesAnalyzed        bool             `json:"filesAnalyzed"`
		VerificationCode     spdxVerification `json:"packageVerificationCode"`
		LicenseConcluded     string           `json:"licenseConcluded"`
		LicenseDeclared      string           `json:"licenseDeclared"`
		LicenseInfoFromFiles []string         `json:"licenseInfoFromFiles"`
		CopyrightText        string           `json:"copyrightText"`
		Comment              string           `json:"comment,omitempty"`
	}
	spdxVerification struct {
		Value string `json:"packageVerificationCodeValue"`
	}
	spdxFile struct {
		FileName           string         `json:"fileName"`
		SPDXID             string         `json:"SPDXID"`
		Checksums          []spdxChecksum `json:"checksums"`
		LicenseConcluded   string         `json:"licenseConcluded"`
		LicenseInfoInFiles []string       `json:"licenseInfoInFiles"`
		CopyrightText      string         `json:"copyrightText"`
	}
	spdxChecksum struct {
		Algorithm string `json:"algorithm"`
		Value     string `json:"checksumValue"`
	}
	spdxRelationship struct {
		Element string `json:"spdxElementId"`
		Type    string `json:"relationshipType"`
		Related string `json:"relatedSpdxElement"`
	}
	spdxExtractedInfo struct {
		LicenseID     string `json:"licenseId"`
		ExtractedText string `json:"extractedText"`
		Name          string `json:"name,omitempty"`
	}
)

const noAssertion = "NOASSERTION"

var licenseRefRe = regexp.MustCompile(`(?:DocumentRef-[A-Za-z0-9.\-]+:)?LicenseRef-[A-Za-z0-9.\-]+`)

// GenerateSBOM renders the SPDX 2.3 JSON document of in. File contents are
// read from the object store to compute SHA1 and SHA256 (BLAKE3 is the
// content address already in the manifest).
func (s *Service) GenerateSBOM(ctx context.Context, in SBOMInput) ([]byte, error) {
	if s.d.Objects == nil {
		return nil, errors.New("release: no object store configured")
	}
	if len(in.Entries) > MaxSBOMFiles {
		return nil, errorf(domain.ErrInvalid, "the snapshot has %d files; an SBOM lists at most %d", len(in.Entries), MaxSBOMFiles)
	}
	now := in.Now
	if now.IsZero() {
		now = s.now()
	}
	entries := slices.Clone(in.Entries)
	slices.SortFunc(entries, func(a, b *syncpb.ManifestEntry) int { return strings.Compare(a.GetPath(), b.GetPath()) })

	files := make([]spdxFile, 0, len(entries))
	rels := []spdxRelationship{{Element: "SPDXRef-DOCUMENT", Type: "DESCRIBES", Related: "SPDXRef-Package"}}
	sha1s := make([]string, 0, len(entries))
	licenseIDs := map[string]bool{}
	refs := map[string]bool{}
	for i, e := range entries {
		h, err := content.EntryHash(e)
		if err != nil {
			return nil, err
		}
		data, err := content.ReadEntry(ctx, s.d.Objects, e)
		if err != nil {
			return nil, fmt.Errorf("release: sbom: read %s: %w", e.GetPath(), err)
		}
		s1 := sha1.Sum(data)
		s1hex := hex.EncodeToString(s1[:])
		sha1s = append(sha1s, s1hex)
		concluded := noAssertion
		if rule, ok := license.Resolve(in.Rules, e.GetPath()); ok && strings.TrimSpace(rule.SPDX) != "" {
			concluded = strings.TrimSpace(rule.SPDX)
			licenseIDs[concluded] = true
			for _, ref := range licenseRefRe.FindAllString(concluded, -1) {
				if !strings.HasPrefix(ref, "DocumentRef-") {
					refs[ref] = true
				}
			}
		}
		checksums := []spdxChecksum{
			{Algorithm: "SHA1", Value: s1hex},
			{Algorithm: "SHA256", Value: sha256Hex(data)},
		}
		if !e.GetChunked() {
			// The manifest hash of a whole file is BLAKE3 of its bytes; a
			// chunked file's hash names its chunk list instead.
			checksums = append(checksums, spdxChecksum{Algorithm: "BLAKE3", Value: h.Hex()})
		}
		id := fmt.Sprintf("SPDXRef-File-%d", i+1)
		files = append(files, spdxFile{
			FileName:           "./" + e.GetPath(),
			SPDXID:             id,
			Checksums:          checksums,
			LicenseConcluded:   concluded,
			LicenseInfoInFiles: []string{noAssertion},
			CopyrightText:      noAssertion,
		})
		rels = append(rels, spdxRelationship{Element: "SPDXRef-Package", Type: "CONTAINS", Related: id})
	}
	slices.Sort(sha1s)
	code := sha1.Sum([]byte(strings.Join(sha1s, "")))

	fromFiles := make([]string, 0, len(licenseIDs))
	for id := range licenseIDs {
		fromFiles = append(fromFiles, id)
	}
	slices.Sort(fromFiles)
	if len(fromFiles) == 0 {
		fromFiles = []string{noAssertion}
	}
	var extracted []spdxExtractedInfo
	for ref := range refs {
		extracted = append(extracted, spdxExtractedInfo{
			LicenseID:     ref,
			Name:          ref,
			ExtractedText: "Declared by a W0rkTree license rule; the license text is not part of the tree's metadata.",
		})
	}
	slices.SortFunc(extracted, func(a, b spdxExtractedInfo) int { return strings.Compare(a.LicenseID, b.LicenseID) })

	comment := fmt.Sprintf("Snapshot %s of %s/%s, tag %s.", in.SnapshotID, in.Tenant, in.Tree, in.Tag)
	if in.Omitted > 0 {
		comment += fmt.Sprintf(" %d path(s) not readable by the requester are omitted.", in.Omitted)
	}
	creators := []string{"Tool: " + orDefault(in.Tool, "W0rkTree")}
	if in.Creator != "" {
		creators = append(creators, "Person: "+in.Creator)
	}
	doc := spdxDocument{
		SPDXVersion:       "SPDX-2.3",
		DataLicense:       "CC0-1.0",
		SPDXID:            "SPDXRef-DOCUMENT",
		Name:              fmt.Sprintf("%s/%s@%s", in.Tenant, in.Tree, in.Tag),
		DocumentNamespace: fmt.Sprintf("%s/%s/%s/%s/%s", strings.TrimRight(orDefault(in.Namespace, "https://spdx.worktree.invalid"), "/"), uriSeg(in.Tenant), uriSeg(in.Tree), uriSeg(in.Tag), newID()),
		CreationInfo:      spdxCreationInfo{Created: now.UTC().Format("2006-01-02T15:04:05Z"), Creators: creators},
		Packages: []spdxPackage{{
			Name:                 in.Tree,
			SPDXID:               "SPDXRef-Package",
			VersionInfo:          in.Tag,
			DownloadLocation:     noAssertion,
			FilesAnalyzed:        true,
			VerificationCode:     spdxVerification{Value: hex.EncodeToString(code[:])},
			LicenseConcluded:     noAssertion,
			LicenseDeclared:      noAssertion,
			LicenseInfoFromFiles: fromFiles,
			CopyrightText:        noAssertion,
			Comment:              comment,
		}},
		Files:             files,
		Relationships:     rels,
		ExtractedLicenses: extracted,
	}
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	enc.SetIndent("", "  ")
	if err := enc.Encode(doc); err != nil {
		return nil, fmt.Errorf("release: encode sbom: %w", err)
	}
	return buf.Bytes(), nil
}

// AttachSBOM stores data as the SBOM asset of the release of tag.
func (s *Service) AttachSBOM(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree, tag string, data []byte) (*domain.ReleaseAsset, error) {
	return s.PutAsset(ctx, p, tenant, tree, tag, SBOMAssetName, SBOMContentType, bytes.NewReader(data))
}

func orDefault(v, def string) string {
	if v == "" {
		return def
	}
	return v
}

// uriSeg keeps URI-safe characters of v and replaces the others with '-'.
func uriSeg(v string) string {
	return strings.Map(func(r rune) rune {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9', r == '.', r == '_', r == '-':
			return r
		}
		return '-'
	}, v)
}
