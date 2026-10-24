package iam

import (
	"context"
	"errors"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestCreateAccount(t *testing.T) {
	ctx := context.Background()
	// Registration is disabled: CreateAccount must still work.
	e := newTestEnv(t, func(o *Options) { o.AllowRegistration = false })

	tests := []struct {
		name    string
		user    string
		email   string
		pw      string
		isAdmin bool
		want    error
	}{
		{name: "regular", user: " Bob ", email: "bob@example.com", pw: testPassword},
		{name: "admin flag", user: "root2", email: "root2@example.com", pw: testPassword, isAdmin: true},
		{name: "duplicate username", user: "bob", email: "other@example.com", pw: testPassword, want: domain.ErrAlreadyExists},
		{name: "duplicate email", user: "carol", email: "BOB@example.com", pw: testPassword, want: domain.ErrAlreadyExists},
		{name: "reserved admin", user: "admin", email: "admin@example.com", pw: testPassword, isAdmin: true, want: domain.ErrInvalid},
		{name: "reserved system", user: "system", email: "system@example.com", pw: testPassword, want: domain.ErrInvalid},
		{name: "invalid username", user: "x", email: "x@example.com", pw: testPassword, want: domain.ErrInvalid},
		{name: "invalid email", user: "dave", email: "dave", pw: testPassword, want: domain.ErrInvalid},
		{name: "weak password", user: "erin", email: "erin@example.com", pw: "short", want: domain.ErrInvalid},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			a, err := e.svc.CreateAccount(ctx, tt.user, tt.email, tt.pw, tt.isAdmin)
			if tt.want != nil {
				if !errors.Is(err, tt.want) {
					t.Fatalf("CreateAccount = %v, want %v", err, tt.want)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if a.Username != NormalizeUsername(tt.user) || a.IsAdmin != tt.isAdmin || a.PasswordHash == "" {
				t.Fatalf("account = %+v", a)
			}
			ten, err := e.store.GetTenant(ctx, a.TenantID)
			if err != nil || ten.Slug != a.Username || ten.Kind != domain.TenantUser {
				t.Fatalf("personal tenant = %+v, %v", ten, err)
			}
			if role, _ := e.svc.RoleOf(ctx, a.ID, ten.ID); role != domain.RoleOwner {
				t.Fatalf("RoleOf = %q", role)
			}
			if _, _, err := e.svc.Login(ctx, a.Username, tt.pw); err != nil {
				t.Fatalf("login with the new account: %v", err)
			}
		})
	}
}
