package nixutil

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
	"testing"
)

const fakeResolvedURL = "git+ssh://git.example.net/vbargl/nixfiles?dir=nix&ref=main.v2"

// stubFastBuild replaces execCommand so BuildMany's real invocations never
// run: `nix flake metadata <flake> --json` is answered with fakeResolvedURL,
// and the nix-fast-build call has its --result-file path located in its args
// and resultJSON written there, mimicking what a real run would leave behind
// for BuildMany to parse.
func stubFastBuild(t *testing.T, resultJSON string) *[]call {
	t.Helper()
	calls := &[]call{}
	orig := execCommand
	execCommand = func(name string, args ...string) *exec.Cmd {
		*calls = append(*calls, call{name: name, args: args})
		if name == "nix" && len(args) > 0 && args[0] == "flake" {
			return exec.Command("printf", "%s", fmt.Sprintf(`{"resolvedUrl": %q}`, fakeResolvedURL))
		}
		for i, a := range args {
			if a == "--result-file" && i+1 < len(args) {
				if err := os.WriteFile(args[i+1], []byte(resultJSON), 0o644); err != nil {
					t.Fatalf("writing fake result file: %v", err)
				}
			}
		}
		return exec.Command("true")
	}
	t.Cleanup(func() { execCommand = orig })
	return calls
}

func TestResolveFlakeURL(t *testing.T) {
	orig := execCommand
	execCommand = func(name string, args ...string) *exec.Cmd {
		return exec.Command("printf", "%s", `{"resolvedUrl": "git+file:///repo?dir=nix"}`)
	}
	t.Cleanup(func() { execCommand = orig })

	got, err := resolveFlakeURL("vbargl2")
	if err != nil {
		t.Fatalf("resolveFlakeURL: unexpected error: %v", err)
	}
	if got != "git+file:///repo?dir=nix" {
		t.Errorf("resolveFlakeURL(vbargl2) = %q, want %q", got, "git+file:///repo?dir=nix")
	}
}

func TestBuildManyArgs(t *testing.T) {
	resultJSON := `{"results": [
		{"attr": "\"terminal.admintools\"", "type": "BUILD", "success": true, "outputs": {"out": "/nix/store/aaa-profile-terminal.admintools"}},
		{"attr": "media", "type": "BUILD", "success": true, "outputs": {"out": "/nix/store/bbb-profile-media"}}
	]}`
	calls := stubFastBuild(t, resultJSON)

	got, err := BuildMany("vbargl2", "x86_64-linux", []string{"terminal.admintools", "media"}, false)
	if err != nil {
		t.Fatalf("BuildMany: unexpected error: %v", err)
	}

	want := map[string]string{
		"terminal.admintools": "/nix/store/aaa-profile-terminal.admintools",
		"media":               "/nix/store/bbb-profile-media",
	}
	for name, path := range want {
		if got[name] != path {
			t.Errorf("BuildMany()[%q] = %q, want %q", name, got[name], path)
		}
	}

	if len(*calls) != 2 {
		t.Fatalf("BuildMany calls = %#v, want exactly 2 (resolve, then nix-fast-build)", *calls)
	}
	resolveArgs := (*calls)[0].args
	if (*calls)[0].name != "nix" || strings.Join(resolveArgs, " ") != "flake metadata vbargl2 --json" {
		t.Errorf("BuildMany first call = %q %v, want a 'nix flake metadata vbargl2 --json' resolution", (*calls)[0].name, resolveArgs)
	}

	args := (*calls)[1].args
	if (*calls)[1].name != NixFastBuildPath {
		t.Errorf("BuildMany command = %q, want %q", (*calls)[1].name, NixFastBuildPath)
	}
	joined := strings.Join(args, " ")
	wantFlake := "--flake " + fakeResolvedURL + "#profileConfigurations.x86_64-linux"
	if !strings.Contains(joined, wantFlake) {
		t.Errorf("BuildMany args = %v, want %q (the *resolved* URL, not the raw registry ref - nix-eval-jobs refuses indirect refs)", args, wantFlake)
	}
	if !strings.Contains(joined, `"terminal.admintools" = null`) || !strings.Contains(joined, `"media" = null`) {
		t.Errorf("BuildMany args = %v, want a --select intersecting both names", args)
	}
	if !strings.Contains(joined, "--result-format json") {
		t.Errorf("BuildMany args = %v, want --result-format json", args)
	}
}

func TestBuildManyRefreshSetsTarballTTLZero(t *testing.T) {
	resultJSON := `{"results": [
		{"attr": "media", "type": "BUILD", "success": true, "outputs": {"out": "/nix/store/bbb-profile-media"}}
	]}`
	calls := stubFastBuild(t, resultJSON)

	if _, err := BuildMany("vbargl2", "x86_64-linux", []string{"media"}, true); err != nil {
		t.Fatalf("BuildMany: unexpected error: %v", err)
	}

	joined := strings.Join((*calls)[len(*calls)-1].args, " ")
	if !strings.Contains(joined, "--option tarball-ttl 0") {
		t.Errorf("BuildMany(refresh=true) args = %v, want --option tarball-ttl 0 (nix-fast-build has no native --refresh)", (*calls)[len(*calls)-1].args)
	}
}

func TestBuildManyPropagatesBuildFailure(t *testing.T) {
	resultJSON := `{"results": [
		{"attr": "media", "type": "BUILD", "success": false, "error": "boom"}
	]}`
	stubFastBuild(t, resultJSON)

	_, err := BuildMany("vbargl2", "x86_64-linux", []string{"media"}, false)
	if err == nil || !strings.Contains(err.Error(), "boom") {
		t.Fatalf("BuildMany: expected error containing 'boom', got %v", err)
	}
}

func TestBuildManyErrorsOnMissingResult(t *testing.T) {
	resultJSON := `{"results": [
		{"attr": "media", "type": "BUILD", "success": true, "outputs": {"out": "/nix/store/bbb-profile-media"}}
	]}`
	stubFastBuild(t, resultJSON)

	_, err := BuildMany("vbargl2", "x86_64-linux", []string{"media", "gaming"}, false)
	if err == nil || !strings.Contains(err.Error(), "gaming") {
		t.Fatalf("BuildMany: expected error mentioning missing 'gaming', got %v", err)
	}
}
