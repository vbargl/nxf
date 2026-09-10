package cmd

import (
	"github.com/spf13/cobra"

	"github.com/vbargl/nxf/internal/apply"
	"github.com/vbargl/nxf/internal/profile"
)

func newProfileCommand() *cobra.Command {
	cmd := &cobra.Command{
		Use:   "profile",
		Short: "Manage user-level nix profiles",
	}
	cmd.AddCommand(
		newBuildCommand(),
		newAddCommand(),
		newRemoveCommand(),
		newUpgradeCommand(),
		newSyncCommand(),
		newListCommand(),
		newProfileGenerationsCommand(),
		newProfileRollbackCommand(),
		newProfileCleanCommand(),
	)
	return cmd
}

func newBuildCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "build <flake-ref>#<profile>",
		Short: "Build a profile without installing it",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Build(args[0])
		},
	}
}

func newAddCommand() *cobra.Command {
	var opts apply.Options
	c := &cobra.Command{
		Use:   "add <flake-ref>#<profile>...",
		Short: "Add one or more profiles, then sync",
		Args:  cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			takePriority(cmd, &opts)
			return profile.Add(args, opts)
		},
	}
	bindApplyFlags(c, &opts)
	c.Flags().BoolVar(&opts.Refresh, "refresh", false, "bypass nix's flake-ref resolution cache")
	bindPriorityFlag(c)
	return c
}

func newRemoveCommand() *cobra.Command {
	var opts apply.Options
	c := &cobra.Command{
		Use:   "remove <name>...",
		Short: "Remove one or more profiles, then sync",
		Args:  cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Remove(args, opts)
		},
	}
	bindApplyFlags(c, &opts)
	return c
}

func newUpgradeCommand() *cobra.Command {
	var opts apply.Options
	var all bool
	c := &cobra.Command{
		Use:   "upgrade [name...]",
		Short: "Rebuild already-applied profiles from their recorded flake URL",
		RunE: func(cmd *cobra.Command, args []string) error {
			takePriority(cmd, &opts)
			return profile.Upgrade(args, all, opts)
		},
	}
	c.Flags().BoolVar(&all, "all", false, "upgrade every profile nxf knows about")
	c.Flags().BoolVar(&opts.Refresh, "refresh", false, "bypass nix's flake-ref resolution cache")
	bindApplyFlags(c, &opts)
	bindPriorityFlag(c)
	return c
}

func newSyncCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "sync",
		Short: "Reconcile units/activation without add/remove",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Sync()
		},
	}
}

func newListCommand() *cobra.Command {
	var verbose, asJSON bool
	c := &cobra.Command{
		Use:   "list [name...]",
		Short: "List applied profiles (table; -v for details; names filter, prefix ok)",
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.List(verbose, asJSON, args)
		},
	}
	c.Flags().BoolVarP(&verbose, "verbose", "v", false, "show units, binaries, desktop files, flake refs")
	bindJSONFlag(c, &asJSON)
	return c
}

func newProfileGenerationsCommand() *cobra.Command {
	var asJSON bool
	c := &cobra.Command{
		Use:     "generations",
		Aliases: []string{"history"},
		Short:   "List user-profile generations",
		Args:    cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Generations(asJSON)
		},
	}
	bindJSONFlag(c, &asJSON)
	return c
}

func newProfileRollbackCommand() *cobra.Command {
	var opts apply.Options
	var to int
	c := &cobra.Command{
		Use:   "rollback",
		Short: "Roll the user profile back to the previous (or --to) generation",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			var target *int
			if cmd.Flags().Changed("to") {
				target = &to
			}
			return profile.Rollback(target, opts)
		},
	}
	c.Flags().IntVar(&to, "to", 0, "generation number to switch to")
	bindApplyFlags(c, &opts)
	return c
}

func newProfileCleanCommand() *cobra.Command {
	var opts apply.Options
	var spec profile.CleanSpec
	c := &cobra.Command{
		Use:   "clean",
		Short: "Delete old user-profile generations",
		Long: `Delete non-current user-profile generations.

Exactly one selector:

  --keep 3           keep the 3 most recent generations
  --keep-since 3d    keep generations from the last 3 days
  --older-than 1w    delete generations older than 1 week
  --delete 10,11     delete these generation numbers

Durations: 30m, 2h, 3d, 1w.`,
		Args: cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Clean(spec, opts)
		},
	}
	c.Flags().IntVar(&spec.Keep, "keep", 0, "keep this many most recent generations")
	c.Flags().StringVar(&spec.KeepSince, "keep-since", "", "keep generations newer than this duration (e.g. 3d)")
	c.Flags().StringVar(&spec.OlderThan, "older-than", "", "delete generations older than this duration (e.g. 1w)")
	c.Flags().IntSliceVar(&spec.Delete, "delete", nil, "delete these generation numbers")
	bindApplyFlags(c, &opts)
	return c
}
