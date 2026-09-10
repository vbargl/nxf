package reconcile

import (
	"fmt"
	"os"
	"os/exec"

	"github.com/vbargl/nxf/internal/paths"
	"github.com/vbargl/nxf/internal/ui"
)

// Run reconciles systemd user units and activation/deactivation scripts
// against the set of profiles currently merged into the nix profile. For
// every unit and activation script it compares the desired nix store path
// against the path recorded the last time it was applied: an unchanged path
// means an unchanged derivation (Nix content-addresses the store), so it's
// skipped; a changed or new path is (re)applied. Removed profiles run their
// deactivate hook (if any) and then tear down every unit they owned.
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

	var firstErr error
	note := func(err error) {
		if err == nil {
			return
		}
		ui.Warn(err.Error())
		if firstErr == nil {
			firstErr = err
		}
	}

	for name, prev := range previous {
		if name == "" {
			continue
		}
		if _, stillPresent := desiredByName[name]; stillPresent {
			continue
		}
		fmt.Printf("nxf: profile %q removed, tearing down\n", name)
		if prev.Deactivate != nil {
			path := hookExecPath(profileLink, name, paths.DeactivationHookName, prev.Deactivate)
			note(runHook("deactivate", name, path))
		}
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
		prev.Units = normalizeUnitKeys(prev.Units)

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
			if err := installUnit(unitDir, m.Name, unit, storePath, wasPresent, m.AutoStart(unit)); err != nil {
				return err
			}
		}

		activateChanged := false
		if m.Activate != nil {
			activateChanged = prev.Activate == nil || *prev.Activate != *m.Activate
		} else if prev.Activate != nil {
			activateChanged = true
		}

		if activateChanged && prev.Deactivate != nil {
			path := hookExecPath(profileLink, m.Name, paths.DeactivationHookName, prev.Deactivate)
			note(runHook("deactivate", m.Name, path))
		}
		if m.Activate != nil && activateChanged {
			path := hookExecPath(profileLink, m.Name, paths.ActivationHookName, m.Activate)
			if err := runHook("activate", m.Name, path); err != nil {
				return err
			}
		}

		if err := saveAppliedState(stateDir, m); err != nil {
			return err
		}
	}

	if err := syncDesktopEntries(profileLink, desktopEntriesDir, desktopStateFile); err != nil {
		return err
	}
	return firstErr
}

func runHook(kind, profile, path string) error {
	fmt.Printf("nxf: running %s script for %q\n", kind, profile)
	cmd := exec.Command(path)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("%s script for %q failed: %w", kind, profile, err)
	}
	return nil
}

func hookExecPath(profileLink, name, hook string, stored *string) string {
	live := paths.HookFile(profileLink, name, hook)
	if _, err := os.Lstat(live); err == nil {
		return live
	}
	if stored != nil {
		return *stored
	}
	return ""
}
