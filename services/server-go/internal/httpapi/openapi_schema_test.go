package httpapi

// Reflection helpers shared by the OpenAPI tests: they derive JSON object
// schemas from the Go types the handlers encode and decode, so that the
// component schemas of openapi.yaml can be checked against (and generated
// from) the code.
//
// Regenerating component schemas (for example after adding a type to
// openAPITypes):
//
//	WT_OPENAPI_GEN=<file> go test -run TestGenerateOpenAPISchemas ./internal/httpapi
//
// writes the YAML of every registered type to <file>; paste the new entries
// into components.schemas of openapi.yaml.

import (
	"bytes"
	"encoding"
	"encoding/json"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"path"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/ci"
	"github.com/seanfilimon/worktree/services/server-go/internal/codesearch"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/mirror"
	"github.com/seanfilimon/worktree/services/server-go/internal/policyhist"
	"github.com/seanfilimon/worktree/services/server-go/internal/release"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
	"github.com/seanfilimon/worktree/services/server-go/internal/todo"
	"github.com/seanfilimon/worktree/services/server-go/internal/treeconfig"
	"github.com/seanfilimon/worktree/services/server-go/internal/webhooks"
)

// openAPITypes maps component schema names to the Go types they describe.
// TestOpenAPISchemasMatchGoTypes requires every entry to exist in
// components.schemas with exactly the type's JSON properties (and the
// non-omitempty ones as required).
var openAPITypes = map[string]any{
	// domain/types.go
	"Tenant":           domain.Tenant{},
	"Account":          domain.Account{},
	"Membership":       domain.Membership{},
	"APIToken":         domain.APIToken{},
	"Tree":             domain.Tree{},
	"Branch":           domain.Branch{},
	"Snapshot":         domain.Snapshot{},
	"Tag":              domain.Tag{},
	"StagedSnapshot":   domain.StagedSnapshot{},
	"BranchProtection": domain.BranchProtection{},
	"Approval":         domain.Approval{},
	"MergeRequest":     domain.MergeRequest{},
	"AuditEvent":       domain.AuditEvent{},
	"LicenseRule":      domain.LicenseRule{},
	"Event":            domain.Event{},
	"ServerStats":      domain.ServerStats{},
	// domain/iam.go
	"Team":             domain.Team{},
	"TeamMember":       domain.TeamMember{},
	"CustomRole":       domain.CustomRole{},
	"PolicyConditions": domain.PolicyConditions{},
	"TimeWindow":       domain.TimeWindow{},
	"PolicyStatement":  domain.PolicyStatement{},
	"Policy":           domain.Policy{},
	"Grant":            domain.Grant{},
	"ServiceAccount":   domain.ServiceAccount{},
	"MFAState":         domain.MFAState{},
	"Invitation":       domain.Invitation{},
	"SSOConfig":        domain.SSOConfig{},
	"Resource":         domain.Resource{},
	"RequestEnv":       domain.RequestEnv{},
	"DecisionReason":   domain.DecisionReason{},
	"Decision":         domain.Decision{},
	// domain/v05.go
	"TenantLimits":      domain.TenantLimits{},
	"Dependency":        domain.Dependency{},
	"Todo":              domain.Todo{},
	"CIStatus":          domain.CIStatus{},
	"ReviewComment":     domain.ReviewComment{},
	"LinkedGroup":       domain.LinkedGroup{},
	"LinkedGroupMember": domain.LinkedGroupMember{},
	"TagProtection":     domain.TagProtection{},
	"Release":           domain.Release{},
	"ReleaseAsset":      domain.ReleaseAsset{},
	"SigningKey":        domain.SigningKey{},
	"AccessConfigDoc":   domain.AccessConfigDoc{},
	"GCReport":          domain.GCReport{},
	// domain/v06.go
	"Webhook":         domain.Webhook{},
	"WebhookDelivery": domain.WebhookDelivery{},
	"PolicyVersion":   domain.PolicyVersion{},
	"CodeSearchHit":   domain.CodeSearchHit{},
	"BlameLine":       domain.BlameLine{},
	// mirrors (§15.6): domain/v07.go, internal/mirror, httpapi/mirrors.go
	"Mirror":              domain.Mirror{},
	"MirrorList":          mirrorList{},
	"MirrorSync":          mirrorSync{},
	"MirrorGitStatus":     mirror.GitStatus{},
	"MirrorSyncResult":    mirror.SyncResult{},
	"MirrorRefUpdate":     mirror.RefUpdate{},
	"CreateMirrorRequest": mirror.CreateInput{},
	"PatchMirrorRequest":  mirror.Patch{},
	// config-as-code (§16.3): internal/treeconfig, httpapi/treeconfig.go
	"TreeConfigState":           treeconfig.State{},
	"TreeConfigApplied":         treeconfig.Applied{},
	"TreeConfigAttempt":         treeconfig.Attempt{},
	"ConfigDiagnostic":          treeconfig.Diagnostic{},
	"ConfigFileInfo":            treeconfig.FileInfo{},
	"BranchDecl":                treeconfig.BranchDecl{},
	"ConfigValidation":          treeconfig.ValidationResult{},
	"ValidateTreeConfigRequest": validateTreeConfigRequest{},
	"PatchBranchRequest":        treeconfig.BranchUpdate{},

	// httpapi request and response types.
	"Error":                       errorBody{},
	"ErrorDetail":                 errorDetail{},
	"Health":                      healthResponse{},
	"AccessConfigRequest":         accessConfigRequest{},
	"PatchAccountRequest":         patchAccountRequest{},
	"RegisterRequest":             registerRequest{},
	"LoginRequest":                loginRequest{},
	"SessionResponse":             sessionResponse{},
	"MembershipView":              membershipView{},
	"MeResponse":                  meResponse{},
	"CreateTokenRequest":          createTokenRequest{},
	"CreateTokenResponse":         createTokenResponse{},
	"MFARequiredResponse":         mfaRequiredResponse{},
	"MFAAccountView":              mfaAccountView{},
	"ActionInfo":                  ActionInfo{},
	"PutGrantRequest":             putGrantRequest{},
	"CreateInvitationRequest":     createInvitationRequest{},
	"CreateInvitationResponse":    createInvitationResponse{},
	"InvitationPreview":           invitationPreview{},
	"ValidationError":             ValidationErr{},
	"PolicyRequest":               policyRequest{},
	"ValidatePolicyRequest":       validatePolicyRequest{},
	"ValidatePolicyResponse":      validatePolicyResponse{},
	"ResetCodeRequest":            resetCodeRequest{},
	"ResetCodeResponse":           resetCodeResponse{},
	"PasswordResetRequest":        passwordResetRequest{},
	"RoleView":                    roleView{},
	"CreateRoleRequest":           createRoleRequest{},
	"PatchRoleRequest":            patchRoleRequest{},
	"LoginMFARequest":             loginMFARequest{},
	"SessionView":                 sessionView{},
	"RevokedResponse":             revokedResponse{},
	"MFAEnrollResponse":           mfaEnrollResponse{},
	"MFACodeRequest":              mfaCodeRequest{},
	"MFADisableRequest":           mfaDisableRequest{},
	"RecoveryCodesResponse":       recoveryCodesResponse{},
	"CreateServiceAccountRequest": createServiceAccountRequest{},
	"PatchServiceAccountRequest":  patchServiceAccountRequest{},
	"ServiceAccountDetail":        serviceAccountDetail{},
	"CreateKeyRequest":            createKeyRequest{},
	"SimulatePrincipal":           simulatePrincipal{},
	"SimulateContext":             simulateContext{},
	"SimulateRequest":             simulateRequest{},
	"TeamRoleView":                teamRoleView{},
	"EffectiveResponse":           effectiveResponse{},
	"CreateTeamRequest":           createTeamRequest{},
	"PatchTeamRequest":            patchTeamRequest{},
	"PutTeamMemberRequest":        putTeamMemberRequest{},
	"TeamMemberView":              teamMemberView{},
	"TeamDetail":                  teamDetail{},
	"FileDiff":                    fileDiff{},
	"CompareStats":                compareStats{},
	"CompareResult":               compareResult{},
	"FileHistoryEntry":            fileHistoryEntry{},
	"ContributorStat":             contributorStat{},
	"ActivityDay":                 activityDay{},
	"LanguageShare":               languageShare{},
	"TreeStats":                   treeStats{},
	"StagedAuthorActivity":        stagedAuthorActivity{},
	"StagedFileOverlap":           stagedFileOverlap{},
	"StagedBranchActivity":        stagedBranchActivity{},
	"CreateSigningKeyRequest":     createSigningKeyRequest{},
	"CreateBranchRequest":         createBranchRequest{},
	"CreateTagRequest":            createTagRequest{},
	"PatchTenantRequest":          patchTenantRequest{},
	"ChangePasswordRequest":       changePasswordRequest{},
	"SetPasswordRequest":          setPasswordRequest{},
	"CreateAccountRequest":        createAccountRequest{},
	"SystemMemory":                systemMemory{},
	"SystemStorage":               systemStorage{},
	"SystemInfo":                  systemInfo{},
	"SearchTreeHit":               searchTreeHit{},
	"SearchBranchHit":             searchBranchHit{},
	"SearchResults":               searchResults{},
	"CreateMRRequest":             createMRRequest{},
	"GCConfig":                    gcConfigView{},
	"GCStatus":                    gcStatusView{},
	"ReleaseList":                 releaseList{},
	"CreateReleaseRequest":        createReleaseRequest{},
	"ReviewRequest":               reviewRequest{},
	"GroupMemberRequest":          groupMemberRequest{},
	"CreateGroupRequest":          createGroupRequest{},
	"GroupMemberView":             groupMemberView{},
	"LinkedGroupView":             linkedGroupView{},
	"FileEntry":                   fileEntry{},
	"SnapshotDetail":              snapshotDetail{},
	"SSOProviderInfo":             ssoProviderInfo{},
	"AuthProviders":               authProvidersResponse{},
	"PutSSORequest":               putSSORequest{},
	"UnstageResponse":             unstageResponse{},
	"CreateTenantRequest":         createTenantRequest{},
	"MemberView":                  memberView{},
	"PutMemberRequest":            putMemberRequest{},
	"PatchTenantStatusRequest":    patchTenantStatusRequest{},
	"TodoList":                    todoList{},
	"CompleteTodoRequest":         completeTodoRequest{},
	"TreeSummary":                 treeSummary{},
	"CreateTreeRequest":           createTreeRequest{},
	"PatchTreeRequest":            patchTreeRequest{},
	// v0.6 (§14)
	"WebhookList":              webhookList{},
	"WebhookDeliveryList":      deliveryList{},
	"CreatedWebhook":           createdWebhook{},
	"BlameResponse":            blameResponse{},
	"RollbackPolicyRequest":    rollbackRequest{},
	"MergeMRRequest":           mergeMRRequest{},
	"MergeConflictError":       mergeConflictBody{},
	"MergeConflictErrorDetail": mergeConflictDetail{},
	"FsckReport":               fsckReport{},
	"FsckMissing":              fsckMissing{},
	"StagedFilesResponse":      stagedFilesResponse{},

	// Service types encoded or decoded directly by handlers.
	"CIReport":             ci.Report{},
	"CICombinedStatus":     ci.Status{},
	"TenantUsage":          limits.Usage{},
	"TodoCreateRequest":    todo.CreateInput{},
	"TodoPatchRequest":     todo.Patch{},
	"TodoRef":              todo.TodoRef{},
	"DependencyOutgoing":   todo.Outgoing{},
	"DependencyReport":     todo.Report{},
	"ReleasePatchRequest":  release.Patch{},
	"AccessConfigResult":   accesscfg.Result{},
	"MergeChecks":          review.MergeChecks{},
	"MergeBlocker":         review.Blocker{},
	"ApprovalSummary":      review.ApprovalSummary{},
	"CISummary":            review.CISummary{},
	"TodoSummary":          review.TodoSummary{},
	"CreateCommentRequest": review.CommentInput{},
	"UpdateCommentRequest": review.CommentPatch{},
	// v0.6 (§14)
	"Mergeability":          review.Mergeability{},
	"MergeConflict":         review.Conflict{},
	"CreateWebhookRequest":  webhooks.CreateInput{},
	"PatchWebhookRequest":   webhooks.Patch{},
	"CodeSearchResult":      codesearch.Result{},
	"PolicyDiff":            policyhist.DiffResult{},
	"PolicyStatementChange": policyhist.StatementChange{},
	"PolicyFieldChange":     policyhist.FieldChange{},

	// ---- v0.7 review (§15.7) ----
	"Suggestion":             domain.Suggestion{},
	"PathOwnerRule":          domain.PathOwnerRule{},
	"PathOwners":             domain.PathOwners{},
	"OwnersConfig":           review.OwnersConfig{},
	"PathOwnership":          review.PathOwnership{},
	"MROwners":               review.MROwners{},
	"OwnersSummary":          review.OwnersSummary{},
	"SuggestionInput":        review.SuggestionInput{},
	"AppliedSuggestion":      review.AppliedSuggestion{},
	"PutOwnersRequest":       putOwnersRequest{},
	"ReviewersRequest":       reviewersRequest{},
	"ApplySuggestionRequest": applySuggestionRequest{},
	// ---- end v0.7 review ----
	// v0.7 search-v2 (§15.9)
	"SymbolSearchResult":     codesearch.SymbolResult{},
	"SymbolHit":              codesearch.SymbolHit{},
	"CodeIndexStatus":        codesearch.IndexStatus{},
	"CodeIndexTree":          codesearch.CodeIndexTree{},
	"CodeIndexBranch":        codesearch.CodeIndexBranch{},
	"CodeIndexRebuildResult": codesearch.RebuildResult{},
	// end search-v2
}

// openAPICommentDirs are the package directories whose doc comments feed
// generated schema descriptions.
var openAPICommentDirs = []string{".", "../domain", "../ci", "../limits", "../todo", "../release", "../accesscfg", "../review",
	"../webhooks", "../codesearch", "../policyhist"}

// jsonField is one member of a struct's JSON object encoding.
type jsonField struct {
	name      string
	typ       reflect.Type
	omitempty bool
	owner     reflect.Type // struct declaring the field (embedding resolved)
	goName    string
}

// jsonFields lists the JSON object members of struct type t the way
// encoding/json encodes it: exported fields, json:"-" skipped, untagged
// embedded structs promoted (a field of the outer struct shadows a promoted
// field of the same JSON name).
func jsonFields(t reflect.Type) []jsonField {
	var out []jsonField
	var walk func(t reflect.Type, shadowed map[string]bool)
	walk = func(t reflect.Type, shadowed map[string]bool) {
		// Names declared at this level shadow those of embedded structs.
		own := map[string]bool{}
		for k := range shadowed {
			own[k] = true
		}
		for i := 0; i < t.NumField(); i++ {
			if f := t.Field(i); !isPromoted(f) && f.IsExported() && f.Tag.Get("json") != "-" {
				own[jsonName(f)] = true
			}
		}
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			if f.Tag.Get("json") == "-" {
				continue
			}
			if isPromoted(f) {
				ft := f.Type
				if ft.Kind() == reflect.Pointer {
					ft = ft.Elem()
				}
				walk(ft, own)
				continue
			}
			if !f.IsExported() {
				continue
			}
			name := jsonName(f)
			if shadowed[name] {
				continue
			}
			_, opts, _ := strings.Cut(f.Tag.Get("json"), ",")
			out = append(out, jsonField{
				name:      name,
				typ:       f.Type,
				omitempty: strings.Contains(","+opts+",", ",omitempty,") || strings.Contains(","+opts+",", ",omitzero,"),
				owner:     t,
				goName:    f.Name,
			})
		}
	}
	walk(t, map[string]bool{})
	return out
}

// isPromoted reports whether f is an untagged embedded struct whose fields
// encoding/json promotes.
func isPromoted(f reflect.StructField) bool {
	if !f.Anonymous {
		return false
	}
	if name, _, _ := strings.Cut(f.Tag.Get("json"), ","); name != "" {
		return false
	}
	ft := f.Type
	if ft.Kind() == reflect.Pointer {
		ft = ft.Elem()
	}
	return ft.Kind() == reflect.Struct
}

// jsonName is the JSON member name of field f.
func jsonName(f reflect.StructField) string {
	if name, _, _ := strings.Cut(f.Tag.Get("json"), ","); name != "" {
		return name
	}
	return f.Name
}

// commentKey is the key of a type's (or field's) doc comment in
// schemaGen.comments: "<package dir>.<Type>[.<Field>]".
func commentKey(t reflect.Type, field string) string {
	k := path.Base(t.PkgPath()) + "." + t.Name()
	if field != "" {
		k += "." + field
	}
	return k
}

// isRequestSchema reports whether a component describes a request body:
// request bodies are decoded leniently (absent fields keep their zero
// value), so their schemas list no required properties.
func isRequestSchema(name string) bool { return strings.HasSuffix(name, "Request") }

// schemaGen renders Go types as OpenAPI 3.1 schemas (YAML text).
type schemaGen struct {
	names    map[reflect.Type]string // registered types -> component names
	comments map[string]string       // "Type" and "Type.Field" -> doc comment
}

func newSchemaGen(t *testing.T) *schemaGen {
	g := &schemaGen{names: map[reflect.Type]string{}, comments: map[string]string{}}
	for name, v := range openAPITypes {
		g.names[reflect.TypeOf(v)] = name
	}
	for _, dir := range openAPICommentDirs {
		g.loadComments(t, dir)
	}
	return g
}

// loadComments records the doc and line comments of struct types and
// fields declared in the non-test Go files of dir.
func (g *schemaGen) loadComments(t *testing.T, dir string) {
	fset := token.NewFileSet()
	files, err := filepath.Glob(filepath.Join(filepath.FromSlash(dir), "*.go"))
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range files {
		if strings.HasSuffix(path, "_test.go") {
			continue
		}
		f, err := parser.ParseFile(fset, path, nil, parser.ParseComments)
		if err != nil {
			t.Fatal(err)
		}
		for _, decl := range f.Decls {
			gd, ok := decl.(*ast.GenDecl)
			if !ok || gd.Tok != token.TYPE {
				continue
			}
			for _, spec := range gd.Specs {
				ts := spec.(*ast.TypeSpec)
				st, ok := ts.Type.(*ast.StructType)
				if !ok {
					continue
				}
				doc := ts.Doc
				if doc == nil && len(gd.Specs) == 1 {
					doc = gd.Doc
				}
				if c := cleanComment(doc); c != "" {
					g.comments[f.Name.Name+"."+ts.Name.Name] = c
				}
				for _, fld := range st.Fields.List {
					c := cleanComment(fld.Doc)
					if lc := cleanComment(fld.Comment); lc != "" {
						c = strings.TrimSpace(c + " " + lc)
					}
					for _, n := range fld.Names {
						if c != "" {
							g.comments[f.Name.Name+"."+ts.Name.Name+"."+n.Name] = c
						}
					}
				}
			}
		}
	}
}

func cleanComment(cg *ast.CommentGroup) string {
	if cg == nil {
		return ""
	}
	return strings.Join(strings.Fields(cg.Text()), " ")
}

var (
	textMarshaler = reflect.TypeOf((*encoding.TextMarshaler)(nil)).Elem()
	timeType      = reflect.TypeOf(time.Time{})
	rawJSONType   = reflect.TypeOf(json.RawMessage{})
)

// yamlString quotes s as a YAML double-quoted scalar.
func yamlString(s string) string {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	_ = enc.Encode(s)
	return strings.TrimSpace(buf.String())
}

// component renders the schema of the registered type name at indent.
func (g *schemaGen) component(name string, indent string) string {
	t := reflect.TypeOf(openAPITypes[name])
	var b strings.Builder
	fmt.Fprintf(&b, "%s%s:\n", indent, name)
	g.object(&b, t, indent+"  ", false, isRequestSchema(name))
	return b.String()
}

// object renders struct type t as an object schema; canNull adds "null"
// to the type.
func (g *schemaGen) object(b *strings.Builder, t reflect.Type, indent string, canNull, request bool) {
	if canNull {
		fmt.Fprintf(b, "%stype: [object, \"null\"]\n", indent)
	} else {
		fmt.Fprintf(b, "%stype: object\n", indent)
	}
	if c := g.comments[commentKey(t, "")]; c != "" {
		fmt.Fprintf(b, "%sdescription: %s\n", indent, yamlString(c))
	}
	fields := jsonFields(t)
	var required []string
	for _, f := range fields {
		if !f.omitempty {
			required = append(required, f.name)
		}
	}
	if len(required) > 0 && !request {
		fmt.Fprintf(b, "%srequired: [%s]\n", indent, strings.Join(required, ", "))
	}
	if len(fields) == 0 {
		return
	}
	fmt.Fprintf(b, "%sproperties:\n", indent)
	for _, f := range fields {
		fmt.Fprintf(b, "%s  %s:\n", indent, f.name)
		desc := g.comments[commentKey(f.owner, f.goName)]
		g.schema(b, f.typ, indent+"    ", !f.omitempty && isNilable(f.typ), desc)
	}
}

// schema renders the schema of a value of type t. canNull reports whether
// the encoded value may be null (a nil pointer, slice or map that is not
// omitted).
func (g *schemaGen) schema(b *strings.Builder, t reflect.Type, indent string, canNull bool, desc string) {
	line := func(format string, args ...any) { fmt.Fprintf(b, indent+format+"\n", args...) }
	typ := func(name string) {
		if canNull {
			line("type: [%s, \"null\"]", name)
		} else {
			line("type: %s", name)
		}
	}
	for t.Kind() == reflect.Pointer {
		t = t.Elem()
	}
	defer func() {
		if desc != "" {
			line("description: %s", yamlString(desc))
		}
	}()
	switch {
	case t == timeType:
		typ("string")
		line("format: date-time")
		return
	case t == rawJSONType:
		if desc == "" {
			desc = "Any JSON value."
		}
		return
	case t.Implements(textMarshaler) || reflect.PointerTo(t).Implements(textMarshaler):
		typ("string")
		return
	}
	if name, ok := g.names[t]; ok {
		if canNull {
			line("anyOf:")
			line("  - $ref: \"#/components/schemas/%s\"", name)
			line("  - type: \"null\"")
		} else {
			line("$ref: \"#/components/schemas/%s\"", name)
		}
		return
	}
	switch t.Kind() {
	case reflect.String:
		typ("string")
	case reflect.Bool:
		typ("boolean")
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64,
		reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		typ("integer")
		if t.Kind() == reflect.Int64 || t.Kind() == reflect.Uint64 {
			line("format: int64")
		}
	case reflect.Float32, reflect.Float64:
		typ("number")
	case reflect.Slice, reflect.Array:
		if t.Elem().Kind() == reflect.Uint8 {
			typ("string")
			line("contentEncoding: base64")
			return
		}
		typ("array")
		line("items:")
		g.schema(b, t.Elem(), indent+"  ", isNilable(t.Elem()), "")
	case reflect.Map:
		typ("object")
		line("additionalProperties:")
		g.schema(b, t.Elem(), indent+"  ", isNilable(t.Elem()), "")
	case reflect.Interface:
		// any JSON value: no constraints
		if desc == "" {
			desc = "Any JSON value."
		}
	case reflect.Struct:
		g.object(b, t, indent, canNull, false)
	default:
		panic("openapi schema: unsupported Go kind " + t.Kind().String())
	}
}

func isNilable(t reflect.Type) bool {
	switch t.Kind() {
	case reflect.Pointer, reflect.Slice, reflect.Map, reflect.Interface:
		return true
	}
	return false
}

// TestGenerateOpenAPISchemas writes the component schemas of every type in
// openAPITypes to the file named by WT_OPENAPI_GEN (skipped otherwise).
func TestGenerateOpenAPISchemas(t *testing.T) {
	out := os.Getenv("WT_OPENAPI_GEN")
	if out == "" {
		t.Skip("set WT_OPENAPI_GEN=<file> to generate component schemas")
	}
	g := newSchemaGen(t)
	names := make([]string, 0, len(openAPITypes))
	for n := range openAPITypes {
		names = append(names, n)
	}
	sort.Strings(names)
	var b strings.Builder
	for _, n := range names {
		b.WriteString(g.component(n, "    "))
	}
	if err := os.WriteFile(out, []byte(b.String()), 0o644); err != nil {
		t.Fatal(err)
	}
}
