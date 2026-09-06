package main

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"

	"github.com/vbargl/nxf/internal/oscmd"
	"github.com/vbargl/nxf/internal/profilecmd"
	"github.com/vbargl/nxf/internal/version"
)

func main() {
	root := &cobra.Command{
		Use:           "nxf",
		Short:         "Manages nix profiles and NixOS system generations for this repo",
		SilenceUsage:  true,
		SilenceErrors: true,
	}
	root.AddCommand(profilecmd.NewCommand())
	root.AddCommand(oscmd.NewCommand())
	root.AddCommand(&cobra.Command{
		Use:   "version",
		Short: "Print the nxf version",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			fmt.Println(version.Version)
			return nil
		},
	})

	if err := root.Execute(); err != nil {
		fmt.Fprintf(os.Stderr, "nxf: error: %v\n", err)
		os.Exit(1)
	}
}
