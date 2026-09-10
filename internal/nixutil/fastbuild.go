package nixutil

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// NixFastBuildPath is overridden at Nix build time via -ldflags to point at
// nix-fast-build's store path, so nxf doesn't depend on it being on the
// caller's PATH. Falls back to a bare PATH lookup for local `go build` runs
// during development.
var NixFastBuildPath = "nix-fast-build"

// fastBuildResult mirrors the entries nix-fast-build writes to its
// --result-file in --result-format json (see nix-fast-build's README) -
// only the fields BuildMany needs are declared here.
type fastBuildResult struct {
	Results []struct {
		Attr    string            `json:"attr"`
		Type    string            `json:"type"`
		Success bool              `json:"success"`
		Error   *string           `json:"error"`
		Outputs map[string]string `json:"outputs"`
	} `json:"results"`
}

// nixAttrSetLiteral renders names as a Nix attribute-set literal suitable
// for builtins.intersectAttrs, e.g. {"a" = null; "b" = null;}. Nix string
// literals only need '"' and '\' escaped - profile names never contain
// either in practice, but this stays correct if that ever changes.
func nixAttrSetLiteral(names []string) string {
	var b strings.Builder
	b.WriteByte('{')
	for _, n := range names {
		// Go's %q is a double-quoted string with the same escapes Nix
		// string literals use (\ and "), so this is valid Nix as-is.
		fmt.Fprintf(&b, "%q = null; ", n)
	}
	b.WriteByte('}')
	return b.String()
}

// resolveFlakeURL turns a possibly-indirect flake reference (a registry
// shorthand like "vbargl2") into its resolved direct URL. nix-eval-jobs
// (which nix-fast-build shells out to) refuses to evaluate an indirect
// reference at all ("registry lookups are not allowed") even though `nix
// build`/`nix eval` resolve one just fine - so BuildMany must resolve it
// itself first. Harmless (a no-op) when flake is already a direct URL.
func resolveFlakeURL(flake string) (string, error) {
	out, err := execCommand("nix", "flake", "metadata", flake, "--json").Output()
	if err != nil {
		return "", fmt.Errorf("resolving flake %s: %w", flake, err)
	}
	var meta struct {
		ResolvedURL string `json:"resolvedUrl"`
	}
	if err := json.Unmarshal(out, &meta); err != nil {
		return "", fmt.Errorf("parsing flake metadata for %s: %w", flake, err)
	}
	return meta.ResolvedURL, nil
}

// BuildMany builds every profile in names under
// <flake>#profileConfigurations.<system> in a single nix-fast-build
// invocation, so shared dependencies across profiles are evaluated and built
// concurrently instead of once per sequential `nix build`. It returns each
// name's resulting store path. Only used when there's more than one profile
// to build in one go - a single profile build stays on the plain `nix build`
// path (see Build), where nom's live tree view is more useful than
// nix-fast-build's per-attribute renderer. refresh bypasses nix's flake-ref
// resolution cache - nix-fast-build has no native --refresh flag, so this is
// done via `--option tarball-ttl 0` instead (see Build for the plain-nix
// equivalent, used by `nxf profile upgrade`).
func BuildMany(flake, system string, names []string, refresh bool) (map[string]string, error) {
	tmp, err := os.MkdirTemp("", "nxf-fast-build-")
	if err != nil {
		return nil, fmt.Errorf("creating temp dir for nix-fast-build result: %w", err)
	}
	defer os.RemoveAll(tmp)
	resultFile := filepath.Join(tmp, "result.json")

	resolved, err := resolveFlakeURL(flake)
	if err != nil {
		return nil, err
	}
	root := fmt.Sprintf("%s#profileConfigurations.%s", resolved, system)
	sel := fmt.Sprintf("profiles: builtins.intersectAttrs %s profiles", nixAttrSetLiteral(names))

	args := []string{
		"--flake", root,
		"--select", sel,
		"--result-file", resultFile,
		"--result-format", "json",
	}
	if refresh {
		args = append(args, "--option", "tarball-ttl", "0")
	}
	cmd := execCommand(NixFastBuildPath, args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("nix-fast-build %s: %w", root, err)
	}

	data, err := os.ReadFile(resultFile)
	if err != nil {
		return nil, fmt.Errorf("reading nix-fast-build result file: %w", err)
	}
	var parsed fastBuildResult
	if err := json.Unmarshal(data, &parsed); err != nil {
		return nil, fmt.Errorf("parsing nix-fast-build result file: %w", err)
	}

	paths := make(map[string]string, len(names))
	for _, r := range parsed.Results {
		if r.Type != "BUILD" {
			continue
		}
		if !r.Success {
			msg := "build failed"
			if r.Error != nil {
				msg = *r.Error
			}
			return nil, fmt.Errorf("nix-fast-build: %s: %s", r.Attr, msg)
		}
		out, ok := r.Outputs["out"]
		if !ok {
			return nil, fmt.Errorf("nix-fast-build: %s: no 'out' output in result", r.Attr)
		}
		// nix-eval-jobs renders a dot-containing attr name quoted as its own
		// segment (e.g. `"terminal.admintools"`, matching nix's own CLI
		// attribute-path syntax) - strip those quotes back off to match names.
		name := strings.Trim(r.Attr, `"`)
		paths[name] = out
	}

	for _, n := range names {
		if _, ok := paths[n]; !ok {
			return nil, fmt.Errorf("nix-fast-build: %s: missing from build results", n)
		}
	}

	return paths, nil
}
