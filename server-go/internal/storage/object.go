package storage

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/zeebo/blake3"
)

var (
	ErrInvalidHash   = errors.New("invalid BLAKE3 hash")
	ErrObjectMissing = errors.New("object not found")
)

type ObjectStore interface {
	Put(ctx context.Context, hash string, data []byte) error
	Exists(ctx context.Context, hash string) (bool, error)
	Get(ctx context.Context, hash string) ([]byte, error)
}

type LocalObjectStore struct {
	root string
}

func NewLocalObjectStore(root string) *LocalObjectStore {
	return &LocalObjectStore{root: root}
}

func (s *LocalObjectStore) Put(ctx context.Context, hash string, data []byte) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	hash = strings.ToLower(hash)
	if !IsValidHash(hash) {
		return ErrInvalidHash
	}
	actual := blake3.Sum256(data)
	if hex.EncodeToString(actual[:]) != hash {
		return fmt.Errorf("%w: content hash mismatch", ErrInvalidHash)
	}
	path := s.objectPath(hash)
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	if _, err := os.Stat(path); err == nil {
		return nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}

	f, err := os.CreateTemp(dir, "obj-*.tmp")
	if err != nil {
		return err
	}
	tmp := f.Name()
	defer os.Remove(tmp)

	if _, err := f.Write(data); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}

	if err := os.Rename(tmp, path); err != nil {
		if _, statErr := os.Stat(path); statErr == nil {
			return nil
		}
		return err
	}
	return nil
}

func (s *LocalObjectStore) Get(ctx context.Context, hash string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	hash = strings.ToLower(hash)
	if !IsValidHash(hash) {
		return nil, ErrInvalidHash
	}
	data, err := os.ReadFile(s.objectPath(hash))
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, ErrObjectMissing
		}
		return nil, err
	}
	actual := blake3.Sum256(data)
	if hex.EncodeToString(actual[:]) != hash {
		return nil, fmt.Errorf("%w: stored content hash mismatch", ErrInvalidHash)
	}
	return data, nil
}

func (s *LocalObjectStore) Exists(ctx context.Context, hash string) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	if !IsValidHash(hash) {
		return false, ErrInvalidHash
	}
	_, err := os.Stat(s.objectPath(strings.ToLower(hash)))
	if err == nil {
		return true, nil
	}
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	return false, err
}

func (s *LocalObjectStore) objectPath(hash string) string {
	return filepath.Join(s.root, "objects", hash[:2], hash[2:])
}

func IsValidHash(hash string) bool {
	if len(hash) != 64 {
		return false
	}
	_, err := hex.DecodeString(hash)
	return err == nil
}
