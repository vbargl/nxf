// Package reconcile reconciles systemd user units and activation scripts
// against the set of profiles currently merged into the nix profile: it
// reads each profile's manifest, compares it against the last-applied state,
// and installs/restarts/tears down units and activation scripts accordingly.
package reconcile

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"sort"
)

// Manifest mirrors the JSON written by nix/mkProfile.nix into
// <profile>/share/nxf/profiles/<name>/nxf.json.
type Manifest struct {
	Name        string            `json:"name"`
	Units       map[string]string `json:"units"`
	ManualUnits []string          `json:"manualUnits"`
	Activate    *string           `json:"activate"`
}

// autoStart reports whether nxf should enable/start (and later restart) unit
// on its own, as opposed to only installing the unit file and leaving
// enable/start to the user (see manualUnits in nix/mkProfile.nix).
func (m Manifest) autoStart(unit string) bool {
	return !slices.Contains(m.ManualUnits, unit)
}

// Discover walks profileLink/share/nxf/profiles/*/nxf.json and returns the
// manifest of every profile currently merged into the nix profile.
func Discover(profileLink string) ([]Manifest, error) {
	base := filepath.Join(profileLink, "share", "nxf", "profiles")
	entries, err := os.ReadDir(base)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("reading %s: %w", base, err)
	}

	var manifests []Manifest
	for _, entry := range entries {
		// entry.IsDir() reflects the raw dirent type and does NOT follow
		// symlinks - once 2+ profiles are merged, nix's own profile builder
		// represents each profile's per-name manifest dir as a symlink (only
		// a single contributor's tree needs merging further down), so an
		// IsDir() check here would wrongly skip every profile past the
		// first. Stat (which does follow symlinks) instead.
		info, err := os.Stat(filepath.Join(base, entry.Name()))
		if err != nil || !info.IsDir() {
			continue
		}
		path := filepath.Join(base, entry.Name(), "nxf.json")
		data, err := os.ReadFile(path)
		if err != nil {
			if os.IsNotExist(err) {
				continue
			}
			return nil, fmt.Errorf("reading %s: %w", path, err)
		}
		var m Manifest
		if err := json.Unmarshal(data, &m); err != nil {
			return nil, fmt.Errorf("parsing %s: %w", path, err)
		}
		if m.Name == "" {
			continue
		}
		manifests = append(manifests, m)
	}

	sort.Slice(manifests, func(i, j int) bool { return manifests[i].Name < manifests[j].Name })
	return manifests, nil
}
