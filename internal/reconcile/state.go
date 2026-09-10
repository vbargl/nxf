package reconcile

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/vbargl/nxf/internal/fsutil"
	"github.com/vbargl/nxf/internal/paths"
)

// The applied-state directory holds one JSON file per profile, a snapshot of
// the manifest that was last successfully reconciled. It is the source of
// truth for "what did we last apply" - used both to decide whether a unit's
// store path changed (skip the restart if not) and, once a profile
// disappears from the nix profile, to know what to tear down.
func loadAppliedStates(dir string) (map[string]Manifest, error) {
	entries, err := os.ReadDir(dir)
	if os.IsNotExist(err) {
		return map[string]Manifest{}, nil
	}
	if err != nil {
		return nil, fmt.Errorf("reading %s: %w", dir, err)
	}

	states := map[string]Manifest{}
	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".json") {
			continue
		}
		path := filepath.Join(dir, entry.Name())
		data, err := os.ReadFile(path)
		if err != nil {
			return nil, fmt.Errorf("reading %s: %w", path, err)
		}
		var m Manifest
		if err := json.Unmarshal(data, &m); err != nil {
			return nil, fmt.Errorf("parsing %s: %w", path, err)
		}
		// Skip JSON that isn't a profile snapshot (empty name) so a
		// stray file in this directory can never be torn down as profile "".
		if m.Name == "" {
			continue
		}
		m.Units = normalizeUnitKeys(m.Units)
		for i, u := range m.ManualUnits {
			m.ManualUnits[i] = paths.EnsureUnitSuffix(u)
		}
		states[m.Name] = m
	}
	return states, nil
}

func saveAppliedState(dir string, m Manifest) error {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return err
	}
	path := filepath.Join(dir, m.Name+".json")
	return fsutil.WriteFileAtomic(path, data, 0o644)
}

func removeAppliedState(dir, name string) error {
	path := filepath.Join(dir, name+".json")
	err := os.Remove(path)
	if os.IsNotExist(err) {
		return nil
	}
	return err
}
