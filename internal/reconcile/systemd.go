package reconcile

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/vbargl/nxf/internal/paths"
)

// execCommand constructs the systemctl command; overridden in tests so
// argument construction can be verified without a real systemd --user
// session.
var execCommand = exec.Command

// hasUserSession reports whether a systemd --user D-Bus session is reachable.
// Overridden in tests. A missing session (root, bare container, no lingering)
// is not a hard error: unit files are still installed, but enable/start/
// restart/disable are skipped with a warning.
var hasUserSession = defaultHasUserSession

func defaultHasUserSession() bool {
	if os.Getenv("DBUS_SESSION_BUS_ADDRESS") != "" {
		return true
	}
	runtimeDir := os.Getenv("XDG_RUNTIME_DIR")
	if runtimeDir == "" {
		return false
	}
	_, err := os.Stat(filepath.Join(runtimeDir, "bus"))
	return err == nil
}

func systemctl(args ...string) error {
	if !hasUserSession() {
		fmt.Fprintf(os.Stderr, "nxf: warning: no systemd user session available; skipping systemctl --user %s\n", strings.Join(args, " "))
		return nil
	}
	cmd := execCommand("systemctl", append([]string{"--user"}, args...)...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}

func daemonReload() error {
	return systemctl("daemon-reload")
}

// installUnit symlinks the unit's store path into the systemd user unit
// directory under a stable filename, then enables/restarts it. wasPresent
// distinguishes "new unit -> enable --now" from "changed store path ->
// restart" (an already-enabled unit doesn't need re-enabling). autoStart
// false means the unit is opt-in (see Manifest.autoStart): install the file
// and reload the daemon, but leave enabling/starting/restarting to the user.
func installUnit(unitDir, profile, unit, storePath string, wasPresent, autoStart bool) error {
	name := paths.UnitFileName(profile, unit)
	link := filepath.Join(unitDir, name)

	if err := os.MkdirAll(unitDir, 0o755); err != nil {
		return err
	}
	if err := os.Remove(link); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("removing stale unit symlink %s: %w", link, err)
	}
	if err := os.Symlink(storePath, link); err != nil {
		return fmt.Errorf("symlinking %s -> %s: %w", link, storePath, err)
	}

	if err := daemonReload(); err != nil {
		return err
	}

	if !autoStart {
		if wasPresent {
			fmt.Printf("nxf: installed updated %s (manual unit - run `systemctl --user restart %s` to pick it up)\n", name, name)
		} else {
			fmt.Printf("nxf: installed %s (manual unit - run `systemctl --user enable --now %s` to start it)\n", name, name)
		}
		return nil
	}

	if wasPresent {
		fmt.Printf("nxf: restarting %s (store path changed)\n", name)
		return systemctl("restart", name)
	}
	fmt.Printf("nxf: enabling %s\n", name)
	if err := systemctl("enable", "--now", name); err != nil {
		_ = os.Remove(link)
		_ = daemonReload()
		return err
	}
	return nil
}

// removeUnit stops, disables, and unlinks a unit that no longer belongs to
// any applied profile.
func removeUnit(unitDir, profile, unit string) error {
	name := paths.UnitFileName(profile, unit)
	link := filepath.Join(unitDir, name)

	fmt.Printf("nxf: stopping %s\n", name)
	if err := systemctl("disable", "--now", name); err != nil {
		// The unit may already be gone (e.g. a previous run partially
		// completed); don't let that block removing the symlink below.
		fmt.Fprintf(os.Stderr, "nxf: warning: disabling %s: %v\n", name, err)
	}

	if err := os.Remove(link); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("removing unit symlink %s: %w", link, err)
	}
	return daemonReload()
}
