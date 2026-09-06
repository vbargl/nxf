// Package cmd wires nxf's cobra command tree - `nxf os ...` and `nxf profile
// ...` - calling into internal/os and internal/profile for the actual logic.
package cmd

import (
	"fmt"

	"github.com/spf13/cobra"

	"github.com/vbargl/nxf/internal/version"
)

// New returns nxf's root command.
func New() *cobra.Command {
	root := &cobra.Command{
		Use:           "nxf",
		Short:         "Manages nix profiles and NixOS system generations for this repo",
		SilenceUsage:  true,
		SilenceErrors: true,
	}
	root.AddCommand(newProfileCommand())
	root.AddCommand(newOSCommand())
	root.AddCommand(&cobra.Command{
		Use:   "version",
		Short: "Print the nxf version",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			fmt.Println(version.Version)
			return nil
		},
	})
	return root
}
