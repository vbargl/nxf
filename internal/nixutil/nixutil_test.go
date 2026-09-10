package nixutil

import (
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

// call records one execCommand invocation's name and args.
type call struct {
	name string
	args []string
}

// stubExec replaces execCommand for the duration of the test, returning the
// slice of calls it records. Every call gets a harmless stand-in command
// (either "true" or, if output is non-empty, one that prints it to stdout)
// rather than actually running nix/nom/nvd.
func stubExec(t *testing.T, output string) *[]call {
	t.Helper()
	calls := &[]call{}
	orig := execCommand
	execCommand = func(name string, args ...string) *exec.Cmd {
		*calls = append(*calls, call{name: name, args: args})
		if output != "" {
			return exec.Command("printf", "%s", output)
		}
		return exec.Command("true")
	}
	t.Cleanup(func() { execCommand = orig })
	return calls
}

func setUseNom(t *testing.T, v bool) {
	t.Helper()
	orig := useNom
	useNom = v
	t.Cleanup(func() { useNom = orig })
}

func TestRunNixArgs(t *testing.T) {
	calls := stubExec(t, "")
	if err := RunNix("profile", "add", "/nix/store/xyz"); err != nil {
		t.Fatalf("RunNix: unexpected error: %v", err)
	}
	want := []call{{name: "nix", args: []string{"profile", "add", "/nix/store/xyz"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("RunNix calls = %#v, want %#v", *calls, want)
	}
}

func TestProfileAddArgs(t *testing.T) {
	calls := stubExec(t, "")
	if err := ProfileAdd("/nix/store/built-path"); err != nil {
		t.Fatalf("ProfileAdd: unexpected error: %v", err)
	}
	want := []call{{name: "nix", args: []string{"profile", "add", "/nix/store/built-path"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("ProfileAdd calls = %#v, want %#v", *calls, want)
	}
}

func TestProfileRemoveArgs(t *testing.T) {
	calls := stubExec(t, "")
	if err := ProfileRemove("profile-media"); err != nil {
		t.Fatalf("ProfileRemove: unexpected error: %v", err)
	}
	want := []call{{name: "nix", args: []string{"profile", "remove", "profile-media"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("ProfileRemove calls = %#v, want %#v", *calls, want)
	}
}

func TestProfileRemoveQuietArgs(t *testing.T) {
	calls := stubExec(t, "")
	ProfileRemoveQuiet("profile-media")
	want := []call{{name: "nix", args: []string{"profile", "remove", "profile-media"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("ProfileRemoveQuiet calls = %#v, want %#v", *calls, want)
	}
}

func TestProfileRollbackArgs(t *testing.T) {
	calls := stubExec(t, "")
	if err := ProfileRollback(); err != nil {
		t.Fatalf("ProfileRollback: unexpected error: %v", err)
	}
	want := []call{{name: "nix", args: []string{"profile", "rollback"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("ProfileRollback calls = %#v, want %#v", *calls, want)
	}
}

func TestRunNixTreeWithoutNom(t *testing.T) {
	setUseNom(t, false)
	calls := stubExec(t, "")
	if err := runNixTree("build", "vbargl2#profileConfigurations.x86_64-linux.media"); err != nil {
		t.Fatalf("runNixTree: unexpected error: %v", err)
	}
	// useNom=false falls back to a single plain RunNix call - nom is never
	// invoked, and none of nom's extra --log-format/--verbose flags are added.
	want := []call{{name: "nix", args: []string{"build", "vbargl2#profileConfigurations.x86_64-linux.media"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("runNixTree(useNom=false) calls = %#v, want %#v", *calls, want)
	}
}

func TestRunNixTreeWithNom(t *testing.T) {
	setUseNom(t, true)
	calls := stubExec(t, "")
	if err := runNixTree("build", "vbargl2#profileConfigurations.x86_64-linux.media"); err != nil {
		t.Fatalf("runNixTree: unexpected error: %v", err)
	}
	want := []call{
		{name: "nix", args: []string{
			"build", "vbargl2#profileConfigurations.x86_64-linux.media",
			"--log-format", "internal-json", "--verbose",
		}},
		{name: NomPath, args: []string{"--json"}},
	}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("runNixTree(useNom=true) calls = %#v, want %#v", *calls, want)
	}
}

func TestBuildRefreshAppendsFlag(t *testing.T) {
	setUseNom(t, false)

	// Build resolves the real --out-link symlink it built against (via
	// filepath.EvalSymlinks, which requires the target to actually exist) -
	// unlike stubExec, the stub here has to create both for Build to succeed.
	fakeStorePath := filepath.Join(t.TempDir(), "fake-media")
	if err := os.Mkdir(fakeStorePath, 0o755); err != nil {
		t.Fatalf("creating fake store path: %v", err)
	}
	calls := &[]call{}
	orig := execCommand
	execCommand = func(name string, args ...string) *exec.Cmd {
		*calls = append(*calls, call{name: name, args: args})
		for i, a := range args {
			if a == "--out-link" && i+1 < len(args) {
				_ = os.Symlink(fakeStorePath, args[i+1])
			}
		}
		return exec.Command("true")
	}
	t.Cleanup(func() { execCommand = orig })

	if _, err := Build("vbargl2#profileConfigurations.x86_64-linux.media", true); err != nil {
		t.Fatalf("Build: unexpected error: %v", err)
	}

	if len(*calls) != 1 {
		t.Fatalf("Build calls = %#v, want exactly 1", *calls)
	}
	args := (*calls)[0].args
	if len(args) == 0 || args[len(args)-1] != "--refresh" {
		t.Errorf("Build(refresh=true) args = %v, want a trailing --refresh", args)
	}
}

func TestCurrentSystemUsesNixEval(t *testing.T) {
	calls := stubExec(t, "x86_64-linux")
	got, err := CurrentSystem()
	if err != nil {
		t.Fatalf("CurrentSystem: unexpected error: %v", err)
	}
	if got != "x86_64-linux" {
		t.Errorf("CurrentSystem() = %q, want %q", got, "x86_64-linux")
	}
	want := []call{{name: "nix", args: []string{"eval", "--raw", "--impure", "--expr", "builtins.currentSystem"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("CurrentSystem calls = %#v, want %#v", *calls, want)
	}
}

func TestCurrentSystemFallsBackWhenNixFails(t *testing.T) {
	orig := execCommand
	execCommand = func(name string, args ...string) *exec.Cmd {
		return exec.Command("false") // always exits non-zero
	}
	t.Cleanup(func() { execCommand = orig })

	got, err := CurrentSystem()
	if err != nil {
		t.Fatalf("CurrentSystem: unexpected error: %v", err)
	}
	// This suite only runs on x86_64-linux/aarch64-linux dev machines, so the
	// runtime.GOOS/GOARCH fallback is deterministic here.
	if got != "x86_64-linux" && got != "aarch64-linux" {
		t.Errorf("CurrentSystem() fallback = %q, want a linux system string", got)
	}
}

func resetEmptyBaselineCache(t *testing.T) {
	t.Helper()
	orig := emptyBaselinePath
	emptyBaselinePath = ""
	t.Cleanup(func() { emptyBaselinePath = orig })
}

func TestEmptyBaselineArgsAndCaching(t *testing.T) {
	resetEmptyBaselineCache(t)
	calls := stubExec(t, "/nix/store/fake-empty-baseline\n")

	got, err := emptyBaseline()
	if err != nil {
		t.Fatalf("emptyBaseline: unexpected error: %v", err)
	}
	if got != "/nix/store/fake-empty-baseline" {
		t.Errorf("emptyBaseline() = %q, want trimmed path", got)
	}
	if len(*calls) != 1 || (*calls)[0].name != "nix" || (*calls)[0].args[0] != "build" {
		t.Fatalf("emptyBaseline calls = %#v, want a single 'nix build ...' call", *calls)
	}
	if !strings.Contains(strings.Join((*calls)[0].args, " "), "nxf-empty-baseline") {
		t.Errorf("emptyBaseline args = %v, want the derivation expr embedded", (*calls)[0].args)
	}

	// Second call must be served from the cache - no further exec calls.
	if _, err := emptyBaseline(); err != nil {
		t.Fatalf("emptyBaseline (cached): unexpected error: %v", err)
	}
	if len(*calls) != 1 {
		t.Errorf("emptyBaseline should be cached after the first build, got %d calls", len(*calls))
	}
}

func TestShowDiffBothEmptySkipsEntirely(t *testing.T) {
	calls := stubExec(t, "")
	if err := ShowDiff("", ""); err != nil {
		t.Fatalf("ShowDiff: unexpected error: %v", err)
	}
	if len(*calls) != 0 {
		t.Errorf("ShowDiff(\"\", \"\") calls = %#v, want none", *calls)
	}
}

func TestShowDiffEqualPathsSkipsNvd(t *testing.T) {
	calls := stubExec(t, "")
	if err := ShowDiff("/nix/store/same", "/nix/store/same"); err != nil {
		t.Fatalf("ShowDiff: unexpected error: %v", err)
	}
	if len(*calls) != 0 {
		t.Errorf("ShowDiff(equal, equal) calls = %#v, want none (no-op, no nvd call)", *calls)
	}
}

func TestShowDiffRunsNvd(t *testing.T) {
	calls := stubExec(t, "")
	if err := ShowDiff("/nix/store/old", "/nix/store/new"); err != nil {
		t.Fatalf("ShowDiff: unexpected error: %v", err)
	}
	want := []call{{name: NvdPath, args: []string{"diff", "/nix/store/old", "/nix/store/new"}}}
	if !reflect.DeepEqual(*calls, want) {
		t.Errorf("ShowDiff calls = %#v, want %#v", *calls, want)
	}
}

func TestShowDiffSubstitutesEmptyBaseline(t *testing.T) {
	resetEmptyBaselineCache(t)
	calls := stubExec(t, "/nix/store/empty-baseline\n")
	if err := ShowDiff("", "/nix/store/new"); err != nil {
		t.Fatalf("ShowDiff: unexpected error: %v", err)
	}
	if len(*calls) != 2 {
		t.Fatalf("ShowDiff(\"\", ...) calls = %#v, want 2 (empty-baseline build, then nvd diff)", *calls)
	}
	if (*calls)[0].name != "nix" {
		t.Errorf("first call = %#v, want the empty-baseline nix build", (*calls)[0])
	}
	want := call{name: NvdPath, args: []string{"diff", "/nix/store/empty-baseline", "/nix/store/new"}}
	if !reflect.DeepEqual((*calls)[1], want) {
		t.Errorf("second call = %#v, want %#v", (*calls)[1], want)
	}
}
