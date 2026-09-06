// Package os builds and activates the NixOS system configuration via
// nixos-rebuild - the logic behind `nxf os`.
package os

import (
	"fmt"
	"os"
	"os/exec"

	"github.com/vbargl/nxf/internal/nixutil"
)

// Run builds host's system closure (host defaults to the current hostname
// when empty), prints an nvd diff against /run/current-system, and - unless
// mode is "build" - activates it via nixos-rebuild. The flake built from
// defaults to ".", override with the NXF_FLAKE environment variable.
func Run(mode, host string) error {
	if host == "" {
		h, err := os.Hostname()
		if err != nil {
			return fmt.Errorf("determining hostname: %w", err)
		}
		host = h
	}

	flake := os.Getenv("NXF_FLAKE")
	if flake == "" {
		flake = "."
	}

	oldPath, _ := os.Readlink("/run/current-system")

	attrPath := fmt.Sprintf("%s#nixosConfigurations.%s.config.system.build.toplevel", flake, host)
	fmt.Printf("nxf: building %s\n", attrPath)
	newPath, err := nixutil.Build(attrPath, false)
	if err != nil {
		return err
	}

	if err := nixutil.ShowDiff(oldPath, newPath); err != nil {
		return err
	}

	if mode == "build" {
		return nil
	}

	target := fmt.Sprintf("%s#%s", flake, host)
	fmt.Printf("nxf: nixos-rebuild %s --flake %s\n", mode, target)

	return runNixosRebuild(mode, target)
}

// execCommand constructs the nixos-rebuild command; overridden in tests so
// argument construction can be verified without actually rebuilding.
var execCommand = exec.Command

func runNixosRebuild(mode, target string) error {
	cmd := execCommand("nixos-rebuild", mode, "--flake", target)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	return cmd.Run()
}
