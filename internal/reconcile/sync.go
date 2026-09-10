package reconcile

import (
	"fmt"
	"os"
	"os/exec"

	"github.com/vbargl/nxf/internal/paths"
)

// Run reconciles systemd user units and activation scripts against the set
// of profiles currently merged into the nix profile. For every unit and
// activation script it compares the desired nix store path against the path
// recorded the last time it was applied: an unchanged path means an
// unchanged derivation (Nix content-addresses the store), so it's skipped;
// a changed or new path is (re)applied.
func Run() error {
	profileLink, err := paths.NixProfileLink()
	if err != nil {
		return err
	}
	unitDir, err := paths.SystemdUserUnitDir()
	if err != nil {
		return err
	}
	stateDir, err := paths.AppliedStateDir()
	if err != nil {
		return err
	}
	desktopEntriesDir, err := paths.DesktopEntriesDir()
	if err != nil {
		return err
	}
	desktopStateFile, err := paths.DesktopEntriesStateFile()
	if err != nil {
		return err
	}

	desired, err := Discover(profileLink)
	if err != nil {
		return err
	}
	previous, err := loadAppliedStates(stateDir)
	if err != nil {
		return err
	}

	desiredByName := map[string]Manifest{}
	for _, m := range desired {
		if m.Name == "" {
			continue
		}
		desiredByName[m.Name] = m
	}

	// Removed profiles: tear down every unit they owned and run their
	// deactivation via a fresh apply is not needed (no deactivate hook in
	// the manifest today - only activation), then drop the state snapshot.
	for name, prev := range previous {
		if name == "" {
			continue
		}
		if _, stillPresent := desiredByName[name]; stillPresent {
			continue
		}
		fmt.Printf("nxf: profile %q removed, tearing down\n", name)
		for unit := range prev.Units {
			if err := removeUnit(unitDir, name, unit); err != nil {
				return err
			}
		}
		if err := removeAppliedState(stateDir, name); err != nil {
			return err
		}
	}

	for _, m := range desired {
		prev := previous[m.Name]

		for unit := range prev.Units {
			if _, stillWanted := m.Units[unit]; !stillWanted {
				if err := removeUnit(unitDir, m.Name, unit); err != nil {
					return err
				}
			}
		}

		for unit, storePath := range m.Units {
			prevPath, wasPresent := prev.Units[unit]
			if wasPresent && prevPath == storePath {
				continue
			}
			if err := installUnit(unitDir, m.Name, unit, storePath, wasPresent, m.autoStart(unit)); err != nil {
				return err
			}
		}

		if m.Activate != nil {
			if prev.Activate == nil || *prev.Activate != *m.Activate {
				fmt.Printf("nxf: running activation script for %q\n", m.Name)
				cmd := exec.Command(*m.Activate)
				cmd.Stdout = os.Stdout
				cmd.Stderr = os.Stderr
				cmd.Env = append(os.Environ(), "NXFP_PROFILE_NAME="+m.Name)
				if err := cmd.Run(); err != nil {
					return fmt.Errorf("activation script for %q failed: %w", m.Name, err)
				}
			}
		}

		if err := saveAppliedState(stateDir, m); err != nil {
			return err
		}
	}

	return syncDesktopEntries(profileLink, desktopEntriesDir, desktopStateFile)
}
