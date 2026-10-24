//go:build windows

package objstore

import (
	"errors"
	"syscall"
)

// Win32 error codes that indicate another handle (a concurrent reader, a
// racing rename, an anti-virus scanner, the search indexer...) temporarily
// holds the file. They are not exported by package syscall, so they are
// spelled out here.
const (
	errorAccessDenied     syscall.Errno = 5  // ERROR_ACCESS_DENIED
	errorSharingViolation syscall.Errno = 32 // ERROR_SHARING_VIOLATION
	errorLockViolation    syscall.Errno = 33 // ERROR_LOCK_VIOLATION
)

// isTransientFSError reports whether err is a Windows sharing/locking error
// that is expected to clear once a concurrent handle on the same file is
// closed. MoveFileEx onto a destination that another goroutine has open
// (or is itself renaming onto) fails with ERROR_ACCESS_DENIED, so on
// Windows that code is treated as transient too.
func isTransientFSError(err error) bool {
	var errno syscall.Errno
	if !errors.As(err, &errno) {
		return false
	}
	switch errno {
	case errorAccessDenied, errorSharingViolation, errorLockViolation:
		return true
	}
	return false
}
