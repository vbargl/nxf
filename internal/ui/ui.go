// Package ui prints nxf's phase markers, terraform-style confirmation, and
// compact/verbose list lines. Honours NO_COLOR by dropping emoji.
package ui

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"strings"
)

var (
	// Out is where phase markers and prompts go (stdout).
	Out io.Writer = os.Stdout
	// Err is where warnings go.
	Err io.Writer = os.Stderr
	// In is read by Confirm.
	In io.Reader = os.Stdin
	// IsTerminal reports whether Confirm should accept interactive input.
	// Overridden in tests. The default checks os.Stdin's character device bit.
	IsTerminal = defaultIsTerminal
)

func defaultIsTerminal() bool {
	info, err := os.Stdin.Stat()
	if err != nil {
		return false
	}
	return info.Mode()&os.ModeCharDevice != 0
}

func color() bool {
	return os.Getenv("NO_COLOR") == ""
}

// Phase starts a named step (evaluate, build, plan, activate, …).
func Phase(name string) {
	if color() {
		fmt.Fprintf(Out, "▶  %s\n", name)
		return
	}
	fmt.Fprintf(Out, "-> %s\n", name)
}

// OK marks a phase as done.
func OK(name string) {
	if color() {
		fmt.Fprintf(Out, "✓  %s\n", name)
		return
	}
	fmt.Fprintf(Out, "ok %s\n", name)
}

// Warn prints a non-fatal warning.
func Warn(msg string) {
	if color() {
		fmt.Fprintf(Err, "⚠  %s\n", msg)
		return
	}
	fmt.Fprintf(Err, "warning: %s\n", msg)
}

// Fail prints a fatal-looking error line (the process still exits via
// cmd.Execute's error handling).
func Fail(msg string) {
	if color() {
		fmt.Fprintf(Err, "✗  %s\n", msg)
		return
	}
	fmt.Fprintf(Err, "error: %s\n", msg)
}

// Confirm asks the user to type "yes", matching terraform's apply prompt.
// Non-interactive stdin without a "yes" on In is rejected so unattended
// runs cannot mutate a live profile/system unless --approve was passed
// (the caller skips Confirm in that case).
func Confirm(action string) error {
	fmt.Fprintf(Out, "\nDo you want to %s?\n  Only 'yes' will be accepted.\n\n  Enter a value: ", action)
	if !IsTerminal() {
		// Still honour a piped "yes\n" (tests, scripted apply) but refuse
		// an empty non-tty so a forgotten pipe doesn't apply.
		scanner := bufio.NewScanner(In)
		if scanner.Scan() && strings.TrimSpace(scanner.Text()) == "yes" {
			fmt.Fprintln(Out)
			return nil
		}
		return fmt.Errorf("refusing to %s without --approve (stdin is not a terminal)", action)
	}
	scanner := bufio.NewScanner(In)
	if !scanner.Scan() {
		if err := scanner.Err(); err != nil {
			return fmt.Errorf("reading confirmation: %w", err)
		}
		return fmt.Errorf("cancelled")
	}
	if strings.TrimSpace(scanner.Text()) != "yes" {
		return fmt.Errorf("cancelled")
	}
	fmt.Fprintln(Out)
	return nil
}

// Bullet is the compact-list marker for an applied profile.
func Bullet() string {
	if color() {
		return "●"
	}
	return "*"
}

// CurrentMarker is shown next to the live generation.
func CurrentMarker() string {
	if color() {
		return "● current"
	}
	return "* current"
}
