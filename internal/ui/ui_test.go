package ui

import (
	"bytes"
	"strings"
	"testing"
)

func TestConfirmYes(t *testing.T) {
	origOut, origIn, origTTY := Out, In, IsTerminal
	t.Cleanup(func() { Out, In, IsTerminal = origOut, origIn, origTTY })

	var out bytes.Buffer
	Out = &out
	In = strings.NewReader("yes\n")
	IsTerminal = func() bool { return true }

	if err := Confirm("add these profiles"); err != nil {
		t.Fatalf("Confirm(yes): %v", err)
	}
	if !strings.Contains(out.String(), "Only 'yes' will be accepted") {
		t.Errorf("prompt missing: %s", out.String())
	}
}

func TestConfirmNo(t *testing.T) {
	origOut, origIn, origTTY := Out, In, IsTerminal
	t.Cleanup(func() { Out, In, IsTerminal = origOut, origIn, origTTY })

	Out = ioDiscard()
	In = strings.NewReader("no\n")
	IsTerminal = func() bool { return true }

	err := Confirm("switch")
	if err == nil || !strings.Contains(err.Error(), "cancelled") {
		t.Fatalf("Confirm(no) = %v, want cancelled", err)
	}
}

func TestConfirmNonTTYWithoutYes(t *testing.T) {
	origOut, origIn, origTTY := Out, In, IsTerminal
	t.Cleanup(func() { Out, In, IsTerminal = origOut, origIn, origTTY })

	Out = ioDiscard()
	In = strings.NewReader("")
	IsTerminal = func() bool { return false }

	err := Confirm("switch")
	if err == nil || !strings.Contains(err.Error(), "--approve") {
		t.Fatalf("Confirm(non-tty) = %v, want --approve error", err)
	}
}

func ioDiscard() *bytes.Buffer { return &bytes.Buffer{} }
