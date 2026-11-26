//go:build !darwin

package objstore

import "os"

// syncData makes f's data durable (fsync / FlushFileBuffers).
func syncData(f *os.File) error { return f.Sync() }

// flushDrive is only needed where syncData leaves data in the drive's
// write cache (macOS).
func flushDrive(string) error { return nil }
