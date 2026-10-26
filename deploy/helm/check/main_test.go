package main

import (
	"strings"
	"testing"
)

// TestChartCases renders the real chart for every case file.
func TestChartCases(t *testing.T) {
	chart, err := loadChart("../worktree")
	if err != nil {
		t.Fatal(err)
	}
	cases, err := loadCases("cases")
	if err != nil {
		t.Fatal(err)
	}
	if len(cases) < 4 {
		t.Fatalf("expected the defaults plus case files, got %d cases", len(cases))
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if _, err := runCase(chart, c); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestMergeValues(t *testing.T) {
	dst := map[string]any{"a": map[string]any{"x": 1.0, "y": 2.0}, "l": []any{1.0}, "gone": "v"}
	src := map[string]any{"a": map[string]any{"y": 3.0}, "l": []any{2.0, 3.0}, "gone": nil}
	got := mergeValues(dst, src)
	a := got["a"].(map[string]any)
	if a["x"] != 1.0 || a["y"] != 3.0 {
		t.Fatalf("maps must merge recursively: %v", a)
	}
	if l := got["l"].([]any); len(l) != 2 {
		t.Fatalf("lists must be replaced: %v", l)
	}
	if _, ok := got["gone"]; ok {
		t.Fatal("null must delete the key")
	}
}

func TestValidateSchema(t *testing.T) {
	schema := map[string]any{
		"type":     "object",
		"required": []any{"port"},
		"properties": map[string]any{
			"port": map[string]any{"type": "integer", "minimum": 1.0, "maximum": 65535.0},
			"mode": map[string]any{"enum": []any{"a", "b"}},
			"name": map[string]any{"type": "string", "pattern": "^[a-z]+$"},
		},
	}
	tests := []struct {
		name    string
		value   map[string]any
		wantErr string
	}{
		{"valid", map[string]any{"port": 80.0, "mode": "a", "name": "abc"}, ""},
		{"missing required", map[string]any{}, "missing required property port"},
		{"not integer", map[string]any{"port": 1.5}, "want type integer"},
		{"above maximum", map[string]any{"port": 70000.0}, "above the maximum"},
		{"enum", map[string]any{"port": 1.0, "mode": "c"}, "is not one of"},
		{"pattern", map[string]any{"port": 1.0, "name": "ABC"}, "does not match"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := validateSchema(schema, tt.value, "values")
			switch {
			case tt.wantErr == "" && err != nil:
				t.Fatalf("unexpected error: %v", err)
			case tt.wantErr != "" && (err == nil || !strings.Contains(err.Error(), tt.wantErr)):
				t.Fatalf("want error containing %q, got %v", tt.wantErr, err)
			}
		})
	}
}

func TestCheckKindCronJob(t *testing.T) {
	job := func(name, image, restart string) map[string]any {
		return map[string]any{
			"metadata": map[string]any{"name": name},
			"spec": map[string]any{
				"schedule": "0 3 * * *",
				"jobTemplate": map[string]any{"spec": map[string]any{"template": map[string]any{"spec": map[string]any{
					"restartPolicy": restart,
					"containers":    []any{map[string]any{"name": "c", "image": image}},
				}}}},
			},
		}
	}
	tests := []struct {
		name    string
		doc     map[string]any
		wantErr string
	}{
		{"valid", job("wt-backup", "curlimages/curl:8.11.1", "Never"), ""},
		{"registry port is not a tag", job("wt-backup", "registry:5000/curl", "Never"), "no image tag"},
		{"no tag", job("wt-backup", "curlimages/curl", "Never"), "no image tag"},
		{"restart always", job("wt-backup", "curl:1", "Always"), "restartPolicy"},
		{"long name", job(strings.Repeat("a", 53), "curl:1", "Never"), "longer than 52"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := checkKind("CronJob/x", "CronJob", tt.doc)
			switch {
			case tt.wantErr == "" && err != nil:
				t.Fatalf("unexpected error: %v", err)
			case tt.wantErr != "" && (err == nil || !strings.Contains(err.Error(), tt.wantErr)):
				t.Fatalf("want error containing %q, got %v", tt.wantErr, err)
			}
		})
	}
}

func TestSprigSemantics(t *testing.T) {
	f := sprigSubset()
	def := f["default"].(func(any, ...any) any)
	if def("d", "") != "d" || def("d", "x") != "x" || def("d", false) != "d" {
		t.Fatal("default must treat empty values as unset")
	}
	if got := f["trunc"].(func(int, string) string)(3, "abcdef"); got != "abc" {
		t.Fatalf("trunc: %q", got)
	}
	if got := f["nindent"].(func(int, string) string)(2, "a\nb"); got != "\n  a\n  b" {
		t.Fatalf("nindent: %q", got)
	}
	merged := f["merge"].(func(map[string]any, ...map[string]any) map[string]any)(
		map[string]any{}, map[string]any{"k": "first"}, map[string]any{"k": "second", "o": "x"})
	if merged["k"] != "first" || merged["o"] != "x" {
		t.Fatalf("merge: earlier maps must win: %v", merged)
	}
	if got := strval(1200.0); got != "1200" {
		t.Fatalf("strval(float): %q", got)
	}
}
