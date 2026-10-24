package main

import (
	"strings"
	"testing"
)

func TestRunExitCodes(t *testing.T) {
	tests := []struct {
		name       string
		args       []string
		code       int
		wantStdout string
		wantStderr string
	}{
		{"version", []string{"--version"}, 0, "worktree-server " + version, ""},
		{"help", []string{"-h"}, 0, "Usage: worktree-server", ""},
		{"unknown flag", []string{"--bogus"}, 2, "", "flag provided but not defined"},
		{"invalid config", []string{"--log-format=xml"}, 2, "", "log-format"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			var stdout, stderr strings.Builder
			if code := run(tc.args, &stdout, &stderr); code != tc.code {
				t.Fatalf("exit = %d, want %d (stderr %q)", code, tc.code, stderr.String())
			}
			if !strings.Contains(stdout.String(), tc.wantStdout) || !strings.Contains(stderr.String(), tc.wantStderr) {
				t.Fatalf("stdout %q stderr %q", stdout.String(), stderr.String())
			}
		})
	}
}
