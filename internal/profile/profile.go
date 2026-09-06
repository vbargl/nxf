// Package profile builds, adds, removes, upgrades, and lists user-level nix
// profiles - the logic behind `nxf profile`.
package profile

import (
	"fmt"
	"path/filepath"
	"sort"
	"strings"

	"github.com/vbargl/nxf/internal/nixutil"
	"github.com/vbargl/nxf/internal/paths"
	"github.com/vbargl/nxf/internal/reconcile"
)

// Sync reconciles systemd user units and activation scripts for the
// currently applied profiles, without adding or removing anything.
func Sync() error {
	return reconcile.Run()
}

// List prints every nxf-aware profile currently applied.
func List() error {
	profileLink, err := paths.NixProfileLink()
	if err != nil {
		return err
	}
	manifests, err := reconcile.Discover(profileLink)
	if err != nil {
		return err
	}
	if len(manifests) == 0 {
		fmt.Println("no nxf-aware profiles applied")
		return nil
	}
	for _, m := range manifests {
		fmt.Printf("%s\n", m.Name)
		for unit, path := range m.Units {
			fmt.Printf("  unit %s -> %s\n", unit, path)
		}
		if m.Activate != nil {
			fmt.Printf("  activate -> %s\n", *m.Activate)
		}
	}
	return nil
}

// currentProfilePath resolves ~/.nix-profile to its current store path, or
// "" if it doesn't exist yet (e.g. nothing has ever been added).
func currentProfilePath() string {
	link, err := paths.NixProfileLink()
	if err != nil {
		return ""
	}
	resolved, err := filepath.EvalSymlinks(link)
	if err != nil {
		return ""
	}
	return resolved
}

// Build builds a profile from ref without installing it.
func Build(ref string) error {
	expanded, _, err := expandProfileRef(ref, nixutil.CurrentSystem)
	if err != nil {
		return err
	}

	oldPath := currentProfilePath()

	newPath, err := nixutil.Build(expanded, false)
	if err != nil {
		return err
	}

	return nixutil.ShowDiff(oldPath, newPath)
}

// Add builds and installs every ref in refs, then shows a single combined
// diff and runs sync once - not once per ref - so adding several profiles in
// one invocation reads as one atomic change instead of N separate ones.
//
// Convenience refs (plain "<flake>#<name>", the common case) that share a
// flake are batched into one nix-fast-build invocation per flake via
// addBatch, so profiles sharing dependencies get evaluated and built
// concurrently instead of once per sequential `nix build`. A lone ref per
// flake, or a ref that's already a fully-qualified
// "<flake>#profileConfigurations.<system>...." path (which might target a
// system other than the current one, so it can't be safely batched), falls
// back to the plain per-ref path in addSingle.
func Add(refs []string) error {
	oldPath := currentProfilePath()

	var flakeOrder []string
	grouped := map[string][]string{}
	var singles []string

	for _, ref := range refs {
		flake, name, ok := splitConvenienceRef(ref)
		if !ok {
			singles = append(singles, ref)
			continue
		}
		if _, seen := grouped[flake]; !seen {
			flakeOrder = append(flakeOrder, flake)
		}
		grouped[flake] = append(grouped[flake], name)
	}

	for _, flake := range flakeOrder {
		names := grouped[flake]
		if len(names) == 1 {
			singles = append(singles, flake+"#"+names[0])
			continue
		}
		if err := addBatch(flake, names, false); err != nil {
			return err
		}
	}

	for _, ref := range singles {
		if err := addSingle(ref, false); err != nil {
			return err
		}
	}

	if err := nixutil.ShowDiff(oldPath, currentProfilePath()); err != nil {
		return err
	}

	return reconcile.Run()
}

// splitConvenienceRef splits a plain "<flake>#<name>" ref (the common case)
// into its flake and name. ok is false for a ref that's missing "#" or is
// already a fully-qualified "profileConfigurations...." path - the latter may
// target an explicit system other than the current one, so batching it
// alongside other refs in addBatch (which builds everything for one shared
// system) wouldn't necessarily be correct.
func splitConvenienceRef(ref string) (flake, name string, ok bool) {
	flake, fragment, found := strings.Cut(ref, "#")
	if !found || strings.HasPrefix(fragment, "profileConfigurations.") {
		return "", "", false
	}
	return flake, fragment, true
}

// addBatch builds every name under <flake>#profileConfigurations.<system> in
// one nix-fast-build invocation (see nixutil.BuildMany) and installs each
// resulting store path. refresh is passed straight through to BuildMany (see
// Upgrade).
func addBatch(flake string, names []string, refresh bool) error {
	system, err := nixutil.CurrentSystem()
	if err != nil {
		return err
	}
	built, err := nixutil.BuildMany(flake, system, names, refresh)
	if err != nil {
		return err
	}
	for _, name := range names {
		// See addSingle for why this is cleared before re-adding.
		nixutil.ProfileRemoveQuiet(profileElementName(name))
		if err := nixutil.ProfileAdd(built[name]); err != nil {
			return fmt.Errorf("nix profile add %s: %w", built[name], err)
		}
		rememberRef(name, flake)
	}
	return nil
}

// addSingle builds and installs a single ref, the same way Add always used
// to before batching existed - nom's tree view only makes sense for a single
// build. refresh is passed straight through to nixutil.Build (see Upgrade).
func addSingle(ref string, refresh bool) error {
	expanded, name, err := expandProfileRef(ref, nixutil.CurrentSystem)
	if err != nil {
		return err
	}

	built, err := nixutil.Build(expanded, refresh)
	if err != nil {
		return err
	}

	// `nix profile add` never replaces an existing element - re-adding a
	// name that's already installed (e.g. re-running add after a rebuild)
	// just appends a second element disambiguated as "profile-<name>-1",
	// and a later `nxf profile remove <name>` then only clears one of the
	// two duplicates. Clear out any prior element for this name first so
	// add is idempotent: same effect whether this is the first install or
	// a re-install. Silent/best-effort since "nothing to remove yet" is
	// the common case (first-ever add) and isn't an error.
	nixutil.ProfileRemoveQuiet(profileElementName(name))

	if err := nixutil.ProfileAdd(built); err != nil {
		return fmt.Errorf("nix profile add %s: %w", built, err)
	}

	flake, _, _ := splitConvenienceRef(ref)
	if flake != "" {
		rememberRef(name, flake)
	}
	return nil
}

// profileElementName maps nxf's short profile name (what `nxf profile list`
// prints, and what a user types) to the identifier `nix profile remove`
// actually matches against - the profile derivation's own name, which
// lib/mkProfile.nix always sets to "profile-<name>".
func profileElementName(name string) string {
	return "profile-" + name
}

// Remove removes every name in names, then shows a single combined diff and
// runs sync once - see Add.
func Remove(names []string) error {
	oldPath := currentProfilePath()

	for _, name := range names {
		if err := nixutil.ProfileRemove(profileElementName(name)); err != nil {
			return fmt.Errorf("nix profile remove %s: %w", name, err)
		}
		forgetRef(name)
	}

	if err := nixutil.ShowDiff(oldPath, currentProfilePath()); err != nil {
		return err
	}

	return reconcile.Run()
}

// Upgrade rebuilds and reinstalls already-applied profiles from the flake
// ref nxf recorded when each was added (see rememberRef) - the same
// build+install path as Add (batched per flake via addBatch, same as
// addBatch/addSingle), just resolving refs from recorded state instead of
// the command line. names selects specific profiles; all selects every
// profile nxf has a recorded ref for - exactly one of the two must be given.
func Upgrade(names []string, all, refresh bool) error {
	if all == (len(names) > 0) {
		return fmt.Errorf("specify either --all or one or more profile names, not both/neither")
	}

	refs, err := loadRefs()
	if err != nil {
		return err
	}

	targets := names
	if all {
		targets = make([]string, 0, len(refs))
		for name := range refs {
			targets = append(targets, name)
		}
		sort.Strings(targets)
	}
	if len(targets) == 0 {
		fmt.Println("nxf: no profiles to upgrade")
		return nil
	}

	var flakeOrder []string
	grouped := map[string][]string{}
	for _, name := range targets {
		flake, ok := refs[name]
		if !ok {
			return fmt.Errorf("profile %q has no recorded flake ref (not installed by this version of nxf, or already removed) - add it first", name)
		}
		if _, seen := grouped[flake]; !seen {
			flakeOrder = append(flakeOrder, flake)
		}
		grouped[flake] = append(grouped[flake], name)
	}

	oldPath := currentProfilePath()

	for _, flake := range flakeOrder {
		names := grouped[flake]
		if len(names) == 1 {
			if err := addSingle(flake+"#"+names[0], refresh); err != nil {
				return err
			}
			continue
		}
		if err := addBatch(flake, names, refresh); err != nil {
			return err
		}
	}

	if err := nixutil.ShowDiff(oldPath, currentProfilePath()); err != nil {
		return err
	}

	return reconcile.Run()
}

// expandProfileRef turns a convenience reference like "<flake>#<name>" into
// the full "<flake>#profileConfigurations.<system>.<name>" flake output
// path, so callers don't have to spell out the current system, and also
// returns name on its own (needed by Add for profileElementName).
//
// name is quoted as its own attribute-path segment (...system."name") since
// nxf's profile names are themselves dot-joined (e.g. "dev.default", see
// flake.nix's nameFor) - the nix CLI's attribute-path syntax splits on every
// unquoted ".", so passing it through unquoted would make nix look for a
// nested "dev"."default" attribute pair instead of the single flat
// "dev.default" key that actually exists.
//
// A fragment that already starts with "profileConfigurations." is passed
// through unchanged; name is extracted from its trailing segment, unquoting
// it first if it's already quoted.
func expandProfileRef(ref string, currentSystem func() (string, error)) (expanded, name string, err error) {
	flake, fragment, found := strings.Cut(ref, "#")
	if !found {
		return "", "", fmt.Errorf("expected a flake reference of the form <flake>#<profile>, got %q", ref)
	}
	if strings.HasPrefix(fragment, "profileConfigurations.") {
		name = fragment
		if strings.HasSuffix(fragment, `"`) {
			// Trailing segment is already quoted (e.g. ...x86_64-linux."dev.default") -
			// take the content between the matching quotes verbatim, since a
			// naive last-dot split would land inside the quoted name itself
			// whenever the name contains a dot.
			if idx := strings.LastIndex(fragment[:len(fragment)-1], `"`); idx != -1 {
				name = fragment[idx+1 : len(fragment)-1]
			}
		} else if idx := strings.LastIndex(fragment, "."); idx != -1 {
			name = fragment[idx+1:]
		}
		return ref, name, nil
	}

	system, err := currentSystem()
	if err != nil {
		return "", "", err
	}
	return fmt.Sprintf("%s#profileConfigurations.%s.%q", flake, system, fragment), fragment, nil
}
