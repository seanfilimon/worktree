package config

import (
	"crypto/tls"
	"log/slog"
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	HTTPAddr             string
	GRPCAddr             string
	Environment          string
	LogLevelName         string
	StorageRoot          string
	AuditPath            string
	AuthMode             string
	IAMMode              string
	AuthToken            string
	AuthCredentialsPath  string
	IAMPolicyPath        string
	AuthTenant           string
	AuthAccount          string
	AuthScopes           []string
	MaxStagedObjectBytes int
	MaxStagedObjects     int
	ShutdownTimeout      time.Duration
	DatabaseURL          string
	RunMigrations        bool
	TLS                  TLSConfig
}

type TLSConfig struct {
	Enabled  bool
	CertFile string
	KeyFile  string
}

func Load() (Config, error) {
	cfg := Config{
		HTTPAddr:             getEnv("WT_SERVER_HTTP_ADDR", "127.0.0.1:8080"),
		GRPCAddr:             getEnv("WT_SERVER_GRPC_ADDR", "127.0.0.1:9877"),
		Environment:          getEnv("WT_SERVER_ENV", "development"),
		LogLevelName:         getEnv("WT_SERVER_LOG_LEVEL", "info"),
		StorageRoot:          getEnv("WT_SERVER_STORAGE_ROOT", ".wt-server-go"),
		AuditPath:            getEnv("WT_SERVER_AUDIT_PATH", ".wt-server-go/audit/audit.jsonl"),
		AuthMode:             getEnv("WT_SERVER_AUTH_MODE", "bearer"),
		IAMMode:              getEnv("WT_SERVER_IAM_MODE", "policy"),
		AuthToken:            os.Getenv("WT_SERVER_AUTH_TOKEN"),
		AuthCredentialsPath:  os.Getenv("WT_SERVER_AUTH_CREDENTIALS_PATH"),
		IAMPolicyPath:        os.Getenv("WT_SERVER_IAM_POLICY_PATH"),
		AuthTenant:           getEnv("WT_SERVER_AUTH_TENANT", "default"),
		AuthAccount:          getEnv("WT_SERVER_AUTH_ACCOUNT", "server-token"),
		AuthScopes:           getCSVEnv("WT_SERVER_AUTH_SCOPES", []string{"staged:*"}),
		MaxStagedObjectBytes: getIntEnv("WT_SERVER_MAX_STAGED_OBJECT_BYTES", 64*1024*1024),
		MaxStagedObjects:     getIntEnv("WT_SERVER_MAX_STAGED_OBJECTS", 1024),
		ShutdownTimeout:      getDurationEnv("WT_SERVER_SHUTDOWN_TIMEOUT", 30*time.Second),
		DatabaseURL:          os.Getenv("WT_SERVER_DATABASE_URL"),
		RunMigrations:        getBoolEnv("WT_SERVER_RUN_MIGRATIONS", false),
		TLS: TLSConfig{
			Enabled:  getBoolEnv("WT_SERVER_TLS_ENABLED", false),
			CertFile: os.Getenv("WT_SERVER_TLS_CERT_FILE"),
			KeyFile:  os.Getenv("WT_SERVER_TLS_KEY_FILE"),
		},
	}

	if cfg.TLS.Enabled {
		if cfg.TLS.CertFile == "" || cfg.TLS.KeyFile == "" {
			return Config{}, ErrMissingTLSFiles
		}
	}
	if strings.EqualFold(cfg.Environment, "production") {
		if cfg.AuthMode != "bearer" || (cfg.AuthToken == "" && cfg.AuthCredentialsPath == "") {
			return Config{}, ErrProductionAuthRequired
		}
		if cfg.IAMMode == "allow-all-dev" {
			return Config{}, ErrProductionIAMRequired
		}
	}

	return cfg, nil
}

func (c Config) LogLevel() slog.Level {
	switch strings.ToLower(c.LogLevelName) {
	case "debug":
		return slog.LevelDebug
	case "warn", "warning":
		return slog.LevelWarn
	case "error":
		return slog.LevelError
	default:
		return slog.LevelInfo
	}
}

func (c Config) TLSConfig() *tls.Config {
	if !c.TLS.Enabled {
		return nil
	}
	return &tls.Config{
		MinVersion: tls.VersionTLS13,
	}
}

func getEnv(key, fallback string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return fallback
}

func getBoolEnv(key string, fallback bool) bool {
	value := os.Getenv(key)
	if value == "" {
		return fallback
	}
	parsed, err := strconv.ParseBool(value)
	if err != nil {
		return fallback
	}
	return parsed
}

func getDurationEnv(key string, fallback time.Duration) time.Duration {
	value := os.Getenv(key)
	if value == "" {
		return fallback
	}
	parsed, err := time.ParseDuration(value)
	if err != nil {
		return fallback
	}
	return parsed
}

func getCSVEnv(key string, fallback []string) []string {
	value := os.Getenv(key)
	if value == "" {
		return fallback
	}
	parts := strings.Split(value, ",")
	out := make([]string, 0, len(parts))
	for _, part := range parts {
		part = strings.TrimSpace(part)
		if part != "" {
			out = append(out, part)
		}
	}
	if len(out) == 0 {
		return fallback
	}
	return out
}

func getIntEnv(key string, fallback int) int {
	value := os.Getenv(key)
	if value == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil || parsed < 0 {
		return fallback
	}
	return parsed
}
