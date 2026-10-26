package config

// Storage backends and backups (docs/ENGINEERING_PLAN.md §15.2): object
// store selection (--object-store disk|s3 and --s3-*), scheduled backups
// (--backup-interval, --backup-keep) and the offline `worktree-server
// restore` subcommand's flags.

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"strconv"
	"strings"
	"time"
)

// Object store kinds (--object-store).
const (
	ObjectStoreDisk = "disk"
	ObjectStoreS3   = "s3"
)

// DefaultBackupKeep is the default --backup-keep.
const DefaultBackupKeep = 7

// S3Config configures the S3 object store. Prefer the environment for the
// secret key (flags are visible in process listings).
type S3Config struct {
	// Endpoint is host[:port] or http(s)://host[:port]. Flag --s3-endpoint,
	// env WT_S3_ENDPOINT.
	Endpoint string
	// Bucket holds the objects (created when missing). Flag --s3-bucket,
	// env WT_S3_BUCKET.
	Bucket string
	// Prefix is prepended to every key. Flag --s3-prefix, env WT_S3_PREFIX.
	Prefix string
	// Region of the bucket ("" = discovered). Flag --s3-region, env
	// WT_S3_REGION.
	Region string
	// AccessKey / SecretKey are static credentials; when both are empty
	// AWS_* / MINIO_* env vars and the instance role are used. Flags
	// --s3-access-key/--s3-secret-key, env WT_S3_ACCESS_KEY/WT_S3_SECRET_KEY.
	AccessKey string
	SecretKey string
	// Insecure talks plain HTTP. Flag --s3-insecure, env WT_S3_INSECURE.
	Insecure bool
}

// backendsFromEnv applies the §15.2 defaults and WT_* variables to cfg.
func backendsFromEnv(cfg *Config, lookup lookupFunc) []error {
	var errs []error
	cfg.ObjectStore = ObjectStoreDisk
	cfg.BackupKeep = DefaultBackupKeep
	str := func(key string, dst *string) {
		if v, ok := lookup(key); ok && v != "" {
			*dst = v
		}
	}
	str("WT_OBJECT_STORE", &cfg.ObjectStore)
	str("WT_S3_ENDPOINT", &cfg.S3.Endpoint)
	str("WT_S3_BUCKET", &cfg.S3.Bucket)
	str("WT_S3_PREFIX", &cfg.S3.Prefix)
	str("WT_S3_REGION", &cfg.S3.Region)
	str("WT_S3_ACCESS_KEY", &cfg.S3.AccessKey)
	str("WT_S3_SECRET_KEY", &cfg.S3.SecretKey)
	if v, ok := lookup("WT_S3_INSECURE"); ok && v != "" {
		b, err := strconv.ParseBool(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_S3_INSECURE: invalid boolean %q", v))
		}
		cfg.S3.Insecure = b
	}
	if v, ok := lookup("WT_BACKUP_INTERVAL"); ok && v != "" {
		d, err := time.ParseDuration(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_BACKUP_INTERVAL: invalid duration %q", v))
		}
		cfg.BackupInterval = d
	}
	if v, ok := lookup("WT_BACKUP_KEEP"); ok && v != "" {
		n, err := strconv.Atoi(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_BACKUP_KEEP: invalid integer %q", v))
		}
		cfg.BackupKeep = n
	}
	return errs
}

// objectStoreFlags binds --object-store and the --s3-* flags.
func objectStoreFlags(fs *flag.FlagSet, cfg *Config) {
	fs.StringVar(&cfg.ObjectStore, "object-store", cfg.ObjectStore, "object store `kind`: disk (objects/ in the data directory) or s3 (env WT_OBJECT_STORE)")
	fs.StringVar(&cfg.S3.Endpoint, "s3-endpoint", cfg.S3.Endpoint, "S3 API `host:port` or http(s)://host:port, e.g. s3.amazonaws.com or minio:9000 (env WT_S3_ENDPOINT)")
	fs.StringVar(&cfg.S3.Bucket, "s3-bucket", cfg.S3.Bucket, "S3 `bucket` holding the objects; created when missing (env WT_S3_BUCKET)")
	fs.StringVar(&cfg.S3.Prefix, "s3-prefix", cfg.S3.Prefix, "key `prefix` inside the bucket, e.g. wt/ (env WT_S3_PREFIX)")
	fs.StringVar(&cfg.S3.Region, "s3-region", cfg.S3.Region, "S3 `region`; empty discovers it (env WT_S3_REGION)")
	fs.StringVar(&cfg.S3.AccessKey, "s3-access-key", cfg.S3.AccessKey, "S3 access key `id` (env WT_S3_ACCESS_KEY; empty uses AWS_*/MINIO_* env or the instance role)")
	fs.StringVar(&cfg.S3.SecretKey, "s3-secret-key", cfg.S3.SecretKey, "S3 secret `key` (env WT_S3_SECRET_KEY; prefer the env var)")
	fs.BoolVar(&cfg.S3.Insecure, "s3-insecure", cfg.S3.Insecure, "talk plain HTTP to the S3 endpoint (env WT_S3_INSECURE)")
}

// backendFlags binds every §15.2 server flag.
func backendFlags(fs *flag.FlagSet, cfg *Config) {
	objectStoreFlags(fs, cfg)
	fs.DurationVar(&cfg.BackupInterval, "backup-interval", cfg.BackupInterval, "automatic backup `period` into <data>/backups; 0 disables (env WT_BACKUP_INTERVAL)")
	fs.IntVar(&cfg.BackupKeep, "backup-keep", cfg.BackupKeep, "number of scheduled backups to keep; 0 keeps all (env WT_BACKUP_KEEP)")
}

// validateBackends checks the §15.2 settings.
func (c *Config) validateBackends() []error {
	var errs []error
	errs = append(errs, validateObjectStore(c.ObjectStore, c.S3)...)
	if c.BackupInterval < 0 {
		errs = append(errs, fmt.Errorf("backup-interval must not be negative, got %s", c.BackupInterval))
	} else if c.BackupInterval > 0 && c.BackupInterval < time.Minute {
		errs = append(errs, fmt.Errorf("backup-interval must be at least 1m, got %s", c.BackupInterval))
	}
	if c.BackupKeep < 0 {
		errs = append(errs, fmt.Errorf("backup-keep must not be negative, got %d", c.BackupKeep))
	}
	return errs
}

func validateObjectStore(kind string, s3 S3Config) []error {
	var errs []error
	switch kind {
	case ObjectStoreDisk:
	case ObjectStoreS3:
		if strings.TrimSpace(s3.Endpoint) == "" {
			errs = append(errs, errors.New("object-store s3 requires --s3-endpoint (WT_S3_ENDPOINT)"))
		}
		if strings.TrimSpace(s3.Bucket) == "" {
			errs = append(errs, errors.New("object-store s3 requires --s3-bucket (WT_S3_BUCKET)"))
		}
		if (s3.AccessKey == "") != (s3.SecretKey == "") {
			errs = append(errs, errors.New("s3-access-key and s3-secret-key must be given together"))
		}
	default:
		errs = append(errs, fmt.Errorf("object-store %q: want disk or s3", kind))
	}
	return errs
}

// restoreUsageHint appends the restore subcommand to the server usage.
func restoreUsageHint(w io.Writer) {
	fmt.Fprintf(w, "\nSubcommands:\n  worktree-server restore [--force] --data DIR [storage flags] FILE.wtbak|BACKUP-ID\n\tRestore a backup into an empty data directory (offline; see `worktree-server restore -h`).\n")
}

// RestoreConfig is the configuration of `worktree-server restore`.
type RestoreConfig struct {
	// DataDir receives meta.db (and objects/ for the disk store). Flag
	// --data, env WT_DATA.
	DataDir string
	// Force allows restoring into a non-empty data directory: meta.db is
	// replaced and objects are added. Flag --force.
	Force bool
	// File is the .wtbak archive.
	File string
	// ObjectStore, S3 and CompressObjects select where objects are
	// restored to (same flags and env as the server).
	ObjectStore     string
	S3              S3Config
	CompressObjects bool

	// server-ops (§16.11): FILE may be a backup id, read from the backup
	// target (BackupTarget/BackupS3Prefix, same flags as the server);
	// List lists the target's backups instead of restoring.
	BackupTarget   string
	BackupS3Prefix string
	List           bool
}

// LoadRestore parses the arguments of `worktree-server restore` (after the
// subcommand name) and the environment. It returns flag.ErrHelp for -h.
func LoadRestore(args []string) (RestoreConfig, error) {
	return loadRestore(args, os.LookupEnv)
}

func loadRestore(args []string, lookup lookupFunc) (RestoreConfig, error) {
	base, err := fromEnv(lookup)
	if err != nil {
		return RestoreConfig{}, err
	}
	var force, list bool
	fs := restoreFlagSet(&base, &force, &list, io.Discard)
	// Flags may come before or after FILE.
	var positional []string
	for {
		if err := fs.Parse(args); err != nil {
			return RestoreConfig{}, err
		}
		if fs.NArg() == 0 {
			break
		}
		positional = append(positional, fs.Arg(0))
		args = fs.Args()[1:]
	}
	rc := RestoreConfig{
		DataDir:         base.DataDir,
		Force:           force,
		ObjectStore:     base.ObjectStore,
		S3:              base.S3,
		CompressObjects: base.CompressObjects,
		// server-ops (§16.11)
		BackupTarget:   base.BackupTarget,
		BackupS3Prefix: base.BackupS3Prefix,
		List:           list,
	}
	switch len(positional) {
	case 0:
		if list { // server-ops (§16.11): --list needs no FILE
			break
		}
		return RestoreConfig{}, errors.New("restore: missing the backup FILE or ID")
	case 1:
		if list {
			return RestoreConfig{}, fmt.Errorf("restore: --list takes no argument, got %q", positional[0])
		}
		rc.File = positional[0]
	default:
		return RestoreConfig{}, fmt.Errorf("restore: unexpected argument %q", positional[1])
	}
	var errs []error
	if strings.TrimSpace(rc.DataDir) == "" {
		errs = append(errs, errors.New("restore: data directory must not be empty"))
	}
	if !rc.List {
		errs = append(errs, validateObjectStore(rc.ObjectStore, rc.S3)...)
	}
	errs = append(errs, validateBackupTarget(rc.BackupTarget, rc.S3)...) // server-ops (§16.11)
	if err := errors.Join(errs...); err != nil {
		return RestoreConfig{}, err
	}
	return rc, nil
}

func restoreFlagSet(cfg *Config, force, list *bool, out io.Writer) *flag.FlagSet {
	fs := flag.NewFlagSet("worktree-server restore", flag.ContinueOnError)
	fs.SetOutput(out)
	fs.StringVar(&cfg.DataDir, "data", cfg.DataDir, "data `directory` to restore into; must be empty (a backups/ directory is ignored) unless --force (env WT_DATA)")
	fs.BoolVar(force, "force", false, "restore into a non-empty data directory (replaces meta.db, adds objects)")
	fs.BoolVar(&cfg.CompressObjects, "compress-objects", cfg.CompressObjects, "store restored objects zstd-compressed when that saves at least 10% (env WT_COMPRESS_OBJECTS)")
	objectStoreFlags(fs, cfg)
	// server-ops (§16.11): restore by id from the backup target, or list it.
	backupTargetFlags(fs, cfg)
	fs.BoolVar(list, "list", false, "list the backups in the backup target (<data>/backups or the S3 bucket) and exit")
	return fs
}

// RestoreUsage writes the restore subcommand documentation to w.
func RestoreUsage(w io.Writer) {
	cfg, _ := fromEnv(func(string) (string, bool) { return "", false })
	fs := restoreFlagSet(&cfg, new(bool), new(bool), w)
	fmt.Fprintf(w, "Usage: worktree-server restore [flags] FILE.wtbak|BACKUP-ID\n"+
		"       worktree-server restore --list [--data DIR] [--backup-target disk|s3 ...]\n\n"+
		"Restores a backup (meta.db and every object, each verified against its hash)\n"+
		"into a stopped server's data directory and object store. The data directory\n"+
		"must be empty (a backups/ directory is ignored) unless --force is given. A\n"+
		"BACKUP-ID (bk-...) is read from the backup target: <data>/backups/<id>.wtbak,\n"+
		"or the S3 bucket with --backup-target s3.\n\nFlags:\n")
	fs.PrintDefaults()
}
