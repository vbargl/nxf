// Package profile builds, adds, removes, upgrades, and lists user-level nix
// profiles - the logic behind `nxf profile`.
package profile

import (
	"fmt"
	"path/filepath"
	"sort"
	"strings"

	"github.com/vbargl/nxf/internal/apply"
	"github.com/vbargl/nxf/internal/gens"
	"github.com/vbargl/nxf/internal/names"
	"github.com/vbargl/nxf/internal/nixutil"
	"github.com/vbargl/nxf/internal/paths"
	"github.com/vbargl/nxf/internal/reconcile"
	"github.com/vbargl/nxf/internal/ui"
)

// Sync reconciles systemd user units and activation scripts for the
// currently applied profiles, without adding or removing anything.
func Sync() error {
	return withLock(reconcile.Run)
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

func currentGeneration() int {
	link, err := paths.NixProfileLink()
	if err != nil {
		return 0
	}
	list, err := gens.List(link)
	if err != nil {
		return 0
	}
	return gens.CurrentNumber(list)
}

func rollbackTo(n int) error {
	if n > 0 {
		return nixutil.ProfileRollbackTo(n)
	}
	if currentGeneration() > 0 {
		return nixutil.ProfileRollback()
	}
	return nil
}

// Build builds a profile from ref without installing it.
func Build(ref string) error {
	expanded, name, err := expandProfileRef(ref, nixutil.CurrentSystem)
	if err != nil {
		return err
	}
	if err := names.Check(name); err != nil {
		return err
	}

	ui.Phase("build " + expanded)
	oldPath := currentProfilePath()
	newPath, err := nixutil.Build(expanded, false)
	if err != nil {
		return err
	}
	ui.OK("build")
	return nixutil.ShowDiff(oldPath, newPath)
}

type planned struct {
	name     string
	expanded string
	oldPath  string
	newPath  string
	priority *int
}

func (p planned) unchanged() bool {
	return p.oldPath != "" && p.oldPath == p.newPath
}

type parsedRef struct {
	flake    string
	name     string
	expanded string
}

// Add builds and installs every ref as one transaction: all builds first
// (nix-fast-build when a flake contributes 2+ profiles), then a plan, then
// install. Unchanged store paths are skipped. A failed install rolls the
// nix profile back to the generation taken at the start of activate.
func Add(refs []string, opts apply.Options) error {
	return withLock(func() error { return addLocked(refs, opts) })
}

func addLocked(refs []string, opts apply.Options) error {
	parsed, err := resolveAddRefs(refs)
	if err != nil {
		return err
	}

	ui.Phase("evaluate / build")
	plans, err := buildParsed(parsed, opts)
	if err != nil {
		return err
	}
	ui.OK("evaluate / build")

	if err := showPlan(plans); err != nil {
		ui.Warn(err.Error())
	}

	if opts.DryRun {
		fmt.Println("Dry run: not applying.")
		return nil
	}

	any := false
	for _, p := range plans {
		if !p.unchanged() {
			any = true
			break
		}
	}
	if !any {
		return nil
	}

	ui.Phase("activate")
	snap := currentGeneration()
	if err := installPlans(plans); err != nil {
		if rb := rollbackTo(snap); rb != nil {
			ui.Warn("rolling back after failed add: " + rb.Error())
		}
		return err
	}
	if err := reconcile.Run(); err != nil {
		if rb := rollbackTo(snap); rb != nil {
			ui.Warn("rolling back after failed reconcile: " + rb.Error())
		} else if snap > 0 {
			_ = reconcile.Run()
		}
		return err
	}
	ui.OK("activate")
	return nil
}

func resolveAddRefs(refs []string) ([]parsedRef, error) {
	stored, err := loadRefs()
	if err != nil {
		stored = map[string]Ref{}
	}
	out := make([]parsedRef, 0, len(refs))
	for _, ref := range refs {
		if !strings.Contains(ref, "#") {
			if err := names.Check(ref); err != nil {
				return nil, err
			}
			installable, err := upgradeInstallable(ref, stored[ref])
			if err != nil {
				return nil, fmt.Errorf("%w\n  hint: nxf profile add <flake>#%s", err, ref)
			}
			ref = installable
		}
		expanded, name, err := expandProfileRef(ref, nixutil.CurrentSystem)
		if err != nil {
			return nil, err
		}
		if err := names.Check(name); err != nil {
			return nil, err
		}
		flake, _, _ := strings.Cut(expanded, "#")
		out = append(out, parsedRef{flake: flake, name: name, expanded: expanded})
	}
	return out, nil
}

func groupByFlake(refs []parsedRef) [][]parsedRef {
	idx := map[string]int{}
	var groups [][]parsedRef
	for _, r := range refs {
		i, ok := idx[r.flake]
		if !ok {
			i = len(groups)
			idx[r.flake] = i
			groups = append(groups, nil)
		}
		groups[i] = append(groups[i], r)
	}
	return groups
}

func buildParsed(refs []parsedRef, opts apply.Options) ([]planned, error) {
	elements, _ := nixutil.ListElements()
	var plans []planned
	for _, group := range groupByFlake(refs) {
		system, err := nixutil.CurrentSystem()
		if err != nil {
			return nil, err
		}
		if len(group) == 1 {
			g := group[0]
			newPath, used, err := nixutil.BuildResolved(g.expanded, opts.Refresh)
			if err != nil {
				return nil, err
			}
			plans = append(plans, planned{
				name:     g.name,
				expanded: used,
				oldPath:  nixutil.StorePathFor(elements, g.name),
				newPath:  newPath,
				priority: opts.Priority,
			})
			continue
		}
		ns := make([]string, len(group))
		for i, g := range group {
			ns[i] = g.name
		}
		pathsByName, err := nixutil.BuildMany(group[0].flake, system, ns, opts.Refresh)
		if err != nil {
			return nil, err
		}
		for _, g := range group {
			expanded := g.expanded
			newPath := pathsByName[g.name]
			if newPath == "" {
				var err error
				newPath, expanded, err = nixutil.BuildResolved(g.expanded, opts.Refresh)
				if err != nil {
					return nil, err
				}
			}
			plans = append(plans, planned{
				name:     g.name,
				expanded: expanded,
				oldPath:  nixutil.StorePathFor(elements, g.name),
				newPath:  newPath,
				priority: opts.Priority,
			})
		}
	}
	return plans, nil
}

func showPlan(plans []planned) error {
	ui.Phase("plan")
	w := 0
	for _, p := range plans {
		if n := len(p.name); n > w {
			w = n
		}
	}
	for _, p := range plans {
		diff, err := nixutil.Diff(p.oldPath, p.newPath)
		if err != nil {
			ui.Warn(p.name + ": " + err.Error())
			continue
		}
		fmt.Print(formatPlanEntry(p.name, w, diff))
	}
	ui.OK("plan")
	return nil
}

func formatPlanEntry(name string, nameWidth int, diff string) string {
	diff = strings.TrimRight(diff, "\n")
	if diff == "" {
		return fmt.Sprintf("%-*s  no changes\n", nameWidth, name)
	}
	var b strings.Builder
	fmt.Fprintf(&b, "%s\n", name)
	for _, line := range strings.Split(diff, "\n") {
		fmt.Fprintf(&b, "  %s\n", line)
	}
	return b.String()
}

func installPlans(plans []planned) error {
	elements, _ := nixutil.ListElements()
	for _, p := range plans {
		if p.unchanged() {
			continue
		}
		prio := p.priority
		if prio == nil {
			if e, ok := nixutil.FindElement(elements, p.name); ok && e.Priority > 0 {
				// keep existing priority on replace unless the caller set one
				n := e.Priority
				prio = &n
			} else if m, err := manifestFor(p.newPath, p.name); err == nil && m.Priority != nil {
				prio = m.Priority
			}
		}
		if e, ok := nixutil.FindElement(elements, p.name); ok {
			nixutil.ProfileRemoveQuiet(e.Name)
		}
		if err := nixutil.ProfileAdd(p.newPath, prio); err != nil {
			return fmt.Errorf("nix profile add %s: %w", p.name, err)
		}
		rememberAfterAdd(p.name, p.expanded)
		if els, err := nixutil.ListElements(); err == nil {
			elements = els
		}
	}
	return nil
}

func manifestFor(store, name string) (reconcile.Manifest, error) {
	ms, err := reconcile.Discover(store)
	if err != nil {
		return reconcile.Manifest{}, err
	}
	for _, m := range ms {
		if m.Name == name {
			return m, nil
		}
	}
	if len(ms) == 1 {
		return ms[0], nil
	}
	return reconcile.Manifest{}, fmt.Errorf("no manifest named %q in %s", name, store)
}

func splitConvenienceRef(ref string) (flake, name string, ok bool) {
	flake, fragment, found := strings.Cut(ref, "#")
	if !found || strings.HasPrefix(fragment, "profileConfigurations.") {
		return "", "", false
	}
	return flake, fragment, true
}

// Remove removes every name, failing if any name is not actually installed.
func Remove(profileNames []string, opts apply.Options) error {
	return withLock(func() error { return removeLocked(profileNames, opts) })
}

func removeLocked(profileNames []string, opts apply.Options) error {
	for _, name := range profileNames {
		if err := names.Check(name); err != nil {
			return err
		}
	}
	elements, err := nixutil.ListElements()
	if err != nil {
		return err
	}

	type hit struct {
		name    string
		element nixutil.Element
	}
	var hits []hit
	for _, name := range profileNames {
		e, ok := nixutil.FindElement(elements, name)
		if !ok {
			return fmt.Errorf("profile %q is not installed\n  hint: nxf profile list", name)
		}
		hits = append(hits, hit{name: name, element: e})
	}

	ui.Phase("plan")
	oldPath := currentProfilePath()
	for _, h := range hits {
		fmt.Printf("  %s  remove %s  (nix element %s)\n", ui.Bullet(), h.name, h.element.Name)
	}
	ui.OK("plan")

	if opts.DryRun {
		fmt.Println("Dry run: not applying.")
		return nil
	}
	if err := opts.Confirm("remove these profiles"); err != nil {
		return err
	}

	ui.Phase("activate")
	snap := currentGeneration()
	for _, h := range hits {
		if err := nixutil.ProfileRemove(h.element.Name); err != nil {
			if rb := rollbackTo(snap); rb != nil {
				ui.Warn("rolling back after failed remove: " + rb.Error())
			}
			return fmt.Errorf("nix profile remove %s: %w", h.name, err)
		}
		forgetRef(h.name)
	}
	showDiffOrWarn(oldPath, currentProfilePath())
	if err := reconcile.Run(); err != nil {
		return err
	}
	ui.OK("activate")
	return nil
}

// Upgrade rebuilds already-applied profiles from the resolved flake URL nxf
// recorded at add time (or recovered from `nix profile list --json`).
func Upgrade(profileNames []string, all bool, opts apply.Options) error {
	return withLock(func() error { return upgradeLocked(profileNames, all, opts) })
}

func upgradeLocked(profileNames []string, all bool, opts apply.Options) error {
	if all == (len(profileNames) > 0) {
		return fmt.Errorf("specify either --all or one or more profile names, not both/neither")
	}

	stored, err := loadRefs()
	if err != nil {
		return err
	}
	elements, _ := nixutil.ListElements()

	targets := profileNames
	if all {
		seen := map[string]bool{}
		for name := range stored {
			targets = append(targets, name)
			seen[name] = true
		}
		if link, err := paths.NixProfileLink(); err == nil {
			if ms, err := reconcile.Discover(link); err == nil {
				for _, m := range ms {
					if !seen[m.Name] {
						targets = append(targets, m.Name)
					}
				}
			}
		}
		sort.Strings(targets)
	}
	if len(targets) == 0 {
		fmt.Println("nxf: no profiles to upgrade")
		return nil
	}

	var refs []string
	for _, name := range targets {
		if err := names.Check(name); err != nil {
			return err
		}
		r, ok := stored[name]
		if !ok {
			if e, found := nixutil.FindElement(elements, name); found && e.OriginalURL != "" {
				r = Ref{OriginalURL: e.OriginalURL, LockedURL: e.LockedURL, AttrPath: e.AttrPath}
			}
		}
		installable, err := upgradeInstallable(name, r)
		if err != nil {
			return err
		}
		refs = append(refs, installable)
	}
	return addLocked(refs, opts)
}

func showDiffOrWarn(oldPath, newPath string) {
	if err := nixutil.ShowDiff(oldPath, newPath); err != nil {
		ui.Warn(err.Error())
	}
}

// expandProfileRef turns a convenience reference like "<flake>#<name>" into
// the full "<flake>#profileConfigurations.<system>.<name>" flake output
// path, so callers don't have to spell out the current system, and also
// returns name on its own (needed by Add).
//
// name is quoted as its own attribute-path segment (...system."name") since
// nxf's profile names are themselves dot-joined (e.g. "gui.daily") - the nix
// CLI's attribute-path syntax splits on every unquoted "." .
func expandProfileRef(ref string, currentSystem func() (string, error)) (expanded, name string, err error) {
	flake, fragment, found := strings.Cut(ref, "#")
	if !found {
		return "", "", fmt.Errorf("expected a flake reference of the form <flake>#<profile>, got %q", ref)
	}
	if strings.HasPrefix(fragment, "profileConfigurations.") {
		name = fragment
		if strings.HasSuffix(fragment, `"`) {
			if idx := strings.LastIndex(fragment[:len(fragment)-1], `"`); idx != -1 {
				name = fragment[idx+1 : len(fragment)-1]
			}
		} else if idx := strings.LastIndex(fragment, "."); idx != -1 {
			name = fragment[idx+1:]
			// Unquoted nested path: take everything after the system segment
			// (profileConfigurations.<system>.<rest>) so gui.daily stays intact.
			rest := strings.TrimPrefix(fragment, "profileConfigurations.")
			if i := strings.Index(rest, "."); i != -1 {
				name = rest[i+1:]
			}
		}
		return ref, name, nil
	}

	system, err := currentSystem()
	if err != nil {
		return "", "", err
	}
	return fmt.Sprintf("%s#profileConfigurations.%s.%q", flake, system, fragment), fragment, nil
}
