package sso

import "testing"

func TestAMRHasMFA(t *testing.T) {
	cases := []struct {
		c    claims
		want bool
	}{
		{claims{}, false},
		{claims{"amr": []any{"pwd"}}, false},
		{claims{"amr": []any{"pwd", "otp"}}, true},
		{claims{"amr": []any{"MFA"}}, true},
		{claims{"amr": "mfa"}, false}, // not an array
	}
	for _, tc := range cases {
		if got := amrHasMFA(tc.c); got != tc.want {
			t.Errorf("amrHasMFA(%v) = %v, want %v", tc.c, got, tc.want)
		}
	}
}
