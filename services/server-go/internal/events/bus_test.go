package events

import (
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// recv reads one event or fails after a timeout.
func recv(t *testing.T, ch <-chan domain.Event) domain.Event {
	t.Helper()
	select {
	case e, ok := <-ch:
		if !ok {
			t.Fatal("channel closed unexpectedly")
		}
		return e
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for event")
	}
	return domain.Event{}
}

// assertEmpty fails if ch has a buffered event.
func assertEmpty(t *testing.T, ch <-chan domain.Event) {
	t.Helper()
	select {
	case e, ok := <-ch:
		if ok {
			t.Fatalf("unexpected event %+v", e)
		}
		t.Fatal("channel unexpectedly closed")
	default:
	}
}

func TestFanOut(t *testing.T) {
	b := NewBus()
	const n = 5
	chans := make([]<-chan domain.Event, n)
	for i := range chans {
		ch, cancel := b.Subscribe(domain.EventFilter{})
		defer cancel()
		chans[i] = ch
	}
	b.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: "t1", TreeID: "tr1", Payload: "x"})
	for i, ch := range chans {
		e := recv(t, ch)
		if e.Type != domain.EventBranchPushed || e.Payload != "x" {
			t.Fatalf("subscriber %d got %+v", i, e)
		}
		if e.At.IsZero() {
			t.Fatalf("subscriber %d: At not set", i)
		}
	}
	st := b.Stats()
	if st.Published != 1 || st.Delivered != n || st.Dropped != 0 || st.Subscribers != n {
		t.Fatalf("stats = %+v", st)
	}
}

func TestPublishKeepsExplicitAt(t *testing.T) {
	b := NewBus()
	ch, cancel := b.Subscribe(domain.EventFilter{})
	defer cancel()
	at := time.Date(2025, 1, 2, 3, 4, 5, 0, time.UTC)
	b.Publish(domain.Event{Type: "x", At: at})
	if got := recv(t, ch).At; !got.Equal(at) {
		t.Fatalf("At = %v, want %v", got, at)
	}
}

func TestFiltering(t *testing.T) {
	events := []domain.Event{
		{Type: domain.EventBranchPushed, TenantID: "t1", TreeID: "a"},
		{Type: domain.EventSnapshotStaged, TenantID: "t1", TreeID: "a"},
		{Type: domain.EventSnapshotStaged, TenantID: "t1", TreeID: "b"},
		{Type: domain.EventMROpened, TenantID: "t2", TreeID: "c"},
		{Type: domain.EventTreeCreated, TenantID: "t2"},
	}
	tests := []struct {
		name   string
		filter domain.EventFilter
		want   []int // indexes into events
	}{
		{"all", domain.EventFilter{}, []int{0, 1, 2, 3, 4}},
		{"tenant", domain.EventFilter{TenantID: "t1"}, []int{0, 1, 2}},
		{"tree", domain.EventFilter{TreeID: "a"}, []int{0, 1}},
		{"tenant+tree", domain.EventFilter{TenantID: "t2", TreeID: "c"}, []int{3}},
		{"tenant+tree mismatch", domain.EventFilter{TenantID: "t1", TreeID: "c"}, nil},
		{"types", domain.EventFilter{Types: []string{domain.EventSnapshotStaged}}, []int{1, 2}},
		{"multi types", domain.EventFilter{Types: []string{domain.EventMROpened, domain.EventTreeCreated}}, []int{3, 4}},
		{"empty type entries ignored", domain.EventFilter{Types: []string{""}}, []int{0, 1, 2, 3, 4}},
		{"all fields", domain.EventFilter{TenantID: "t1", TreeID: "b", Types: []string{domain.EventSnapshotStaged}}, []int{2}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			b := NewBus()
			ch, cancel := b.Subscribe(tt.filter)
			defer cancel()
			for _, e := range events {
				b.Publish(e)
			}
			for _, idx := range tt.want {
				got := recv(t, ch)
				if got.Type != events[idx].Type || got.TreeID != events[idx].TreeID || got.TenantID != events[idx].TenantID {
					t.Fatalf("got %+v, want %+v", got, events[idx])
				}
			}
			assertEmpty(t, ch)

			// Recent must apply the same filter.
			rec := b.Recent(tt.filter, 0)
			if len(rec) != len(tt.want) {
				t.Fatalf("Recent returned %d events, want %d", len(rec), len(tt.want))
			}
		})
	}
}

func TestFilterTypesCopied(t *testing.T) {
	b := NewBus()
	types := []string{domain.EventBranchPushed}
	ch, cancel := b.Subscribe(domain.EventFilter{Types: types})
	defer cancel()
	types[0] = domain.EventMROpened // mutating caller's slice must not matter
	b.Publish(domain.Event{Type: domain.EventBranchPushed})
	recv(t, ch)
}

func TestSlowSubscriberDrops(t *testing.T) {
	b := NewBusWithBuffer(4)
	slow, cancelSlow := b.Subscribe(domain.EventFilter{})
	defer cancelSlow()
	fast, cancelFast := b.Subscribe(domain.EventFilter{})
	defer cancelFast()

	var fastGot []int
	var wg sync.WaitGroup
	done := make(chan struct{})
	wg.Add(1)
	go func() {
		defer wg.Done()
		for e := range fast {
			fastGot = append(fastGot, e.Payload.(int))
			if len(fastGot) == 10 {
				close(done)
				return
			}
		}
	}()

	for i := 0; i < 10; i++ {
		b.Publish(domain.Event{Type: "x", Payload: i})
		// Give the fast reader a chance so its buffer never fills.
		deadline := time.Now().Add(2 * time.Second)
		for len(fast) > 0 && time.Now().Before(deadline) {
			time.Sleep(time.Millisecond)
		}
	}
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("fast subscriber did not receive all events")
	}
	wg.Wait()

	// The slow subscriber kept only the first 4 (buffer size) events.
	for i := 0; i < 4; i++ {
		if got := recv(t, slow).Payload.(int); got != i {
			t.Fatalf("slow got %d, want %d", got, i)
		}
	}
	assertEmpty(t, slow)
	for i, v := range fastGot {
		if v != i {
			t.Fatalf("fast order broken: %v", fastGot)
		}
	}

	st := b.Stats()
	if st.Published != 10 || st.Delivered != 14 || st.Dropped != 6 {
		t.Fatalf("stats = %+v", st)
	}
}

func TestPublishNeverBlocks(t *testing.T) {
	b := NewBusWithBuffer(1)
	_, cancel := b.Subscribe(domain.EventFilter{}) // never drained
	defer cancel()
	done := make(chan struct{})
	go func() {
		for i := 0; i < 10000; i++ {
			b.Publish(domain.Event{Type: "x"})
		}
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("Publish blocked on a slow subscriber")
	}
	if st := b.Stats(); st.Dropped != 9999 || st.Delivered != 1 {
		t.Fatalf("stats = %+v", st)
	}
}

func TestBufferDefaults(t *testing.T) {
	for _, n := range []int{0, -3} {
		b := NewBusWithBuffer(n)
		ch, cancel := b.Subscribe(domain.EventFilter{})
		if cap(ch) != DefaultBuffer {
			t.Fatalf("NewBusWithBuffer(%d) cap = %d, want %d", n, cap(ch), DefaultBuffer)
		}
		cancel()
	}
	b := NewBusWithBuffer(7)
	ch, cancel := b.Subscribe(domain.EventFilter{})
	defer cancel()
	if cap(ch) != 7 {
		t.Fatalf("cap = %d, want 7", cap(ch))
	}
}

func TestCancelSemantics(t *testing.T) {
	b := NewBus()
	ch, cancel := b.Subscribe(domain.EventFilter{})
	other, cancelOther := b.Subscribe(domain.EventFilter{})
	defer cancelOther()

	b.Publish(domain.Event{Type: "before"})
	cancel()
	cancel() // idempotent: must not panic
	b.Publish(domain.Event{Type: "after"})

	// Buffered event is still readable, then the channel is closed.
	if e := recv(t, ch); e.Type != "before" {
		t.Fatalf("got %q", e.Type)
	}
	select {
	case e, ok := <-ch:
		if ok {
			t.Fatalf("received %+v after cancel", e)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("channel not closed after cancel")
	}

	// The other subscriber is unaffected.
	if recv(t, other).Type != "before" || recv(t, other).Type != "after" {
		t.Fatal("other subscriber lost events")
	}
	if st := b.Stats(); st.Subscribers != 1 {
		t.Fatalf("subscribers = %d, want 1", st.Subscribers)
	}
}

func TestClose(t *testing.T) {
	b := NewBus()
	ch, cancel := b.Subscribe(domain.EventFilter{})
	b.Close()
	b.Close() // idempotent
	cancel()  // cancel after Close must not double-close
	if _, ok := <-ch; ok {
		t.Fatal("channel open after Close")
	}
	ch2, cancel2 := b.Subscribe(domain.EventFilter{})
	defer cancel2()
	if _, ok := <-ch2; ok {
		t.Fatal("Subscribe after Close returned an open channel")
	}
	b.Publish(domain.Event{Type: "x"}) // must not panic
	if got := len(b.Recent(domain.EventFilter{}, 0)); got != 1 {
		t.Fatalf("history after Close = %d, want 1", got)
	}
	if st := b.Stats(); st.Subscribers != 0 {
		t.Fatalf("subscribers = %d", st.Subscribers)
	}
}

func TestZeroValueBus(t *testing.T) {
	var b Bus
	ch, cancel := b.Subscribe(domain.EventFilter{})
	defer cancel()
	if cap(ch) != DefaultBuffer {
		t.Fatalf("cap = %d", cap(ch))
	}
	b.Publish(domain.Event{Type: "x"})
	recv(t, ch)
	if len(b.Recent(domain.EventFilter{}, 0)) != 1 {
		t.Fatal("zero-value bus did not record history")
	}
}

func TestRecent(t *testing.T) {
	b := NewBusWithOptions(Options{History: 5})
	if got := b.Recent(domain.EventFilter{}, 10); got == nil || len(got) != 0 {
		t.Fatalf("empty Recent = %#v", got)
	}
	for i := 0; i < 8; i++ {
		tree := "a"
		if i%2 == 1 {
			tree = "b"
		}
		b.Publish(domain.Event{Type: "x", TreeID: tree, Payload: i})
	}
	payloads := func(es []domain.Event) []int {
		out := make([]int, len(es))
		for i, e := range es {
			out[i] = e.Payload.(int)
		}
		return out
	}
	tests := []struct {
		name   string
		filter domain.EventFilter
		n      int
		want   []int
	}{
		{"all retained", domain.EventFilter{}, 0, []int{3, 4, 5, 6, 7}},
		{"last two", domain.EventFilter{}, 2, []int{6, 7}},
		{"n larger than ring", domain.EventFilter{}, 100, []int{3, 4, 5, 6, 7}},
		{"filtered", domain.EventFilter{TreeID: "a"}, 0, []int{4, 6}},
		{"filtered last one", domain.EventFilter{TreeID: "b"}, 1, []int{7}},
		{"no match", domain.EventFilter{TreeID: "zzz"}, 0, []int{}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := payloads(b.Recent(tt.filter, tt.n))
			if fmt.Sprint(got) != fmt.Sprint(tt.want) {
				t.Fatalf("Recent = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestRecentDefaultAndDisabled(t *testing.T) {
	b := NewBus()
	for i := 0; i < DefaultHistory+50; i++ {
		b.Publish(domain.Event{Type: "x", Payload: i})
	}
	got := b.Recent(domain.EventFilter{}, 0)
	if len(got) != DefaultHistory {
		t.Fatalf("len = %d, want %d", len(got), DefaultHistory)
	}
	if got[0].Payload.(int) != 50 || got[len(got)-1].Payload.(int) != DefaultHistory+49 {
		t.Fatalf("window = [%v..%v]", got[0].Payload, got[len(got)-1].Payload)
	}

	off := NewBusWithOptions(Options{History: -1})
	off.Publish(domain.Event{Type: "x"})
	if n := len(off.Recent(domain.EventFilter{}, 0)); n != 0 {
		t.Fatalf("disabled history returned %d events", n)
	}
}

// TestConcurrentStress hammers Publish/Subscribe/cancel/Recent/Stats from
// many goroutines; run with -race to validate the locking design.
func TestConcurrentStress(t *testing.T) {
	b := NewBusWithOptions(Options{Buffer: 8, History: 50})
	const (
		publishers  = 32
		perPub      = 500
		subscribers = 32
	)
	var wg sync.WaitGroup
	stop := make(chan struct{})

	// Long-lived subscribers that drain until their channel closes.
	var drained [subscribers]int
	var subWG sync.WaitGroup
	cancels := make([]func(), subscribers)
	for i := 0; i < subscribers; i++ {
		f := domain.EventFilter{}
		if i%2 == 0 {
			f.TenantID = fmt.Sprintf("t%d", i%4)
		}
		ch, cancel := b.Subscribe(f)
		cancels[i] = cancel
		subWG.Add(1)
		go func(i int) {
			defer subWG.Done()
			for range ch {
				drained[i]++
			}
		}(i)
	}

	// Churning subscribers: subscribe, maybe read, cancel (sometimes twice,
	// sometimes concurrently).
	for i := 0; i < 16; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			for {
				select {
				case <-stop:
					return
				default:
				}
				ch, cancel := b.Subscribe(domain.EventFilter{Types: []string{"x"}})
				select {
				case <-ch:
				default:
				}
				var cw sync.WaitGroup
				cw.Add(2)
				go func() { defer cw.Done(); cancel() }()
				go func() { defer cw.Done(); cancel() }()
				cw.Wait()
				for range ch { // must terminate: channel is closed
				}
			}
		}(i)
	}

	// Readers of Recent/Stats.
	for i := 0; i < 4; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for {
				select {
				case <-stop:
					return
				default:
				}
				_ = b.Recent(domain.EventFilter{TenantID: "t1"}, 10)
				_ = b.Stats()
			}
		}()
	}

	var pubWG sync.WaitGroup
	for p := 0; p < publishers; p++ {
		pubWG.Add(1)
		go func(p int) {
			defer pubWG.Done()
			for i := 0; i < perPub; i++ {
				b.Publish(domain.Event{Type: "x", TenantID: fmt.Sprintf("t%d", (p+i)%4), Payload: i})
			}
		}(p)
	}
	pubWG.Wait()
	close(stop)
	wg.Wait()

	for _, c := range cancels {
		c()
	}
	subWG.Wait()

	st := b.Stats()
	if st.Published != publishers*perPub {
		t.Fatalf("published = %d, want %d", st.Published, publishers*perPub)
	}
	if st.Subscribers != 0 {
		t.Fatalf("subscribers = %d after cancel", st.Subscribers)
	}
	var total uint64
	for _, n := range drained {
		total += uint64(n)
	}
	if total > st.Delivered {
		t.Fatalf("long-lived subscribers drained %d > delivered %d", total, st.Delivered)
	}
	if got := len(b.Recent(domain.EventFilter{}, 0)); got != 50 {
		t.Fatalf("history len = %d, want 50", got)
	}
}
