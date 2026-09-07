// Package os builds and activates the NixOS system configuration via
// nixos-rebuild - the logic behind `nxf os`.
package os

import (
	"fmt"
	"os"
	"os/exec"
	"strings"

	"github.com/vbargl/nxf/internal/nixutil"
)

// Run builds ref's system closure, prints an nvd diff against
// /run/current-system, and - unless mode is "build" - activates it via
// nixos-rebuild. ref follows the same convention as `nh os switch` and `nix
// profile add`: either a bare host name (the flake defaults to ".", override
// with the NXF_FLAKE environment variable), or a "<flake>#<host>" pair like
// ".#saber" where the part before "#" overrides the flake path. An empty ref,
// or a ref with nothing after "#", defaults the host to the current hostname.
func Run(mode, ref string) error {
	flake, host, err := splitTarget(ref)
	if err != nil {
		return err
	}

	oldPath, _ := os.Readlink("/run/current-system")

	attrPath := fmt.Sprintf("%s#nixosConfigurations.%s.config.system.build.toplevel", flake, host)
	fmt.Printf("nxf: building %s\n", attrPath)
	newPath, err := nixutil.Build(attrPath, false)
	if err != nil {
		return err
	}

	if err := nixutil.ShowDiff(oldPath, newPath); err != nil {
		return err
	}

	if mode == "build" {
		return nil
	}

	target := fmt.Sprintf("%s#%s", flake, host)
	fmt.Printf("nxf: nixos-rebuild %s --flake %s\n", mode, target)

	return runNixosRebuild(mode, target)
}

// splitTarget resolves ref into a flake reference and host. ref is either a
// bare host name (flake stays at its default), or a "<flake>#<host>" pair
// like ".#saber" - the same convention `nh os switch` and `nix profile add`
// use. An empty part before "#" (e.g. "#saber") leaves the default flake in
// place, mirroring nix's own "#attr" shorthand for ".". host defaults to the
// current hostname when ref is empty or ends at "#" with nothing after it.
func splitTarget(ref string) (flake, host string, err error) {
	flake = os.Getenv("NXF_FLAKE")
	if flake == "" {
		flake = "."
	}

	if f, h, found := strings.Cut(ref, "#"); found {
		if f != "" {
			flake = f
		}
		host = h
	} else {
		host = ref
	}

	if host == "" {
		h, err := os.Hostname()
		if err != nil {
			return "", "", fmt.Errorf("determining hostname: %w", err)
		}
		host = h
	}

	return flake, host, nil
}

// execCommand constructs the nixos-rebuild command; overridden in tests so
// argument construction can be verified without actually rebuilding.
var execCommand = exec.Command

// geteuid is overridden in tests to simulate running as a non-root user.
var geteuid = os.Geteuid

func runNixosRebuild(mode, target string) error {
	args := []string{mode, "--flake", target}
	if geteuid() != 0 {
		// nixos-rebuild-ng doesn't self-elevate: without --sudo it just
		// errors out on activation ("also pass '--sudo' or run the command
		// as root"). Passing it here lets nixos-rebuild prompt for sudo
		// itself, only for the activation steps that actually need root.
		args = append(args, "--sudo")
	}
	cmd := execCommand("nixos-rebuild", args...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	return cmd.Run()
}
