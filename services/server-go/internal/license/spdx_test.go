package license

import (
	"strings"
	"testing"
)

func TestValidateSPDX(t *testing.T) {
	valid := []string{
		"MIT",
		"Apache-2.0",
		"GPL-3.0-or-later",
		"GPL-2.0+",
		"Proprietary",
		"proprietary",
		"LicenseRef-Proprietary",
		"LicenseRef-AcmeCorp-Internal-v2",
		"DocumentRef-spdx-tool-1.2:LicenseRef-MIT-Style-2",
		"MIT OR Apache-2.0",
		"MIT AND BSD-3-Clause",
		"GPL-2.0-only WITH Classpath-exception-2.0",
		"(MIT OR Apache-2.0) AND LicenseRef-Proprietary",
		"((MIT))",
		"MIT OR (GPL-2.0-only WITH Classpath-exception-2.0 AND BSD-2-Clause)",
		"  MIT  ",
	}
	for _, s := range valid {
		if err := ValidateSPDX(s); err != nil {
			t.Errorf("ValidateSPDX(%q) = %v, want nil", s, err)
		}
	}
	invalid := []string{
		"",
		"   ",
		"MIT Apache-2.0",
		"MIT OR",
		"OR MIT",
		"MIT or Apache-2.0",
		"MIT AND",
		"(MIT",
		"MIT)",
		"()",
		"MIT WITH",
		"MIT WITH (x)",
		"(MIT OR BSD-2-Clause) WITH Classpath-exception-2.0",
		"MIT_License",
		"MIT/X11",
		"LicenseRef-",
		"LicenseRef-a_b",
		"LicenseRef-x+",
		"DocumentRef-:LicenseRef-x",
		"DocumentRef-x",
		"Foo:LicenseRef-x",
		"MIT++",
		"+",
		strings.Repeat("(", 40) + "MIT" + strings.Repeat(")", 40),
		strings.Repeat("MIT OR ", 200) + "MIT",
	}
	for _, s := range invalid {
		if err := ValidateSPDX(s); err == nil {
			t.Errorf("ValidateSPDX(%q) = nil, want error", s)
		}
	}
}

func TestIsProprietary(t *testing.T) {
	tests := []struct {
		spdx string
		want bool
	}{
		{"MIT", false},
		{"Apache-2.0", false},
		{"Proprietary", true},
		{"proprietary", true},
		{"PROPRIETARY", true},
		{"LicenseRef-Proprietary", true},
		{"LicenseRef-proprietary", true},
		{"LicenseRef-Acme-Proprietary-v2", true},
		{"DocumentRef-x:LicenseRef-Proprietary", true},
		{"LicenseRef-AcmeCorp-Internal", false},
		{"MIT OR LicenseRef-Proprietary", false},
		{"MIT AND LicenseRef-Proprietary", true},
		{"Proprietary OR LicenseRef-Proprietary", true},
		{"(MIT AND Proprietary) OR (Apache-2.0 AND Proprietary)", true},
		{"(MIT AND Proprietary) OR Apache-2.0", false},
		{"Proprietary WITH Classpath-exception-2.0", true},
		{"not a valid expression", true}, // fail closed
		{"", true},
	}
	for _, tt := range tests {
		if got := IsProprietary(tt.spdx); got != tt.want {
			t.Errorf("IsProprietary(%q) = %v, want %v", tt.spdx, got, tt.want)
		}
	}
}
