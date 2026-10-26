package staged

// v0.8 (docs/ENGINEERING_PLAN.md §16.8, workstream staged-v2): labeled
// staged slots.

import (
	"fmt"
	"regexp"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

const (
	// MaxLabelsPerBranch is the maximum number of live staged snapshots
	// (default slot included) one account may have on one branch.
	MaxLabelsPerBranch = 10

	// LabelPattern is the syntax of a non-empty label.
	LabelPattern = `^[a-z0-9][a-z0-9_.-]{0,63}$`
)

var labelRE = regexp.MustCompile(LabelPattern)

// ValidateLabel accepts "" (the default slot) and labels matching
// LabelPattern; anything else is domain.ErrInvalid.
func ValidateLabel(label string) error {
	if label == "" || labelRE.MatchString(label) {
		return nil
	}
	return fmt.Errorf("invalid label %q: use 1-64 characters a-z, 0-9, '_', '.' or '-', starting with a letter or digit: %w",
		label, domain.ErrInvalid)
}
