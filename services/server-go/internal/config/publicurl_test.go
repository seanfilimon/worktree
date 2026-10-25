package config

import "testing"

func TestPublicURL(t *testing.T) {
	tests := []struct {
		name string
		args []string
		env  map[string]string
		want string
		ok   bool
	}{
		{"unset", nil, nil, "", true},
		{"env", nil, map[string]string{"WT_PUBLIC_URL": "https://wt.example"}, "https://wt.example", true},
		{"flag overrides env", []string{"--public-url", "http://localhost:7420"}, map[string]string{"WT_PUBLIC_URL": "https://wt.example"}, "http://localhost:7420", true},
		{"with path", []string{"--public-url", "https://example.com/wt"}, nil, "https://example.com/wt", true},
		{"no scheme", []string{"--public-url", "wt.example"}, nil, "", false},
		{"query", []string{"--public-url", "https://wt.example/?a=1"}, nil, "", false},
		{"credentials", []string{"--public-url", "https://u:p@wt.example"}, nil, "", false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			cfg, err := load(tc.args, env(tc.env))
			if !tc.ok {
				if err == nil {
					t.Fatalf("expected error, got %+v", cfg)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if cfg.PublicURL != tc.want {
				t.Fatalf("PublicURL = %q, want %q", cfg.PublicURL, tc.want)
			}
		})
	}
}
