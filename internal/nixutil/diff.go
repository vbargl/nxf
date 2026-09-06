package nixutil

import (
	"fmt"
	"os"
	"strings"
)

// NvdPath is overridden at Nix build time via -ldflags to point at nvd's
// store path, so nxf doesn't depend on nvd being on the caller's PATH. Falls
// back to a bare PATH lookup for local `go build` runs during development.
var NvdPath = "nvd"

// emptyBaselinePath is lazily built and cached: a trivial empty store path
// used as a stand-in for "no profile"/"no system" so a first-ever add or a
// last-ever remove still gets a real nvd diff (everything shown as added or
// removed) instead of being silently skipped.
var emptyBaselinePath string

func emptyBaseline() (string, error) {
	if emptyBaselinePath != "" {
		return emptyBaselinePath, nil
	}

	// "> $out" is a shell builtin (plain redirection), not an external
	// command - the build sandbox's PATH is empty, so anything relying on a
	// real binary (mkdir, touch, ...) would fail with "not found".
	cmd := execCommand(
		"nix", "build", "--impure", "--no-link", "--print-out-paths", "--expr",
		`derivation { name = "nxf-empty-baseline"; system = builtins.currentSystem; builder = "/bin/sh"; args = [ "-c" "> $out" ]; }`,
	)
	out, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("building empty baseline: %w", err)
	}
	emptyBaselinePath = strings.TrimSpace(string(out))
	return emptyBaselinePath, nil
}

// ShowDiff prints an nvd package-level tree diff (added/removed/changed
// packages, plus its own closure size / disk usage delta line) between
// oldPath and newPath. Either side may be "" (nothing to compare against -
// e.g. no /run/current-system yet, or a first-ever profile add / last
// profile remove); that side is substituted with an empty baseline so the
// diff still shows the full set of packages gained or lost, rather than
// being skipped.
func ShowDiff(oldPath, newPath string) error {
	if oldPath == "" && newPath == "" {
		return nil
	}

	if oldPath == "" {
		p, err := emptyBaseline()
		if err != nil {
			return err
		}
		oldPath = p
	}
	if newPath == "" {
		p, err := emptyBaseline()
		if err != nil {
			return err
		}
		newPath = p
	}

	if oldPath == newPath {
		fmt.Println("No changes.")
		return nil
	}

	cmd := execCommand(NvdPath, "diff", oldPath, newPath)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("nvd diff: %w", err)
	}
	return nil
}
