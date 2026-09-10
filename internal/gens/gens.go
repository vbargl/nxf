// Package gens lists, selects, and deletes nix profile generations (user
// profiles and the NixOS system profile share the same profile-N-link layout).
package gens

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"
)

// Generation is one profile-N-link (or system-N-link) snapshot.
type Generation struct {
	Number  int
	Time    time.Time
	Path    string // store path the link points at
	Link    string // the …/profile-N-link path
	Current bool
}

// JSONGeneration is one generation in --json output.
type JSONGeneration struct {
	Number        int    `json:"number"`
	Time          string `json:"time"`
	Current       bool   `json:"current"`
	NixosVersion  string `json:"nixosVersion,omitempty"`
	KernelVersion string `json:"kernelVersion,omitempty"`
}

// JSONList converts generations to JSON objects with RFC3339 UTC times.
// The input order (newest-first) is preserved. The returned slice is never nil.
func JSONList(gens []Generation) []JSONGeneration {
	out := make([]JSONGeneration, 0, len(gens))
	for _, g := range gens {
		out = append(out, JSONGeneration{
			Number:  g.Number,
			Time:    g.Time.UTC().Format(time.RFC3339),
			Current: g.Current,
		})
	}
	return out
}

var linkRe = regexp.MustCompile(`^(.*)-([0-9]+)-link$`)

// ResolveBase turns a profile symlink (~/.nix-profile, /nix/var/nix/profiles/system)
// into the directory that holds the numbered -N-link siblings and the basename
// prefix ("profile" or "system").
func ResolveBase(profileSymlink string) (dir, prefix string, err error) {
	abs, err := filepath.Abs(profileSymlink)
	if err != nil {
		return "", "", err
	}
	info, err := os.Lstat(abs)
	if err != nil {
		return "", "", err
	}
	// ~/.nix-profile -> …/profiles/profile -> profile-N-link -> store
	// Walk until we sit on a *-N-link, then the parent dir is the base.
	cur := abs
	for i := 0; i < 4; i++ {
		if info.Mode()&os.ModeSymlink == 0 {
			break
		}
		target, err := os.Readlink(cur)
		if err != nil {
			return "", "", err
		}
		if !filepath.IsAbs(target) {
			target = filepath.Join(filepath.Dir(cur), target)
		}
		base := filepath.Base(target)
		if m := linkRe.FindStringSubmatch(base); m != nil {
			return filepath.Dir(target), m[1], nil
		}
		cur = target
		info, err = os.Lstat(cur)
		if err != nil {
			return "", "", err
		}
	}
	// Already pointing at the profile base (…/profiles/profile) without
	// having seen a numbered link - list its directory using its basename.
	return filepath.Dir(cur), filepath.Base(cur), nil
}

// List returns generations newest-first.
func List(profileSymlink string) ([]Generation, error) {
	dir, prefix, err := ResolveBase(profileSymlink)
	if err != nil {
		return nil, err
	}

	currentNum := 0
	baseLink := filepath.Join(dir, prefix)
	if target, err := os.Readlink(baseLink); err == nil {
		if m := linkRe.FindStringSubmatch(filepath.Base(target)); m != nil {
			currentNum, _ = strconv.Atoi(m[2])
		}
	}

	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}
	var gens []Generation
	for _, e := range entries {
		name := e.Name()
		m := linkRe.FindStringSubmatch(name)
		if m == nil || m[1] != prefix {
			continue
		}
		n, _ := strconv.Atoi(m[2])
		link := filepath.Join(dir, name)
		store, _ := os.Readlink(link)
		if store != "" && !filepath.IsAbs(store) {
			store = filepath.Join(dir, store)
		}
		info, err := os.Lstat(link)
		mtime := time.Time{}
		if err == nil {
			mtime = info.ModTime()
		}
		gens = append(gens, Generation{
			Number:  n,
			Time:    mtime,
			Path:    store,
			Link:    link,
			Current: n == currentNum,
		})
	}
	sort.Slice(gens, func(i, j int) bool { return gens[i].Number > gens[j].Number })
	return gens, nil
}

// Spec selects generations to delete. Exactly one field should be set.
type Spec struct {
	Keep       int           // keep the N most recent (including current)
	KeepSince  time.Duration // keep gens with mtime >= now-KeepSince
	OlderThan  time.Duration // delete gens with mtime < now-OlderThan
	DeleteNums []int         // delete these numbers
}

// SelectDelete returns the generations Spec would remove. The current
// generation is never selected.
func SelectDelete(gens []Generation, spec Spec, now time.Time) ([]Generation, error) {
	set := 0
	if spec.Keep > 0 {
		set++
	}
	if spec.KeepSince > 0 {
		set++
	}
	if spec.OlderThan > 0 {
		set++
	}
	if len(spec.DeleteNums) > 0 {
		set++
	}
	if set != 1 {
		return nil, fmt.Errorf("specify exactly one of --keep, --keep-since, --older-than, or --delete")
	}

	var out []Generation
	switch {
	case spec.Keep > 0:
		// gens is newest-first; skip the first Keep (or all current-included).
		kept := 0
		for _, g := range gens {
			if g.Current {
				kept++
				continue
			}
			if kept < spec.Keep {
				kept++
				continue
			}
			out = append(out, g)
		}
	case spec.KeepSince > 0:
		cutoff := now.Add(-spec.KeepSince)
		for _, g := range gens {
			if g.Current {
				continue
			}
			if g.Time.Before(cutoff) {
				out = append(out, g)
			}
		}
	case spec.OlderThan > 0:
		cutoff := now.Add(-spec.OlderThan)
		for _, g := range gens {
			if g.Current {
				continue
			}
			if g.Time.Before(cutoff) {
				out = append(out, g)
			}
		}
	case len(spec.DeleteNums) > 0:
		want := map[int]bool{}
		for _, n := range spec.DeleteNums {
			want[n] = true
		}
		have := map[int]Generation{}
		for _, g := range gens {
			have[g.Number] = g
		}
		for _, n := range spec.DeleteNums {
			g, ok := have[n]
			if !ok {
				return nil, fmt.Errorf("generation %d does not exist", n)
			}
			if g.Current {
				return nil, fmt.Errorf("refusing to delete the current generation (%d)", n)
			}
			out = append(out, g)
		}
		_ = want
	}
	return out, nil
}

// ParseDuration accepts 3d, 1w, 2h, 30m (days/weeks/hours/minutes).
func ParseDuration(s string) (time.Duration, error) {
	s = strings.TrimSpace(s)
	if s == "" {
		return 0, fmt.Errorf("empty duration")
	}
	var n int
	var unit rune
	if _, err := fmt.Sscanf(s, "%d%c", &n, &unit); err != nil || n <= 0 {
		return 0, fmt.Errorf("invalid duration %q (want e.g. 3d, 1w, 2h, 30m)", s)
	}
	if extra := strings.TrimPrefix(s, strconv.Itoa(n)); len(extra) != 1 {
		return 0, fmt.Errorf("invalid duration %q (want e.g. 3d, 1w, 2h, 30m)", s)
	}
	switch unit {
	case 'm':
		return time.Duration(n) * time.Minute, nil
	case 'h':
		return time.Duration(n) * time.Hour, nil
	case 'd':
		return time.Duration(n) * 24 * time.Hour, nil
	case 'w':
		return time.Duration(n) * 7 * 24 * time.Hour, nil
	default:
		return 0, fmt.Errorf("invalid duration unit %q in %q (use m, h, d, or w)", string(unit), s)
	}
}

// execCommand is overridden in tests.
var execCommand = exec.Command

// Delete runs `nix-env --delete-generations` for the given numbers.
func Delete(profileSymlink string, numbers []int, sudo bool) error {
	if len(numbers) == 0 {
		return nil
	}
	args := []string{"-p", profileSymlink, "--delete-generations"}
	for _, n := range numbers {
		args = append(args, strconv.Itoa(n))
	}
	var cmd *exec.Cmd
	if sudo {
		cmd = execCommand("sudo", append([]string{"nix-env"}, args...)...)
	} else {
		cmd = execCommand("nix-env", args...)
	}
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}

// SwitchTo moves the profile to generation n (`nix-env --switch-generation`).
func SwitchTo(profileSymlink string, n int, sudo bool) error {
	args := []string{"-p", profileSymlink, "--switch-generation", strconv.Itoa(n)}
	var cmd *exec.Cmd
	if sudo {
		cmd = execCommand("sudo", append([]string{"nix-env"}, args...)...)
	} else {
		cmd = execCommand("nix-env", args...)
	}
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}

// CurrentNumber returns the live generation number, or 0 if unknown.
func CurrentNumber(gens []Generation) int {
	for _, g := range gens {
		if g.Current {
			return g.Number
		}
	}
	return 0
}
