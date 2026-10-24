package syncsvc

import (
	"context"
	"errors"
	"fmt"
	"io"
	"reflect"
	"sync"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

func TestPing(t *testing.T) {
	h := newHarness(t)

	resp, err := h.client.Ping(h.as(""), &syncpb.PingRequest{})
	if err != nil {
		t.Fatalf("anonymous ping: %v", err)
	}
	if resp.GetServerVersion() != "test-1.0" || resp.AccountId != nil || resp.Username != nil {
		t.Fatalf("anonymous ping = %v", resp)
	}

	resp, err = h.client.Ping(h.as("alice"), &syncpb.PingRequest{})
	if err != nil {
		t.Fatalf("ping: %v", err)
	}
	if resp.GetAccountId().GetValue() != alice.AccountID || resp.GetUsername() != "alice" || resp.GetTenantSlug() != "alice" {
		t.Fatalf("ping = %v", resp)
	}

	_, err = h.client.Ping(h.as("mallory"), &syncpb.PingRequest{})
	wantCode(t, err, codes.Unauthenticated)
}

func TestResolveTree(t *testing.T) {
	h := newHarness(t)
	req := func(slug, name string, create bool) *syncpb.ResolveTreeRequest {
		return &syncpb.ResolveTreeRequest{TenantSlug: slug, TreeName: name, CreateIfMissing: create}
	}

	_, err := h.client.ResolveTree(h.as("alice"), req("acme", "web", false))
	wantCode(t, err, codes.NotFound)

	info, err := h.client.ResolveTree(h.as("alice"), req("acme", "web", true))
	if err != nil {
		t.Fatalf("create: %v", err)
	}
	if !info.GetCreated() || info.GetName() != "web" || info.GetDefaultBranch() != "main" ||
		info.GetTenantSlug() != "acme" || info.GetTenantId().GetValue() != tenantAcme || info.GetTreeId().GetValue() == "" {
		t.Fatalf("created info = %v", info)
	}
	tree, _ := h.store.GetTree(context.Background(), info.GetTreeId().GetValue())
	if tree.Visibility != domain.VisibilityPrivate {
		t.Fatalf("visibility = %q", tree.Visibility)
	}
	if ev := h.bus.ofType(domain.EventTreeCreated); len(ev) != 1 || ev[0].TreeID != tree.ID || ev[0].Actor != "alice" {
		t.Fatalf("tree.created events = %+v", ev)
	}
	if au := h.audit.actions("tree.create"); len(au) != 1 || au[0].ActorID != alice.AccountID || au[0].ResourceID != tree.ID {
		t.Fatalf("tree.create audit = %+v", au)
	}

	again, err := h.client.ResolveTree(h.as("alice"), req("acme", "web", true))
	if err != nil || again.GetCreated() || again.GetTreeId().GetValue() != tree.ID {
		t.Fatalf("resolve existing = %v, %v", again, err)
	}
	// A viewer can resolve but not create.
	if _, err := h.client.ResolveTree(h.as("bob"), req("acme", "web", false)); err != nil {
		t.Fatalf("viewer resolve: %v", err)
	}

	cases := []struct {
		name string
		user string
		req  *syncpb.ResolveTreeRequest
		code codes.Code
	}{
		{"viewer cannot create", "bob", req("acme", "api", true), codes.PermissionDenied},
		{"non-member cannot read", "carol", req("acme", "web", false), codes.PermissionDenied},
		{"anonymous cannot create", "", req("acme", "api", true), codes.Unauthenticated},
		{"anonymous cannot read private", "", req("acme", "web", false), codes.Unauthenticated},
		{"unknown tenant", "alice", req("nope", "web", true), codes.NotFound},
		{"invalid name", "alice", req("acme", "bad/name", true), codes.InvalidArgument},
		{"missing slug", "alice", req("", "web", true), codes.InvalidArgument},
		{"missing name", "alice", req("acme", "", true), codes.InvalidArgument},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := h.client.ResolveTree(h.as(tc.user), tc.req)
			wantCode(t, err, tc.code)
		})
	}
	if n := len(h.bus.ofType(domain.EventTreeCreated)); n != 1 {
		t.Fatalf("unexpected extra tree.created events: %d", n)
	}
}

func TestUploadObjects(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	blob := []byte("hello world")
	bh := hash.Sum(blob)
	good := &syncpb.ObjectData{Hash: hashToPB(bh), Kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, Data: blob}
	mismatch := &syncpb.ObjectData{Hash: hashToPB(hash.Sum([]byte("other"))), Kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, Data: blob}
	shortHash := &syncpb.ObjectData{Hash: &syncpb.ContentHash{Value: []byte{1, 2, 3}}, Data: blob}

	garbage := []byte{0xff, 0xff, 0xff}
	badManifest := &syncpb.ObjectData{Hash: hashToPB(hash.Sum(garbage)), Kind: syncpb.ObjectKind_OBJECT_KIND_MANIFEST, Data: garbage}
	unsorted, _ := proto.Marshal(&syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{
		{Path: "b", BlobHash: hashToPB(bh)}, {Path: "a", BlobHash: hashToPB(bh)},
	}})
	unsortedManifest := &syncpb.ObjectData{Hash: hashToPB(hash.Sum(unsorted)), Kind: syncpb.ObjectKind_OBJECT_KIND_MANIFEST, Data: unsorted}
	okm, _ := proto.Marshal(&syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{{Path: "a/b.txt", BlobHash: hashToPB(bh), Size: 11}}})
	goodManifest := &syncpb.ObjectData{Hash: hashToPB(hash.Sum(okm)), Kind: syncpb.ObjectKind_OBJECT_KIND_MANIFEST, Data: okm}

	resp, err := h.uploadObjects(ctx, good, mismatch, shortHash, badManifest, unsortedManifest, goodManifest, good)
	if err != nil {
		t.Fatalf("upload: %v", err)
	}
	if resp.GetReceived() != 7 || resp.GetStored() != 2 || len(resp.GetRejected()) != 4 {
		t.Fatalf("upload resp = received %d stored %d rejected %d", resp.GetReceived(), resp.GetStored(), len(resp.GetRejected()))
	}
	for _, o := range []*syncpb.ObjectData{mismatch, badManifest, unsortedManifest} {
		if ok, _ := h.objects.Has(context.Background(), hash.Hash(o.GetHash().GetValue())); ok {
			t.Fatalf("rejected object %x was stored", o.GetHash().GetValue())
		}
	}
	if data, err := h.objects.Get(context.Background(), bh); err != nil || string(data) != "hello world" {
		t.Fatalf("stored blob = %q, %v", data, err)
	}

	// Re-upload is deduplicated.
	resp, err = h.uploadObjects(ctx, good)
	if err != nil || resp.GetReceived() != 1 || resp.GetStored() != 0 || len(resp.GetRejected()) != 0 {
		t.Fatalf("re-upload = %v, %v", resp, err)
	}

	_, err = h.uploadObjects(h.as(""), good)
	wantCode(t, err, codes.Unauthenticated)
}

func TestConcurrentUploadsOfSameObjects(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	c := makeCommit("t", map[string]string{"a": "shared 1", "b": "shared 2", "c": "shared 3"})
	var wg sync.WaitGroup
	errs := make(chan error, 12)
	for i := 0; i < 12; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			resp, err := h.uploadObjects(ctx, c.objects...)
			if err == nil && len(resp.GetRejected()) != 0 {
				err = fmt.Errorf("rejected %d objects", len(resp.GetRejected()))
			}
			errs <- err
		}()
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatalf("concurrent upload: %v", err)
		}
	}
	for _, o := range c.objects {
		if ok, _ := h.objects.Has(context.Background(), hash.Hash(o.GetHash().GetValue())); !ok {
			t.Fatalf("object missing after concurrent uploads")
		}
	}
}

func TestUploadRejectsOversizedObject(t *testing.T) {
	h := newHarness(t)
	big := make([]byte, MaxObjectSize+1)
	resp, err := h.uploadObjects(h.as("alice"), &syncpb.ObjectData{Hash: hashToPB(hash.Sum(big)), Data: big})
	if err != nil {
		t.Fatalf("upload: %v", err)
	}
	if resp.GetStored() != 0 || len(resp.GetRejected()) != 1 {
		t.Fatalf("oversized object resp = stored %d rejected %d", resp.GetStored(), len(resp.GetRejected()))
	}
}

func TestNegotiate(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	c := makeCommit(tree, map[string]string{"a": "1", "b": "2"})
	h.upload(ctx, c)
	extra := hash.Sum([]byte("not uploaded"))

	have := []*syncpb.ContentHash{hashToPB(extra)}
	for _, o := range c.objects {
		have = append(have, o.GetHash())
	}
	have = append(have, hashToPB(extra)) // duplicates are collapsed
	plan, err := h.client.Negotiate(ctx, &syncpb.NegotiateRequest{Have: &syncpb.HaveMessage{Hashes: have}})
	if err != nil {
		t.Fatalf("negotiate: %v", err)
	}
	if got := hexes(plan.GetObjectsToSend()); len(got) != 1 || !got[extra.Hex()] || len(plan.GetObjectsToSend()) != 1 {
		t.Fatalf("objects_to_send = %v", got)
	}
	if plan.GetTotalSize() != 0 {
		t.Fatalf("total_size = %d", plan.GetTotalSize())
	}

	_, err = h.client.Negotiate(h.as(""), &syncpb.NegotiateRequest{})
	wantCode(t, err, codes.Unauthenticated)
	_, err = h.client.Negotiate(ctx, &syncpb.NegotiateRequest{Have: &syncpb.HaveMessage{Hashes: []*syncpb.ContentHash{{Value: []byte{1}}}}})
	wantCode(t, err, codes.InvalidArgument)
}

func TestPushPullRoundTrip(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")

	c1 := makeCommit(tree, map[string]string{"README.md": "hi", "src/main.go": "package main"})
	c2 := makeCommit(tree, map[string]string{"README.md": "hello", "src/main.go": "package main", "src/util.go": "package main // util"}, c1)

	// Negotiate tells the client to upload everything, then nothing.
	var have []*syncpb.ContentHash
	for _, o := range append(append([]*syncpb.ObjectData{}, c1.objects...), c2.objects...) {
		have = append(have, o.GetHash())
	}
	plan, err := h.client.Negotiate(ctx, &syncpb.NegotiateRequest{Have: &syncpb.HaveMessage{Hashes: have}})
	if err != nil {
		t.Fatalf("negotiate: %v", err)
	}
	// Distinct objects: blobs "hi", "hello", "package main" (shared),
	// "package main // util" and the two manifests.
	if n := len(plan.GetObjectsToSend()); n != 6 {
		t.Fatalf("objects_to_send = %d, want 6", n)
	}
	h.upload(ctx, c1, c2)
	plan, _ = h.client.Negotiate(ctx, &syncpb.NegotiateRequest{Have: &syncpb.HaveMessage{Hashes: have}})
	if len(plan.GetObjectsToSend()) != 0 {
		t.Fatalf("after upload objects_to_send = %d", len(plan.GetObjectsToSend()))
	}

	// Stage something so the push can clear it.
	if _, err := h.client.Stage(ctx, &syncpb.StageUploadRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchName: "main", SnapshotId: &syncpb.SnapshotId{Value: c2.id}}); err != nil {
		t.Fatalf("stage: %v", err)
	}

	resp, err := h.client.Push(ctx, pushReq(tree, "main", "", c1, c2))
	if err != nil {
		t.Fatalf("push: %v", err)
	}
	if !resp.GetAccepted() || resp.GetNewTip().GetValue() != c2.id {
		t.Fatalf("push resp = %v", resp)
	}
	if got := h.tip(tree, "main"); got != c2.id {
		t.Fatalf("branch tip = %q, want %q", got, c2.id)
	}
	stored, _ := h.store.GetSnapshot(context.Background(), c2.id)
	if stored.PushedBy != alice.AccountID || stored.PushedAt.IsZero() || stored.Message != c2.rec.GetMessage() {
		t.Fatalf("stored snapshot = %+v", stored)
	}
	if got := h.license.lastCall(); !reflect.DeepEqual(got, []string{"README.md", "src/main.go", "src/util.go"}) {
		t.Fatalf("license changed paths = %v", got)
	}
	ev := h.bus.ofType(domain.EventBranchPushed)
	if len(ev) != 1 {
		t.Fatalf("branch.pushed events = %d", len(ev))
	}
	payload := ev[0].Payload.(map[string]any)
	if payload["branch"] != "main" || payload["old_tip"] != "" || payload["new_tip"] != c2.id || payload["count"] != 2 {
		t.Fatalf("payload = %v", payload)
	}
	if au := h.audit.actions("branch.push"); len(au) != 1 || au[0].Details["new_tip"] != c2.id {
		t.Fatalf("branch.push audit = %+v", au)
	}
	if !reflect.DeepEqual(h.staged.cleared, []string{alice.AccountID + "/main"}) {
		t.Fatalf("staged cleared = %v", h.staged.cleared)
	}

	// ListBranches reflects the tip.
	br, err := h.client.ListBranches(ctx, &syncpb.ListBranchesRequest{TreeId: &syncpb.TreeId{Value: tree}})
	if err != nil || len(br.GetBranches()) != 1 || br.GetBranches()[0].GetTip().GetValue() != c2.id {
		t.Fatalf("list branches = %v, %v", br, err)
	}

	// Fresh clone: full history, all objects.
	pull, err := h.client.Pull(ctx, &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchId: &syncpb.BranchId{Value: "main"}})
	if err != nil {
		t.Fatalf("pull: %v", err)
	}
	if !pull.GetHasUpdates() || pull.GetNewTip().GetValue() != c2.id || !reflect.DeepEqual(ids(pull.GetSnapshotChain()), []string{c1.id, c2.id}) {
		t.Fatalf("pull = %v", pull)
	}
	if len(pull.GetSnapshots()) != 2 || pull.GetSnapshots()[1].GetAuthor() != "alice" || pull.GetSnapshots()[1].GetParents()[0].GetValue() != c1.id {
		t.Fatalf("pull snapshots = %v", pull.GetSnapshots())
	}
	want := map[string]bool{}
	for _, o := range append(append([]*syncpb.ObjectData{}, c1.objects...), c2.objects...) {
		want[hash.Hash(o.GetHash().GetValue()).Hex()] = true
	}
	if got := hexes(pull.GetObjectsToFetch()); !reflect.DeepEqual(got, want) || len(pull.GetObjectsToFetch()) != len(want) {
		t.Fatalf("objects_to_fetch = %v, want %v", got, want)
	}

	// Download them (tree-scoped) and verify contents.
	dctx := metadata.AppendToOutgoingContext(ctx, TreeIDMetadataKey, tree)
	got := h.download(t, dctx, pull.GetObjectsToFetch())
	if len(got) != len(want) {
		t.Fatalf("downloaded %d objects, want %d", len(got), len(want))
	}
	for _, o := range got {
		if hash.Sum(o.GetData()) != hash.Hash(o.GetHash().GetValue()) {
			t.Fatalf("downloaded object hash mismatch")
		}
		isManifest := hash.Hash(o.GetHash().GetValue()) == c1.manifest || hash.Hash(o.GetHash().GetValue()) == c2.manifest
		if wantKind := map[bool]syncpb.ObjectKind{true: syncpb.ObjectKind_OBJECT_KIND_MANIFEST, false: syncpb.ObjectKind_OBJECT_KIND_BLOB}[isManifest]; o.GetKind() != wantKind {
			t.Fatalf("kind = %v, want %v", o.GetKind(), wantKind)
		}
	}

	// Incremental pull.
	pull, err = h.client.Pull(ctx, &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}, CurrentTip: &syncpb.SnapshotId{Value: c1.id}})
	if err != nil || !reflect.DeepEqual(ids(pull.GetSnapshotChain()), []string{c2.id}) {
		t.Fatalf("incremental pull = %v, %v", pull, err)
	}
	// Up to date.
	pull, err = h.client.Pull(ctx, &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}, CurrentTip: &syncpb.SnapshotId{Value: c2.id}})
	if err != nil || pull.GetHasUpdates() || pull.GetNewTip().GetValue() != c2.id {
		t.Fatalf("up-to-date pull = %v, %v", pull, err)
	}

	// Fast-forward with a stale old_tip still works; license sees only the diff.
	c3 := makeCommit(tree, map[string]string{"README.md": "hello", "src/main.go": "package main"}, c2)
	h.upload(ctx, c3)
	h.mustPush(ctx, pushReq(tree, "main", c1.id, c3))
	if got := h.license.lastCall(); !reflect.DeepEqual(got, []string{"src/util.go"}) {
		t.Fatalf("license changed paths = %v", got)
	}
	if got := h.tip(tree, "main"); got != c3.id {
		t.Fatalf("tip = %q", got)
	}
}

func (h *harness) download(t *testing.T, ctx context.Context, want []*syncpb.ContentHash) []*syncpb.ObjectData {
	t.Helper()
	stream, err := h.client.DownloadObjects(ctx, &syncpb.WantMessage{Hashes: want})
	if err != nil {
		t.Fatalf("download: %v", err)
	}
	var out []*syncpb.ObjectData
	for {
		o, err := stream.Recv()
		if errors.Is(err, io.EOF) {
			return out
		}
		if err != nil {
			t.Fatalf("download recv: %v", err)
		}
		out = append(out, o)
	}
}

func (h *harness) downloadErr(ctx context.Context, want []*syncpb.ContentHash) error {
	stream, err := h.client.DownloadObjects(ctx, &syncpb.WantMessage{Hashes: want})
	if err != nil {
		return err
	}
	for {
		if _, err := stream.Recv(); err != nil {
			if errors.Is(err, io.EOF) {
				return nil
			}
			return err
		}
	}
}

func TestDownloadObjectsAccess(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	c1 := makeCommit(tree, map[string]string{"a": "pushed"})
	h.upload(ctx, c1)
	h.mustPush(ctx, pushReq(tree, "main", "", c1))
	loose := makeCommit(tree, map[string]string{"b": "uploaded but never pushed"})
	h.upload(ctx, loose)
	unknown := hashToPB(hash.Sum([]byte("nobody has this")))
	all := []*syncpb.ContentHash{c1.objects[0].GetHash(), loose.objects[0].GetHash(), unknown}
	scoped := func(user string) context.Context {
		return metadata.AppendToOutgoingContext(h.as(user), TreeIDMetadataKey, tree)
	}

	// Unscoped: authenticated callers get every stored object, unknown skipped.
	if got := h.download(t, ctx, all); len(got) != 2 || got[0].GetKind() != syncpb.ObjectKind_OBJECT_KIND_UNSPECIFIED {
		t.Fatalf("unscoped download = %d objects", len(got))
	}
	wantCode(t, h.downloadErr(h.as(""), all), codes.Unauthenticated)

	// Tree-scoped: only reachable objects.
	got := h.download(t, scoped("alice"), all)
	if len(got) != 1 || string(got[0].GetData()) != "pushed" || got[0].GetKind() != syncpb.ObjectKind_OBJECT_KIND_BLOB {
		t.Fatalf("scoped download = %v", got)
	}
	wantCode(t, h.downloadErr(scoped(""), all), codes.Unauthenticated)
	wantCode(t, h.downloadErr(scoped("carol"), all), codes.PermissionDenied)

	// Public trees are readable anonymously when scoped.
	h.store.setVisibility(tree, domain.VisibilityPublic)
	if got := h.download(t, scoped(""), all); len(got) != 1 {
		t.Fatalf("anonymous public download = %d objects", len(got))
	}
	pull, err := h.client.Pull(h.as(""), &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}})
	if err != nil || !pull.GetHasUpdates() {
		t.Fatalf("anonymous public pull = %v, %v", pull, err)
	}

	wantCode(t, h.downloadErr(ctx, []*syncpb.ContentHash{{Value: []byte("short")}}), codes.InvalidArgument)
}

func TestPushNonFastForward(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	base := makeCommit(tree, map[string]string{"f": "0"})
	ours := makeCommit(tree, map[string]string{"f": "ours"}, base)
	theirs := makeCommit(tree, map[string]string{"f": "theirs"}, base)
	h.upload(ctx, base, ours, theirs)
	h.mustPush(ctx, pushReq(tree, "main", "", base, ours))

	resp, err := h.client.Push(ctx, pushReq(tree, "main", base.id, theirs))
	if err != nil {
		t.Fatalf("push: %v", err)
	}
	cd := resp.GetRejectionReason().GetConflictDetected()
	if resp.GetAccepted() || cd == nil || cd.GetServerTip().GetValue() != ours.id {
		t.Fatalf("resp = %v", resp)
	}
	if got := h.tip(tree, "main"); got != ours.id {
		t.Fatalf("tip moved to %q", got)
	}
	// The rejected snapshot was not persisted.
	if _, err := h.store.GetSnapshot(context.Background(), theirs.id); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("rejected snapshot stored: %v", err)
	}
	// Rewinding to an ancestor is not a fast-forward either.
	resp, _ = h.client.Push(ctx, &syncpb.PushRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchName: "main", NewTip: &syncpb.SnapshotId{Value: base.id}})
	if resp.GetRejectionReason().GetConflictDetected() == nil {
		t.Fatalf("rewind resp = %v", resp)
	}
}

func TestPushValidationErrors(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	other := h.newTree("other")
	base := makeCommit(tree, map[string]string{"f": "0"})
	h.upload(ctx, base)
	h.mustPush(ctx, pushReq(tree, "main", "", base))

	notUploaded := makeCommit(tree, map[string]string{"f": "never uploaded"}, base)
	// Manifest uploaded, blob missing.
	blobMissing := makeCommit(tree, map[string]string{"g": "blob missing"}, base)
	if _, err := h.uploadObjects(ctx, blobMissing.objects[len(blobMissing.objects)-1]); err != nil {
		t.Fatal(err)
	}
	orphanParent := commit{id: newID()}
	unknownParent := makeCommit(tree, map[string]string{"f": "1"}, orphanParent)
	foreign := makeCommit(other, map[string]string{"f": "2"}, base)
	extra := makeCommit(tree, map[string]string{"x": "unrelated"})
	child := makeCommit(tree, map[string]string{"f": "3"}, base)
	h.upload(ctx, unknownParent, foreign, extra, child)
	cycA := makeCommit(tree, map[string]string{"f": "a"})
	cycB := makeCommit(tree, map[string]string{"f": "b"}, cycA)
	cycA.rec.Parents = []*syncpb.SnapshotId{{Value: cycB.id}}
	h.upload(ctx, cycA, cycB)
	conflicting := proto.Clone(base.rec).(*syncpb.SnapshotRecord)
	conflicting.ManifestHash = hashToPB(child.manifest)

	cases := []struct {
		name string
		user string
		req  *syncpb.PushRequest
		code codes.Code
	}{
		{"anonymous", "", pushReq(tree, "main", base.id, child), codes.Unauthenticated},
		{"missing objects", "alice", pushReq(tree, "main", base.id, notUploaded), codes.FailedPrecondition},
		{"missing blob", "alice", pushReq(tree, "main", base.id, blobMissing), codes.FailedPrecondition},
		{"unknown parent", "alice", pushReq(tree, "main", base.id, unknownParent), codes.FailedPrecondition},
		{"tree id mismatch", "alice", pushReq(tree, "main", base.id, foreign), codes.InvalidArgument},
		{"record not ancestor of tip", "alice", pushReq(tree, "main", base.id, extra, child), codes.InvalidArgument},
		{"cycle", "alice", pushReq(tree, "main", base.id, cycA, cycB), codes.InvalidArgument},
		{"conflicting stored snapshot", "alice", &syncpb.PushRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchName: "main",
			NewTip: &syncpb.SnapshotId{Value: base.id}, Snapshots: []*syncpb.SnapshotRecord{conflicting}}, codes.FailedPrecondition},
		{"unknown new tip", "alice", &syncpb.PushRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchName: "main",
			NewTip: &syncpb.SnapshotId{Value: newID()}}, codes.FailedPrecondition},
		{"unknown chain entry", "alice", &syncpb.PushRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchName: "main",
			NewTip: &syncpb.SnapshotId{Value: base.id}, SnapshotChain: []*syncpb.SnapshotId{{Value: newID()}}}, codes.FailedPrecondition},
		{"missing new tip", "alice", &syncpb.PushRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchName: "main"}, codes.InvalidArgument},
		{"bad branch name", "alice", pushReq(tree, "../main", base.id, child), codes.InvalidArgument},
		{"unknown tree", "alice", pushReq(newID(), "main", base.id, child), codes.NotFound},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := h.client.Push(h.as(tc.user), tc.req)
			wantCode(t, err, tc.code)
			if got := h.tip(tree, "main"); got != base.id {
				t.Fatalf("tip moved to %q", got)
			}
		})
	}
	if n := len(h.bus.ofType(domain.EventBranchPushed)); n != 1 {
		t.Fatalf("branch.pushed events = %d, want 1", n)
	}
}

func TestPushPolicyRejections(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	h.store.protections = append(h.store.protections, domain.BranchProtection{TreeID: tree, Pattern: "rel*", RequiredApprovals: 2})
	base := makeCommit(tree, map[string]string{"LICENSE": "MIT", "vendor/lib.c": "int x;"})
	next := makeCommit(tree, map[string]string{"LICENSE": "MIT", "vendor/lib.c": "int y;"}, base)
	h.upload(ctx, base, next)
	h.mustPush(ctx, pushReq(tree, "main", "", base))

	cases := []struct {
		name    string
		user    string
		branch  string
		review  func(string) error
		license func([]string) error
		check   func(*syncpb.PushRejection) bool
	}{
		{name: "viewer lacks write", user: "bob", branch: "main",
			check: func(r *syncpb.PushRejection) bool { return r.GetAccessDenied() != nil }},
		{name: "non-member lacks write", user: "carol", branch: "main",
			check: func(r *syncpb.PushRejection) bool { return r.GetAccessDenied() != nil }},
		{name: "branch protection", user: "alice", branch: "main",
			review: func(string) error {
				return &domain.PushDenied{Kind: "branch_protection", Reason: "main: no direct push"}
			},
			check: func(r *syncpb.PushRejection) bool { return r.GetBranchProtection().GetRule() == "main: no direct push" }},
		{name: "review required", user: "alice", branch: "release",
			review: func(string) error { return &domain.PushDenied{Kind: "review_required", Reason: "needs approvals"} },
			check: func(r *syncpb.PushRejection) bool {
				return r.GetReviewRequired() != nil && r.GetReviewRequired().GetRequired() == 2 && r.GetReviewRequired().GetCurrent() == 0
			}},
		{name: "review forbidden", user: "alice", branch: "main",
			review: func(string) error { return fmt.Errorf("allowed pushers: %w", domain.ErrForbidden) },
			check:  func(r *syncpb.PushRejection) bool { return r.GetAccessDenied() != nil }},
		{name: "review access denied kind", user: "alice", branch: "main",
			review: func(string) error { return &domain.PushDenied{Kind: "access_denied", Reason: "not an allowed pusher"} },
			check:  func(r *syncpb.PushRejection) bool { return r.GetAccessDenied().GetReason() == "not an allowed pusher" }},
		{name: "quota", user: "alice", branch: "main",
			review: func(string) error { return &domain.PushDenied{Kind: "quota_exceeded", Reason: "storage"} },
			check:  func(r *syncpb.PushRejection) bool { return r.GetQuotaExceeded().GetLimit() == "storage" }},
		{name: "license violation", user: "alice", branch: "main",
			license: func(paths []string) error {
				return &domain.PushDenied{Kind: "license_violation", Reason: "read-only", Path: paths[0], License: "GPL-3.0"}
			},
			check: func(r *syncpb.PushRejection) bool {
				lv := r.GetLicenseViolation()
				return lv.GetPath() == "vendor/lib.c" && lv.GetLicense() == "GPL-3.0"
			}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h.review.set(tc.review)
			h.license.set(tc.license)
			defer h.review.set(nil)
			defer h.license.set(nil)
			req := pushReq(tree, tc.branch, base.id, base, next)
			if tc.branch != "main" {
				req.OldTip = nil
			}
			resp, err := h.client.Push(h.as(tc.user), req)
			if err != nil {
				t.Fatalf("push: %v", err)
			}
			if resp.GetAccepted() || !tc.check(resp.GetRejectionReason()) {
				t.Fatalf("resp = %v", resp)
			}
			if got := h.tip(tree, "main"); got != base.id {
				t.Fatalf("tip moved to %q", got)
			}
		})
	}
	if n := len(h.audit.actions("branch.push_rejected")); n != len(cases) {
		t.Fatalf("push_rejected audits = %d, want %d", n, len(cases))
	}
	// Non-policy failures from the checkers surface as status errors.
	h.review.set(func(string) error { return errors.New("database on fire") })
	_, err := h.client.Push(ctx, pushReq(tree, "main", base.id, next))
	wantCode(t, err, codes.Internal)
}

func TestPushIdempotentAndNewBranch(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	c1 := makeCommit(tree, map[string]string{"a": "1"})
	c2 := makeCommit(tree, map[string]string{"a": "2"}, c1)
	h.upload(ctx, c1, c2)
	h.mustPush(ctx, pushReq(tree, "main", "", c1, c2))
	first, _ := h.store.GetSnapshot(context.Background(), c2.id)

	// Re-pushing the same request is a no-op success.
	h.mustPush(ctx, pushReq(tree, "main", "", c1, c2))
	if n := len(h.bus.ofType(domain.EventBranchPushed)); n != 1 {
		t.Fatalf("branch.pushed events = %d, want 1", n)
	}
	again, _ := h.store.GetSnapshot(context.Background(), c2.id)
	if !again.PushedAt.Equal(first.PushedAt) {
		t.Fatalf("re-push rewrote snapshot metadata")
	}

	// A new branch at an existing snapshot needs no records.
	h.mustPush(ctx, &syncpb.PushRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchName: "feature/x", NewTip: &syncpb.SnapshotId{Value: c1.id}})
	if got := h.tip(tree, "feature/x"); got != c1.id {
		t.Fatalf("feature tip = %q", got)
	}
	// Fast-forward that branch by re-sending already stored records.
	h.mustPush(ctx, pushReq(tree, "feature/x", c1.id, c1, c2))
	if got := h.tip(tree, "feature/x"); got != c2.id {
		t.Fatalf("feature tip = %q", got)
	}
}

func TestConcurrentPushesOneWins(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	base := makeCommit(tree, map[string]string{"f": "base"})
	h.upload(ctx, base)
	h.mustPush(ctx, pushReq(tree, "main", "", base))

	const n = 16
	children := make([]commit, n)
	for i := range children {
		children[i] = makeCommit(tree, map[string]string{"f": fmt.Sprintf("child %d", i)}, base)
		h.upload(ctx, children[i])
	}

	var (
		wg        sync.WaitGroup
		mu        sync.Mutex
		accepted  []string
		conflicts []string
	)
	start := make(chan struct{})
	for i := range children {
		wg.Add(1)
		go func(c commit) {
			defer wg.Done()
			<-start
			resp, err := h.client.Push(ctx, pushReq(tree, "main", base.id, c))
			if err != nil {
				t.Errorf("push: %v", err)
				return
			}
			mu.Lock()
			defer mu.Unlock()
			if resp.GetAccepted() {
				accepted = append(accepted, c.id)
			} else if cd := resp.GetRejectionReason().GetConflictDetected(); cd != nil {
				conflicts = append(conflicts, cd.GetServerTip().GetValue())
			} else {
				t.Errorf("unexpected rejection: %v", resp.GetRejectionReason())
			}
		}(children[i])
	}
	close(start)
	wg.Wait()

	if len(accepted) != 1 || len(conflicts) != n-1 {
		t.Fatalf("accepted %d, conflicts %d", len(accepted), len(conflicts))
	}
	winner := accepted[0]
	if got := h.tip(tree, "main"); got != winner {
		t.Fatalf("tip = %q, want winner %q", got, winner)
	}
	for _, tip := range conflicts {
		if tip != winner && tip != base.id {
			t.Fatalf("conflict reported server tip %q", tip)
		}
	}
}

func TestConcurrentIdenticalPushes(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	c1 := makeCommit(tree, map[string]string{"f": "1"})
	c2 := makeCommit(tree, map[string]string{"f": "2"}, c1)
	h.upload(ctx, c1, c2)

	var wg sync.WaitGroup
	errs := make(chan error, 8)
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			resp, err := h.client.Push(ctx, pushReq(tree, "main", "", c1, c2))
			if err == nil && !resp.GetAccepted() {
				err = fmt.Errorf("rejected: %v", resp.GetRejectionReason())
			}
			errs <- err
		}()
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatalf("identical concurrent push failed: %v", err)
		}
	}
	if got := h.tip(tree, "main"); got != c2.id {
		t.Fatalf("tip = %q", got)
	}
}

func TestPullDepthAndBranches(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")

	// Empty tree: default branch has no updates; other branches are unknown.
	pull, err := h.client.Pull(ctx, &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}})
	if err != nil || pull.GetHasUpdates() {
		t.Fatalf("empty pull = %v, %v", pull, err)
	}
	_, err = h.client.Pull(ctx, &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchId: &syncpb.BranchId{Value: "dev"}})
	wantCode(t, err, codes.NotFound)

	var chain []commit
	for i := 0; i < 5; i++ {
		var c commit
		if i == 0 {
			c = makeCommit(tree, map[string]string{"n": "0"})
		} else {
			c = makeCommit(tree, map[string]string{"n": fmt.Sprint(i), fmt.Sprintf("f%d", i): "x"}, chain[i-1])
		}
		chain = append(chain, c)
	}
	h.upload(ctx, chain...)
	h.mustPush(ctx, pushReq(tree, "main", "", chain...))

	pull, err = h.client.Pull(ctx, &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}, Depth: proto.Uint32(2)})
	if err != nil {
		t.Fatalf("pull: %v", err)
	}
	if !reflect.DeepEqual(ids(pull.GetSnapshotChain()), []string{chain[3].id, chain[4].id}) || len(pull.GetSnapshots()) != 2 {
		t.Fatalf("depth-2 chain = %v", ids(pull.GetSnapshotChain()))
	}
	// objects_to_fetch covers only the two returned snapshots.
	want := map[string]bool{}
	for _, c := range chain[3:] {
		for _, o := range c.objects {
			want[hash.Hash(o.GetHash().GetValue()).Hex()] = true
		}
	}
	if got := hexes(pull.GetObjectsToFetch()); !reflect.DeepEqual(got, want) {
		t.Fatalf("objects_to_fetch = %d entries, want %d", len(got), len(want))
	}

	// Depth larger than history returns everything; depth 0 = unlimited.
	for _, d := range []*uint32{proto.Uint32(50), proto.Uint32(0), nil} {
		pull, _ = h.client.Pull(ctx, &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}, Depth: d})
		if len(pull.GetSnapshotChain()) != 5 {
			t.Fatalf("depth %v chain len = %d", d, len(pull.GetSnapshotChain()))
		}
	}
	// Unknown current tip (local-only work) gets the full history.
	pull, _ = h.client.Pull(ctx, &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}, CurrentTip: &syncpb.SnapshotId{Value: newID()}})
	if len(pull.GetSnapshotChain()) != 5 {
		t.Fatalf("unknown-tip chain len = %d", len(pull.GetSnapshotChain()))
	}

	_, err = h.client.Pull(h.as("carol"), &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}})
	wantCode(t, err, codes.PermissionDenied)
	_, err = h.client.Pull(h.as(""), &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: tree}})
	wantCode(t, err, codes.Unauthenticated)
	_, err = h.client.Pull(ctx, &syncpb.PullRequest{})
	wantCode(t, err, codes.InvalidArgument)
}

func TestStageAndListStaged(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	snap := newID()

	resp, err := h.client.Stage(ctx, &syncpb.StageUploadRequest{
		TreeId:       &syncpb.TreeId{Value: tree},
		BranchName:   "main",
		SnapshotId:   &syncpb.SnapshotId{Value: snap},
		FilesChanged: []string{"a.txt", "b.txt"},
		FilesAdded:   1,
		FilesDeleted: 1,
		Message:      proto.String("wip"),
		AccountId:    &syncpb.AccountId{Value: "spoofed"},
	})
	if err != nil || !resp.GetAccepted() {
		t.Fatalf("stage = %v, %v", resp, err)
	}

	list, err := h.client.ListStaged(h.as("bob"), &syncpb.ListStagedRequest{TreeId: &syncpb.TreeId{Value: tree}})
	if err != nil || len(list.GetStaged()) != 1 {
		t.Fatalf("list staged = %v, %v", list, err)
	}
	st := list.GetStaged()[0]
	if st.GetAuthor() != "alice" || st.GetSnapshotId().GetValue() != snap || st.GetMessage() != "wip" ||
		!reflect.DeepEqual(st.GetFilesChanged(), []string{"a.txt", "b.txt"}) || st.GetFilesAdded() != 1 || st.GetStagedAt() == nil {
		t.Fatalf("staged info = %v", st)
	}
	for _, e := range h.staged.entries {
		if e.AccountID != alice.AccountID || e.TenantID != tenantAcme {
			t.Fatalf("staged entry identity = %+v", e)
		}
	}
	list, _ = h.client.ListStaged(ctx, &syncpb.ListStagedRequest{TreeId: &syncpb.TreeId{Value: tree}, BranchName: proto.String("other")})
	if len(list.GetStaged()) != 0 {
		t.Fatalf("branch filter returned %d", len(list.GetStaged()))
	}

	// Domain validation failures are reported in the body.
	resp, err = h.client.Stage(ctx, &syncpb.StageUploadRequest{TreeId: &syncpb.TreeId{Value: tree}, SnapshotId: &syncpb.SnapshotId{Value: snap}, Message: proto.String("invalid!")})
	if err != nil || resp.GetAccepted() || resp.GetError() == "" {
		t.Fatalf("invalid stage = %v, %v", resp, err)
	}

	stageReq := &syncpb.StageUploadRequest{TreeId: &syncpb.TreeId{Value: tree}, SnapshotId: &syncpb.SnapshotId{Value: snap}}
	_, err = h.client.Stage(h.as("bob"), stageReq)
	wantCode(t, err, codes.PermissionDenied)
	_, err = h.client.Stage(h.as(""), stageReq)
	wantCode(t, err, codes.Unauthenticated)
	_, err = h.client.Stage(ctx, &syncpb.StageUploadRequest{TreeId: &syncpb.TreeId{Value: tree}})
	wantCode(t, err, codes.InvalidArgument)
	_, err = h.client.ListStaged(h.as(""), &syncpb.ListStagedRequest{TreeId: &syncpb.TreeId{Value: tree}})
	wantCode(t, err, codes.Unauthenticated)
}

func TestTags(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	other := h.newTree("other")
	c1 := makeCommit(tree, map[string]string{"a": "1"})
	o1 := makeCommit(other, map[string]string{"a": "1"})
	h.upload(ctx, c1, o1)
	h.mustPush(ctx, pushReq(tree, "main", "", c1))
	h.mustPush(ctx, pushReq(other, "main", "", o1))

	tagReq := func(name, target string) *syncpb.PushTagRequest {
		return &syncpb.PushTagRequest{TreeId: &syncpb.TreeId{Value: tree}, Tag: &syncpb.TagRecord{
			Name: name, Target: &syncpb.SnapshotId{Value: target}, Message: proto.String("release"), Tagger: "spoofed",
		}}
	}
	resp, err := h.client.PushTag(ctx, tagReq("v1.0.0", c1.id))
	if err != nil || !resp.GetAccepted() {
		t.Fatalf("push tag = %v, %v", resp, err)
	}
	if ev := h.bus.ofType(domain.EventTagPushed); len(ev) != 1 {
		t.Fatalf("tag.pushed events = %d", len(ev))
	}

	for name, req := range map[string]*syncpb.PushTagRequest{
		"duplicate":      tagReq("v1.0.0", c1.id),
		"unknown target": tagReq("v2", newID()),
		"foreign target": tagReq("v3", o1.id),
	} {
		resp, err := h.client.PushTag(ctx, req)
		if err != nil || resp.GetAccepted() || resp.GetError() == "" {
			t.Fatalf("%s: resp = %v, %v", name, resp, err)
		}
	}

	_, err = h.client.PushTag(ctx, tagReq("bad name", c1.id))
	wantCode(t, err, codes.InvalidArgument)
	_, err = h.client.PushTag(h.as("bob"), tagReq("v9", c1.id))
	wantCode(t, err, codes.PermissionDenied)

	list, err := h.client.ListTags(h.as("bob"), &syncpb.ListTagsRequest{TreeId: &syncpb.TreeId{Value: tree}})
	if err != nil || len(list.GetTags()) != 1 {
		t.Fatalf("list tags = %v, %v", list, err)
	}
	tg := list.GetTags()[0]
	if tg.GetName() != "v1.0.0" || tg.GetTarget().GetValue() != c1.id || tg.GetTagger() != "alice" || tg.GetMessage() != "release" {
		t.Fatalf("tag = %v", tg)
	}
	_, err = h.client.ListTags(h.as("carol"), &syncpb.ListTagsRequest{TreeId: &syncpb.TreeId{Value: tree}})
	wantCode(t, err, codes.PermissionDenied)
}

func TestSyncAccessConfig(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	content := []byte("[roles.reviewer]\npermissions = [\n  \"tree:read\",\n  \"mr:approve\", # comment\n]\n")
	req := func(tenant string, typ syncpb.AccessConfigType, content []byte) *syncpb.AccessConfigSyncRequest {
		return &syncpb.AccessConfigSyncRequest{TenantId: &syncpb.TenantId{Value: tenant}, ConfigType: typ, Content: content}
	}

	resp, err := h.client.SyncAccessConfig(ctx, req("acme", syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, content))
	if err != nil || !resp.GetAccepted() {
		t.Fatalf("sync = %v, %v", resp, err)
	}
	if string(h.store.configs[key(tenantAcme, "roles")]) != string(content) {
		t.Fatalf("stored config = %q", h.store.configs[key(tenantAcme, "roles")])
	}
	if len(h.audit.actions("access_config.sync")) != 1 {
		t.Fatalf("missing audit")
	}

	// By tenant id, with a matching hash.
	r := req(tenantAcme, syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_POLICIES, content)
	r.ConfigHash = hashToPB(hash.Sum(content))
	if resp, err := h.client.SyncAccessConfig(ctx, r); err != nil || !resp.GetAccepted() {
		t.Fatalf("sync by id = %v, %v", resp, err)
	}

	rejectCases := map[string]*syncpb.AccessConfigSyncRequest{
		"empty":        req("acme", syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, nil),
		"not utf8":     req("acme", syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, []byte{0xff, 0xfe}),
		"not toml":     req("acme", syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, []byte("this is not toml")),
		"unspecified":  req("acme", syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_UNSPECIFIED, content),
		"bad checksum": {TenantId: &syncpb.TenantId{Value: "acme"}, ConfigType: syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, Content: content, ConfigHash: hashToPB(hash.Sum([]byte("x")))},
	}
	for name, r := range rejectCases {
		resp, err := h.client.SyncAccessConfig(ctx, r)
		if err != nil || resp.GetAccepted() || len(resp.GetValidationErrors()) == 0 {
			t.Fatalf("%s: resp = %v, %v", name, resp, err)
		}
	}

	_, err = h.client.SyncAccessConfig(h.as("bob"), req("acme", syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, content))
	wantCode(t, err, codes.PermissionDenied)
	_, err = h.client.SyncAccessConfig(h.as(""), req("acme", syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, content))
	wantCode(t, err, codes.Unauthenticated)
	_, err = h.client.SyncAccessConfig(ctx, req("nope", syncpb.AccessConfigType_ACCESS_CONFIG_TYPE_ROLES, content))
	wantCode(t, err, codes.NotFound)
}
