package meta

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	bolt "go.etcd.io/bbolt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Generic JSON document collections (domain.DocStore) used by the v0.5
// features (docs/ENGINEERING_PLAN.md §13). Bolt keeps each collection in a
// nested bucket of bktDocs; counters live in bktDocSeq.

var (
	bktDocs   = []byte("docs")    // collection -> (key -> JSON)
	bktDocSeq = []byte("doc_seq") // counter name -> be64
)

func validColl(coll string) error {
	if coll == "" || strings.ContainsRune(coll, 0) {
		return fmt.Errorf("%w: invalid collection name %q", domain.ErrInvalid, coll)
	}
	return nil
}

func encodeDoc(coll, key string, v any) ([]byte, error) {
	if key == "" {
		return nil, fmt.Errorf("%w: empty %s key", domain.ErrInvalid, coll)
	}
	raw, err := json.Marshal(v)
	if err != nil {
		return nil, fmt.Errorf("meta: encode %s/%q: %w", coll, key, err)
	}
	return raw, nil
}

func decodeDoc(coll, key string, raw []byte, v any) error {
	if err := json.Unmarshal(raw, v); err != nil {
		return fmt.Errorf("meta: decode %s/%q: %w", coll, key, err)
	}
	return nil
}

// ---- Bolt ---------------------------------------------------------------

func docBucket(tx *bolt.Tx, coll string, create bool) (*bolt.Bucket, error) {
	root := tx.Bucket(bktDocs) // created by NewBolt
	if root == nil {
		return nil, fmt.Errorf("meta: bucket %s missing", bktDocs)
	}
	if create {
		return root.CreateBucketIfNotExists([]byte(coll))
	}
	return root.Bucket([]byte(coll)), nil
}

// PutDoc implements domain.DocStore.
func (b *Bolt) PutDoc(ctx context.Context, coll, key string, v any) error {
	if err := validColl(coll); err != nil {
		return err
	}
	raw, err := encodeDoc(coll, key, v)
	if err != nil {
		return err
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk, err := docBucket(tx, coll, true)
		if err != nil {
			return err
		}
		return bk.Put([]byte(key), raw)
	})
}

// CreateDoc implements domain.DocStore.
func (b *Bolt) CreateDoc(ctx context.Context, coll, key string, v any) error {
	if err := validColl(coll); err != nil {
		return err
	}
	raw, err := encodeDoc(coll, key, v)
	if err != nil {
		return err
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk, err := docBucket(tx, coll, true)
		if err != nil {
			return err
		}
		if bk.Get([]byte(key)) != nil {
			return fmt.Errorf("%s %q: %w", coll, key, domain.ErrAlreadyExists)
		}
		return bk.Put([]byte(key), raw)
	})
}

// GetDoc implements domain.DocStore.
func (b *Bolt) GetDoc(ctx context.Context, coll, key string, v any) error {
	if err := validColl(coll); err != nil {
		return err
	}
	return b.view(ctx, func(tx *bolt.Tx) error {
		bk, err := docBucket(tx, coll, false)
		if err != nil {
			return err
		}
		var raw []byte
		if bk != nil {
			raw = bk.Get([]byte(key))
		}
		if raw == nil {
			return fmt.Errorf("%s %q: %w", coll, key, domain.ErrNotFound)
		}
		return decodeDoc(coll, key, raw, v)
	})
}

// ListDocs implements domain.DocStore.
func (b *Bolt) ListDocs(ctx context.Context, coll, prefix string) ([][]byte, error) {
	if err := validColl(coll); err != nil {
		return nil, err
	}
	out := [][]byte{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		bk, err := docBucket(tx, coll, false)
		if err != nil || bk == nil {
			return err
		}
		c := bk.Cursor()
		p := []byte(prefix)
		for k, v := c.Seek(p); k != nil && bytes.HasPrefix(k, p); k, v = c.Next() {
			out = append(out, slices.Clone(v))
		}
		return nil
	})
	return out, err
}

// DeleteDoc implements domain.DocStore.
func (b *Bolt) DeleteDoc(ctx context.Context, coll, key string) error {
	if err := validColl(coll); err != nil {
		return err
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk, err := docBucket(tx, coll, true)
		if err != nil {
			return err
		}
		if bk.Get([]byte(key)) == nil {
			return fmt.Errorf("%s %q: %w", coll, key, domain.ErrNotFound)
		}
		return bk.Delete([]byte(key))
	})
}

// DeleteDocs implements domain.DocStore.
func (b *Bolt) DeleteDocs(ctx context.Context, coll, prefix string) (int, error) {
	if err := validColl(coll); err != nil {
		return 0, err
	}
	n := 0
	err := b.update(ctx, func(tx *bolt.Tx) error {
		bk, err := docBucket(tx, coll, true)
		if err != nil {
			return err
		}
		var keys [][]byte
		c := bk.Cursor()
		p := []byte(prefix)
		for k, _ := c.Seek(p); k != nil && bytes.HasPrefix(k, p); k, _ = c.Next() {
			keys = append(keys, slices.Clone(k))
		}
		for _, k := range keys {
			if err := bk.Delete(k); err != nil {
				return err
			}
		}
		n = len(keys)
		return nil
	})
	return n, err
}

// UpdateDoc implements domain.DocStore.
func (b *Bolt) UpdateDoc(ctx context.Context, coll, key string, fn func(raw []byte) ([]byte, error)) error {
	if err := validColl(coll); err != nil {
		return err
	}
	if key == "" {
		return fmt.Errorf("%w: empty %s key", domain.ErrInvalid, coll)
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk, err := docBucket(tx, coll, true)
		if err != nil {
			return err
		}
		old := bk.Get([]byte(key))
		next, err := fn(slices.Clone(old))
		if err != nil {
			return err
		}
		if next == nil {
			if old == nil {
				return nil
			}
			return bk.Delete([]byte(key))
		}
		return bk.Put([]byte(key), next)
	})
}

// NextSeq implements domain.DocStore.
func (b *Bolt) NextSeq(ctx context.Context, name string) (int64, error) {
	var n uint64
	err := b.update(ctx, func(tx *bolt.Tx) error {
		bk, err := tx.CreateBucketIfNotExists(bktDocSeq)
		if err != nil {
			return err
		}
		if v := bk.Get([]byte(name)); len(v) == 8 {
			n = binary.BigEndian.Uint64(v)
		}
		n++
		var buf [8]byte
		binary.BigEndian.PutUint64(buf[:], n)
		return bk.Put([]byte(name), buf[:])
	})
	return int64(n), err
}

// ---- Memory -------------------------------------------------------------

// memDocs holds Memory's document collections. It is guarded by Memory.mu.
type memDocs struct {
	colls map[string]map[string][]byte
	seq   map[string]int64
}

func newMemDocs() memDocs {
	return memDocs{colls: map[string]map[string][]byte{}, seq: map[string]int64{}}
}

// PutDoc implements domain.DocStore.
func (m *Memory) PutDoc(ctx context.Context, coll, key string, v any) error {
	if err := validColl(coll); err != nil {
		return err
	}
	raw, err := encodeDoc(coll, key, v)
	if err != nil {
		return err
	}
	return m.write(ctx, func() error {
		inner(m.docs.colls, coll)[key] = raw
		return nil
	})
}

// CreateDoc implements domain.DocStore.
func (m *Memory) CreateDoc(ctx context.Context, coll, key string, v any) error {
	if err := validColl(coll); err != nil {
		return err
	}
	raw, err := encodeDoc(coll, key, v)
	if err != nil {
		return err
	}
	return m.write(ctx, func() error {
		c := inner(m.docs.colls, coll)
		if _, ok := c[key]; ok {
			return fmt.Errorf("%s %q: %w", coll, key, domain.ErrAlreadyExists)
		}
		c[key] = raw
		return nil
	})
}

// GetDoc implements domain.DocStore.
func (m *Memory) GetDoc(ctx context.Context, coll, key string, v any) error {
	if err := validColl(coll); err != nil {
		return err
	}
	return m.read(ctx, func() error {
		raw, ok := m.docs.colls[coll][key]
		if !ok {
			return fmt.Errorf("%s %q: %w", coll, key, domain.ErrNotFound)
		}
		return decodeDoc(coll, key, raw, v)
	})
}

// ListDocs implements domain.DocStore.
func (m *Memory) ListDocs(ctx context.Context, coll, prefix string) ([][]byte, error) {
	if err := validColl(coll); err != nil {
		return nil, err
	}
	out := [][]byte{}
	err := m.read(ctx, func() error {
		c := m.docs.colls[coll]
		keys := make([]string, 0, len(c))
		for k := range c {
			if strings.HasPrefix(k, prefix) {
				keys = append(keys, k)
			}
		}
		slices.Sort(keys)
		for _, k := range keys {
			out = append(out, slices.Clone(c[k]))
		}
		return nil
	})
	return out, err
}

// DeleteDoc implements domain.DocStore.
func (m *Memory) DeleteDoc(ctx context.Context, coll, key string) error {
	if err := validColl(coll); err != nil {
		return err
	}
	return m.write(ctx, func() error {
		c := m.docs.colls[coll]
		if _, ok := c[key]; !ok {
			return fmt.Errorf("%s %q: %w", coll, key, domain.ErrNotFound)
		}
		delete(c, key)
		return nil
	})
}

// DeleteDocs implements domain.DocStore.
func (m *Memory) DeleteDocs(ctx context.Context, coll, prefix string) (int, error) {
	if err := validColl(coll); err != nil {
		return 0, err
	}
	n := 0
	err := m.write(ctx, func() error {
		c := m.docs.colls[coll]
		for k := range c {
			if strings.HasPrefix(k, prefix) {
				delete(c, k)
				n++
			}
		}
		return nil
	})
	return n, err
}

// UpdateDoc implements domain.DocStore.
func (m *Memory) UpdateDoc(ctx context.Context, coll, key string, fn func(raw []byte) ([]byte, error)) error {
	if err := validColl(coll); err != nil {
		return err
	}
	if key == "" {
		return fmt.Errorf("%w: empty %s key", domain.ErrInvalid, coll)
	}
	return m.write(ctx, func() error {
		c := inner(m.docs.colls, coll)
		old, ok := c[key]
		var in []byte
		if ok {
			in = slices.Clone(old)
		}
		next, err := fn(in)
		if err != nil {
			return err
		}
		if next == nil {
			delete(c, key)
			return nil
		}
		c[key] = slices.Clone(next)
		return nil
	})
}

// NextSeq implements domain.DocStore.
func (m *Memory) NextSeq(ctx context.Context, name string) (int64, error) {
	var n int64
	err := m.write(ctx, func() error {
		m.docs.seq[name]++
		n = m.docs.seq[name]
		return nil
	})
	return n, err
}

// ---- typed helpers ------------------------------------------------------

// GetDocT decodes one document of type T.
func GetDocT[T any](ctx context.Context, s domain.DocStore, coll, key string) (*T, error) {
	var v T
	if err := s.GetDoc(ctx, coll, key, &v); err != nil {
		return nil, err
	}
	return &v, nil
}

// ListDocsT decodes every document under prefix as T (key order).
func ListDocsT[T any](ctx context.Context, s domain.DocStore, coll, prefix string) ([]T, error) {
	raws, err := s.ListDocs(ctx, coll, prefix)
	if err != nil {
		return nil, err
	}
	out := make([]T, 0, len(raws))
	for _, raw := range raws {
		var v T
		if err := json.Unmarshal(raw, &v); err != nil {
			return nil, fmt.Errorf("meta: decode %s: %w", coll, err)
		}
		out = append(out, v)
	}
	return out, nil
}

// UpdateDocT atomically transforms a document of type T. fn receives nil
// when the document does not exist; returning nil deletes it.
func UpdateDocT[T any](ctx context.Context, s domain.DocStore, coll, key string, fn func(cur *T) (*T, error)) error {
	return s.UpdateDoc(ctx, coll, key, func(raw []byte) ([]byte, error) {
		var cur *T
		if raw != nil {
			cur = new(T)
			if err := json.Unmarshal(raw, cur); err != nil {
				return nil, fmt.Errorf("meta: decode %s/%q: %w", coll, key, err)
			}
		}
		next, err := fn(cur)
		if err != nil || next == nil {
			return nil, err
		}
		return json.Marshal(next)
	})
}

// DocPrefix returns the key prefix covering every DocKey that starts with
// parts (each part followed by the separator).
func DocPrefix(parts ...string) string {
	return domain.DocKey(parts...) + "\x00"
}
