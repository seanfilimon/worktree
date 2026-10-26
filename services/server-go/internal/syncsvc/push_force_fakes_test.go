package syncsvc

// history-v2 (§16.10): fakeStore gains the DocStore methods the push path
// now uses (push_ops, branch_reflog), backed by one meta.Memory per fake.

import (
	"context"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

var fakeDocStores sync.Map // *fakeStore → *meta.Memory

func (f *fakeStore) docs() *meta.Memory {
	if m, ok := fakeDocStores.Load(f); ok {
		return m.(*meta.Memory)
	}
	m, _ := fakeDocStores.LoadOrStore(f, meta.NewMemory())
	return m.(*meta.Memory)
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
