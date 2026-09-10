package os

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/vbargl/nxf/internal/gens"
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

func TestCollectOSGenerationsJSON(t *testing.T) {
	dir := t.TempDir()
	now := time.Date(2026, 9, 9, 12, 35, 51, 0, time.UTC)
	mk := func(n int, current bool, nixos, kernel string) {
		store := filepath.Join(dir, "store-"+itoa(n))
		if err := os.MkdirAll(filepath.Join(store, "kernel-modules", "lib", "modules", kernel), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(store, "nixos-version"), []byte(nixos+"\n"), 0o644); err != nil {
			t.Fatal(err)
		}
		link := filepath.Join(dir, "system-"+itoa(n)+"-link")
		if err := os.Symlink(store, link); err != nil {
			t.Fatal(err)
		}
		_ = os.Chtimes(link, now.Add(time.Duration(n)*time.Hour), now.Add(time.Duration(n)*time.Hour))
		if current {
			if err := os.Symlink("system-"+itoa(n)+"-link", filepath.Join(dir, "system")); err != nil {
				t.Fatal(err)
			}
		}
	}
	mk(1, false, "26.11.old", "7.2.1")
	mk(2, true, "26.11.test", "7.2.2")

	origProfile, origExec := systemProfile, execCommand
	systemProfile = filepath.Join(dir, "system")
	execCommand = func(name string, args ...string) *exec.Cmd {
		return exec.Command("false")
	}
	t.Cleanup(func() { systemProfile, execCommand = origProfile, origExec })

	items, err := collectOSGenerations()
	if err != nil {
		t.Fatalf("collectOSGenerations: %v", err)
	}
	raw, err := json.MarshalIndent(struct {
		Generations []gens.JSONGeneration `json:"generations"`
	}{items}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	var parsed struct {
		Generations []struct {
			Number        int    `json:"number"`
			Time          string `json:"time"`
			Current       bool   `json:"current"`
			NixosVersion  string `json:"nixosVersion"`
			KernelVersion string `json:"kernelVersion"`
		} `json:"generations"`
	}
	if err := json.Unmarshal(raw, &parsed); err != nil {
		t.Fatalf("unmarshal: %v\n%s", err, raw)
	}
	if len(parsed.Generations) != 2 {
		t.Fatalf("len = %d\n%s", len(parsed.Generations), raw)
	}
	cur := parsed.Generations[0]
	if cur.Number != 2 || !cur.Current || cur.NixosVersion != "26.11.test" || cur.KernelVersion != "7.2.2" {
		t.Errorf("current = %+v", cur)
	}
	if _, err := time.Parse(time.RFC3339, cur.Time); err != nil {
		t.Errorf("time %q is not RFC3339: %v", cur.Time, err)
	}
	old := parsed.Generations[1]
	if old.Number != 1 || old.Current || old.NixosVersion != "26.11.old" || old.KernelVersion != "7.2.1" {
		t.Errorf("older = %+v", old)
	}
}

func TestFormatOSTable(t *testing.T) {
	got := formatOSTable([]gens.JSONGeneration{
		{Number: 38, Time: "2026-09-09T12:35:51Z", Current: true, NixosVersion: "26.11.34ab990", KernelVersion: "7.2.2"},
		{Number: 37, Time: "2026-09-09T12:30:11Z", Current: false, NixosVersion: "26.05.5dfba62", KernelVersion: "6.18.48"},
	})
	if !strings.Contains(got, "#") || !strings.Contains(got, "BUILT") || !strings.Contains(got, "NIXOS") {
		t.Fatalf("missing header: %s", got)
	}
	if !strings.Contains(got, "> 38") {
		t.Fatalf("current marker should be '> 38': %q", got)
	}
	lines := strings.Split(strings.TrimRight(got, "\n"), "\n")
	var current, older string
	for _, ln := range lines {
		if strings.Contains(ln, "38") && strings.Contains(ln, "7.2.2") {
			current = ln
		}
		if strings.Contains(ln, "37") && strings.Contains(ln, "6.18.48") {
			older = ln
		}
	}
	if current == "" || older == "" {
		t.Fatalf("rows missing:\n%s", got)
	}
	if !strings.Contains(strings.TrimLeft(current, " "), ">") {
		t.Errorf("current row should start with >: %q", current)
	}
	if strings.Contains(older, ">") {
		t.Errorf("older row should not have >: %q", older)
	}
}

func TestShortNixos(t *testing.T) {
	if got := shortNixos("26.11.20260831.34ab990"); got != "26.11.34ab990" {
		t.Errorf("shortNixos = %q", got)
	}
	if got := shortNixos("26.05"); got != "26.05" {
		t.Errorf("passthrough = %q", got)
	}
}

func TestParseRebuildDate(t *testing.T) {
	got := parseRebuildDate("2026-09-09 12:35:51")
	if _, err := time.Parse(time.RFC3339, got); err != nil {
		t.Errorf("parseRebuildDate = %q, not RFC3339: %v", got, err)
	}
	if got := parseRebuildDate("2026-09-09T12:35:51Z"); got != "2026-09-09T12:35:51Z" {
		t.Errorf("RFC3339 passthrough = %q", got)
	}
}

func TestCollectOSGenerationsJSONEmpty(t *testing.T) {
	dir := t.TempDir()
	link := filepath.Join(dir, "system")
	if err := os.Mkdir(link, 0o755); err != nil {
		t.Fatal(err)
	}
	origProfile, origExec := systemProfile, execCommand
	systemProfile = link
	execCommand = func(name string, args ...string) *exec.Cmd { return exec.Command("false") }
	t.Cleanup(func() { systemProfile, execCommand = origProfile, origExec })

	items, err := collectOSGenerations()
	if err != nil {
		t.Fatalf("empty: %v", err)
	}
	if items == nil || len(items) != 0 {
		t.Errorf("empty = %#v, want []", items)
	}
}

func itoa(n int) string {
	return []string{"0", "1", "2", "3", "4", "5"}[n]
}
