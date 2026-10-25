package syncsvc

import (
	"context"
	"strings"
	"testing"

	"google.golang.org/grpc/codes"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// declarativeEnv wires the sync service to a real store, authorizer and
// accesscfg service.
func declarativeEnv(t *testing.T) (*Service, func(string) context.Context) {
	t.Helper()
	ctx := context.Background()
	st := meta.NewMemory()
	az := authz.New(st, authz.Options{})
	mustDo := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
	}
	mustDo(st.CreateTenant(ctx, &domain.Tenant{ID: "t-acme", Slug: "acme", Kind: domain.TenantOrg}))
	mustDo(st.CreateTree(ctx, &domain.Tree{ID: "tree-web", TenantID: "t-acme", Name: "web", DefaultBranch: "main"}))
	for name, role := range map[string]string{"olga": domain.RoleOwner, "dev": domain.RoleDeveloper} {
		mustDo(st.CreateAccount(ctx, &domain.Account{ID: "acct-" + name, Username: name, TenantID: "t-acme"}))
		mustDo(st.PutMembership(ctx, &domain.Membership{TenantID: "t-acme", AccountID: "acct-" + name, Role: role}))
	}
	svc := New(Deps{Store: st, Authz: az, AccessCfg: accesscfg.New(accesscfg.Deps{Store: st, Authz: az})})
	as := func(name string) context.Context {
		if name == "" {
			return ctx
		}
		return iam.WithPrincipal(ctx, &domain.Principal{AccountID: "acct-" + name, Username: name})
	}
	return svc, as
}

func TestSyncAccessConfigDeclarative(t *testing.T) {
	svc, as := declarativeEnv(t)
	doc := []byte(`{"roles":[{"name":"release-manager","permissions":["release:*"]}],"policies":[]}`)
	req := func(dry bool) *syncpb.AccessConfigSyncRequest {
		return &syncpb.AccessConfigSyncRequest{TenantId: &syncpb.TenantId{Value: "acme"},
			ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES, Content: doc, DryRun: dry,
			ConfigHash: hashToPB(hash.Sum(doc))}
	}

	resp, err := svc.SyncAccessConfig(as("olga"), req(true))
	if err != nil || !resp.GetAccepted() || len(resp.GetChanges()) != 1 || resp.GetChanges()[0] != "role release-manager: create (1 permission)" {
		t.Fatalf("plan = %v, %v", resp, err)
	}
	got, err := svc.GetAccessConfig(as("olga"), &syncpb.GetAccessConfigRequest{TenantId: &syncpb.TenantId{Value: "acme"}})
	if err != nil || got.GetFound() {
		t.Fatalf("dry run stored a document: %v, %v", got, err)
	}

	if resp, err = svc.SyncAccessConfig(as("olga"), req(false)); err != nil || !resp.GetAccepted() {
		t.Fatalf("apply = %v, %v", resp, err)
	}
	got, err = svc.GetAccessConfig(as("olga"), &syncpb.GetAccessConfigRequest{TenantId: &syncpb.TenantId{Value: "t-acme"},
		ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES})
	if err != nil || !got.GetFound() || string(got.GetContent()) != string(doc) || got.GetUpdatedBy() != "olga" || got.GetUpdatedAt() == nil {
		t.Fatalf("get = %v, %v", got, err)
	}

	// Refusals of the document travel in the response.
	bad := req(false)
	bad.Content = []byte(`{"roles":[{"name":"x y"}]}`)
	bad.ConfigHash = nil
	if resp, err := svc.SyncAccessConfig(as("olga"), bad); err != nil || resp.GetAccepted() || len(resp.GetValidationErrors()) == 0 {
		t.Fatalf("invalid = %v, %v", resp, err)
	}
	bad = req(false)
	bad.ConfigHash = hashToPB(hash.Sum([]byte("other")))
	if resp, _ := svc.SyncAccessConfig(as("olga"), bad); resp.GetAccepted() || !strings.Contains(resp.GetValidationErrors()[0], "config_hash") {
		t.Fatalf("hash mismatch = %v", resp)
	}
	bad = req(false)
	bad.ConfigType = syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_LICENSE
	bad.DryRun = true
	if resp, _ := svc.SyncAccessConfig(as("olga"), bad); resp.GetAccepted() {
		t.Fatalf("wrong config type accepted: %v", resp)
	}

	// Authorization.
	_, err = svc.SyncAccessConfig(as("dev"), req(false))
	wantCode(t, err, codes.PermissionDenied)
	_, err = svc.SyncAccessConfig(as(""), req(false))
	wantCode(t, err, codes.Unauthenticated)
	_, err = svc.GetAccessConfig(as("dev"), &syncpb.GetAccessConfigRequest{TenantId: &syncpb.TenantId{Value: "acme"}})
	wantCode(t, err, codes.PermissionDenied)

	// Tree documents.
	treeReq := &syncpb.AccessConfigSyncRequest{TenantId: &syncpb.TenantId{Value: "acme"}, TreeId: &syncpb.TreeId{Value: "tree-web"},
		ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES,
		Content:    []byte(`{"policies":[{"name":"freeze","enabled":true,"statements":[{"effect":"allow","actions":["tree:write"],"resources":["wt:tree/acme/web"],"principals":["*"]}]}]}`)}
	if resp, err := svc.SyncAccessConfig(as("olga"), treeReq); err != nil || resp.GetAccepted() ||
		!strings.Contains(strings.Join(resp.GetValidationErrors(), "|"), "only contain deny statements") {
		t.Fatalf("tree allow = %v, %v", resp, err)
	}
	treeReq.Content = []byte(strings.Replace(string(treeReq.Content), `"allow"`, `"deny"`, 1))
	if resp, err := svc.SyncAccessConfig(as("olga"), treeReq); err != nil || !resp.GetAccepted() {
		t.Fatalf("tree deny = %v, %v", resp, err)
	}
	got, err = svc.GetAccessConfig(as("dev"), &syncpb.GetAccessConfigRequest{TenantId: &syncpb.TenantId{Value: "acme"}, TreeId: &syncpb.TreeId{Value: "tree-web"}})
	if err != nil || !got.GetFound() {
		t.Fatalf("tree get = %v, %v", got, err)
	}
	treeReq.TreeId = &syncpb.TreeId{Value: "tree-nope"}
	_, err = svc.SyncAccessConfig(as("olga"), treeReq)
	wantCode(t, err, codes.NotFound)
}

func TestDeclarativeDetection(t *testing.T) {
	tests := []struct {
		req  *syncpb.AccessConfigSyncRequest
		want bool
	}{
		{&syncpb.AccessConfigSyncRequest{ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES, Content: []byte(" {\"policies\":[]}")}, true},
		{&syncpb.AccessConfigSyncRequest{ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, Content: []byte("{}")}, true},
		{&syncpb.AccessConfigSyncRequest{ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, Content: []byte("[roles.x]")}, false},
		{&syncpb.AccessConfigSyncRequest{ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_LICENSE, Content: []byte("{}")}, false},
		{&syncpb.AccessConfigSyncRequest{ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_LICENSE, DryRun: true}, true},
		{&syncpb.AccessConfigSyncRequest{TreeId: &syncpb.TreeId{Value: "t"}}, true},
	}
	for i, tt := range tests {
		if got := declarative(tt.req); got != tt.want {
			t.Errorf("%d: declarative = %v, want %v", i, got, tt.want)
		}
	}
	// Without the v0.5 service, declarative requests are unimplemented.
	svc := New(Deps{})
	_, err := svc.applyAccessDocument(context.Background(), &domain.Principal{}, &domain.Tenant{}, tests[0].req)
	wantCode(t, err, codes.Unimplemented)
}
