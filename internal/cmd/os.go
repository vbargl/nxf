package cmd

import (
	"github.com/spf13/cobra"

	"github.com/vbargl/nxf/internal/os"
)

const osLongDescription = `Builds and activates the NixOS system configuration.

Every mode prints an nvd package diff (with its closure size delta) against
/run/current-system before doing anything else. The argument is either a bare
host name (host defaults to the current hostname when omitted; the flake
defaults to ".", override with the NXF_FLAKE environment variable), or a
"<flake>#<host>" pair like ".#saber" - the same convention nh and nix profile
use.`

// newOSCommand returns the `nxf os` command tree: building and activating
// the NixOS system configuration via nixos-rebuild.
func newOSCommand() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "os",
		Short: "Manage the NixOS system configuration",
		Long:  osLongDescription,
	}
	cmd.AddCommand(
		newOSModeCommand("build", "Build the system closure without activating it (./result)"),
		newOSModeCommand("test", "Build and activate, without setting the boot default"),
		newOSModeCommand("switch", "Build, activate, and set the boot default"),
		newOSModeCommand("boot", "Build and set the boot default, without activating now"),
	)
	return cmd
}

func newOSModeCommand(mode, short string) *cobra.Command {
	return &cobra.Command{
		Use:   mode + " [host]",
		Short: short,
		Args:  cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			host := ""
			if len(args) == 1 {
				host = args[0]
			}
			return os.Run(mode, host)
		},
	}
}
