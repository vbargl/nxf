package reconcile

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/vbargl/nxf/internal/paths"
)

func TestRunActivateDeactivate(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, "config"))
	t.Setenv("XDG_STATE_HOME", filepath.Join(home, "state"))
	t.Setenv("XDG_DATA_HOME", filepath.Join(home, "data"))

	profile := filepath.Join(home, "nix-profile")
	t.Setenv("NXF_PROFILE", profile)

	actLog := filepath.Join(home, "hooks.log")
	writeScript := func(name, body string) string {
		p := filepath.Join(home, name)
		if err := os.WriteFile(p, []byte("#!/bin/sh\n"+body+"\n"), 0o755); err != nil {
			t.Fatal(err)
		}
		return p
	}
	// Live activation execs the profile path, so $0 is
	// .../etc/nxf/hooks/media/activationHook.sh.
	act1 := writeScript("act1", `printf 'activate %s\n' "$(basename "$(dirname "$0")")" >> '`+actLog+`'`)
	// Teardown execs the recorded store path (the profile tree is gone).
	deact1 := writeScript("deact1", `printf 'deactivate media\n' >> '`+actLog+`'`)

	writeManifest(t, profile, "media", `{"name":"media","units":{"greeter.service":"/nix/store/u1"}}`)
	hookDir := filepath.Join(profile, "etc", "nxf", "hooks", "media")
	if err := os.MkdirAll(hookDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(act1, filepath.Join(hookDir, paths.ActivationHookName)); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(deact1, filepath.Join(hookDir, paths.DeactivationHookName)); err != nil {
		t.Fatal(err)
	}

	origExec, origSession := execCommand, hasUserSession
	hasUserSession = func() bool { return true }
	execCommand = func(name string, args ...string) *exec.Cmd { return exec.Command("true") }
	t.Cleanup(func() { execCommand, hasUserSession = origExec, origSession })

	if err := Run(); err != nil {
		t.Fatalf("first Run: %v", err)
	}
	got, _ := os.ReadFile(actLog)
	if string(got) != "activate media\n" {
		t.Fatalf("after add, log = %q, want activate (name from hook path)", got)
	}

	if err := Run(); err != nil {
		t.Fatalf("second Run: %v", err)
	}
	got, _ = os.ReadFile(actLog)
	if string(got) != "activate media\n" {
		t.Fatalf("after noop, log = %q", got)
	}

	if err := os.RemoveAll(filepath.Join(profile, "share", "nxf", "profiles", "media")); err != nil {
		t.Fatal(err)
	}
	if err := os.RemoveAll(hookDir); err != nil {
		t.Fatal(err)
	}
	if err := Run(); err != nil {
		t.Fatalf("teardown Run: %v", err)
	}
	got, _ = os.ReadFile(actLog)
	if string(got) != "activate media\ndeactivate media\n" {
		t.Fatalf("after remove, log = %q", got)
	}
	unit := filepath.Join(home, "config", "systemd", "user", "nxf-media-greeter.service")
	if _, err := os.Lstat(unit); !os.IsNotExist(err) {
		t.Errorf("unit symlink still present after teardown: %v", err)
	}
}
