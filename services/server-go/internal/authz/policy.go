package authz

import (
	"fmt"
	"net"
	"net/netip"
	"strconv"
	"strings"
	"time"

	// Embed the IANA time zone database so TimeWindow.Timezone resolves on
	// hosts without one (notably Windows).
	_ "time/tzdata"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Principal patterns of policy statements.
const (
	principalAny           = "*"
	principalAuthenticated = "authenticated"
	principalAnonymous     = "anonymous"
	prefixUser             = "user:"
	prefixTeam             = "team:"
	prefixRole             = "role:"
	prefixSA               = "sa:"
	prefixTenant           = "tenant:" // v0.5: every member and service account of a tenant
)

// weekdays maps TimeWindow day names to time.Weekday.
var weekdays = map[string]time.Weekday{
	"sun": time.Sunday, "mon": time.Monday, "tue": time.Tuesday, "wed": time.Wednesday,
	"thu": time.Thursday, "fri": time.Friday, "sat": time.Saturday,
}

// compiledPolicy is an enabled policy with its statements pre-parsed.
type compiledPolicy struct {
	id, name    string
	statements  []compiledStatement
	usesTeams   bool // some statement has a "team:" principal
	usesTenants bool // some statement has a "tenant:" principal
}

// compiledStatement is a PolicyStatement ready for matching.
type compiledStatement struct {
	index      int
	sid        string
	effect     string
	actions    []string
	resources  []parsedResource
	principals []string
	cond       compiledConditions
	// invalid is set when the statement itself is malformed (bad effect,
	// unparsable resource). Such statements never match: policies are
	// validated on write, so this only guards against corrupt data.
	invalid error
}

// compiledConditions holds the parsed conditions. err records a condition
// that could not be parsed: allow statements with a broken condition never
// apply, deny statements with one always do (fail closed).
type compiledConditions struct {
	ipRanges   []netip.Prefix
	requireMFA bool
	notBefore  *time.Time
	notAfter   *time.Time
	window     *compiledWindow
	err        error
}

type compiledWindow struct {
	days       map[time.Weekday]bool // nil = every day
	start, end int                   // minutes since midnight
	loc        *time.Location
	tzName     string
}

func (s *compiledStatement) label(p *compiledPolicy) string {
	if s.sid != "" {
		return fmt.Sprintf("policy %q statement %q", p.name, s.sid)
	}
	return fmt.Sprintf("policy %q statement #%d", p.name, s.index+1)
}

// needsRequest reports whether the statement has conditions that only a
// concrete request can decide (the client IP).
func (s *compiledStatement) needsRequest() bool { return len(s.cond.ipRanges) > 0 }

// compilePolicies compiles the enabled policies, in store order.
func compilePolicies(pols []domain.Policy) []compiledPolicy {
	out := make([]compiledPolicy, 0, len(pols))
	for _, p := range pols {
		if !p.Enabled {
			continue
		}
		cp := compiledPolicy{id: p.ID, name: p.Name, statements: make([]compiledStatement, 0, len(p.Statements))}
		for i, st := range p.Statements {
			cp.statements = append(cp.statements, compileStatement(i, st))
			for _, pr := range st.Principals {
				if strings.HasPrefix(strings.TrimSpace(pr), prefixTeam) {
					cp.usesTeams = true
				}
				if strings.HasPrefix(strings.TrimSpace(pr), prefixTenant) {
					cp.usesTenants = true
				}
			}
		}
		out = append(out, cp)
	}
	return out
}

func compileStatement(i int, st domain.PolicyStatement) compiledStatement {
	cs := compiledStatement{
		index:      i,
		sid:        st.Sid,
		effect:     strings.ToLower(strings.TrimSpace(st.Effect)),
		actions:    st.Actions,
		principals: st.Principals,
	}
	if cs.effect != domain.EffectAllow && cs.effect != domain.EffectDeny {
		cs.invalid = fmt.Errorf("unknown effect %q", st.Effect)
	}
	for _, r := range st.Resources {
		pr, err := parseResource(r)
		if err != nil {
			if cs.invalid == nil {
				cs.invalid = err
			}
			continue
		}
		cs.resources = append(cs.resources, pr)
	}
	cs.cond = compileConditions(st.Conditions)
	return cs
}

func compileConditions(c domain.PolicyConditions) compiledConditions {
	cc := compiledConditions{requireMFA: c.RequireMFA, notBefore: c.NotBefore, notAfter: c.NotAfter}
	for _, r := range c.IPRanges {
		pfx, err := parsePrefix(r)
		if err != nil {
			cc.err = err
			continue
		}
		cc.ipRanges = append(cc.ipRanges, pfx)
	}
	if c.TimeWindow != nil {
		w, err := compileWindow(*c.TimeWindow)
		if err != nil {
			cc.err = err
		} else {
			cc.window = w
		}
	}
	return cc
}

// parsePrefix parses a CIDR, or a bare address as a single-host prefix.
func parsePrefix(s string) (netip.Prefix, error) {
	s = strings.TrimSpace(s)
	if strings.Contains(s, "/") {
		p, err := netip.ParsePrefix(s)
		if err != nil {
			return netip.Prefix{}, fmt.Errorf("invalid CIDR %q", s)
		}
		if p.Addr().Is4In6() {
			p = netip.PrefixFrom(p.Addr().Unmap(), max(p.Bits()-96, 0))
		}
		return p.Masked(), nil
	}
	a, err := netip.ParseAddr(s)
	if err != nil {
		return netip.Prefix{}, fmt.Errorf("invalid CIDR %q", s)
	}
	a = a.Unmap().WithZone("")
	return netip.PrefixFrom(a, a.BitLen()), nil
}

// parseClock parses "HH:MM" (00:00..24:00) into minutes since midnight.
func parseClock(s string) (int, error) {
	h, m, ok := strings.Cut(strings.TrimSpace(s), ":")
	if !ok || len(h) != 2 || len(m) != 2 {
		return 0, fmt.Errorf("time %q must be HH:MM", s)
	}
	hh, err1 := strconv.Atoi(h)
	mm, err2 := strconv.Atoi(m)
	if err1 != nil || err2 != nil || hh < 0 || mm < 0 || mm > 59 || hh > 24 || (hh == 24 && mm != 0) {
		return 0, fmt.Errorf("time %q must be HH:MM between 00:00 and 24:00", s)
	}
	return hh*60 + mm, nil
}

func compileWindow(tw domain.TimeWindow) (*compiledWindow, error) {
	w := &compiledWindow{tzName: tw.Timezone}
	if w.tzName == "" {
		w.tzName = "UTC"
	}
	loc, err := time.LoadLocation(w.tzName)
	if err != nil {
		return nil, fmt.Errorf("unknown time zone %q", tw.Timezone)
	}
	w.loc = loc
	if w.start, err = parseClock(tw.Start); err != nil {
		return nil, err
	}
	if w.end, err = parseClock(tw.End); err != nil {
		return nil, err
	}
	if w.start == w.end {
		return nil, fmt.Errorf("time window start and end are both %s", tw.Start)
	}
	if w.start == 24*60 {
		return nil, fmt.Errorf("time window cannot start at 24:00")
	}
	if len(tw.Days) > 0 {
		w.days = map[time.Weekday]bool{}
		for _, d := range tw.Days {
			wd, ok := weekdays[strings.ToLower(strings.TrimSpace(d))]
			if !ok {
				return nil, fmt.Errorf("unknown day %q (want mon..sun)", d)
			}
			w.days[wd] = true
		}
	}
	return w, nil
}

// contains reports whether t falls inside the weekly window. A window that
// wraps midnight (start > end) belongs to the day on which it starts.
func (w *compiledWindow) contains(t time.Time) bool {
	lt := t.In(w.loc)
	minute := lt.Hour()*60 + lt.Minute()
	day := lt.Weekday()
	onDay := func(d time.Weekday) bool { return w.days == nil || w.days[d] }
	if w.start < w.end {
		return onDay(day) && minute >= w.start && minute < w.end
	}
	if minute >= w.start {
		return onDay(day)
	}
	return minute < w.end && onDay((day+6)%7)
}

func (w *compiledWindow) String() string {
	days := "every day"
	if w.days != nil {
		var names []string
		for _, n := range []string{"mon", "tue", "wed", "thu", "fri", "sat", "sun"} {
			if w.days[weekdays[n]] {
				names = append(names, n)
			}
		}
		days = strings.Join(names, ",")
	}
	return fmt.Sprintf("%s %02d:%02d-%02d:%02d %s", days, w.start/60, w.start%60, w.end/60, w.end%60, w.tzName)
}

// parseRemoteIP extracts the address from "ip", "ip:port" or "[ip]:port".
func parseRemoteIP(s string) (netip.Addr, bool) {
	s = strings.TrimSpace(s)
	if s == "" {
		return netip.Addr{}, false
	}
	if host, _, err := net.SplitHostPort(s); err == nil {
		s = host
	}
	s = strings.TrimSuffix(strings.TrimPrefix(s, "["), "]")
	a, err := netip.ParseAddr(s)
	if err != nil {
		return netip.Addr{}, false
	}
	return a.Unmap().WithZone(""), true
}

// condResult is the outcome of checking a statement's conditions.
type condResult struct {
	holds  bool
	detail string // why the conditions do not hold (or how a broken one was treated)
}

// check evaluates the conditions for a request. failClosed selects how
// conditions that cannot be decided (broken condition, unknown client IP)
// are treated: true for deny statements, so they still apply.
func (c *compiledConditions) check(env evalEnv, failClosed bool) condResult {
	if c.err != nil {
		if failClosed {
			return condResult{holds: true, detail: "invalid condition (" + c.err.Error() + ") treated as satisfied"}
		}
		return condResult{detail: "invalid condition: " + c.err.Error()}
	}
	if c.requireMFA && !env.mfa {
		return condResult{detail: "requires MFA and the session did not use a second factor"}
	}
	if c.notBefore != nil && env.now.Before(*c.notBefore) {
		return condResult{detail: "not before " + c.notBefore.UTC().Format(time.RFC3339)}
	}
	if c.notAfter != nil && env.now.After(*c.notAfter) {
		return condResult{detail: "expired after " + c.notAfter.UTC().Format(time.RFC3339)}
	}
	if c.window != nil && !c.window.contains(env.now) {
		return condResult{detail: "outside the time window " + c.window.String()}
	}
	if len(c.ipRanges) > 0 {
		ip, ok := parseRemoteIP(env.remoteIP)
		if !ok {
			if failClosed {
				return condResult{holds: true, detail: "client IP unknown; IP condition treated as satisfied"}
			}
			return condResult{detail: "client IP unknown; IP condition not satisfied"}
		}
		for _, p := range c.ipRanges {
			if p.Contains(ip) {
				return condResult{holds: true}
			}
		}
		return condResult{detail: "client IP " + ip.String() + " is outside " + prefixesString(c.ipRanges)}
	}
	return condResult{holds: true}
}

func prefixesString(ps []netip.Prefix) string {
	s := make([]string, len(ps))
	for i, p := range ps {
		s[i] = p.String()
	}
	return strings.Join(s, ", ")
}

// principalFacts are the attributes of the caller that principal patterns
// match against, in the context of one tenant (the policy's tenant).
type principalFacts struct {
	anonymous bool
	username  string // users only
	saName    string // service accounts only
	saTenant  string // slug of the service account's tenant
	saLocal   bool   // service account belongs to the policy tenant
	roles     []string
	teams     []string // team slugs in the policy tenant (loaded only when a statement names a team)
	tenants   []string // slugs of the tenants the caller belongs to (loaded only when a statement names a tenant)
}

// principalMatches reports whether pattern covers the caller.
func principalMatches(pattern string, f *principalFacts) bool {
	pattern = strings.TrimSpace(pattern)
	switch pattern {
	case principalAny:
		return true
	case principalAnonymous:
		return f.anonymous
	case principalAuthenticated:
		return !f.anonymous
	}
	if f.anonymous {
		return false
	}
	switch {
	case strings.HasPrefix(pattern, prefixUser):
		return f.username != "" && globMatch(strings.TrimPrefix(pattern, prefixUser), f.username)
	case strings.HasPrefix(pattern, prefixSA):
		if f.saName == "" {
			return false
		}
		name := strings.TrimPrefix(pattern, prefixSA)
		if tenant, n, ok := strings.Cut(name, "/"); ok {
			return globMatch(tenant, f.saTenant) && globMatch(n, f.saName)
		}
		return f.saLocal && globMatch(name, f.saName)
	case strings.HasPrefix(pattern, prefixRole):
		r := strings.ToLower(strings.TrimPrefix(pattern, prefixRole))
		for _, have := range f.roles {
			if globMatch(r, have) {
				return true
			}
		}
	case strings.HasPrefix(pattern, prefixTeam):
		t := strings.TrimPrefix(pattern, prefixTeam)
		for _, have := range f.teams {
			if globMatch(t, have) {
				return true
			}
		}
	case strings.HasPrefix(pattern, prefixTenant):
		t := strings.TrimPrefix(pattern, prefixTenant)
		for _, have := range f.tenants {
			if globMatch(t, have) {
				return true
			}
		}
	}
	return false
}

// validatePrincipalPattern checks a statement principal.
func validatePrincipalPattern(p string) error {
	switch p {
	case principalAny, principalAnonymous, principalAuthenticated:
		return nil
	case "":
		return fmt.Errorf("principal is empty")
	}
	for _, prefix := range []string{prefixUser, prefixTeam, prefixRole, prefixSA, prefixTenant} {
		if name, ok := strings.CutPrefix(p, prefix); ok {
			if name == "" {
				return fmt.Errorf("principal %q has no name after %q", p, prefix)
			}
			if strings.ContainsAny(name, " \t\r\n") {
				return fmt.Errorf("principal %q contains whitespace", p)
			}
			if prefix == prefixSA {
				if t, n, ok := strings.Cut(name, "/"); ok && (t == "" || n == "" || strings.Contains(n, "/")) {
					return fmt.Errorf("principal %q must be sa:<name> or sa:<tenant>/<name>", p)
				}
			} else if strings.Contains(name, "/") {
				return fmt.Errorf("principal %q must not contain '/'", p)
			}
			return nil
		}
	}
	return fmt.Errorf("unknown principal %q (want user:, team:, role:, sa:, tenant:, authenticated, anonymous or *)", p)
}
