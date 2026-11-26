package mirror

import "testing"

// A symbolic link is exported with Git's link mode; its blob is the stored
// target text.
func TestFileModes(t *testing.T) {
	tests := []struct {
		name string
		f    file
		want string
	}{
		{name: "regular", f: file{}, want: "100644"},
		{name: "executable", f: file{exec: true}, want: "100755"},
		{name: "symlink", f: file{link: true}, want: "120000"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := tt.f.mode(); got != tt.want {
				t.Fatalf("mode = %s, want %s", got, tt.want)
			}
		})
	}
}
