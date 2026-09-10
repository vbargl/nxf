// Package cmd wires nxf's cobra command tree - `nxf os ...` and `nxf profile
// ...` - calling into internal/os and internal/profile for the actual logic.
package cmd

import (
	"fmt"

	"github.com/spf13/cobra"

	"github.com/vbargl/nxf/internal/apply"
	"github.com/vbargl/nxf/internal/version"
)

// New returns nxf's root command.
func New() *cobra.Command {
	root := &cobra.Command{
		Use:           "nxf",
		Short:         "Manages nix profiles and NixOS system generations",
		SilenceUsage:  true,
		SilenceErrors: true,
		Version:       version.Version,
	}
	root.SetVersionTemplate("{{.Version}}\n")
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

func bindApplyFlags(cmd *cobra.Command, opts *apply.Options) {
	cmd.Flags().BoolVar(&opts.DryRun, "dry-run", false, "Build and show the plan without applying")
	cmd.Flags().BoolVar(&opts.Approve, "approve", false, "Apply without asking for confirmation")
}

func bindPriorityFlag(cmd *cobra.Command) {
	cmd.Flags().Int("priority", 5, "nix profile file-collision priority (lower wins)")
}

func takePriority(cmd *cobra.Command, opts *apply.Options) {
	if !cmd.Flags().Changed("priority") {
		return
	}
	p, _ := cmd.Flags().GetInt("priority")
	opts.Priority = &p
}
