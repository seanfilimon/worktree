package audit

import (
	"crypto/rand"
	"encoding/hex"
	"sync"
	"time"
)

// idGenerator produces RFC 9562 UUIDv7 strings that are strictly
// increasing (in lexical order) within a process: the 48-bit Unix
// millisecond timestamp leads, and the 12-bit rand_a field is used as a
// counter for IDs generated within the same millisecond (method 1 of
// RFC 9562 §6.2). When the counter overflows, or the clock moves backwards,
// the timestamp is advanced artificially so ordering is preserved.
type idGenerator struct {
	mu     sync.Mutex
	lastMS int64
	seq    uint16 // 12 bits
}

func newIDGenerator() *idGenerator { return &idGenerator{} }

func (g *idGenerator) next(now time.Time) string {
	ms := now.UnixMilli()
	if ms < 0 {
		ms = 0
	}

	g.mu.Lock()
	switch {
	case ms > g.lastMS:
		g.lastMS = ms
		g.seq = 0
	default: // same millisecond or clock went backwards
		g.seq++
		if g.seq > 0x0fff {
			g.lastMS++
			g.seq = 0
		}
	}
	ms, seq := g.lastMS, g.seq
	g.mu.Unlock()

	var u [16]byte
	u[0] = byte(ms >> 40)
	u[1] = byte(ms >> 32)
	u[2] = byte(ms >> 24)
	u[3] = byte(ms >> 16)
	u[4] = byte(ms >> 8)
	u[5] = byte(ms)
	u[6] = 0x70 | byte(seq>>8)&0x0f // version 7
	u[7] = byte(seq)
	// rand_b: 62 random bits. crypto/rand.Read never returns an error.
	_, _ = rand.Read(u[8:])
	u[8] = 0x80 | u[8]&0x3f // RFC 4122 variant

	var s [36]byte
	hex.Encode(s[0:8], u[0:4])
	s[8] = '-'
	hex.Encode(s[9:13], u[4:6])
	s[13] = '-'
	hex.Encode(s[14:18], u[6:8])
	s[18] = '-'
	hex.Encode(s[19:23], u[8:10])
	s[23] = '-'
	hex.Encode(s[24:], u[10:])
	return string(s[:])
}
