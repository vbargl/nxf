// Package paths centralizes the env-var/XDG-derived filesystem locations nxf
// reads from and writes to (nix profile symlink, systemd user unit dir,
// applied-state dir).
package paths

import (
	"os"
	"path/filepath"
	"strings"
)

func HomeDir() (string, error) {
	if h := os.Getenv("HOME"); h != "" {
		return h, nil
	}
	return os.UserHomeDir()
}

func NixProfileLink() (string, error) {
	// NXF_PROFILE is a test-only override (throwaway profile in
	// test/integration.sh and unit tests). Normal use is ~/.nix-profile,
	// the same default as `nix profile`.
	if p := os.Getenv("NXF_PROFILE"); p != "" {
		return p, nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".nix-profile"), nil
}

func XDGConfigHome() (string, error) {
	if p := os.Getenv("XDG_CONFIG_HOME"); p != "" {
		return p, nil
	}
	home, err := HomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".config"), nil
}

func XDGStateHome() (string, error) {
	if p := os.Getenv("XDG_STATE_HOME"); p != "" {
		return p, nil
	}
	home, err := HomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "state"), nil
}

func XDGDataHome() (string, error) {
	if p := os.Getenv("XDG_DATA_HOME"); p != "" {
		return p, nil
	}
	home, err := HomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "share"), nil
}

// DesktopEntriesDir is the standard per-user XDG applications directory -
// unlike ~/.nix-profile/share/applications, it's never itself replaced by a
// symlink swap, so every desktop environment's file watcher reacts correctly
// to entries appearing/disappearing here (see reconcile.syncDesktopEntries).
func DesktopEntriesDir() (string, error) {
	data, err := XDGDataHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(data, "applications"), nil
}

func SystemdUserUnitDir() (string, error) {
	config, err := XDGConfigHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(config, "systemd", "user"), nil
}

func StateDir() (string, error) {
	state, err := XDGStateHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(state, "nxf"), nil
}

func LockFile() (string, error) {
	state, err := StateDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(state, "lock"), nil
}

func AppliedStateDir() (string, error) {
	state, err := StateDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(state, "applied"), nil
}

// DesktopEntriesStateFile records the .desktop names nxf last mirrored into
// DesktopEntriesDir, keyed to their store-path targets. It lives next to
// (not inside) AppliedStateDir so loadAppliedStates never treats it as a
// profile snapshot.
func DesktopEntriesStateFile() (string, error) {
	state, err := XDGStateHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(state, "nxf", "desktop-entries.json"), nil
}

// ProfileRefsFile records, per installed profile name, the flake it was
// installed from (see internal/profile's rememberRef) - the convenience ref
// (e.g. a relative path or registry name) `nxf profile upgrade` rebuilds
// from, since only the fully-qualified ref actually passed to nix profile
// add (see nixutil.ProfileAdd) is visible in `nix profile list`, not the
// shorthand the user originally typed.
const (
	ActivationHookName   = "activationHook.sh"
	DeactivationHookName = "deactivationHook.sh"
)

// HookFile is the conventional path of a profile hook inside a nix profile
// (etc/nxf/hooks/<name>/activationHook.sh or deactivationHook.sh).
func HookFile(profileLink, name, hook string) string {
	return filepath.Join(profileLink, "etc", "nxf", "hooks", name, hook)
}

func ProfileRefsFile() (string, error) {
	state, err := XDGStateHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(state, "nxf", "refs.json"), nil
}

// Known systemd unit type suffixes nxf will install (not just .service).
var KnownUnitTypes = []string{
	"service", "timer", "socket", "path", "target", "slice",
	"mount", "automount", "swap", "scope",
}

func UnitHasTypeSuffix(unit string) bool {
	for _, t := range KnownUnitTypes {
		if strings.HasSuffix(unit, "."+t) {
			return true
		}
	}
	return false
}

func EnsureUnitSuffix(unit string) string {
	if UnitHasTypeSuffix(unit) {
		return unit
	}
	return unit + ".service"
}

func UnitFileName(profile, unit string) string {
	return "nxf-" + profile + "-" + EnsureUnitSuffix(unit)
}
