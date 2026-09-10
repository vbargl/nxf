package profile

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/vbargl/nxf/internal/paths"
)

func mustRefsFile(t *testing.T) string {
	t.Helper()
	p, err := paths.ProfileRefsFile()
	if err != nil {
		t.Fatal(err)
	}
	return p
}

func writeFile(path string, data []byte) error {
	return os.WriteFile(path, data, 0o644)
}

func TestRefsRoundTrip(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())

	refs, err := loadRefs()
	if err != nil {
		t.Fatalf("loadRefs (no file yet): unexpected error: %v", err)
	}
	if len(refs) != 0 {
		t.Fatalf("loadRefs (no file yet) = %#v, want empty", refs)
	}

	rememberRef("dev.default", Ref{OriginalURL: "flake:vbargl", Installable: `flake:vbargl#profileConfigurations.x86_64-linux."dev.default"`})
	rememberRef("media", Ref{OriginalURL: "flake:vbargl", Installable: `flake:vbargl#profileConfigurations.x86_64-linux."media"`})

	refs, err = loadRefs()
	if err != nil {
		t.Fatalf("loadRefs: unexpected error: %v", err)
	}
	if refs["dev.default"].OriginalURL != "flake:vbargl" {
		t.Errorf("dev.default = %#v", refs["dev.default"])
	}

	rememberRef("media", Ref{OriginalURL: "other-flake", Installable: "other-flake#media"})
	refs, _ = loadRefs()
	if refs["media"].OriginalURL != "other-flake" {
		t.Errorf("loadRefs()[media] = %#v, want overwritten", refs["media"])
	}

	forgetRef("dev.default")
	refs, _ = loadRefs()
	if _, ok := refs["dev.default"]; ok {
		t.Errorf("loadRefs() still has dev.default after forgetRef")
	}
	forgetRef("never-added")
}

func TestLoadRefsMigratesLegacyStringMap(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())
	refsFile := mustRefsFile(t)
	if err := os.MkdirAll(filepath.Dir(refsFile), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := writeFile(refsFile, []byte(`{"media":"vbargl2"}`)); err != nil {
		t.Fatal(err)
	}
	got, err := loadRefs()
	if err != nil {
		t.Fatal(err)
	}
	if got["media"].OriginalURL != "vbargl2" {
		t.Errorf("legacy media = %#v", got["media"])
	}
}

func TestUpgradeInstallable(t *testing.T) {
	got, err := upgradeInstallable("gui.daily", Ref{OriginalURL: "flake:vbargl"})
	if err != nil {
		t.Fatal(err)
	}
	if want := `flake:vbargl#profileConfigurations.`; len(got) < len(want) || got[:len(want)] != want {
		t.Errorf("got %q, want prefix %q", got, want)
	}
	if _, err := upgradeInstallable("gui.daily", Ref{OriginalURL: "."}); err == nil {
		t.Error("relative original URL should fail")
	}
}
