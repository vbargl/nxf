// Package os builds and activates the NixOS system configuration via
// nixos-rebuild - the logic behind `nxf os`.
package os

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/vbargl/nxf/internal/apply"
	"github.com/vbargl/nxf/internal/gens"
	"github.com/vbargl/nxf/internal/nixutil"
	"github.com/vbargl/nxf/internal/ui"
)

var systemProfile = "/nix/var/nix/profiles/system"

// Run builds ref's system closure, prints an nvd diff against
// /run/current-system, then (unless mode is "build" or --dry-run) asks
// and activates via nixos-rebuild.
func Run(mode, ref string, opts apply.Options) error {
	flake, host, err := splitTarget(ref)
	if err != nil {
		return err
	}

	oldPath, _ := os.Readlink("/run/current-system")

	attrPath := fmt.Sprintf("%s#nixosConfigurations.%s.config.system.build.toplevel", flake, host)
	ui.Phase("evaluate / build")
	fmt.Printf("nxf: building %s\n", attrPath)
	newPath, err := nixutil.Build(attrPath, opts.Refresh)
	if err != nil {
		return err
	}
	ui.OK("evaluate / build")

	ui.Phase("plan")
	if err := nixutil.ShowDiff(oldPath, newPath); err != nil {
		return err
	}
	ui.OK("plan")

	if mode == "build" || opts.DryRun {
		if opts.DryRun && mode != "build" {
			fmt.Println("Dry run: not applying.")
		}
		return nil
	}

	if err := opts.Confirm(fmt.Sprintf("nixos-rebuild %s %s#%s", mode, flake, host)); err != nil {
		return err
	}

	target := fmt.Sprintf("%s#%s", flake, host)
	ui.Phase("activate")
	fmt.Printf("nxf: nixos-rebuild %s --flake %s\n", mode, target)
	if err := runNixosRebuild(mode, target); err != nil {
		return err
	}
	ui.OK("activate")
	return nil
}

func splitTarget(ref string) (flake, host string, err error) {
	flake = os.Getenv("NXF_FLAKE")
	if flake == "" {
		flake = "."
	}

	if f, h, found := strings.Cut(ref, "#"); found {
		if f != "" {
			flake = f
		}
		host = h
	} else {
		host = ref
	}

	if host == "" {
		h, err := os.Hostname()
		if err != nil {
			return "", "", fmt.Errorf("determining hostname: %w", err)
		}
		host = h
	}

	return flake, host, nil
}

var execCommand = exec.Command
var geteuid = os.Geteuid

func runNixosRebuild(mode, target string) error {
	args := []string{mode, "--flake", target}
	if geteuid() != 0 {
		args = append(args, "--sudo")
	}
	cmd := execCommand("nixos-rebuild", args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	return cmd.Run()
}

type generationJSON struct {
	Generation    int    `json:"generation"`
	Date          string `json:"date"`
	NixosVersion  string `json:"nixosVersion"`
	KernelVersion string `json:"kernelVersion"`
	Current       bool   `json:"current"`
}

// Generations lists NixOS system generations.
func Generations(asJSON bool) error {
	items, err := collectOSGenerations()
	if err != nil {
		return err
	}
	if asJSON {
		b, err := json.MarshalIndent(struct {
			Generations []gens.JSONGeneration `json:"generations"`
		}{items}, "", "  ")
		if err != nil {
			return err
		}
		fmt.Println(string(b))
		return nil
	}
	if len(items) == 0 {
		fmt.Println("no system generations")
		return nil
	}
	fmt.Print(formatOSTable(items))
	return nil
}

func formatOSTable(items []gens.JSONGeneration) string {
	var b strings.Builder
	w := tabwriter.NewWriter(&b, 0, 8, 2, ' ', 0)
	fmt.Fprintln(w, "#\tBUILT\tNIXOS\tKERNEL")
	for _, g := range items {
		fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", genNum(g), formatGenTime(g.Time), shortNixos(g.NixosVersion), g.KernelVersion)
	}
	_ = w.Flush()
	return b.String()
}

func genNum(g gens.JSONGeneration) string {
	if g.Current {
		return fmt.Sprintf(">%3d", g.Number)
	}
	return fmt.Sprintf(" %3d", g.Number)
}

func shortNixos(v string) string {
	parts := strings.Split(v, ".")
	if len(parts) == 4 && len(parts[2]) == 8 && isDigits(parts[2]) {
		return parts[0] + "." + parts[1] + "." + parts[3]
	}
	return v
}

func isDigits(s string) bool {
	for _, r := range s {
		if r < '0' || r > '9' {
			return false
		}
	}
	return s != ""
}

func formatGenTime(rfc3339 string) string {
	t, err := time.Parse(time.RFC3339, rfc3339)
	if err != nil {
		return rfc3339
	}
	return t.Local().Format("2006-01-02 15:04")
}

func collectOSGenerations() ([]gens.JSONGeneration, error) {
	list, err := gens.List(systemProfile)
	if err == nil {
		out := make([]gens.JSONGeneration, 0, len(list))
		for _, g := range list {
			nixosVer, kernelVer := generationExtras(g.Path)
			out = append(out, gens.JSONGeneration{
				Number:        g.Number,
				Time:          g.Time.UTC().Format(time.RFC3339),
				Current:       g.Current,
				NixosVersion:  nixosVer,
				KernelVersion: kernelVer,
			})
		}
		return out, nil
	}

	data, rebuildErr := nixosRebuildJSON("list-generations")
	if rebuildErr != nil {
		return nil, fmt.Errorf("listing system generations: %w", err)
	}
	var rebuild []generationJSON
	if uerr := json.Unmarshal(data, &rebuild); uerr != nil || len(rebuild) == 0 {
		return nil, fmt.Errorf("listing system generations: %w", err)
	}
	out := make([]gens.JSONGeneration, 0, len(rebuild))
	for _, g := range rebuild {
		out = append(out, gens.JSONGeneration{
			Number:        g.Generation,
			Time:          parseRebuildDate(g.Date),
			Current:       g.Current,
			NixosVersion:  g.NixosVersion,
			KernelVersion: g.KernelVersion,
		})
	}
	return out, nil
}

func generationExtras(store string) (nixosVer, kernelVer string) {
	if b, err := os.ReadFile(filepath.Join(store, "nixos-version")); err == nil {
		nixosVer = strings.TrimSpace(string(b))
	}
	entries, err := os.ReadDir(filepath.Join(store, "kernel-modules", "lib", "modules"))
	if err != nil {
		return nixosVer, kernelVer
	}
	for _, e := range entries {
		if e.IsDir() {
			return nixosVer, e.Name()
		}
	}
	return nixosVer, kernelVer
}

func parseRebuildDate(date string) string {
	if t, err := time.ParseInLocation("2006-01-02 15:04:05", date, time.Local); err == nil {
		return t.UTC().Format(time.RFC3339)
	}
	if t, err := time.Parse(time.RFC3339, date); err == nil {
		return t.UTC().Format(time.RFC3339)
	}
	return date
}

func nixosRebuildJSON(mode string) ([]byte, error) {
	cmd := execCommand("nixos-rebuild", mode, "--json")
	return cmd.Output()
}

// Rollback switches the system profile to `to` (or the previous generation)
// and runs switch-to-configuration.
func Rollback(to *int, opts apply.Options) error {
	list, err := gens.List(systemProfile)
	if err != nil {
		return err
	}
	cur := gens.CurrentNumber(list)
	target := 0
	if to != nil {
		target = *to
	} else {
		for _, g := range list {
			if g.Number < cur {
				target = g.Number
				break
			}
		}
		if target == 0 {
			return fmt.Errorf("no previous generation to roll back to")
		}
	}
	var dest gens.Generation
	found := false
	for _, g := range list {
		if g.Number == target {
			dest = g
			found = true
			break
		}
	}
	if !found {
		return fmt.Errorf("generation %d does not exist", target)
	}

	fmt.Printf("  current %d -> %d  %s\n", cur, target, dest.Path)
	if opts.DryRun {
		fmt.Println("Dry run: not applying.")
		return nil
	}
	if err := opts.Confirm(fmt.Sprintf("roll the system back to generation %d", target)); err != nil {
		return err
	}
	ui.Phase("activate")
	sudo := geteuid() != 0
	if err := gens.SwitchTo(systemProfile, target, sudo); err != nil {
		return err
	}
	switchTo := filepath.Join(systemProfile, "bin", "switch-to-configuration")
	var cmd *exec.Cmd
	if sudo {
		cmd = execCommand("sudo", switchTo, "switch")
	} else {
		cmd = execCommand(switchTo, "switch")
	}
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		return err
	}
	ui.OK("activate")
	return nil
}

// Clean deletes selected system generations.
func Clean(spec gens.Spec, opts apply.Options) error {
	list, err := gens.List(systemProfile)
	if err != nil {
		return err
	}
	del, err := gens.SelectDelete(list, spec, time.Now())
	if err != nil {
		return err
	}
	if len(del) == 0 {
		fmt.Println("nothing to delete")
		return nil
	}
	ui.Phase("plan")
	var nums []int
	for _, g := range del {
		nums = append(nums, g.Number)
		fmt.Printf("  %s  delete generation %d  (%s)\n", ui.Bullet(), g.Number, g.Time.Format("2006-01-02 15:04"))
	}
	ui.OK("plan")
	if opts.DryRun {
		fmt.Println("Dry run: not applying.")
		return nil
	}
	if err := opts.Confirm(fmt.Sprintf("delete %d system generation(s)", len(nums))); err != nil {
		return err
	}
	ui.Phase("activate")
	if err := gens.Delete(systemProfile, nums, geteuid() != 0); err != nil {
		return err
	}
	ui.OK("activate")
	return nil
}
