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
	"time"

	"github.com/vbargl/nxf/internal/apply"
	"github.com/vbargl/nxf/internal/gens"
	"github.com/vbargl/nxf/internal/nixutil"
	"github.com/vbargl/nxf/internal/ui"
)

const systemProfile = "/nix/var/nix/profiles/system"

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
func Generations() error {
	if data, err := nixosRebuildJSON("list-generations"); err == nil {
		var list []generationJSON
		if err := json.Unmarshal(data, &list); err == nil && len(list) > 0 {
			for _, g := range list {
				mark := " "
				if g.Current {
					mark = ui.CurrentMarker()
				}
				fmt.Printf("  %4d  %s  %s  kernel %s  %s\n", g.Generation, g.Date, g.NixosVersion, g.KernelVersion, mark)
			}
			return nil
		}
	}
	list, err := gens.List(systemProfile)
	if err != nil {
		return fmt.Errorf("listing system generations: %w", err)
	}
	for _, g := range list {
		mark := " "
		if g.Current {
			mark = ui.CurrentMarker()
		}
		fmt.Printf("  %4d  %s  %s\n", g.Number, g.Time.Format("2006-01-02 15:04"), mark)
	}
	return nil
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
