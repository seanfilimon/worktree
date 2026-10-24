// Package license implements domain.LicenseChecker. STUB: replaced by the
// go-audit-license workstream (delete this file when implementing).
package license

import "github.com/seanfilimon/worktree/services/server-go/internal/domain"

// Checker enforces per-path SPDX license rules.
type Checker struct{ domain.LicenseChecker }

// New returns a license checker.
func New(store domain.MetaStore) *Checker { return &Checker{} }
