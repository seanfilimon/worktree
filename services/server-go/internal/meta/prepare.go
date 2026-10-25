package meta

import (
	"fmt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Argument preparation shared by both stores.
//
// prepare* functions validate an argument and fill its store-managed
// defaults (ID, timestamps) in place. They validate before mutating, so a
// rejected argument is left untouched.

func prepareTenant(t *domain.Tenant) error {
	if err := checkOptionalKey("tenant id", t.ID); err != nil {
		return err
	}
	if err := checkKey("tenant slug", t.Slug); err != nil {
		return err
	}
	if t.ID == "" {
		t.ID = newID()
	}
	if t.CreatedAt.IsZero() {
		t.CreatedAt = now()
	}
	return nil
}

func prepareAccount(a *domain.Account) error {
	if err := checkOptionalKey("account id", a.ID); err != nil {
		return err
	}
	if err := checkKey("account username", a.Username); err != nil {
		return err
	}
	if a.ID == "" {
		a.ID = newID()
	}
	if a.CreatedAt.IsZero() {
		a.CreatedAt = now()
	}
	return nil
}

func prepareToken(t *domain.APIToken) error {
	if err := checkOptionalKey("token id", t.ID); err != nil {
		return err
	}
	if err := checkKey("token hash", t.TokenHash); err != nil {
		return err
	}
	// A token belongs to exactly one owner: an account, or (service-account
	// keys) a service account with an empty AccountID.
	if t.ServiceAccountID != "" {
		if t.AccountID != "" {
			return invalid("token has both an account and a service account")
		}
		if err := checkKey("service account id", t.ServiceAccountID); err != nil {
			return err
		}
	} else if err := checkKey("account id", t.AccountID); err != nil {
		return err
	}
	if t.ID == "" {
		t.ID = newID()
	}
	if t.CreatedAt.IsZero() {
		t.CreatedAt = now()
	}
	return nil
}

func prepareTree(t *domain.Tree) error {
	if err := checkOptionalKey("tree id", t.ID); err != nil {
		return err
	}
	if err := checkKeys("tenant id", t.TenantID, "tree name", t.Name); err != nil {
		return err
	}
	if t.ID == "" {
		t.ID = newID()
	}
	if t.CreatedAt.IsZero() {
		t.CreatedAt = now()
	}
	if t.UpdatedAt.IsZero() {
		t.UpdatedAt = t.CreatedAt
	}
	return nil
}

// prepareBranchPut fills timestamps for PutBranch given the stored record
// (old, meaningful only when exists).
func prepareBranchPut(b *domain.Branch, old domain.Branch, exists bool) {
	t := now()
	if b.CreatedAt.IsZero() {
		if exists {
			b.CreatedAt = old.CreatedAt
		} else {
			b.CreatedAt = t
		}
	}
	b.UpdatedAt = t
}

// casBranch computes the branch record resulting from a compare-and-swap
// of cur (meaningful only when exists) from oldTip to newTip.
func casBranch(treeID, name string, cur domain.Branch, exists bool, oldTip, newTip string) (domain.Branch, error) {
	curTip := ""
	if exists {
		curTip = cur.Tip
	}
	if curTip != oldTip {
		return domain.Branch{}, fmt.Errorf("meta: branch %s/%s tip is %q, expected %q: %w",
			treeID, name, curTip, oldTip, domain.ErrConflict)
	}
	t := now()
	if !exists {
		cur = domain.Branch{TreeID: treeID, Name: name, CreatedAt: t}
	}
	cur.Tip = newTip
	cur.UpdatedAt = t
	return normBranch(cur), nil
}

func prepareSnapshot(s *domain.Snapshot) error {
	if err := checkOptionalKey("snapshot id", s.ID); err != nil {
		return err
	}
	if err := checkKey("tree id", s.TreeID); err != nil {
		return err
	}
	if s.ID == "" {
		s.ID = newID()
	}
	return nil
}

func prepareTag(t *domain.Tag) error {
	if err := checkKeys("tree id", t.TreeID, "tag name", t.Name); err != nil {
		return err
	}
	if t.CreatedAt.IsZero() {
		t.CreatedAt = now()
	}
	return nil
}

func prepareStaged(s *domain.StagedSnapshot) error {
	if err := checkKeys("tree id", s.TreeID, "account id", s.AccountID, "branch name", s.BranchName); err != nil {
		return err
	}
	if s.StagedAt.IsZero() {
		s.StagedAt = now()
	}
	return nil
}

func prepareAudit(e *domain.AuditEvent) {
	if e.ID == "" {
		e.ID = newAuditID()
	}
	if e.At.IsZero() {
		e.At = now()
	}
}
