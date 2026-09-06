// Package nixutil wraps the external nix/nom CLIs: building flake outputs,
// adding/removing nix profile elements, and resolving the current system.
package nixutil

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

// NomPath is overridden at Nix build time via -ldflags to point at
// nix-output-monitor's store path, so nxf doesn't depend on nom being on the
// caller's PATH. Falls back to a bare PATH lookup for local `go build` runs
// during development.
var NomPath = "nom"

// useNom controls whether nix build/profile output is piped through nom for
// a live dependency-graph display. Disable with NXF_NO_NOM=1 (e.g. for
// scripting, where a tree TUI just adds noise).
var useNom = os.Getenv("NXF_NO_NOM") == ""

// execCommand constructs every external command this package runs. Tests
// override it to capture the name/args a call would have used, without
// actually invoking nix/nom.
var execCommand = exec.Command

// RunNix runs a plain nix subcommand, streaming output as-is.
func RunNix(args ...string) error {
	cmd := execCommand("nix", args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	return cmd.Run()
}

// runNixTree runs a nix build, piping its combined output through nom for a
// live dependency-graph display when enabled. Falls back to RunNix
// otherwise. Only used for actual builds - `nix profile add`/`remove`
// installing an already-built store path has nothing left to build, and
// nvd's post-add/remove diff (see nixutil.ShowDiff) is a better report of
// what changed than nom's build-progress view.
func runNixTree(args ...string) error {
	if !useNom {
		return RunNix(args...)
	}

	nixArgs := append(append([]string{}, args...), "--log-format", "internal-json", "--verbose")
	nixCmd := execCommand("nix", nixArgs...)
	nixCmd.Stdin = os.Stdin

	pipeR, err := nixCmd.StdoutPipe()
	if err != nil {
		return fmt.Errorf("creating nom pipe: %w", err)
	}
	nixCmd.Stderr = nixCmd.Stdout // merge stderr into the same stream nom reads

	nomCmd := execCommand(NomPath, "--json")
	nomCmd.Stdin = pipeR
	nomCmd.Stdout = os.Stdout
	nomCmd.Stderr = os.Stderr

	if err := nixCmd.Start(); err != nil {
		return fmt.Errorf("starting nix: %w", err)
	}
	if err := nomCmd.Start(); err != nil {
		return fmt.Errorf("starting nom: %w", err)
	}

	// nom's own exit status doesn't reflect nix's (it always succeeds even
	// when nix fails), so nix's is the one that determines our result.
	nixErr := nixCmd.Wait()
	_ = nomCmd.Wait()

	return nixErr
}

// ProfileAdd installs an already-built store path (see internal/profile) -
// nothing to build here, so this is a plain, near-instant command.
func ProfileAdd(storePath string) error {
	return RunNix("profile", "add", storePath)
}

func ProfileRemove(name string) error {
	return RunNix("profile", "remove", name)
}

// ProfileRemoveQuiet best-effort removes name without printing anything -
// used to clear out a prior element before re-adding under the same name
// (see internal/profile), where "no such element" is the expected common
// case and shouldn't be reported as if something went wrong.
func ProfileRemoveQuiet(name string) {
	cmd := execCommand("nix", "profile", "remove", name)
	_ = cmd.Run()
}

// Build builds ref (progress goes through nom when enabled, see
// runNixTree) and returns the resulting store path so callers can diff it
// against a prior state. Uses a throwaway --out-link instead of
// --print-out-paths, since nom already consumes stdout+stderr for its own
// live display. refresh bypasses nix's flake-ref resolution cache (see
// `nix build --refresh`) - used by `nxf profile upgrade` to actually pick up
// a moved ref instead of the stale, already-cached resolution.
func Build(ref string, refresh bool) (string, error) {
	tmp, err := os.MkdirTemp("", "nxf-build-")
	if err != nil {
		return "", fmt.Errorf("creating temp dir for build result: %w", err)
	}
	defer os.RemoveAll(tmp)
	outLink := filepath.Join(tmp, "result")

	args := []string{"build", ref, "--out-link", outLink}
	if refresh {
		args = append(args, "--refresh")
	}
	if err := runNixTree(args...); err != nil {
		return "", fmt.Errorf("nix build %s: %w", ref, err)
	}

	resolved, err := filepath.EvalSymlinks(outLink)
	if err != nil {
		return "", fmt.Errorf("resolving build result %s: %w", outLink, err)
	}
	return resolved, nil
}
