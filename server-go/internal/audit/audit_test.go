package audit

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestFileRecorderWritesJSONLine(t *testing.T) {
	path := filepath.Join(t.TempDir(), "audit.jsonl")
	recorder := NewFileRecorder(path)
	err := recorder.Record(context.Background(), Event{
		Event:    "access_decision",
		Action:   "staged:list",
		Decision: DecisionAllow,
		Tenant:   "acme",
	})
	if err != nil {
		t.Fatalf("Record() error = %v", err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile() error = %v", err)
	}
	var event Event
	if err := json.Unmarshal(data[:len(data)-1], &event); err != nil {
		t.Fatalf("Unmarshal() error = %v", err)
	}
	if event.Action != "staged:list" || event.Decision != DecisionAllow {
		t.Fatalf("event = %#v", event)
	}
}
