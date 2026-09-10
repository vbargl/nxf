package apply

import (
	"strings"
	"testing"

	"github.com/vbargl/nxf/internal/ui"
)

func TestConfirmApproveSkipsPrompt(t *testing.T) {
	origIn, origTTY := ui.In, ui.IsTerminal
	t.Cleanup(func() { ui.In, ui.IsTerminal = origIn, origTTY })
	ui.In = strings.NewReader("")
	ui.IsTerminal = func() bool { return false }

	opts := Options{Approve: true}
	if err := opts.Confirm("switch"); err != nil {
		t.Fatalf("Approve should skip Confirm: %v", err)
	}
}

func TestConfirmWithoutApprove(t *testing.T) {
	origOut, origIn, origTTY := ui.Out, ui.In, ui.IsTerminal
	t.Cleanup(func() { ui.Out, ui.In, ui.IsTerminal = origOut, origIn, origTTY })
	ui.Out = ioBuf()
	ui.In = strings.NewReader("yes\n")
	ui.IsTerminal = func() bool { return true }

	opts := Options{}
	if err := opts.Confirm("add"); err != nil {
		t.Fatalf("Confirm(yes): %v", err)
	}
}

func ioBuf() *strings.Builder { return &strings.Builder{} }
