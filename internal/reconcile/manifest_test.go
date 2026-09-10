package reconcile

import (
	"os"
	"path/filepath"
	"testing"
)

func writeManifest(t *testing.T, profileLink, name, jsonBody string) {
	t.Helper()
	dir := filepath.Join(profileLink, "share", "nxf", "profiles", name)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("mkdir %s: %v", dir, err)
	}
	if err := os.WriteFile(filepath.Join(dir, "nxf.json"), []byte(jsonBody), 0o644); err != nil {
		t.Fatalf("write nxf.json: %v", err)
	}
}

func TestDiscoverEmptyProfileLink(t *testing.T) {
	got, err := Discover(t.TempDir())
	if err != nil {
		t.Fatalf("Discover: unexpected error: %v", err)
	}
	if len(got) != 0 {
		t.Errorf("Discover(empty) = %#v, want none", got)
	}
}

func TestDiscoverReadsManifestsAndFollowsSymlinks(t *testing.T) {
	root := t.TempDir()
	writeManifest(t, root, "media", `{"name":"media","units":{"player":"/nix/store/u1"}}`)
	writeManifest(t, root, "dev.default", `{"name":"dev.default","units":{},"activate":"/nix/store/act"}`)

	// Second profile as a symlink, matching how `nix profile` merges
	// contributor trees (entry.IsDir() would skip this).
	real := filepath.Join(t.TempDir(), "terminal.admintools")
	if err := os.MkdirAll(real, 0o755); err != nil {
		t.Fatalf("mkdir real: %v", err)
	}
	if err := os.WriteFile(filepath.Join(real, "nxf.json"), []byte(`{"name":"terminal.admintools","manualUnits":["syncthing"],"units":{"syncthing":"/nix/store/u2"}}`), 0o644); err != nil {
		t.Fatalf("write symlink target: %v", err)
	}
	link := filepath.Join(root, "share", "nxf", "profiles", "terminal.admintools")
	if err := os.Symlink(real, link); err != nil {
		t.Fatalf("symlink: %v", err)
	}

	got, err := Discover(root)
	if err != nil {
		t.Fatalf("Discover: unexpected error: %v", err)
	}
	if len(got) != 3 {
		t.Fatalf("Discover returned %d manifests, want 3: %#v", len(got), got)
	}
	if got[0].Name != "dev.default" || got[1].Name != "media" || got[2].Name != "terminal.admintools" {
		t.Errorf("Discover order/names = %q, %q, %q", got[0].Name, got[1].Name, got[2].Name)
	}
	if got[2].AutoStart("syncthing") || got[2].AutoStart("syncthing.service") {
		t.Errorf("terminal.admintools syncthing autoStart = true, want false")
	}
	if got[1].Units["player.service"] != "/nix/store/u1" {
		t.Errorf("media units = %#v", got[1].Units)
	}
}

func TestDiscoverResolvesHookFiles(t *testing.T) {
	root := t.TempDir()
	writeManifest(t, root, "media", `{"name":"media"}`)
	storeAct := filepath.Join(t.TempDir(), "act")
	if err := os.WriteFile(storeAct, []byte("#!/bin/sh\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(root, "etc", "nxf", "hooks", "media")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(storeAct, filepath.Join(dir, "activationHook.sh")); err != nil {
		t.Fatal(err)
	}

	got, err := Discover(root)
	if err != nil {
		t.Fatalf("Discover: %v", err)
	}
	if len(got) != 1 || got[0].Activate == nil || *got[0].Activate != storeAct {
		t.Fatalf("Discover hooks = %#v, want activate -> %s", got, storeAct)
	}
	if got[0].Deactivate != nil {
		t.Errorf("unexpected deactivate: %#v", got[0].Deactivate)
	}
}

func TestDiscoverSkipsEmptyName(t *testing.T) {
	root := t.TempDir()
	writeManifest(t, root, "desktop-entries", `{"foo.desktop":"/nix/store/x"}`)
	writeManifest(t, root, "media", `{"name":"media"}`)

	got, err := Discover(root)
	if err != nil {
		t.Fatalf("Discover: unexpected error: %v", err)
	}
	if len(got) != 1 || got[0].Name != "media" {
		t.Errorf("Discover = %#v, want only media", got)
	}
}
