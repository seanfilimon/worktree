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
	HTTPAddr        string
	Environment     string
	LogLevelName    string
	ShutdownTimeout time.Duration
	TLS             TLSConfig
}

type TLSConfig struct {
	Enabled  bool
	CertFile string
	KeyFile  string
}

func Load() (Config, error) {
	cfg := Config{
		HTTPAddr:        getEnv("WT_SERVER_HTTP_ADDR", "127.0.0.1:8080"),
		Environment:     getEnv("WT_SERVER_ENV", "development"),
		LogLevelName:    getEnv("WT_SERVER_LOG_LEVEL", "info"),
		ShutdownTimeout: getDurationEnv("WT_SERVER_SHUTDOWN_TIMEOUT", 30*time.Second),
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
