package iam

import (
	"context"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// device-auth (§16.6): the refresh-token families and device codes live in
// the DocStore, so the test fake delegates its DocStore methods to an
// in-memory meta store (created on first use).

var (
	fakeDocsMu sync.Mutex
	fakeDocs   = map[*fakeStore]*meta.Memory{}
)

func (f *fakeStore) docs() *meta.Memory {
	fakeDocsMu.Lock()
	defer fakeDocsMu.Unlock()
	m := fakeDocs[f]
	if m == nil {
		m = meta.NewMemory()
		fakeDocs[f] = m
	}
	return m
}

func (f *fakeStore) PutDoc(ctx context.Context, coll, key string, v any) error {
	return f.docs().PutDoc(ctx, coll, key, v)
}

func (f *fakeStore) CreateDoc(ctx context.Context, coll, key string, v any) error {
	return f.docs().CreateDoc(ctx, coll, key, v)
}

func (f *fakeStore) GetDoc(ctx context.Context, coll, key string, v any) error {
	return f.docs().GetDoc(ctx, coll, key, v)
}

func (f *fakeStore) ListDocs(ctx context.Context, coll, prefix string) ([][]byte, error) {
	return f.docs().ListDocs(ctx, coll, prefix)
}

func (f *fakeStore) DeleteDoc(ctx context.Context, coll, key string) error {
	return f.docs().DeleteDoc(ctx, coll, key)
}

func (f *fakeStore) DeleteDocs(ctx context.Context, coll, prefix string) (int, error) {
	return f.docs().DeleteDocs(ctx, coll, prefix)
}

func (f *fakeStore) UpdateDoc(ctx context.Context, coll, key string, fn func(raw []byte) ([]byte, error)) error {
	return f.docs().UpdateDoc(ctx, coll, key, fn)
}

func (f *fakeStore) NextSeq(ctx context.Context, name string) (int64, error) {
	return f.docs().NextSeq(ctx, name)
}
