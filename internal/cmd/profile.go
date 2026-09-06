package cmd

import (
	"github.com/spf13/cobra"

	"github.com/vbargl/nxf/internal/profile"
)

// newProfileCommand returns the `nxf profile` command tree: building,
// adding, removing, upgrading, syncing, and listing user-level nix profiles.
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
	)
	return cmd
}

func newBuildCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "build <flake-ref>#<profile>",
		Short: "Build a profile without installing it (./result)",
		Args:  cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Build(args[0])
		},
	}
}

func newAddCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "add <flake-ref>#<profile>...",
		Short: "Add one or more profiles (nix profile add), then sync",
		Args:  cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Add(args)
		},
	}
}

func newRemoveCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "remove <name>...",
		Short: "Remove one or more profiles (nix profile remove), then sync",
		Args:  cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Remove(args)
		},
	}
}

func newUpgradeCommand() *cobra.Command {
	var all, refresh bool
	c := &cobra.Command{
		Use:   "upgrade [name...]",
		Short: "Rebuild and reinstall already-applied profiles from the flake ref they were added with",
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.Upgrade(args, all, refresh)
		},
	}
	c.Flags().BoolVar(&all, "all", false, "upgrade every profile nxf has a recorded ref for")
	c.Flags().BoolVar(&refresh, "refresh", false, "bypass nix's flake-ref resolution cache and re-check upstream")
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
	return &cobra.Command{
		Use:   "list",
		Short: "List profiles currently applied",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			return profile.List()
		},
	}
}
