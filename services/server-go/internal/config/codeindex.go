package config

import (
	"flag"
	"fmt"
	"math"
	"strconv"
	"strings"
)

// Code search v2 settings (docs/ENGINEERING_PLAN.md §15.9).

// Defaults of the code search index.
const (
	DefaultCodeIndexMemory   = 256 << 20
	DefaultCodeIndexBranches = 10
	maxCodeIndexBranches     = 1000
)

// ParseByteSize parses a size such as "256MiB", "512M", "1.5G", "10KB" or
// a plain number of bytes. K/M/G/T and KiB/MiB/GiB/TiB are powers of 1024,
// KB/MB/GB/TB powers of 1000; a trailing "B" alone means bytes.
func ParseByteSize(s string) (int64, error) {
	t := strings.TrimSpace(s)
	if t == "" {
		return 0, fmt.Errorf("empty size")
	}
	i := len(t)
	for i > 0 && (t[i-1] < '0' || t[i-1] > '9') && t[i-1] != '.' {
		i--
	}
	num, unit := strings.TrimSpace(t[:i]), strings.ToUpper(strings.TrimSpace(t[i:]))
	mult := float64(1)
	switch unit {
	case "", "B":
	case "K", "KIB":
		mult = 1 << 10
	case "M", "MIB":
		mult = 1 << 20
	case "G", "GIB":
		mult = 1 << 30
	case "T", "TIB":
		mult = 1 << 40
	case "KB":
		mult = 1e3
	case "MB":
		mult = 1e6
	case "GB":
		mult = 1e9
	case "TB":
		mult = 1e12
	default:
		return 0, fmt.Errorf("size %q: unknown unit %q", s, t[i:])
	}
	f, err := strconv.ParseFloat(num, 64)
	if err != nil || f < 0 || math.IsInf(f, 0) || math.IsNaN(f) {
		return 0, fmt.Errorf("size %q: want a non-negative number with an optional unit (e.g. 256MiB)", s)
	}
	v := f * mult
	if v > math.MaxInt64 {
		return 0, fmt.Errorf("size %q is too large", s)
	}
	return int64(v), nil
}

// codeIndexFromEnv applies the defaults and WT_CODE_INDEX_* variables.
func codeIndexFromEnv(cfg *Config, lookup lookupFunc) []error {
	var errs []error
	cfg.CodeIndexMemory = DefaultCodeIndexMemory
	cfg.CodeIndexBranches = DefaultCodeIndexBranches
	if v, ok := lookup("WT_CODE_INDEX_MEMORY"); ok && v != "" {
		n, err := ParseByteSize(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_CODE_INDEX_MEMORY: %v", err))
		} else {
			cfg.CodeIndexMemory = n
		}
	}
	if v, ok := lookup("WT_CODE_INDEX_BRANCHES"); ok && v != "" {
		n, err := strconv.Atoi(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_CODE_INDEX_BRANCHES: invalid integer %q", v))
		} else {
			cfg.CodeIndexBranches = n
		}
	}
	return errs
}

// byteSizeValue is a flag.Value for sizes.
type byteSizeValue struct{ p *int64 }

func (b byteSizeValue) String() string {
	if b.p == nil {
		return ""
	}
	return strconv.FormatInt(*b.p, 10)
}

func (b byteSizeValue) Set(s string) error {
	n, err := ParseByteSize(s)
	if err != nil {
		return err
	}
	*b.p = n
	return nil
}

// codeIndexFlags binds the code search flags.
func codeIndexFlags(fs *flag.FlagSet, cfg *Config) {
	fs.Var(byteSizeValue{&cfg.CodeIndexMemory}, "code-index-memory",
		"memory `size` for loaded code search index segments, e.g. 256MiB or 1G; 0 = unlimited (env WT_CODE_INDEX_MEMORY)")
	fs.IntVar(&cfg.CodeIndexBranches, "code-index-branches", cfg.CodeIndexBranches,
		"most branches per tree indexed for all-branches code searches (env WT_CODE_INDEX_BRANCHES)")
}

// validateCodeIndex checks the code search settings.
func (c *Config) validateCodeIndex() []error {
	var errs []error
	if c.CodeIndexMemory < 0 {
		errs = append(errs, fmt.Errorf("code-index-memory must not be negative, got %d", c.CodeIndexMemory))
	}
	if c.CodeIndexBranches < 1 || c.CodeIndexBranches > maxCodeIndexBranches {
		errs = append(errs, fmt.Errorf("code-index-branches must be 1-%d, got %d", maxCodeIndexBranches, c.CodeIndexBranches))
	}
	return errs
}
