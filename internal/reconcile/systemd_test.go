package reconcile

import (
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"testing"
)

type call struct {
	name string
	args []string
}

func stubExec(t *testing.T) *[]call {
	t.Helper()
	calls := &[]call{}
	orig := execCommand
	origSession := hasUserSession
	hasUserSession = func() bool { return true }
	execCommand = func(name string, args ...string) *exec.Cmd {
		*calls = append(*calls, call{name: name, args: args})
		return exec.Command("true")
	}
	t.Cleanup(func() {
		execCommand = orig
		hasUserSession = origSession
	})
	return calls
}

func TestSystemctlArgs(t *testing.T) {
	calls := stubExec(t)
	if err := systemctl("enable", "--now", "nxf-media-greeter.service"); err != nil {
		t.Fatalf("systemctl: unexpected error: %v", err)
	}
	want := []call{{name: "systemctl", args: []string{"--user", "enable", "--now", "nxf-media-greeter.service"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("systemctl calls = %#v, want %#v", *calls, want)
	}
}

func TestInstallUnitNewAutoStartEnables(t *testing.T) {
	calls := stubExec(t)
	unitDir := t.TempDir()

	err := installUnit(unitDir, "with-unit", "greeter", "/nix/store/unit-v1", false, true)
	if err != nil {
		t.Fatalf("installUnit: unexpected error: %v", err)
	}

	want := []call{
		{name: "systemctl", args: []string{"--user", "daemon-reload"}},
		{name: "systemctl", args: []string{"--user", "enable", "--now", "nxf-with-unit-greeter.service"}},
	}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("installUnit (new, autoStart) calls = %#v, want %#v", *calls, want)
	}

	link := filepath.Join(unitDir, "nxf-with-unit-greeter.service")
	target, err := os.Readlink(link)
	if err != nil {
		t.Fatalf("unit symlink not created: %v", err)
	}
	if target != "/nix/store/unit-v1" {
		t.Errorf("unit symlink points at %q, want %q", target, "/nix/store/unit-v1")
	}
}

func TestInstallUnitChangedStorePathRestarts(t *testing.T) {
	calls := stubExec(t)
	unitDir := t.TempDir()

	err := installUnit(unitDir, "with-unit", "greeter", "/nix/store/unit-v2", true, true)
	if err != nil {
		t.Fatalf("installUnit: unexpected error: %v", err)
	}

	want := []call{
		{name: "systemctl", args: []string{"--user", "daemon-reload"}},
		{name: "systemctl", args: []string{"--user", "restart", "nxf-with-unit-greeter.service"}},
	}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("installUnit (wasPresent, autoStart) calls = %#v, want %#v", *calls, want)
	}
}

func TestInstallUnitManualUnitOnlyReloads(t *testing.T) {
	calls := stubExec(t)
	unitDir := t.TempDir()

	// autoStart=false (a manual unit, see Manifest.autoStart): install the
	// file and reload the daemon, but never enable/restart it ourselves.
	if err := installUnit(unitDir, "with-unit", "greeter", "/nix/store/unit-v1", false, false); err != nil {
		t.Fatalf("installUnit: unexpected error: %v", err)
	}
	want := []call{{name: "systemctl", args: []string{"--user", "daemon-reload"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("installUnit (manual) calls = %#v, want %#v", *calls, want)
	}
}

func TestRemoveUnitDisablesAndReloads(t *testing.T) {
	calls := stubExec(t)
	unitDir := t.TempDir()
	link := filepath.Join(unitDir, "nxf-with-unit-greeter.service")
	if err := os.Symlink("/nix/store/unit-v1", link); err != nil {
		t.Fatalf("setting up unit symlink: %v", err)
	}

	if err := removeUnit(unitDir, "with-unit", "greeter"); err != nil {
		t.Fatalf("removeUnit: unexpected error: %v", err)
	}

	want := []call{
		{name: "systemctl", args: []string{"--user", "disable", "--now", "nxf-with-unit-greeter.service"}},
		{name: "systemctl", args: []string{"--user", "daemon-reload"}},
	}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("removeUnit calls = %#v, want %#v", *calls, want)
	}
	if _, err := os.Lstat(link); !os.IsNotExist(err) {
		t.Errorf("removeUnit left the unit symlink in place, want it removed (lstat err: %v)", err)
	}
}

func TestSystemctlSkipsWhenNoUserSession(t *testing.T) {
	calls := stubExec(t)
	orig := hasUserSession
	hasUserSession = func() bool { return false }
	t.Cleanup(func() { hasUserSession = orig })

	if err := systemctl("enable", "--now", "nxf-with-unit-greeter.service"); err != nil {
		t.Fatalf("systemctl with no session: unexpected error: %v", err)
	}
	if len(*calls) != 0 {
		t.Errorf("systemctl with no session calls = %#v, want none", *calls)
	}
}
