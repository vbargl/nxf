package reconcile

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLoadAppliedStatesSkipsNamelessJSON(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "desktop-entries.json"), []byte(`{"app.desktop":"/nix/store/x"}`), 0o644); err != nil {
		t.Fatalf("write stray json: %v", err)
	}
	if err := os.WriteFile(filepath.Join(dir, "media.json"), []byte(`{"name":"media","units":{"u":"/nix/store/u"}}`), 0o644); err != nil {
		t.Fatalf("write media json: %v", err)
	}

	got, err := loadAppliedStates(dir)
	if err != nil {
		t.Fatalf("loadAppliedStates: unexpected error: %v", err)
	}
	if _, ok := got[""]; ok {
		t.Errorf("loadAppliedStates picked up a nameless snapshot: %#v", got)
	}
	if got["media"].Units["u.service"] != "/nix/store/u" {
		t.Errorf("loadAppliedStates[media] = %#v", got["media"])
	}
}

func TestAppliedStateRoundTrip(t *testing.T) {
	dir := t.TempDir()
	act := "/nix/store/act"
	m := Manifest{Name: "dev.default", Units: map[string]string{"a": "/nix/store/a"}, Activate: &act}
	if err := saveAppliedState(dir, m); err != nil {
		t.Fatalf("saveAppliedState: %v", err)
	}
	got, err := loadAppliedStates(dir)
	if err != nil {
		t.Fatalf("loadAppliedStates: %v", err)
	}
	if got["dev.default"].Units["a.service"] != "/nix/store/a" {
		t.Errorf("round-trip = %#v", got["dev.default"])
	}
	if got["dev.default"].Activate == nil || *got["dev.default"].Activate != act {
		t.Errorf("activate round-trip = %#v", got["dev.default"].Activate)
	}

	if err := removeAppliedState(dir, "dev.default"); err != nil {
		t.Fatalf("removeAppliedState: %v", err)
	}
	got, err = loadAppliedStates(dir)
	if err != nil {
		t.Fatalf("loadAppliedStates after remove: %v", err)
	}
	if _, ok := got["dev.default"]; ok {
		t.Errorf("removeAppliedState left the snapshot: %#v", got)
	}
}
