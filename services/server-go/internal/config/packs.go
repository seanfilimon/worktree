package config

import (
	"flag"
	"fmt"
	"strconv"
)

// Pack transfer settings (docs/ENGINEERING_PLAN.md §16.1,
// crates/worktree-protocol/specs/sync/Packs.md).

// DefaultMaxPackSize bounds one pack upload.
const DefaultMaxPackSize = 4 << 30

// minMaxPackSize is the smallest accepted --max-pack-size.
const minMaxPackSize = 1 << 20

// PackConfig configures UploadPack / QueryUpload / DownloadPack.
type PackConfig struct {
	// Disabled answers every pack RPC with Unimplemented, so clients fall
	// back to UploadObjects / DownloadObjects. Flag --disable-packs, env
	// WT_DISABLE_PACKS.
	Disabled bool
	// MaxPackSize bounds one pack upload in bytes. Flag --max-pack-size
	// (e.g. 4GiB), env WT_MAX_PACK_SIZE (default 4 GiB).
	MaxPackSize int64
}

// packsFromEnv applies the defaults and WT_DISABLE_PACKS / WT_MAX_PACK_SIZE.
func packsFromEnv(cfg *Config, lookup lookupFunc) []error {
	var errs []error
	cfg.Packs.MaxPackSize = DefaultMaxPackSize
	if v, ok := lookup("WT_DISABLE_PACKS"); ok && v != "" {
		b, err := strconv.ParseBool(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_DISABLE_PACKS: invalid boolean %q", v))
		}
		cfg.Packs.Disabled = b
	}
	if v, ok := lookup("WT_MAX_PACK_SIZE"); ok && v != "" {
		n, err := ParseByteSize(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_MAX_PACK_SIZE: %v", err))
		} else {
			cfg.Packs.MaxPackSize = n
		}
	}
	return errs
}

// packFlags binds the pack flags.
func packFlags(fs *flag.FlagSet, cfg *Config) {
	fs.BoolVar(&cfg.Packs.Disabled, "disable-packs", cfg.Packs.Disabled,
		"answer the pack RPCs (UploadPack, QueryUpload, DownloadPack) with Unimplemented so clients transfer whole objects (env WT_DISABLE_PACKS)")
	fs.Var(byteSizeValue{&cfg.Packs.MaxPackSize}, "max-pack-size",
		"largest pack `size` one upload may send, e.g. 4GiB (env WT_MAX_PACK_SIZE)")
}

// validatePacks checks the pack settings (0 = the default).
func (c *Config) validatePacks() []error {
	if c.Packs.MaxPackSize != 0 && c.Packs.MaxPackSize < minMaxPackSize {
		return []error{fmt.Errorf("max-pack-size must be at least 1MiB, got %d", c.Packs.MaxPackSize)}
	}
	return nil
}
