// Package nixutil wraps the external nix/nom CLIs: building flake outputs,
// adding/removing nix profile elements, and resolving the current system.
package nixutil

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"

	"github.com/vbargl/nxf/internal/paths"
)

// profileCmd prepends `profile --profile <link>` so mutations hit the
// same profile NixProfileLink() resolved (test-only NXF_PROFILE, else
// ~/.nix-profile).
func profileCmd(sub string, extra ...string) []string {
	args := []string{"profile", sub}
	if p, err := paths.NixProfileLink(); err == nil && p != "" {
		args = append(args, "--profile", p)
	}
	return append(args, extra...)
}

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

// ProfileAdd installs ref. Callers pass an already-built store path so the
// element name is the derivation name (`gui.daily`) rather than the last
// flake attr-path segment (`daily`). Flake provenance is recorded by nxf
// itself (refs.json), not by nix.
func ProfileAdd(ref string, priority *int) error {
	var extra []string
	if priority != nil {
		extra = append(extra, "--priority", strconv.Itoa(*priority))
	}
	extra = append(extra, ref)
	return RunNix(profileCmd("add", extra...)...)
}

func ProfileRemove(name string) error {
	return RunNix(profileCmd("remove", name)...)
}

func ProfileListJSON(profile string) ([]byte, error) {
	args := []string{"profile", "list", "--json"}
	if profile != "" {
		args = append(args, "--profile", profile)
	}
	cmd := execCommand("nix", args...)
	out, err := cmd.Output()
	if err != nil {
		return nil, fmt.Errorf("nix profile list --json: %w", err)
	}
	return out, nil
}

// ProfileRemoveQuiet best-effort removes name without printing anything -
// used to clear out a prior element before re-adding under the same name
// (see internal/profile), where "no such element" is the expected common
// case and shouldn't be reported as if something went wrong.
func ProfileRemoveQuiet(name string) {
	cmd := execCommand("nix", profileCmd("remove", name)...)
	_ = cmd.Run()
}

// ProfileRollback restores the previous nix profile generation. Used when
// a remove-then-add sequence fails after the remove has already taken
// effect, so the user is not left with the package gone.
func ProfileRollback() error {
	return RunNix(profileCmd("rollback")...)
}

// ProfileRollbackTo switches the nix profile to generation n.
func ProfileRollbackTo(n int) error {
	return RunNix(profileCmd("rollback", "--to", strconv.Itoa(n))...)
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
