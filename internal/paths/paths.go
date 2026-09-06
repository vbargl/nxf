// Package paths centralizes the env-var/XDG-derived filesystem locations nxf
// reads from and writes to (nix profile symlink, systemd user unit dir,
// applied-state dir).
package paths

import (
	"os"
	"path/filepath"
)

func HomeDir() (string, error) {
	if h := os.Getenv("HOME"); h != "" {
		return h, nil
	}
	return os.UserHomeDir()
}

func NixProfileLink() (string, error) {
	if p := os.Getenv("NXFP_PROFILE"); p != "" {
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

func SystemdUserUnitDir() (string, error) {
	config, err := XDGConfigHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(config, "systemd", "user"), nil
}

func AppliedStateDir() (string, error) {
	state, err := XDGStateHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(state, "nxf", "applied"), nil
}

// ProfileRefsFile records, per installed profile name, the flake it was
// installed from (see internal/profile's rememberRef) - the source `nxf
// profile upgrade` rebuilds from, since profiles are installed as plain
// built store paths (see nixutil.ProfileAdd) and so carry no flake-ref
// metadata of their own for `nix profile upgrade` to work from directly.
func ProfileRefsFile() (string, error) {
	state, err := XDGStateHome()
	if err != nil {
		return "", err
	}
	return filepath.Join(state, "nxf", "refs.json"), nil
}

func UnitFileName(profile, unit string) string {
	return "nxf-" + profile + "-" + unit + ".service"
}
