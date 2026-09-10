package reconcile

import (
	"os"
	"path/filepath"
	"testing"
)

func TestSyncDesktopEntriesAddUpdateRemove(t *testing.T) {
	profile := t.TempDir()
	dest := t.TempDir()
	stateFile := filepath.Join(t.TempDir(), "desktop-entries.json")

	apps := filepath.Join(profile, "share", "applications")
	if err := os.MkdirAll(apps, 0o755); err != nil {
		t.Fatalf("mkdir apps: %v", err)
	}
	storeV1 := filepath.Join(t.TempDir(), "app-v1.desktop")
	if err := os.WriteFile(storeV1, []byte("v1"), 0o644); err != nil {
		t.Fatalf("write v1: %v", err)
	}
	if err := os.Symlink(storeV1, filepath.Join(apps, "app.desktop")); err != nil {
		t.Fatalf("symlink v1 into profile: %v", err)
	}

	if err := syncDesktopEntries(profile, dest, stateFile); err != nil {
		t.Fatalf("sync (add): %v", err)
	}
	link := filepath.Join(dest, "app.desktop")
	got, err := os.Readlink(link)
	if err != nil {
		t.Fatalf("readlink after add: %v", err)
	}
	if got != storeV1 {
		t.Errorf("link target = %q, want %q", got, storeV1)
	}

	storeV2 := filepath.Join(t.TempDir(), "app-v2.desktop")
	if err := os.WriteFile(storeV2, []byte("v2"), 0o644); err != nil {
		t.Fatalf("write v2: %v", err)
	}
	if err := os.Remove(filepath.Join(apps, "app.desktop")); err != nil {
		t.Fatalf("remove v1 symlink: %v", err)
	}
	if err := os.Symlink(storeV2, filepath.Join(apps, "app.desktop")); err != nil {
		t.Fatalf("symlink v2 into profile: %v", err)
	}
	if err := syncDesktopEntries(profile, dest, stateFile); err != nil {
		t.Fatalf("sync (update): %v", err)
	}
	got, err = os.Readlink(link)
	if err != nil {
		t.Fatalf("readlink after update: %v", err)
	}
	if got != storeV2 {
		t.Errorf("updated link target = %q, want %q", got, storeV2)
	}

	if err := os.Remove(filepath.Join(apps, "app.desktop")); err != nil {
		t.Fatalf("remove v2 symlink: %v", err)
	}
	if err := syncDesktopEntries(profile, dest, stateFile); err != nil {
		t.Fatalf("sync (remove): %v", err)
	}
	if _, err := os.Lstat(link); !os.IsNotExist(err) {
		t.Errorf("expected dest link removed, lstat err: %v", err)
	}
}

func TestRemoveManagedLinkLeavesUserReplacement(t *testing.T) {
	dest := t.TempDir()
	link := filepath.Join(dest, "app.desktop")
	if err := os.Symlink("/somewhere-else", link); err != nil {
		t.Fatalf("user symlink: %v", err)
	}
	removeManagedLink(dest, "app.desktop", "/nix/store/old")
	got, err := os.Readlink(link)
	if err != nil {
		t.Fatalf("user replacement was removed: %v", err)
	}
	if got != "/somewhere-else" {
		t.Errorf("link target = %q, want the user's replacement", got)
	}
}
