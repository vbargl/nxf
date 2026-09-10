package reconcile

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/vbargl/nxf/internal/fsutil"
)

// syncDesktopEntries mirrors every .desktop file under
// profileLink/share/applications into destDir (paths.DesktopEntriesDir() in
// production - normally ~/.local/share/applications) as a symlink straight
// into the nix store, and removes any such symlink nxf previously created
// whose source has since disappeared from the profile.
//
// This exists because nix profile switches generations by re-pointing
// ~/.nix-profile at a new store path rather than mutating the old one in
// place, which breaks any file watch a desktop environment has directly on
// ~/.nix-profile/share/applications (the watched directory itself gets
// swapped out from under it). ~/.local/share/applications, by contrast, is
// never replaced wholesale - only individual entries inside it come and go -
// so every XDG-compliant launcher (GNOME, KDE, and everything else that
// reads $XDG_DATA_HOME/applications) picks up real add/remove events there
// with no desktop-environment-specific cache-rebuild tool required.
func syncDesktopEntries(profileLink, destDir, stateFile string) error {
	desired, err := desktopEntriesIn(profileLink)
	if err != nil {
		return err
	}

	previous, err := loadDesktopEntryState(stateFile)
	if err != nil {
		return err
	}

	if err := os.MkdirAll(destDir, 0o755); err != nil {
		return fmt.Errorf("creating %s: %w", destDir, err)
	}

	for name, prevTarget := range previous {
		if _, stillWanted := desired[name]; stillWanted {
			continue
		}
		removeManagedLink(destDir, name, prevTarget)
	}

	for name, target := range desired {
		if previous[name] == target {
			continue
		}
		link := filepath.Join(destDir, name)
		// Removed and recreated, not just left alone, so the directory
		// itself sees an add/remove event even when only the *target*
		// changed (e.g. a package update) - see the doc comment above.
		_ = os.Remove(link)
		if err := os.Symlink(target, link); err != nil {
			return fmt.Errorf("linking desktop entry %q: %w", name, err)
		}
	}

	return saveDesktopEntryState(stateFile, desired)
}

// desktopEntriesIn returns every *.desktop file under
// profileLink/share/applications, keyed by file name and resolved to its
// underlying store path so the symlink nxf creates points directly into the
// store rather than through the profile symlink it might outlive.
func desktopEntriesIn(profileLink string) (map[string]string, error) {
	dir := filepath.Join(profileLink, "share", "applications")
	entries, err := os.ReadDir(dir)
	if os.IsNotExist(err) {
		return map[string]string{}, nil
	}
	if err != nil {
		return nil, fmt.Errorf("reading %s: %w", dir, err)
	}

	desired := map[string]string{}
	for _, entry := range entries {
		name := entry.Name()
		if entry.IsDir() || !strings.HasSuffix(name, ".desktop") {
			continue
		}
		resolved, err := filepath.EvalSymlinks(filepath.Join(dir, name))
		if err != nil {
			return nil, fmt.Errorf("resolving %s: %w", name, err)
		}
		desired[name] = resolved
	}
	return desired, nil
}

// removeManagedLink removes destDir/name only if it's still the symlink nxf
// created pointing at wantTarget - if the user replaced it with something
// else in the meantime, that's left alone.
func removeManagedLink(destDir, name, wantTarget string) {
	link := filepath.Join(destDir, name)
	if got, err := os.Readlink(link); err != nil || got != wantTarget {
		return
	}
	_ = os.Remove(link)
}

func loadDesktopEntryState(path string) (map[string]string, error) {
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return map[string]string{}, nil
	}
	if err != nil {
		return nil, fmt.Errorf("reading %s: %w", path, err)
	}
	var state map[string]string
	if err := json.Unmarshal(data, &state); err != nil {
		return nil, fmt.Errorf("parsing %s: %w", path, err)
	}
	return state, nil
}

func saveDesktopEntryState(path string, state map[string]string) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return err
	}
	return fsutil.WriteFileAtomic(path, data, 0o644)
}
