package audit

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sync"
	"time"
)

type Decision string

const (
	DecisionAllow Decision = "allow"
	DecisionDeny  Decision = "deny"
)

type Event struct {
	Timestamp  time.Time `json:"timestamp"`
	Event      string    `json:"event"`
	Action     string    `json:"action"`
	Decision   Decision  `json:"decision"`
	Reason     string    `json:"reason,omitempty"`
	Tenant     string    `json:"tenant,omitempty"`
	Account    string    `json:"account,omitempty"`
	TokenID    string    `json:"token_id,omitempty"`
	AuthMethod string    `json:"auth_method,omitempty"`
	Resource   string    `json:"resource,omitempty"`
	RequestID  string    `json:"request_id,omitempty"`
	HTTPMethod string    `json:"http_method,omitempty"`
	HTTPPath   string    `json:"http_path,omitempty"`
}

type Recorder interface {
	Record(ctx context.Context, event Event) error
}

type FileRecorder struct {
	path string
	mu   sync.Mutex
}

func NewFileRecorder(path string) *FileRecorder {
	return &FileRecorder{path: path}
}

func (r *FileRecorder) Record(ctx context.Context, event Event) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if event.Timestamp.IsZero() {
		event.Timestamp = time.Now().UTC()
	}
	r.mu.Lock()
	defer r.mu.Unlock()

	if err := os.MkdirAll(filepath.Dir(r.path), 0o755); err != nil {
		return err
	}
	file, err := os.OpenFile(r.path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return err
	}
	defer file.Close()

	data, err := json.Marshal(event)
	if err != nil {
		return err
	}
	if _, err := file.Write(append(data, '\n')); err != nil {
		return err
	}
	return nil
}

type NoopRecorder struct{}

func (NoopRecorder) Record(context.Context, Event) error {
	return nil
}
