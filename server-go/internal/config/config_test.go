package config

import "testing"

func TestLoadReadsStagedUploadLimits(t *testing.T) {
	t.Setenv("WT_SERVER_MAX_STAGED_OBJECT_BYTES", "123")
	t.Setenv("WT_SERVER_MAX_STAGED_OBJECTS", "7")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.MaxStagedObjectBytes != 123 {
		t.Fatalf("MaxStagedObjectBytes = %d", cfg.MaxStagedObjectBytes)
	}
	if cfg.MaxStagedObjects != 7 {
		t.Fatalf("MaxStagedObjects = %d", cfg.MaxStagedObjects)
	}
}

func TestLoadFallsBackForInvalidStagedUploadLimits(t *testing.T) {
	t.Setenv("WT_SERVER_MAX_STAGED_OBJECT_BYTES", "-1")
	t.Setenv("WT_SERVER_MAX_STAGED_OBJECTS", "invalid")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.MaxStagedObjectBytes != 64*1024*1024 {
		t.Fatalf("MaxStagedObjectBytes = %d", cfg.MaxStagedObjectBytes)
	}
	if cfg.MaxStagedObjects != 1024 {
		t.Fatalf("MaxStagedObjects = %d", cfg.MaxStagedObjects)
	}
}
