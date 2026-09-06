package nixutil

import (
	"fmt"
	"runtime"
)

// CurrentSystem resolves the current nix "system" string (e.g.
// "x86_64-linux"), used to expand a bare profile name into a full
// profileConfigurations.<system>.<name> flake output path.
func CurrentSystem() (string, error) {
	out, err := execCommand("nix", "eval", "--raw", "--impure", "--expr", "builtins.currentSystem").Output()
	if err == nil {
		return string(out), nil
	}

	// Fall back to a best-effort mapping if `nix` isn't reachable for some
	// reason; covers the two platforms this repo actually targets.
	switch runtime.GOOS + "/" + runtime.GOARCH {
	case "linux/amd64":
		return "x86_64-linux", nil
	case "linux/arm64":
		return "aarch64-linux", nil
	default:
		return "", fmt.Errorf("determining current nix system: %w", err)
	}
}
