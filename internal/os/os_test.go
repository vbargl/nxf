package os

import (
	"os"
	"os/exec"
	"reflect"
	"testing"
)

func TestSplitTarget(t *testing.T) {
	hostname, err := os.Hostname()
	if err != nil {
		t.Fatalf("determining hostname: %v", err)
	}

	cases := []struct {
		name      string
		ref       string
		wantFlake string
		wantHost  string
	}{
		{
			name:      "empty ref defaults flake and host",
			ref:       "",
			wantFlake: ".",
			wantHost:  hostname,
		},
		{
			name:      "bare host, no #",
			ref:       "saber",
			wantFlake: ".",
			wantHost:  "saber",
		},
		{
			name:      "flake#host pair",
			ref:       ".#saber",
			wantFlake: ".",
			wantHost:  "saber",
		},
		{
			name:      "path flake#host pair",
			ref:       "/home/vbargl/personal/nixfiles/nix#ash-twin",
			wantFlake: "/home/vbargl/personal/nixfiles/nix",
			wantHost:  "ash-twin",
		},
		{
			name:      "empty flake before # keeps default, mirrors nix's #attr shorthand",
			ref:       "#saber",
			wantFlake: ".",
			wantHost:  "saber",
		},
		{
			name:      "flake with nothing after # defaults host",
			ref:       ".#",
			wantFlake: ".",
			wantHost:  hostname,
		},
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			flake, host, err := splitTarget(c.ref)
			if err != nil {
				t.Fatalf("splitTarget(%q): unexpected error: %v", c.ref, err)
			}
			if flake != c.wantFlake {
				t.Errorf("splitTarget(%q) flake = %q, want %q", c.ref, flake, c.wantFlake)
			}
			if host != c.wantHost {
				t.Errorf("splitTarget(%q) host = %q, want %q", c.ref, host, c.wantHost)
			}
		})
	}
}

func TestSplitTargetRespectsNxfFlakeEnv(t *testing.T) {
	t.Setenv("NXF_FLAKE", "/etc/nixos")

	flake, host, err := splitTarget("saber")
	if err != nil {
		t.Fatalf("splitTarget: unexpected error: %v", err)
	}
	if flake != "/etc/nixos" {
		t.Errorf("splitTarget flake = %q, want %q (from NXF_FLAKE)", flake, "/etc/nixos")
	}
	if host != "saber" {
		t.Errorf("splitTarget host = %q, want %q", host, "saber")
	}
}

func TestRunNixosRebuildArgs(t *testing.T) {
	cases := []struct {
		name     string
		euid     int
		wantArgs []string
	}{
		{
			name:     "root: no --sudo",
			euid:     0,
			wantArgs: []string{"switch", "--flake", ".#saber"},
		},
		{
			name:     "non-root: appends --sudo so nixos-rebuild self-elevates",
			euid:     1000,
			wantArgs: []string{"switch", "--flake", ".#saber", "--sudo"},
		},
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			var got struct {
				name string
				args []string
			}
			origExec, origEuid := execCommand, geteuid
			execCommand = func(name string, args ...string) *exec.Cmd {
				got.name, got.args = name, args
				return exec.Command("true")
			}
			geteuid = func() int { return c.euid }
			t.Cleanup(func() { execCommand, geteuid = origExec, origEuid })

			if err := runNixosRebuild("switch", ".#saber"); err != nil {
				t.Fatalf("runNixosRebuild: unexpected error: %v", err)
			}
			if got.name != "nixos-rebuild" {
				t.Errorf("runNixosRebuild command = %q, want %q", got.name, "nixos-rebuild")
			}
			if !reflect.DeepEqual(got.args, c.wantArgs) {
				t.Errorf("runNixosRebuild args = %v, want %v", got.args, c.wantArgs)
			}
		})
	}
}
