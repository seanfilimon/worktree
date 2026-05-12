package staged

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"time"
)

type Snapshot struct {
	SnapshotID string    `json:"snapshot_id"`
	Tenant     string    `json:"tenant"`
	Worktree   string    `json:"worktree"`
	TreeID     string    `json:"tree_id"`
	Branch     string    `json:"branch"`
	ObjectIDs  []string  `json:"object_ids"`
	CreatedAt  time.Time `json:"created_at"`
}

type Store interface {
	Add(ctx context.Context, snapshot Snapshot) error
}

type FileStore struct {
	root string
}

func NewFileStore(root string) *FileStore {
	return &FileStore{root: root}
}

func (s *FileStore) Add(ctx context.Context, snapshot Snapshot) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if snapshot.CreatedAt.IsZero() {
		snapshot.CreatedAt = time.Now().UTC()
	}
	path := filepath.Join(s.root, "staged", "index.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	index, err := s.load(path)
	if err != nil {
		return err
	}
	index.Snapshots = append(index.Snapshots, snapshot)
	data, err := json.MarshalIndent(index, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, data, 0o644)
}

type indexFile struct {
	Snapshots []Snapshot `json:"snapshots"`
}

func (s *FileStore) load(path string) (indexFile, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return indexFile{}, nil
		}
		return indexFile{}, err
	}
	var index indexFile
	if err := json.Unmarshal(data, &index); err != nil {
		return indexFile{}, err
	}
	return index, nil
}
