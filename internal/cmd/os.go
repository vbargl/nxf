package cmd

import (
	"github.com/spf13/cobra"

	"github.com/vbargl/nxf/internal/apply"
	"github.com/vbargl/nxf/internal/os"
	"github.com/vbargl/nxf/internal/profile"
)

const osLongDescription = `Builds and activates the NixOS system configuration.

Phases: evaluate/build, plan (nvd diff), then (unless --dry-run) a terraform-style
confirmation, then activate. Pass --approve to skip the prompt.

The argument is either a bare host name (host defaults to the current hostname
when omitted; the flake defaults to ".", override with NXF_FLAKE), or a
"<flake>#<host>" pair like ".#saber".`

func newOSCommand() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "os",
		Short: "Manage the NixOS system configuration",
		Long:  osLongDescription,
	}
	cmd.AddCommand(
		newOSModeCommand("build", "Build the system closure without activating it"),
		newOSModeCommand("test", "Build and activate, without setting the boot default"),
		newOSModeCommand("switch", "Build, activate, and set the boot default"),
		newOSModeCommand("boot", "Build and set the boot default, without activating now"),
		newOSGenerationsCommand(),
		newOSRollbackCommand(),
		newOSCleanCommand(),
	)
	return cmd
}

func newOSModeCommand(mode, short string) *cobra.Command {
	var opts apply.Options
	c := &cobra.Command{
		Use:   mode + " [host]",
		Short: short,
		Args:  cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			host := ""
			if len(args) == 1 {
				host = args[0]
			}
			return os.Run(mode, host, opts)
		},
	}
	bindApplyFlags(c, &opts)
	c.Flags().BoolVar(&opts.Refresh, "refresh", false, "bypass nix's flake-ref resolution cache")
	return c
}

func newOSGenerationsCommand() *cobra.Command {
	return &cobra.Command{
		Use:     "generations",
		Aliases: []string{"list", "history"},
		Short:   "List NixOS system generations",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			return os.Generations()
		},
	}
}

func newOSRollbackCommand() *cobra.Command {
	var opts apply.Options
	var to int
	c := &cobra.Command{
		Use:   "rollback",
		Short: "Roll the system back to the previous (or --to) generation",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			var target *int
			if cmd.Flags().Changed("to") {
				target = &to
			}
			return os.Rollback(target, opts)
		},
	}
	c.Flags().IntVar(&to, "to", 0, "generation number to switch to")
	bindApplyFlags(c, &opts)
	return c
}

func newOSCleanCommand() *cobra.Command {
	var opts apply.Options
	var spec profile.CleanSpec
	c := &cobra.Command{
		Use:   "clean",
		Short: "Delete old NixOS system generations",
		Long: `Delete non-current system generations.

Exactly one selector: --keep N, --keep-since 3d, --older-than 1w, or --delete 10,11.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			parsed, err := spec.Spec()
			if err != nil {
				return err
			}
			return os.Clean(parsed, opts)
		},
	}
	c.Flags().IntVar(&spec.Keep, "keep", 0, "keep this many most recent generations")
	c.Flags().StringVar(&spec.KeepSince, "keep-since", "", "keep generations newer than this duration (e.g. 3d)")
	c.Flags().StringVar(&spec.OlderThan, "older-than", "", "delete generations older than this duration (e.g. 1w)")
	c.Flags().IntSliceVar(&spec.Delete, "delete", nil, "delete these generation numbers")
	bindApplyFlags(c, &opts)
	return c
}
