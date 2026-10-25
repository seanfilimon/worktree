package policyhist

import (
	"fmt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Diff ops (StatementChange.Op).
const (
	OpAdd    = "add"
	OpRemove = "remove"
	OpChange = "change"
)

// StatementChange is one statement-level difference between two versions.
//
// Statements are matched by sid; a sid that occurs more than once is
// matched occurrence by occurrence. Statements without a sid are matched
// by position among the statements without a sid: the n-th one of `from`
// is compared with the n-th one of `to`.
type StatementChange struct {
	Op  string `json:"op"`  // OpAdd | OpRemove | OpChange
	Sid string `json:"sid"` // "" for statements without a sid
	// Index is the statement's position in the `to` version (add, change)
	// or in the `from` version (remove).
	Index int `json:"index"`
	// Statement is the added or removed statement.
	Statement *domain.PolicyStatement `json:"statement,omitempty"`
	// Before and After are the two sides of a changed statement.
	Before *domain.PolicyStatement `json:"before,omitempty"`
	After  *domain.PolicyStatement `json:"after,omitempty"`
}

// FieldChange is a difference in a policy-level field (name, description,
// enabled).
type FieldChange struct {
	Field  string `json:"field"`
	Before any    `json:"before"`
	After  any    `json:"after"`
}

// DiffResult answers GET .../policies/{id}/diff.
type DiffResult struct {
	From int `json:"from"` // version; 0 = the empty policy
	To   int `json:"to"`
	// Changes lists the added and changed statements in the order of the
	// `to` version, followed by the removed ones in the order of `from`.
	Changes []StatementChange `json:"changes"`
	Fields  []FieldChange     `json:"fields"`
}

// matchKeys identifies each statement for matching (see StatementChange).
func matchKeys(stmts []domain.PolicyStatement) []string {
	keys := make([]string, len(stmts))
	bySid := map[string]int{}
	anon := 0
	for i, st := range stmts {
		sid := canon(st).Sid
		if sid == "" {
			keys[i] = fmt.Sprintf("\x00#%d", anon)
			anon++
			continue
		}
		keys[i] = fmt.Sprintf("%s\x00%d", sid, bySid[sid])
		bySid[sid]++
	}
	return keys
}

// Diff compares two policies statement by statement. A nil side is the
// empty policy.
func Diff(from, to *domain.Policy) ([]StatementChange, []FieldChange) {
	var a, b []domain.PolicyStatement
	if from != nil {
		a = from.Statements
	}
	if to != nil {
		b = to.Statements
	}
	ka, kb := matchKeys(a), matchKeys(b)
	inA := make(map[string]int, len(ka))
	for i, k := range ka {
		inA[k] = i
	}
	inB := make(map[string]bool, len(kb))
	changes := []StatementChange{}
	for j, k := range kb {
		inB[k] = true
		after := b[j]
		i, ok := inA[k]
		if !ok {
			changes = append(changes, StatementChange{Op: OpAdd, Sid: canon(after).Sid, Index: j, Statement: &after})
			continue
		}
		before := a[i]
		if !sameStatement(before, after) {
			changes = append(changes, StatementChange{Op: OpChange, Sid: canon(after).Sid, Index: j, Before: &before, After: &after})
		}
	}
	for i, k := range ka {
		if !inB[k] {
			removed := a[i]
			changes = append(changes, StatementChange{Op: OpRemove, Sid: canon(removed).Sid, Index: i, Statement: &removed})
		}
	}

	fields := []FieldChange{}
	switch {
	case from == nil && to == nil:
	case from == nil:
		fields = append(fields,
			FieldChange{Field: "name", Before: nil, After: to.Name},
			FieldChange{Field: "description", Before: nil, After: to.Description},
			FieldChange{Field: "enabled", Before: nil, After: to.Enabled})
	case to == nil:
		fields = append(fields,
			FieldChange{Field: "name", Before: from.Name, After: nil},
			FieldChange{Field: "description", Before: from.Description, After: nil},
			FieldChange{Field: "enabled", Before: from.Enabled, After: nil})
	default:
		if from.Name != to.Name {
			fields = append(fields, FieldChange{Field: "name", Before: from.Name, After: to.Name})
		}
		if from.Description != to.Description {
			fields = append(fields, FieldChange{Field: "description", Before: from.Description, After: to.Description})
		}
		if from.Enabled != to.Enabled {
			fields = append(fields, FieldChange{Field: "enabled", Before: from.Enabled, After: to.Enabled})
		}
	}
	return changes, fields
}
