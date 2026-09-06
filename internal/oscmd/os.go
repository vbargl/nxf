// Package oscmd implements the `nxf os` subcommand: building and activating
// the NixOS system configuration via nixos-rebuild.
package oscmd

import (
	"fmt"
	"os"
	"os/exec"

	"github.com/spf13/cobra"

	"github.com/vbargl/nxf/internal/nixutil"
)

const longDescription = `Builds and activates the NixOS system configuration.

Every mode prints an nvd package diff (with its closure size delta) against
/run/current-system before doing anything else. host defaults to the current
hostname. The flake built from defaults to ".", override with the NXF_FLAKE
environment variable.`

// NewCommand returns the `nxf os` command tree: building and activating the
// NixOS system configuration via nixos-rebuild.
func NewCommand() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "os",
		Short: "Manage the NixOS system configuration",
		Long:  longDescription,
	}
	cmd.AddCommand(
		newModeCommand("build", "Build the system closure without activating it (./result)"),
		newModeCommand("test", "Build and activate, without setting the boot default"),
		newModeCommand("switch", "Build, activate, and set the boot default"),
		newModeCommand("boot", "Build and set the boot default, without activating now"),
	)
	return cmd
}

func newModeCommand(mode, short string) *cobra.Command {
	return &cobra.Command{
		Use:   mode + " [host]",
		Short: short,
		Args:  cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			host := ""
			if len(args) == 1 {
				host = args[0]
			}
			return doOS(mode, host)
		},
	}
}

func doOS(mode, host string) error {
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
