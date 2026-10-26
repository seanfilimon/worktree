package license

import (
	"sort"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// NoticePath is the generated license notice inside an export (archives,
// Git exports and mirrors; shared with `wt archive` / `wt git export`,
// crates/worktree-git/src/license.rs).
const NoticePath = "LICENSES/NOTICE"

// BuildNotice renders the LICENSES/NOTICE file of an export (same layout
// as the CLI): the header lines, then the SPDX id of every included path
// covered by one of rules, grouped by SPDX id, then footer verbatim (for
// example a line saying how many paths were omitted). It returns nil when
// no included path is covered and footer is empty.
//
// rules should be the general (grantee-less), unexpired rules: grantee
// rules describe one tenant's rights and do not change a file's license.
func BuildNotice(rules []domain.LicenseRule, paths []string, header []string, footer string) []byte {
	bySPDX := map[string][]string{}
	for _, p := range paths {
		if rule, ok := Resolve(rules, p); ok {
			bySPDX[rule.SPDX] = append(bySPDX[rule.SPDX], p)
		}
	}
	if len(bySPDX) == 0 && footer == "" {
		return nil
	}
	var b strings.Builder
	b.WriteString("W0rkTree license notice\n=======================\n\n")
	for _, line := range header {
		b.WriteString(line + "\n")
	}
	if len(header) > 0 {
		b.WriteString("\n")
	}
	b.WriteString("SPDX license identifiers of the files in this export, from the tree's\n" +
		"license rules. Files that are not listed are not covered by a rule.\n")
	ids := make([]string, 0, len(bySPDX))
	for id := range bySPDX {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	for _, id := range ids {
		b.WriteString("\n" + id + "\n")
		ps := bySPDX[id]
		sort.Strings(ps)
		for _, p := range ps {
			b.WriteString("    " + p + "\n")
		}
	}
	b.WriteString(footer)
	return []byte(b.String())
}
