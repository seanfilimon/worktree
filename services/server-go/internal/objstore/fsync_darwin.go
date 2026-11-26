//go:build darwin

package objstore

import (
	"os"
	"syscall"
)

// syncData pushes f's data to the drive with fsync(2). On macOS that does
// not flush the drive's own write cache (os.File.Sync does, with
// F_FULLFSYNC, at roughly ten times the cost); flushDrive does that once
// per batch.
func syncData(f *os.File) error {
	for {
		err := syscall.Fsync(int(f.Fd()))
		if err != syscall.EINTR {
			return err
		}
	}
}

// flushDrive makes everything synced with syncData durable against power
// loss: F_FULLFSYNC flushes the whole device's write cache.
func flushDrive(dir string) error {
	d, err := os.Open(dir)
	if err != nil {
		return err
	}
	defer d.Close()
	return d.Sync()
}
