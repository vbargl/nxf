// Package apply holds the shared flags for mutating nxf commands: dry-run,
// terraform-style confirmation, and optional nix profile priority.
package apply

import "github.com/vbargl/nxf/internal/ui"

// Options control whether a mutating command plans, asks, or applies.
type Options struct {
	DryRun   bool
	Approve  bool
	Priority *int
	Refresh  bool
	Verbose  bool
}

// Confirm asks unless --approve is set. Dry-run callers must not call this.
func (o Options) Confirm(action string) error {
	if o.Approve {
		return nil
	}
	return ui.Confirm(action)
}
