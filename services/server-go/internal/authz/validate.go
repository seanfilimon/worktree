package authz

import (
	"fmt"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ValidationError is one problem in a list of policy statements
// (POST /tenants/{t}/policies/validate). Statement is the 0-based index of
// the statement; Field names the offending field, e.g. "effect",
// "actions[1]", "conditions.time_window.timezone".
type ValidationError struct {
	Statement int    `json:"statement"`
	Field     string `json:"field"`
	Message   string `json:"message"`
}

func (e ValidationError) Error() string {
	return fmt.Sprintf("statement %d: %s: %s", e.Statement, e.Field, e.Message)
}

// maxSidLen bounds statement ids.
const maxSidLen = 128

// ValidateStatements checks policy statements and returns every problem
// found (nil when they are all valid). It accepts exactly what the
// evaluator understands:
//
//   - effect: "allow" or "deny";
//   - actions (non-empty): catalog actions, "<area>:*" or "*";
//   - resources (non-empty): "*" or "wt:tenant/<t>", "wt:tree/<t>/<tree>",
//     "wt:branch/<t>/<tree>/<branch>", "wt:path/<t>/<tree>/<path>", where a
//     segment may contain "*" (within the segment) or be "**" (any depth);
//   - principals (non-empty): "user:<name>", "team:<slug>", "role:<role>",
//     "sa:<name>", "sa:<tenant>/<name>", "authenticated", "anonymous", "*";
//     names may contain "*";
//   - conditions: ip_ranges are CIDRs (or single addresses), not_before is
//     before not_after, time_window days are mon..sun, start/end are HH:MM
//     and differ, timezone is an IANA name; attributes (v0.8) have keys
//     principal.<k> / tree.<k> / tenant.<k>, a known op and the number of
//     values the op takes (attributes.go).
func ValidateStatements(statements []domain.PolicyStatement) []ValidationError {
	var errs []ValidationError
	for i, st := range statements {
		add := func(field, format string, args ...any) {
			errs = append(errs, ValidationError{Statement: i, Field: field, Message: fmt.Sprintf(format, args...)})
		}
		if len(st.Sid) > maxSidLen {
			add("sid", "must be at most %d characters", maxSidLen)
		}
		switch strings.ToLower(strings.TrimSpace(st.Effect)) {
		case domain.EffectAllow, domain.EffectDeny:
		case "":
			add("effect", "is required (allow or deny)")
		default:
			add("effect", "must be allow or deny, got %q", st.Effect)
		}

		if len(st.Actions) == 0 {
			add("actions", "at least one action is required")
		}
		for j, a := range st.Actions {
			if err := validateActionPattern(strings.TrimSpace(a)); err != nil {
				add(fmt.Sprintf("actions[%d]", j), "%v", err)
			}
		}

		if len(st.Resources) == 0 {
			add("resources", "at least one resource is required (use \"*\" for all)")
		}
		for j, r := range st.Resources {
			if err := validateResourcePattern(r); err != nil {
				add(fmt.Sprintf("resources[%d]", j), "%v", err)
			}
		}

		if len(st.Principals) == 0 {
			add("principals", "at least one principal is required (use \"*\" for everyone)")
		}
		for j, p := range st.Principals {
			if err := validatePrincipalPattern(strings.TrimSpace(p)); err != nil {
				add(fmt.Sprintf("principals[%d]", j), "%v", err)
			}
		}

		c := st.Conditions
		for j, r := range c.IPRanges {
			if _, err := parsePrefix(r); err != nil {
				add(fmt.Sprintf("conditions.ip_ranges[%d]", j), "%v", err)
			}
		}
		if c.NotBefore != nil && c.NotAfter != nil && !c.NotBefore.Before(*c.NotAfter) {
			add("conditions.not_after", "must be after not_before")
		}
		if tw := c.TimeWindow; tw != nil {
			for j, d := range tw.Days {
				if _, ok := weekdays[strings.ToLower(strings.TrimSpace(d))]; !ok {
					add(fmt.Sprintf("conditions.time_window.days[%d]", j), "unknown day %q (want mon, tue, wed, thu, fri, sat or sun)", d)
				}
			}
			start, serr := parseClock(tw.Start)
			if serr != nil {
				add("conditions.time_window.start", "%v", serr)
			} else if start == 24*60 {
				add("conditions.time_window.start", "cannot be 24:00")
			}
			end, eerr := parseClock(tw.End)
			if eerr != nil {
				add("conditions.time_window.end", "%v", eerr)
			}
			if serr == nil && eerr == nil && start == end {
				add("conditions.time_window.end", "must differ from start")
			}
			if tw.Timezone != "" {
				if _, err := compileWindow(domain.TimeWindow{Start: "00:00", End: "01:00", Timezone: tw.Timezone}); err != nil {
					add("conditions.time_window.timezone", "unknown IANA time zone %q", tw.Timezone)
				}
			}
		}
		errs = append(errs, validateAttributeConditions(i, c.Attributes)...)
	}
	return errs
}
