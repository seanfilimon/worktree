//go:build !windows

package objstore

import "os"

// syncDir makes the entries of dir (renames into it, files and directories
// created in it) durable.
func syncDir(dir string) error {
	d, err := os.Open(dir)
	if err != nil {
		return err
	}
	defer d.Close()
	return syncData(d)
}
