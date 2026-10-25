package syncsvc

import (
	"slices"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestPushScopedWidensBranchPushTokens(t *testing.T) {
	cases := []struct {
		scopes []string
		want   []string
	}{
		{nil, nil},
		{[]string{"tree:read"}, []string{"tree:read"}},
		{[]string{"branch:push"}, []string{"branch:push", "tree:write"}},
		{[]string{"tree:read", "branch:*"}, []string{"tree:read", "branch:*", "tree:write"}},
	}
	for _, c := range cases {
		p := &domain.Principal{AccountID: "a", Scopes: c.scopes}
		got := pushScoped(p)
		if !slices.Equal(got.Scopes, c.want) {
			t.Errorf("pushScoped(%v) = %v, want %v", c.scopes, got.Scopes, c.want)
		}
		if !slices.Equal(p.Scopes, c.scopes) {
			t.Errorf("pushScoped mutated the caller's scopes: %v", p.Scopes)
		}
	}
	if pushScoped(nil) != nil {
		t.Error("pushScoped(nil) != nil")
	}
}
