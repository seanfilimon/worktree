package syncsvc

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"errors"
	"math/rand"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// vectorsPath is the golden-vector file shared with the Rust pack module.
var vectorsPath = filepath.Join("..", "..", "..", "..", "crates", "worktree-protocol", "testdata", "pack", "vectors.json")

type packVectorObject struct {
	Kind uint8  `json:"kind"`
	Data string `json:"data"`
	Base *int   `json:"base"`
}

func mustHex(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatalf("hex: %v", err)
	}
	return b
}

// buildTestPack writes objects (kind, data, base data or nil) to a pack.
func buildTestPack(t *testing.T, compress bool, objects []testPackObject) ([]byte, packStats) {
	t.Helper()
	var buf bytes.Buffer
	pw := newPackWriter(&buf, compress)
	for _, o := range objects {
		var bh *hash.Hash
		if o.base != nil {
			h := hash.Sum(o.base)
			bh = &h
		}
		if _, err := pw.add(hash.Sum(o.data), o.kind, o.data, bh, o.base); err != nil {
			t.Fatal(err)
		}
	}
	st, err := pw.finish()
	if err != nil {
		t.Fatal(err)
	}
	return buf.Bytes(), st
}

type testPackObject struct {
	kind uint8
	data []byte
	base []byte
}

// readTestPack decodes a pack, resolving bases from haves and earlier
// objects.
func readTestPack(pack []byte, haves [][]byte) ([]*packObject, error) {
	store := map[hash.Hash][]byte{}
	for _, h := range haves {
		store[hash.Sum(h)] = h
	}
	pr, err := newPackReader(bytes.NewReader(pack))
	if err != nil {
		return nil, err
	}
	var out []*packObject
	for {
		obj, err := pr.next(func(h hash.Hash) ([]byte, bool) { d, ok := store[h]; return d, ok })
		if err != nil {
			return nil, err
		}
		if obj == nil {
			return out, nil
		}
		if obj.Err == nil {
			store[obj.Hash] = obj.Data
		}
		out = append(out, obj)
	}
}

func editBytes(base []byte) []byte {
	t := append([]byte(nil), base...)
	copy(t[len(base)/2:], "0123456789")
	at := len(base) / 4
	t = append(t[:at], append([]byte("inserted!"), t[at:]...)...)
	cut := len(base) * 3 / 4
	return append(t[:cut], t[cut+40:]...)
}

// TestPackGolden checks the Go implementation against the vectors the
// Rust implementation generated: identical delta and raw-pack bytes, and
// every stored pack (raw, Rust zstd, Go zstd) decodes. With
// WT_UPDATE_GOLDEN=1 it (re)writes zstd_pack_go.
func TestPackGolden(t *testing.T) {
	raw, err := os.ReadFile(vectorsPath)
	if err != nil {
		t.Fatalf("read vectors: %v", err)
	}
	var doc map[string]any
	if err := json.Unmarshal(raw, &doc); err != nil {
		t.Fatal(err)
	}
	var vec struct {
		Deltas []struct{ Name, Base, Target, Delta string } `json:"deltas"`
		Packs  []struct {
			Name         string             `json:"name"`
			Haves        []string           `json:"haves"`
			Objects      []packVectorObject `json:"objects"`
			RawPack      string             `json:"raw_pack"`
			ZstdPackRust *string            `json:"zstd_pack_rust"`
			ZstdPackGo   *string            `json:"zstd_pack_go"`
		} `json:"packs"`
	}
	if err := json.Unmarshal(raw, &vec); err != nil {
		t.Fatal(err)
	}
	if len(vec.Deltas) == 0 || len(vec.Packs) == 0 {
		t.Fatal("no vectors")
	}
	for _, d := range vec.Deltas {
		base, target := mustHex(t, d.Base), mustHex(t, d.Target)
		if got := hex.EncodeToString(encodeDelta(base, target)); got != d.Delta {
			t.Errorf("delta %s: got %s, want %s", d.Name, got, d.Delta)
		}
		out, err := applyDelta(base, mustHex(t, d.Delta), PackMaxObject)
		if err != nil || !bytes.Equal(out, target) {
			t.Errorf("delta %s: apply = %v", d.Name, err)
		}
	}
	update := os.Getenv("WT_UPDATE_GOLDEN") == "1"
	docPacks, _ := doc["packs"].([]any)
	for i, p := range vec.Packs {
		haves := make([][]byte, len(p.Haves))
		for j, h := range p.Haves {
			haves[j] = mustHex(t, h)
		}
		var objects []testPackObject
		var want [][]byte
		for _, o := range p.Objects {
			data := mustHex(t, o.Data)
			var base []byte
			if o.Base != nil {
				if *o.Base < len(haves) {
					base = haves[*o.Base]
				} else {
					base = mustHex(t, p.Objects[*o.Base-len(haves)].Data)
				}
			}
			objects = append(objects, testPackObject{kind: o.Kind, data: data, base: base})
			dup := false
			for _, w := range want {
				dup = dup || bytes.Equal(w, data)
			}
			if !dup {
				want = append(want, data)
			}
		}
		rawPack, _ := buildTestPack(t, false, objects)
		if hex.EncodeToString(rawPack) != p.RawPack {
			t.Errorf("pack %s: raw pack bytes differ from the Rust writer", p.Name)
		}
		goPack, _ := buildTestPack(t, true, objects)
		packs := map[string]string{"raw_pack": p.RawPack, "go (fresh)": hex.EncodeToString(goPack)}
		if p.ZstdPackRust != nil {
			packs["zstd_pack_rust"] = *p.ZstdPackRust
		} else {
			t.Errorf("pack %s: zstd_pack_rust missing", p.Name)
		}
		if p.ZstdPackGo != nil {
			packs["zstd_pack_go"] = *p.ZstdPackGo
		} else if !update {
			t.Errorf("pack %s: zstd_pack_go missing; run WT_UPDATE_GOLDEN=1 go test ./internal/syncsvc -run PackGolden", p.Name)
		}
		for field, h := range packs {
			got, err := readTestPack(mustHex(t, h), haves)
			if err != nil {
				t.Errorf("pack %s %s: %v", p.Name, field, err)
				continue
			}
			if len(got) != len(want) {
				t.Errorf("pack %s %s: %d objects, want %d", p.Name, field, len(got), len(want))
				continue
			}
			for k, o := range got {
				if o.Err != nil || !bytes.Equal(o.Data, want[k]) {
					t.Errorf("pack %s %s: object %d: %v", p.Name, field, k, o.Err)
				}
			}
		}
		if update {
			if m, ok := docPacks[i].(map[string]any); ok {
				m["zstd_pack_go"] = hex.EncodeToString(goPack)
			}
		}
	}
	if update {
		out, err := json.MarshalIndent(doc, "", "  ")
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(vectorsPath, append(out, '\n'), 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

func TestDeltaRoundTrips(t *testing.T) {
	rng := rand.New(rand.NewSource(1))
	base := make([]byte, 8192)
	rng.Read(base)
	other := make([]byte, 5000)
	rng.Read(other)
	cases := [][]byte{nil, []byte("tiny"), base, editBytes(base), append(append([]byte("head"), base...), "tail"...), base[4096:], other, bytes.Repeat([]byte{'a'}, 3000)}
	for i, target := range cases {
		d := encodeDelta(base, target)
		out, err := applyDelta(base, d, PackMaxObject)
		if err != nil || !bytes.Equal(out, target) {
			t.Fatalf("case %d: %v", i, err)
		}
	}
	if d := encodeDelta(base, editBytes(base)); len(d) > 100 || !deltaPaysOff(len(d), len(base)) {
		t.Fatalf("edit delta is %d bytes", len(d))
	}
	if deltaPaysOff(len(encodeDelta(base, other)), len(other)) {
		t.Fatal("unrelated content should not pay off")
	}
	// Malformed deltas.
	good := encodeDelta(base[:32], append(append([]byte(nil), base[:16]...), "!!"...))
	if _, err := applyDelta(base[:32], good, 1<<20); err != nil {
		t.Fatal(err)
	}
	if _, err := applyDelta([]byte("short"), good, 1<<20); err == nil {
		t.Fatal("wrong base accepted")
	}
	if _, err := applyDelta(base[:32], good, 4); err == nil {
		t.Fatal("too large accepted")
	}
	mk := func(ops []byte, target uint64) []byte {
		d := appendUvarint(nil, 32)
		d = appendUvarint(d, target)
		return append(d, ops...)
	}
	for _, c := range []struct {
		ops    []byte
		target uint64
	}{{[]byte{1, 30, 5}, 5}, {[]byte{1, 0, 0}, 0}, {[]byte{0, 0}, 0}, {[]byte{0, 5, 'a'}, 5}, {[]byte{7}, 1}, {[]byte{0, 1, 'a'}, 2}, {[]byte{1, 0, 4, 1, 0, 4}, 4}} {
		if _, err := applyDelta(base[:32], mk(c.ops, c.target), 1<<20); err == nil {
			t.Errorf("ops %v accepted", c.ops)
		}
	}
}

func TestPackRoundTripAndDepth(t *testing.T) {
	rng := rand.New(rand.NewSource(2))
	have := make([]byte, 6000)
	rng.Read(have)
	v1 := editBytes(have)
	v2 := editBytes(v1)
	objs := []testPackObject{{1, []byte("hello"), nil}, {1, v1, have}, {1, v2, v1}, {2, bytes.Repeat([]byte("line\n"), 400), nil}, {1, v1, have}}
	for _, compress := range []bool{true, false} {
		pack, st := buildTestPack(t, compress, objs)
		if st.Entries != 4 || st.Deltas != 2 || st.PackBytes != int64(len(pack)) {
			t.Fatalf("stats %+v", st)
		}
		got, err := readTestPack(pack, [][]byte{have})
		if err != nil || len(got) != 4 {
			t.Fatalf("read: %v (%d)", err, len(got))
		}
		if !bytes.Equal(got[2].Data, v2) || got[2].Base == nil || *got[2].Base != hash.Sum(v1) {
			t.Fatal("chained delta")
		}
	}
	// Depth: a chain of 12 edits restarts after depth 8.
	versions := [][]byte{have}
	for i := 0; i < 12; i++ {
		versions = append(versions, editBytes(versions[len(versions)-1]))
	}
	var chain []testPackObject
	for i := 1; i < len(versions); i++ {
		chain = append(chain, testPackObject{1, versions[i], versions[i-1]})
	}
	pack, _ := buildTestPack(t, true, chain)
	got, err := readTestPack(pack, versions[:1])
	if err != nil {
		t.Fatal(err)
	}
	var whole []int
	for i, o := range got {
		if o.Err != nil {
			t.Fatal(o.Err)
		}
		if o.Base == nil {
			whole = append(whole, i)
		}
	}
	if len(whole) != 1 || whole[0] != 8 {
		t.Fatalf("whole entries %v", whole)
	}
	// A hand-made chain of 9 deltas is refused.
	var b bytes.Buffer
	b.Write([]byte{'W', 'T', 'P', 'K', 1, 0, 0, 0})
	for i := 1; i <= 9; i++ {
		d := encodeDelta(versions[i-1], versions[i])
		h, bh := hash.Sum(versions[i]), hash.Sum(versions[i-1])
		b.Write([]byte{packTypeDelta, 1, packCodecRaw})
		b.Write(h[:])
		b.Write(bh[:])
		b.Write(appendUvarint(appendUvarint(appendUvarint(nil, uint64(len(versions[i]))), uint64(len(d))), uint64(len(d))))
		b.Write(d)
	}
	b.WriteByte(packTypeEnd)
	sum := hash.Sum(b.Bytes())
	b.Write(sum[:])
	if _, err := readTestPack(b.Bytes(), versions[:1]); err == nil || !strings.Contains(err.Error(), "deeper") {
		t.Fatalf("deep chain: %v", err)
	}
}

func TestPackReaderErrors(t *testing.T) {
	pack, _ := buildTestPack(t, true, []testPackObject{{1, []byte("some object content"), nil}})
	for _, cut := range []int{0, 4, 8, 9, 20, len(pack) - 33, len(pack) - 1} {
		if _, err := readTestPack(pack[:cut], nil); !errors.Is(err, errPackTruncated) {
			t.Errorf("cut %d: %v", cut, err)
		}
	}
	mut := func(f func([]byte) []byte) []byte { return f(append([]byte(nil), pack...)) }
	if _, err := readTestPack(mut(func(b []byte) []byte { b[len(b)-1] ^= 1; return b }), nil); !errors.Is(err, errPackChecksum) {
		t.Errorf("checksum: %v", err)
	}
	for name, b := range map[string][]byte{
		"magic":   mut(func(b []byte) []byte { b[0] = 'X'; return b }),
		"version": mut(func(b []byte) []byte { b[4] = 9; return b }),
		"type":    mut(func(b []byte) []byte { b[8] = 7; return b }),
		"extra":   mut(func(b []byte) []byte { return append(b, 0) }),
	} {
		if _, err := readTestPack(b, nil); !errors.Is(err, errPackCorrupt) {
			t.Errorf("%s: %v", name, err)
		}
	}
	// Per-entry problems: missing base, lying hash.
	base := bytes.Repeat([]byte("base content "), 200)
	p2, _ := buildTestPack(t, true, []testPackObject{{1, editBytes(base), base}, {1, []byte("fine"), nil}})
	got, err := readTestPack(p2, nil)
	if err != nil || got[0].Err == nil || got[1].Err != nil {
		t.Fatalf("missing base: %v %+v", err, got)
	}
}
