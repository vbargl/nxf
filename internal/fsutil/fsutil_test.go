package fsutil

import (
	"os"
	"path/filepath"
	"testing"
)

func TestWriteFileAtomic(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "state.json")
	if err := WriteFileAtomic(path, []byte(`{"a":1}`), 0o644); err != nil {
		t.Fatalf("WriteFileAtomic: %v", err)
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if string(got) != `{"a":1}` {
		t.Errorf("content = %q", got)
	}
	if err := WriteFileAtomic(path, []byte(`{"a":2}`), 0o644); err != nil {
		t.Fatalf("overwrite: %v", err)
	}
	got, _ = os.ReadFile(path)
	if string(got) != `{"a":2}` {
		t.Errorf("overwrite content = %q", got)
	}
}
