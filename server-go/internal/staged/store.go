package staged

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sort"
	"sync"
	"time"
)

var ErrConflict = errors.New("staged snapshot conflicts with existing idempotency record")

type AddStatus string

const (
	AddStatusCreated       AddStatus = "created"
	AddStatusAlreadyExists AddStatus = "already_exists"
)

type AddResult struct {
	Status   AddStatus
	Snapshot Snapshot
}

func (r AddResult) IdempotentReplay() bool {
	return r.Status == AddStatusAlreadyExists
}

type ObjectRef struct {
	Path string `json:"path"`
	Hash string `json:"hash"`
	Size int    `json:"size"`
}

type Snapshot struct {
	SnapshotID  string      `json:"snapshot_id"`
	Tenant      string      `json:"tenant"`
	Worktree    string      `json:"worktree"`
	TreeID      string      `json:"tree_id"`
	Branch      string      `json:"branch"`
	ObjectIDs   []string    `json:"object_ids,omitempty"`
	Objects     []ObjectRef `json:"objects,omitempty"`
	PayloadHash string      `json:"payload_hash,omitempty"`
	CreatedAt   time.Time   `json:"created_at"`
}

type Store interface {
	Add(ctx context.Context, snapshot Snapshot) (AddResult, error)
	List(ctx context.Context, filter ListFilter) ([]Snapshot, error)
}

type ListFilter struct {
	Tenant   string
	Worktree string
	Branch   string
}

type FileStore struct {
	root string
	mu   sync.Mutex
}

func NewFileStore(root string) *FileStore {
	return &FileStore{root: root}
}

func (s *FileStore) Add(ctx context.Context, snapshot Snapshot) (AddResult, error) {
	if err := ctx.Err(); err != nil {
		return AddResult{}, err
	}
	snapshot = NormalizeSnapshot(snapshot)

	s.mu.Lock()
	defer s.mu.Unlock()

	path := s.indexPath()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return AddResult{}, err
	}
	index, err := s.load(path)
	if err != nil {
		return AddResult{}, err
	}
	for i := range index.Snapshots {
		existing := NormalizeSnapshot(index.Snapshots[i])
		index.Snapshots[i] = existing
		if !sameIdentity(existing, snapshot) {
			continue
		}
		if existing.PayloadHash == snapshot.PayloadHash {
			return AddResult{Status: AddStatusAlreadyExists, Snapshot: existing}, nil
		}
		return AddResult{}, ErrConflict
	}

	index.Snapshots = append(index.Snapshots, snapshot)
	data, err := json.MarshalIndent(index, "", "  ")
	if err != nil {
		return AddResult{}, err
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return AddResult{}, err
	}
	if err := os.Rename(tmp, path); err != nil {
		return AddResult{}, err
	}
	return AddResult{Status: AddStatusCreated, Snapshot: snapshot}, nil
}

func (s *FileStore) List(ctx context.Context, filter ListFilter) ([]Snapshot, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	index, err := s.load(s.indexPath())
	if err != nil {
		return nil, err
	}
	snapshots := make([]Snapshot, 0, len(index.Snapshots))
	for _, snapshot := range index.Snapshots {
		snapshot = NormalizeSnapshot(snapshot)
		if filter.Tenant != "" && snapshot.Tenant != filter.Tenant {
			continue
		}
		if filter.Worktree != "" && snapshot.Worktree != filter.Worktree {
			continue
		}
		if filter.Branch != "" && snapshot.Branch != filter.Branch {
			continue
		}
		snapshots = append(snapshots, snapshot)
	}
	return snapshots, nil
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

func (s *FileStore) indexPath() string {
	return filepath.Join(s.root, "staged", "index.json")
}

func NormalizeSnapshot(snapshot Snapshot) Snapshot {
	if snapshot.CreatedAt.IsZero() {
		snapshot.CreatedAt = time.Now().UTC()
	}
	if len(snapshot.ObjectIDs) == 0 && len(snapshot.Objects) > 0 {
		snapshot.ObjectIDs = make([]string, 0, len(snapshot.Objects))
		for _, obj := range snapshot.Objects {
			snapshot.ObjectIDs = append(snapshot.ObjectIDs, obj.Hash)
		}
	}
	if len(snapshot.Objects) == 0 && len(snapshot.ObjectIDs) > 0 {
		snapshot.Objects = make([]ObjectRef, 0, len(snapshot.ObjectIDs))
		for _, id := range snapshot.ObjectIDs {
			snapshot.Objects = append(snapshot.Objects, ObjectRef{Hash: id})
		}
	}
	if snapshot.PayloadHash == "" {
		snapshot.PayloadHash = CanonicalPayloadHash(snapshot)
	}
	return snapshot
}

func CanonicalPayloadHash(snapshot Snapshot) string {
	type payload struct {
		SnapshotID string      `json:"snapshot_id"`
		Tenant     string      `json:"tenant"`
		Worktree   string      `json:"worktree"`
		TreeID     string      `json:"tree_id"`
		Branch     string      `json:"branch"`
		Objects    []ObjectRef `json:"objects"`
	}
	objects := append([]ObjectRef(nil), snapshot.Objects...)
	sort.Slice(objects, func(i, j int) bool {
		if objects[i].Path != objects[j].Path {
			return objects[i].Path < objects[j].Path
		}
		if objects[i].Hash != objects[j].Hash {
			return objects[i].Hash < objects[j].Hash
		}
		return objects[i].Size < objects[j].Size
	})
	data, _ := json.Marshal(payload{
		SnapshotID: snapshot.SnapshotID,
		Tenant:     snapshot.Tenant,
		Worktree:   snapshot.Worktree,
		TreeID:     snapshot.TreeID,
		Branch:     snapshot.Branch,
		Objects:    objects,
	})
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func sameIdentity(a, b Snapshot) bool {
	return a.SnapshotID == b.SnapshotID &&
		a.Tenant == b.Tenant &&
		a.Worktree == b.Worktree &&
		a.TreeID == b.TreeID &&
		a.Branch == b.Branch
}
