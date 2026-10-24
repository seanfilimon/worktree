//go:build !windows

package objstore

// isTransientFSError reports whether err is a transient sharing violation.
// POSIX rename(2) atomically replaces the destination and open handles do
// not block it, so no error is considered transient outside Windows.
func isTransientFSError(error) bool { return false }
