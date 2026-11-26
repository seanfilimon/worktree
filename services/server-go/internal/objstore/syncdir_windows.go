//go:build windows

package objstore

// syncDir: Windows cannot flush a directory handle opened this way; NTFS
// journals the rename itself.
func syncDir(string) error { return nil }
