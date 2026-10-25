package meta

import (
	"context"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

type docT struct {
	Name string `json:"name"`
	N    int    `json:"n"`
}

func testDocs(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	const c = "things"

	var got docT
	wantErr(t, s.GetDoc(ctx, c, "a", &got), domain.ErrNotFound)
	list, err := s.ListDocs(ctx, c, "")
	must(t, err)
	if list == nil || len(list) != 0 {
		t.Fatalf("empty list = %v", list)
	}

	must(t, s.PutDoc(ctx, c, domain.DocKey("t1", "b"), docT{"b", 2}))
	must(t, s.PutDoc(ctx, c, domain.DocKey("t1", "a"), docT{"a", 1}))
	must(t, s.PutDoc(ctx, c, domain.DocKey("t2", "a"), docT{"x", 9}))
	wantErr(t, s.CreateDoc(ctx, c, domain.DocKey("t1", "a"), docT{}), domain.ErrAlreadyExists)
	wantErr(t, s.PutDoc(ctx, c, "", docT{}), domain.ErrInvalid)
	wantErr(t, s.PutDoc(ctx, "", "k", docT{}), domain.ErrInvalid)

	items, err := ListDocsT[docT](ctx, s, c, DocPrefix("t1"))
	must(t, err)
	wantEqual(t, "t1 docs", items, []docT{{"a", 1}, {"b", 2}})

	must(t, UpdateDocT(ctx, s, c, domain.DocKey("t1", "a"), func(cur *docT) (*docT, error) {
		cur.N += 10
		return cur, nil
	}))
	one, err := GetDocT[docT](ctx, s, c, domain.DocKey("t1", "a"))
	must(t, err)
	wantEqual(t, "updated", *one, docT{"a", 11})

	must(t, UpdateDocT(ctx, s, c, "new", func(cur *docT) (*docT, error) {
		if cur != nil {
			t.Fatalf("expected nil for missing doc")
		}
		return &docT{"new", 0}, nil
	}))
	must(t, UpdateDocT(ctx, s, c, "new", func(*docT) (*docT, error) { return nil, nil }))
	wantErr(t, s.GetDoc(ctx, c, "new", &got), domain.ErrNotFound)

	n, err := s.DeleteDocs(ctx, c, DocPrefix("t1"))
	must(t, err)
	if n != 2 {
		t.Fatalf("DeleteDocs = %d, want 2", n)
	}
	must(t, s.DeleteDoc(ctx, c, domain.DocKey("t2", "a")))
	wantErr(t, s.DeleteDoc(ctx, c, domain.DocKey("t2", "a")), domain.ErrNotFound)

	for want := int64(1); want <= 3; want++ {
		got, err := s.NextSeq(ctx, "todo/t1")
		must(t, err)
		if got != want {
			t.Fatalf("NextSeq = %d, want %d", got, want)
		}
	}
	if got, _ := s.NextSeq(ctx, "todo/t2"); got != 1 {
		t.Fatalf("independent counter = %d", got)
	}
}
