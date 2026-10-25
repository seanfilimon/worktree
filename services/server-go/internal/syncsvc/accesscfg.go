package syncsvc

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// MaxAccessConfigSize bounds access-config documents.
const MaxAccessConfigSize = 1 << 20

// maxValidationErrors caps the number of validation messages returned.
const maxValidationErrors = 20

// accessConfigKinds maps wire config types to the MetaStore kind key.
var accessConfigKinds = map[syncpb.AccessConfigType]string{
	syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES:             "roles",
	syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES:          "policies",
	syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_TENANT_ACCESS:     "tenant_access",
	syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_BRANCH_PROTECTION: "branch_protection",
	syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_LICENSE:           "license",
}

// SyncAccessConfig applies or stores an access-config document for a
// tenant (tenant_id may hold the tenant's id or its slug).
//
// v0.5 declarative documents (§13.7) — config_type ROLES or POLICIES with
// canonical JSON content `{"roles":[...],"policies":[...]}`, optionally
// with tree_id (a tree-level, deny-only document) and dry_run — are
// validated and applied by accesscfg.Service.Apply; the response carries
// the plan in `changes`, and refusals of the document (validation,
// privilege escalation) come back as accepted=false with
// validation_errors.
//
// Anything else is the v0.4 behaviour: the caller must be a tenant admin
// and the raw document must be non-empty UTF-8 that passes a structural
// TOML check (see validateTOMLish); it is stored as is.
func (s *Service) SyncAccessConfig(ctx context.Context, req *syncpb.AccessConfigSyncRequest) (*syncpb.AccessConfigSyncResponse, error) {
	p, err := requireAuth(ctx)
	if err != nil {
		return nil, err
	}
	tenant, err := s.accessTenant(ctx, req.GetTenantId())
	if err != nil {
		return nil, err
	}
	if declarative(req) {
		return s.applyAccessDocument(ctx, p, tenant, req)
	}
	if err := s.authorize(ctx, p, domain.ActTenantAdmin, tenant.ID, nil); err != nil {
		return nil, err
	}

	kind, ok := accessConfigKinds[req.GetConfigType()]
	if !ok {
		return reject("config_type is unspecified or unknown"), nil
	}
	content := req.GetContent()
	if len(content) > MaxAccessConfigSize {
		return reject(fmt.Sprintf("content is larger than %d bytes", MaxAccessConfigSize)), nil
	}
	if ch := req.GetConfigHash(); ch != nil && len(ch.GetValue()) > 0 {
		want, err := hashFromPB(ch)
		if err != nil {
			return reject("config_hash: " + err.Error()), nil
		}
		if hash.Sum(content) != want {
			return reject("config_hash does not match BLAKE3(content)"), nil
		}
	}
	if errs := validateTOMLish(content); len(errs) > 0 {
		return &syncpb.AccessConfigSyncResponse{Accepted: false, ValidationErrors: errs}, nil
	}

	if err := s.d.Store.PutAccessConfig(ctx, tenant.ID, kind, content); err != nil {
		return nil, s.toStatus(ctx, err, "store access config")
	}
	s.audit(ctx, p, domain.AuditEvent{
		TenantID:     tenant.ID,
		Action:       "access_config.sync",
		ResourceType: "access_config",
		ResourceID:   tenant.ID + "/" + kind,
		Details: map[string]string{
			"kind": kind,
			"hash": hash.Sum(content).Hex(),
			"size": fmt.Sprint(len(content)),
		},
	})
	return &syncpb.AccessConfigSyncResponse{Accepted: true}, nil
}

func reject(msg string) *syncpb.AccessConfigSyncResponse {
	return &syncpb.AccessConfigSyncResponse{Accepted: false, ValidationErrors: []string{msg}}
}

// accessTenant resolves a tenant reference (id or slug).
func (s *Service) accessTenant(ctx context.Context, id *syncpb.TenantId) (*domain.Tenant, error) {
	ref := strings.TrimSpace(id.GetValue())
	if ref == "" {
		return nil, invalidf("tenant_id is required")
	}
	tenant, err := s.d.Store.GetTenant(ctx, ref)
	if errors.Is(err, domain.ErrNotFound) {
		tenant, err = s.d.Store.GetTenantBySlug(ctx, ref)
	}
	if err != nil {
		return nil, s.toStatus(ctx, err, "tenant")
	}
	return tenant, nil
}

// declarative reports whether req carries a v0.5 declarative document: a
// tree-level or dry-run request, or roles/policies content that is a JSON
// object.
func declarative(req *syncpb.AccessConfigSyncRequest) bool {
	if req.TreeId != nil || req.GetDryRun() {
		return true
	}
	switch req.GetConfigType() {
	case syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES:
		return bytes.HasPrefix(bytes.TrimSpace(req.GetContent()), []byte("{"))
	}
	return false
}

// applyAccessDocument hands a declarative document to accesscfg.
func (s *Service) applyAccessDocument(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, req *syncpb.AccessConfigSyncRequest) (*syncpb.AccessConfigSyncResponse, error) {
	if s.d.AccessCfg == nil {
		return nil, status.Error(codes.Unimplemented, "declarative access documents are not supported by this server")
	}
	switch req.GetConfigType() {
	case syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES:
	default:
		return reject("config_type must be ACCESS_CONFIG_TYPE_POLICIES for declarative access documents"), nil
	}
	content := req.GetContent()
	if ch := req.GetConfigHash(); ch != nil && len(ch.GetValue()) > 0 {
		want, err := hashFromPB(ch)
		if err != nil {
			return reject("config_hash: " + err.Error()), nil
		}
		if hash.Sum(content) != want {
			return reject("config_hash does not match BLAKE3(content)"), nil
		}
	}
	var tree *domain.Tree
	if req.TreeId != nil {
		t, err := s.loadTree(ctx, req.TreeId)
		if err != nil {
			return nil, err
		}
		if t.TenantID != tenant.ID {
			return nil, invalidf("tree %s is not in tenant %s", t.Name, tenant.Slug)
		}
		tree = t
	}
	res, err := s.d.AccessCfg.Apply(ctx, p, tenant, tree, content, req.GetDryRun())
	if err != nil {
		return nil, s.accessStatus(ctx, err, "apply access config")
	}
	return &syncpb.AccessConfigSyncResponse{Accepted: res.Accepted, ValidationErrors: res.Errors, Changes: res.Changes}, nil
}

// accessStatus is toStatus, keeping the authorizer's explanation of a
// refusal (e.g. "tenant is suspended").
func (s *Service) accessStatus(ctx context.Context, err error, what string) error {
	if errors.Is(err, domain.ErrForbidden) {
		return status.Errorf(codes.PermissionDenied, "%s: %v", what, err)
	}
	return s.toStatus(ctx, err, what)
}

// GetAccessConfig returns the last applied access document of a tenant
// (tree_id unset) or tree. With the v0.5 service that is the declarative
// document (reading needs iam:read on the tenant or tree:read on the
// tree); otherwise the raw v0.4 document of config_type (tenant admins).
// found=false when there is none.
func (s *Service) GetAccessConfig(ctx context.Context, req *syncpb.GetAccessConfigRequest) (*syncpb.GetAccessConfigResponse, error) {
	p, err := requireAuth(ctx)
	if err != nil {
		return nil, err
	}
	tenant, err := s.accessTenant(ctx, req.GetTenantId())
	if err != nil {
		return nil, err
	}
	typ := req.GetConfigType()
	if s.d.AccessCfg != nil && (req.TreeId != nil || typ == syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_UNSPECIFIED ||
		typ == syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES || typ == syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES) {
		var tree *domain.Tree
		if req.TreeId != nil {
			if tree, err = s.loadTree(ctx, req.TreeId); err != nil {
				return nil, err
			}
			if tree.TenantID != tenant.ID {
				return nil, invalidf("tree %s is not in tenant %s", tree.Name, tenant.Slug)
			}
		}
		doc, err := s.d.AccessCfg.Get(ctx, p, tenant, tree)
		if errors.Is(err, domain.ErrNotFound) {
			return &syncpb.GetAccessConfigResponse{Found: false}, nil
		}
		if err != nil {
			return nil, s.accessStatus(ctx, err, "read access config")
		}
		resp := &syncpb.GetAccessConfigResponse{Found: true, Content: doc.Content, UpdatedAt: tsToPB(doc.UpdatedAt), UpdatedBy: doc.UpdatedBy}
		resp.ConfigHash = hashToPB(hash.Sum(doc.Content))
		return resp, nil
	}
	if req.TreeId != nil {
		return nil, status.Error(codes.Unimplemented, "tree-level access documents are not supported by this server")
	}
	if err := s.authorize(ctx, p, domain.ActTenantAdmin, tenant.ID, nil); err != nil {
		return nil, err
	}
	kind, ok := accessConfigKinds[typ]
	if !ok {
		return nil, invalidf("config_type is unspecified or unknown")
	}
	content, err := s.d.Store.GetAccessConfig(ctx, tenant.ID, kind)
	if errors.Is(err, domain.ErrNotFound) {
		return &syncpb.GetAccessConfigResponse{Found: false}, nil
	}
	if err != nil {
		return nil, s.toStatus(ctx, err, "read access config")
	}
	return &syncpb.GetAccessConfigResponse{Found: true, Content: content, ConfigHash: hashToPB(hash.Sum(content))}, nil
}

// validateTOMLish performs a structural, line-oriented sanity check of a
// TOML document without a full TOML parser (none is in the allowed
// dependency set). It verifies:
//
//   - the document is non-empty, valid UTF-8 without NUL bytes;
//   - every logical line is blank, a comment, a table header ([a] / [[a]])
//     or a `key = value` pair with a non-empty key and value;
//   - multi-line arrays / inline tables are balanced and multi-line strings
//     (triple-quoted, basic or literal) are terminated;
//   - the document defines at least one table or key.
//
// It returns human-readable messages ("line N: ..."), at most
// maxValidationErrors of them; nil means the document looks valid.
func validateTOMLish(content []byte) []string {
	if len(bytes.TrimSpace(content)) == 0 {
		return []string{"content is empty"}
	}
	if !utf8.Valid(content) {
		return []string{"content is not valid UTF-8"}
	}
	if bytes.IndexByte(content, 0) >= 0 {
		return []string{"content contains NUL bytes"}
	}

	var (
		errs        []string
		depth       int    // open [ / { inside a value spanning lines
		inMultiline string // `"""` or `'''` while inside a multi-line string
		startLine   int    // where the current multi-line construct began
		definitions int
	)
	addErr := func(format string, args ...any) {
		if len(errs) < maxValidationErrors {
			errs = append(errs, fmt.Sprintf(format, args...))
		}
	}

	lines := strings.Split(strings.ReplaceAll(string(content), "\r\n", "\n"), "\n")
	for i, raw := range lines {
		lineNo := i + 1
		line := raw
		if inMultiline != "" || depth > 0 {
			var err error
			depth, inMultiline, err = scanValue(line, depth, inMultiline)
			if err != nil {
				addErr("line %d: %v", lineNo, err)
				depth, inMultiline = 0, ""
			}
			continue
		}
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "#") {
			continue
		}
		if strings.HasPrefix(trimmed, "[") {
			if err := checkHeader(trimmed); err != nil {
				addErr("line %d: %v", lineNo, err)
				continue
			}
			definitions++
			continue
		}
		key, value, ok := splitKeyValue(trimmed)
		if !ok {
			addErr("line %d: expected `key = value` or a [table] header", lineNo)
			continue
		}
		if key == "" {
			addErr("line %d: empty key", lineNo)
			continue
		}
		if value == "" {
			addErr("line %d: missing value for key %q", lineNo, key)
			continue
		}
		var err error
		depth, inMultiline, err = scanValue(value, 0, "")
		if err != nil {
			addErr("line %d: %v", lineNo, err)
			depth, inMultiline = 0, ""
			continue
		}
		if depth > 0 || inMultiline != "" {
			startLine = lineNo
		}
		definitions++
	}
	if inMultiline != "" {
		addErr("line %d: unterminated multi-line string", startLine)
	} else if depth > 0 {
		addErr("line %d: unterminated array or inline table", startLine)
	}
	if len(errs) == 0 && definitions == 0 {
		addErr("content defines no tables or keys")
	}
	return errs
}

// checkHeader validates a table header line such as `[a.b]` or `[[a]]`,
// optionally followed by a comment.
func checkHeader(line string) error {
	open, closeTok := "[", "]"
	if strings.HasPrefix(line, "[[") {
		open, closeTok = "[[", "]]"
	}
	end := strings.Index(line[len(open):], closeTok)
	if end < 0 {
		return errors.New("unterminated table header")
	}
	name := strings.TrimSpace(line[len(open) : len(open)+end])
	if name == "" {
		return errors.New("empty table name")
	}
	rest := strings.TrimSpace(line[len(open)+end+len(closeTok):])
	if rest != "" && !strings.HasPrefix(rest, "#") {
		return errors.New("unexpected text after table header")
	}
	return nil
}

// splitKeyValue splits `key = value` at the first '=' outside a quoted key.
func splitKeyValue(line string) (key, value string, ok bool) {
	var quote byte
	for i := 0; i < len(line); i++ {
		c := line[i]
		switch {
		case quote != 0:
			if c == quote {
				quote = 0
			}
		case c == '"' || c == '\'':
			quote = c
		case c == '=':
			return strings.TrimSpace(line[:i]), strings.TrimSpace(line[i+1:]), true
		}
	}
	return "", "", false
}

// scanValue scans (part of) a value, tracking bracket depth and string
// state across lines. It returns the depth and multi-line string delimiter
// in effect at the end of s.
func scanValue(s string, depth int, multiline string) (int, string, error) {
	i := 0
	for i < len(s) {
		if multiline != "" {
			end := strings.Index(s[i:], multiline)
			if end < 0 {
				return depth, multiline, nil
			}
			i += end + len(multiline)
			multiline = ""
			continue
		}
		c := s[i]
		switch c {
		case '#':
			return depth, "", nil // comment to end of line
		case '"', '\'':
			delim := strings.Repeat(string(c), 3)
			if strings.HasPrefix(s[i:], delim) {
				multiline = delim
				i += 3
				continue
			}
			j := i + 1
			for j < len(s) && s[j] != c {
				if c == '"' && s[j] == '\\' {
					j++
				}
				j++
			}
			if j >= len(s) {
				return depth, "", errors.New("unterminated string")
			}
			i = j + 1
			continue
		case '[', '{':
			depth++
		case ']', '}':
			depth--
			if depth < 0 {
				return 0, "", fmt.Errorf("unbalanced %q", c)
			}
		}
		i++
	}
	return depth, multiline, nil
}
