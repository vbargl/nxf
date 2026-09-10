package profile

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/vbargl/nxf/internal/nixutil"
	"github.com/vbargl/nxf/internal/paths"
	"github.com/vbargl/nxf/internal/reconcile"
	"github.com/vbargl/nxf/internal/ui"
)

// List prints applied nxf profiles, one per line. verbose adds units,
// binaries, desktop files and other XDG paths.
func List(verbose bool, filter []string) error {
	link, err := paths.NixProfileLink()
	if err != nil {
		return err
	}
	manifests, err := reconcile.Discover(link)
	if err != nil {
		return err
	}
	if len(manifests) == 0 {
		fmt.Println("no nxf-aware profiles applied")
		return nil
	}

	want := map[string]bool{}
	for _, n := range filter {
		want[n] = true
	}

	elements, _ := nixutil.ListElements()
	var shown int
	for _, m := range manifests {
		if len(want) > 0 && !want[m.Name] {
			continue
		}
		shown++
		store := nixutil.StorePathFor(elements, m.Name)
		inv := inspect(store, m)
		if verbose {
			printVerbose(m, elements, inv)
			continue
		}
		printCompact(m, inv)
	}
	if len(want) > 0 && shown == 0 {
		return fmt.Errorf("no matching profiles")
	}
	return nil
}

type inventory struct {
	bins     []string
	desktop  []string
	xdg      []string
	priority string
	nixName  string
	flake    string
	store    string
}

func inspect(store string, m reconcile.Manifest) inventory {
	inv := inventory{store: store}
	if store == "" {
		return inv
	}
	inv.bins = listNames(filepath.Join(store, "bin"), "")
	inv.desktop = listNames(filepath.Join(store, "share", "applications"), ".desktop")
	for _, rel := range []string{
		"share/icons",
		"share/fonts",
		"share/mime",
		"share/dbus-1",
		"share/systemd/user",
		"etc/xdg",
	} {
		p := filepath.Join(store, filepath.FromSlash(rel))
		if info, err := os.Stat(p); err == nil && info.IsDir() {
			inv.xdg = append(inv.xdg, rel)
		}
	}
	return inv
}

func listNames(dir, suffix string) []string {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil
	}
	var out []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		n := e.Name()
		if suffix != "" && !strings.HasSuffix(n, suffix) {
			continue
		}
		out = append(out, n)
	}
	sort.Strings(out)
	return out
}

func printCompact(m reconcile.Manifest, inv inventory) {
	parts := []string{fmt.Sprintf("%s  %s", ui.Bullet(), m.Name)}
	if n := len(inv.bins); n > 0 {
		parts = append(parts, fmt.Sprintf("%d bins", n))
	}
	if n := len(m.Units); n > 0 {
		parts = append(parts, fmt.Sprintf("%d units", n))
	}
	if n := len(inv.desktop); n > 0 {
		parts = append(parts, fmt.Sprintf("%d desktop", n))
	}
	if n := len(inv.xdg); n > 0 {
		parts = append(parts, fmt.Sprintf("%d xdg", n))
	}
	if m.Activate != nil {
		parts = append(parts, "activate")
	}
	if m.Deactivate != nil {
		parts = append(parts, "deactivate")
	}
	fmt.Println(strings.Join(parts, "  "))
}

func printVerbose(m reconcile.Manifest, elements []nixutil.Element, inv inventory) {
	fmt.Printf("%s  %s\n", ui.Bullet(), m.Name)
	if e, ok := nixutil.FindElement(elements, m.Name); ok {
		fmt.Printf("     nix name   %s\n", e.Name)
		if e.OriginalURL != "" {
			fmt.Printf("     flake      %s\n", e.OriginalURL)
		}
		if e.LockedURL != "" {
			fmt.Printf("     locked     %s\n", e.LockedURL)
		}
		fmt.Printf("     priority   %d\n", e.Priority)
		if len(e.StorePaths) > 0 {
			fmt.Printf("     store      %s\n", e.StorePaths[0])
		}
	} else if inv.store != "" {
		fmt.Printf("     store      %s\n", inv.store)
	}
	if len(m.Units) > 0 {
		fmt.Printf("     units\n")
		for _, u := range m.UnitNames() {
			kind := "auto"
			if !m.AutoStart(u) {
				kind = "manual"
			}
			fmt.Printf("       %s (%s)\n", paths.UnitFileName(m.Name, u), kind)
		}
	}
	if m.Activate != nil {
		fmt.Printf("     activate   %s\n", *m.Activate)
	}
	if m.Deactivate != nil {
		fmt.Printf("     deactivate %s\n", *m.Deactivate)
	}
	if len(inv.bins) > 0 {
		fmt.Printf("     bins       %s\n", strings.Join(inv.bins, ", "))
	}
	if len(inv.desktop) > 0 {
		fmt.Printf("     desktop    %s\n", strings.Join(inv.desktop, ", "))
	}
	if len(inv.xdg) > 0 {
		fmt.Printf("     xdg        %s\n", strings.Join(inv.xdg, ", "))
	}
}
