package profile

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"text/tabwriter"

	"github.com/vbargl/nxf/internal/nixutil"
	"github.com/vbargl/nxf/internal/paths"
	"github.com/vbargl/nxf/internal/reconcile"
)

var listElements = nixutil.ListElements

// List prints applied nxf profiles, one per line. verbose adds units,
// binaries, desktop files and other XDG paths. asJSON prints the full
// inventory as a single JSON document instead.
func List(verbose, asJSON bool, filter []string) error {
	infos, err := collectProfiles(filter)
	if err != nil {
		return err
	}
	if asJSON {
		return printJSON(profilesJSON{Profiles: infos})
	}
	if len(infos) == 0 {
		fmt.Println("no nxf-aware profiles applied")
		return nil
	}
	if verbose {
		fmt.Print(verboseList(infos))
		return nil
	}
	fmt.Print(compactTable(infos))
	return nil
}

type profilesJSON struct {
	Profiles []profileInfo `json:"profiles"`
}

type unitInfo struct {
	Name      string `json:"name"`
	AutoStart bool   `json:"autoStart"`
}

type profileInfo struct {
	Name         string     `json:"name"`
	NixName      string     `json:"nixName"`
	Flake        string     `json:"flake"`
	Locked       string     `json:"locked"`
	Priority     int        `json:"priority"`
	Store        string     `json:"store"`
	Units        []unitInfo `json:"units"`
	Bins         []string   `json:"bins"`
	Desktop      []string   `json:"desktop"`
	XDG          []string   `json:"xdg"`
	Activate     *string    `json:"activate"`
	Deactivate   *string    `json:"deactivate"`
	foundElement bool       `json:"-"`
}

func collectProfiles(filter []string) ([]profileInfo, error) {
	link, err := paths.NixProfileLink()
	if err != nil {
		return nil, err
	}
	manifests, err := reconcile.Discover(link)
	if err != nil {
		return nil, err
	}
	elements, _ := listElements()
	return profilesFrom(manifests, elements, filter)
}

func profilesFrom(manifests []reconcile.Manifest, elements []nixutil.Element, filter []string) ([]profileInfo, error) {
	all := make([]profileInfo, 0, len(manifests))
	for _, m := range manifests {
		all = append(all, profileFrom(m, elements))
	}
	return filterProfiles(all, filter)
}

// filterProfiles keeps exact names and dotted prefixes (gui → gui.daily),
// in the order the filter terms were given.
func filterProfiles(infos []profileInfo, filter []string) ([]profileInfo, error) {
	if len(filter) == 0 {
		return infos, nil
	}
	byName := make(map[string]profileInfo, len(infos))
	for _, p := range infos {
		byName[p.Name] = p
	}
	var out []profileInfo
	seen := map[string]bool{}
	var missing []string
	for _, term := range filter {
		if p, ok := byName[term]; ok {
			if !seen[p.Name] {
				out = append(out, p)
				seen[p.Name] = true
			}
			continue
		}
		matched := false
		for _, p := range infos {
			if !strings.HasPrefix(p.Name, term+".") {
				continue
			}
			matched = true
			if seen[p.Name] {
				continue
			}
			out = append(out, p)
			seen[p.Name] = true
		}
		if !matched {
			missing = append(missing, term)
		}
	}
	if len(missing) > 0 {
		return nil, fmt.Errorf("unknown profile: %s", strings.Join(missing, ", "))
	}
	return out, nil
}

func profileFrom(m reconcile.Manifest, elements []nixutil.Element) profileInfo {
	store := nixutil.StorePathFor(elements, m.Name)
	inv := inspect(store, m)
	p := profileInfo{
		Name:       m.Name,
		Store:      inv.store,
		Units:      unitInfos(m),
		Bins:       emptySlice(inv.bins),
		Desktop:    emptySlice(inv.desktop),
		XDG:        emptySlice(inv.xdg),
		Activate:   m.Activate,
		Deactivate: m.Deactivate,
	}
	if e, ok := nixutil.FindElement(elements, m.Name); ok {
		p.foundElement = true
		p.NixName = e.Name
		p.Flake = e.OriginalURL
		p.Locked = e.LockedURL
		p.Priority = e.Priority
		if len(e.StorePaths) > 0 {
			p.Store = e.StorePaths[0]
		}
	}
	return p
}

func unitInfos(m reconcile.Manifest) []unitInfo {
	names := m.UnitNames()
	out := make([]unitInfo, 0, len(names))
	for _, u := range names {
		out = append(out, unitInfo{
			Name:      paths.UnitFileName(m.Name, u),
			AutoStart: m.AutoStart(u),
		})
	}
	return out
}

func emptySlice[T any](s []T) []T {
	if s == nil {
		return []T{}
	}
	return s
}

func printJSON(v any) error {
	b, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	fmt.Println(string(b))
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

const defaultPriority = 5

func compactTable(infos []profileInfo) string {
	showHooks := false
	for _, p := range infos {
		if p.Activate != nil || p.Deactivate != nil {
			showHooks = true
			break
		}
	}
	var b strings.Builder
	w := tabwriter.NewWriter(&b, 0, 8, 2, ' ', 0)
	if showHooks {
		fmt.Fprintln(w, "NAME\tBINS\tUNITS\tDESKTOP\tXDG\tHOOKS")
	} else {
		fmt.Fprintln(w, "NAME\tBINS\tUNITS\tDESKTOP\tXDG")
	}
	for _, p := range infos {
		row := fmt.Sprintf("%s\t%4d\t%5d\t%7d\t%3d", p.Name, len(p.Bins), len(p.Units), len(p.Desktop), len(p.XDG))
		if showHooks {
			row += "\t" + hookCell(p)
		}
		fmt.Fprintln(w, row)
	}
	_ = w.Flush()
	return b.String()
}

func hookCell(p profileInfo) string {
	var parts []string
	if p.Activate != nil {
		parts = append(parts, "activate")
	}
	if p.Deactivate != nil {
		parts = append(parts, "deactivate")
	}
	if len(parts) == 0 {
		return "-"
	}
	return strings.Join(parts, ",")
}

func verboseList(infos []profileInfo) string {
	nameWidth := 0
	for _, p := range infos {
		if n := len(p.Name); n > nameWidth {
			nameWidth = n
		}
	}
	var b strings.Builder
	for i, p := range infos {
		if i > 0 {
			b.WriteByte('\n')
		}
		b.WriteString(verboseBlock(p, nameWidth))
	}
	return b.String()
}

func verboseBlock(p profileInfo, nameWidth int) string {
	var b strings.Builder
	meta := verboseMeta(p)
	if meta == "" {
		fmt.Fprintf(&b, "%s\n", p.Name)
	} else {
		fmt.Fprintf(&b, "%-*s    %s\n", nameWidth, p.Name, meta)
	}
	if p.Store != "" {
		appendSection(&b, "store", []string{p.Store})
	}
	appendSection(&b, "bins", visibleBins(p.Bins))
	appendSection(&b, "desktop", shortDesktop(p.Desktop))
	appendSection(&b, "xdg", shortXDG(p.XDG))
	appendSection(&b, "units", unitLabels(p.Units))
	if p.Activate != nil {
		appendSection(&b, "activate", []string{*p.Activate})
	}
	if p.Deactivate != nil {
		appendSection(&b, "deactivate", []string{*p.Deactivate})
	}
	return b.String()
}

func verboseMeta(p profileInfo) string {
	var parts []string
	if p.Flake != "" {
		s := p.Flake
		if rev := shortRev(p.Locked); rev != "" {
			s += " (" + rev + ")"
		}
		parts = append(parts, s)
	}
	if p.Priority != 0 && p.Priority != defaultPriority {
		parts = append(parts, fmt.Sprintf("prio %d", p.Priority))
	}
	return strings.Join(parts, "  ")
}

func shortRev(locked string) string {
	const key = "rev="
	i := strings.Index(locked, key)
	if i < 0 {
		return ""
	}
	rev := locked[i+len(key):]
	if j := strings.IndexAny(rev, "&?"); j >= 0 {
		rev = rev[:j]
	}
	if len(rev) > 7 {
		return rev[:7]
	}
	return rev
}

func visibleBins(bins []string) []string {
	out := make([]string, 0, len(bins))
	for _, b := range bins {
		if strings.HasPrefix(b, ".") {
			continue
		}
		out = append(out, b)
	}
	return out
}

func shortDesktop(names []string) []string {
	out := make([]string, len(names))
	for i, n := range names {
		out[i] = strings.TrimSuffix(n, ".desktop")
	}
	return out
}

func shortXDG(rels []string) []string {
	out := make([]string, len(rels))
	for i, r := range rels {
		r = strings.TrimPrefix(r, "share/")
		r = strings.TrimPrefix(r, "etc/")
		out[i] = r
	}
	return out
}

func unitLabels(units []unitInfo) []string {
	out := make([]string, 0, len(units))
	for _, u := range units {
		if u.AutoStart {
			out = append(out, u.Name)
			continue
		}
		out = append(out, u.Name+" (manual)")
	}
	return out
}

const (
	sectionIndent = "  "
	sectionLabel  = 10
	sectionCols   = 2
	sectionColW   = 36
)

func appendSection(b *strings.Builder, label string, items []string) {
	if len(items) == 0 {
		return
	}
	clip := len(items) > 1
	prefix := sectionIndent + padRight(label, sectionLabel)
	cont := strings.Repeat(" ", len(sectionIndent)+sectionLabel)
	for i := 0; i < len(items); i += sectionCols {
		end := i + sectionCols
		if end > len(items) {
			end = len(items)
		}
		cells := make([]string, 0, end-i)
		row := items[i:end]
		for j, it := range row {
			cell := it
			if clip && len([]rune(it)) > sectionColW {
				cell = fit(it, sectionColW)
			}
			if j < len(row)-1 {
				cells = append(cells, padRight(cell, sectionColW))
			} else {
				cells = append(cells, cell)
			}
		}
		head := prefix
		if i > 0 {
			head = cont
		}
		b.WriteString(head)
		b.WriteString(strings.Join(cells, "  "))
		b.WriteByte('\n')
	}
}

func fit(s string, n int) string {
	r := []rune(s)
	if n <= 0 || len(r) <= n {
		return s
	}
	if n == 1 {
		return "…"
	}
	return string(r[:n-1]) + "…"
}

func padRight(s string, n int) string {
	if len(s) >= n {
		return s
	}
	return s + strings.Repeat(" ", n-len(s))
}
