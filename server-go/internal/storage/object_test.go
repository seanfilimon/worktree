package storage

import (
	"context"
	"encoding/hex"
	"testing"

	"github.com/zeebo/blake3"
)

func TestLocalObjectStorePutVerifiesAndStoresObject(t *testing.T) {
	store := NewLocalObjectStore(t.TempDir())
	data := []byte("hello staged world")
	hash := blake3.Sum256(data)
	hashHex := hex.EncodeToString(hash[:])

	if err := store.Put(context.Background(), hashHex, data); err != nil {
		t.Fatalf("Put() error = %v", err)
	}
	exists, err := store.Exists(context.Background(), hashHex)
	if err != nil {
		t.Fatalf("Exists() error = %v", err)
	}
	if !exists {
		t.Fatal("expected object to exist")
	}
}

func TestLocalObjectStoreRejectsHashMismatch(t *testing.T) {
	store := NewLocalObjectStore(t.TempDir())
	hash := blake3.Sum256([]byte("expected"))
	hashHex := hex.EncodeToString(hash[:])

	if err := store.Put(context.Background(), hashHex, []byte("actual")); err == nil {
		t.Fatal("expected hash mismatch error")
	}
}
