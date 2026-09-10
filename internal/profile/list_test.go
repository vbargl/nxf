package profile

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/vbargl/nxf/internal/nixutil"
	"github.com/vbargl/nxf/internal/paths"
)

func writeManifest(t *testing.T, profileLink, name, jsonBody string) {
	t.Helper()
	dir := filepath.Join(profileLink, "share", "nxf", "profiles", name)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "nxf.json"), []byte(jsonBody), 0o644); err != nil {
		t.Fatal(err)
	}
}

func touchFile(t *testing.T, path string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestCollectProfilesJSON(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_STATE_HOME", filepath.Join(home, "state"))
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, "config"))
	t.Setenv("XDG_DATA_HOME", filepath.Join(home, "data"))

	profile := filepath.Join(home, "nix-profile")
	t.Setenv("NXF_PROFILE", profile)

	store := filepath.Join(home, "store-gui.daily")
	touchFile(t, filepath.Join(store, "bin", "ghostty"))
	touchFile(t, filepath.Join(store, "bin", "keepassxc"))
	touchFile(t, filepath.Join(store, "share", "applications", "com.mitchellh.ghostty.desktop"))
	for _, rel := range []string{"share/icons", "share/fonts"} {
		if err := os.MkdirAll(filepath.Join(store, filepath.FromSlash(rel)), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	writeManifest(t, profile, "gui.daily", `{"name":"gui.daily","units":{"terminal":"/nix/store/u1","syncthing":"/nix/store/u2"},"manualUnits":["syncthing"]}`)
	writeManifest(t, profile, "media", `{"name":"media","units":{}}`)

	act := filepath.Join(home, "act.sh")
	if err := os.WriteFile(act, []byte("#!/bin/sh\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	hookDir := filepath.Join(profile, "etc", "nxf", "hooks", "gui.daily")
	if err := os.MkdirAll(hookDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(act, filepath.Join(hookDir, paths.ActivationHookName)); err != nil {
		t.Fatal(err)
	}

	orig := listElements
	listElements = func() ([]nixutil.Element, error) {
		return []nixutil.Element{{
			Name:        "gui.daily",
			OriginalURL: "flake:vbargl",
			LockedURL:   "git+ssh://git.example.net/nixfiles",
			Priority:    5,
			StorePaths:  []string{store},
		}}, nil
	}
	t.Cleanup(func() { listElements = orig })

	infos, err := collectProfiles(nil)
	if err != nil {
		t.Fatalf("collectProfiles: %v", err)
	}
	raw, err := json.MarshalIndent(profilesJSON{Profiles: infos}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}

	var parsed struct {
		Profiles []struct {
			Name     string `json:"name"`
			NixName  string `json:"nixName"`
			Flake    string `json:"flake"`
			Locked   string `json:"locked"`
			Priority int    `json:"priority"`
			Store    string `json:"store"`
			Units    []struct {
				Name      string `json:"name"`
				AutoStart bool   `json:"autoStart"`
			} `json:"units"`
			Bins       []string `json:"bins"`
			Desktop    []string `json:"desktop"`
			XDG        []string `json:"xdg"`
			Activate   *string  `json:"activate"`
			Deactivate *string  `json:"deactivate"`
		} `json:"profiles"`
	}
	if err := json.Unmarshal(raw, &parsed); err != nil {
		t.Fatalf("unmarshal: %v\n%s", err, raw)
	}
	if len(parsed.Profiles) != 2 {
		t.Fatalf("profiles = %d, want 2\n%s", len(parsed.Profiles), raw)
	}
	if parsed.Profiles[0].Name != "gui.daily" || parsed.Profiles[1].Name != "media" {
		t.Fatalf("order/names = %q, %q", parsed.Profiles[0].Name, parsed.Profiles[1].Name)
	}

	got := parsed.Profiles[0]
	if got.NixName != "gui.daily" || got.Flake != "flake:vbargl" || got.Locked != "git+ssh://git.example.net/nixfiles" {
		t.Errorf("nix fields = %+v", got)
	}
	if got.Priority != 5 || got.Store != store {
		t.Errorf("priority/store = %d %q", got.Priority, got.Store)
	}
	if got.Activate == nil || *got.Activate != act {
		t.Errorf("activate = %#v, want %s", got.Activate, act)
	}
	if got.Deactivate != nil {
		t.Errorf("deactivate = %#v, want null", got.Deactivate)
	}
	if len(got.Bins) != 2 || got.Bins[0] != "ghostty" || got.Bins[1] != "keepassxc" {
		t.Errorf("bins = %#v", got.Bins)
	}
	if len(got.Desktop) != 1 || got.Desktop[0] != "com.mitchellh.ghostty.desktop" {
		t.Errorf("desktop = %#v", got.Desktop)
	}
	if len(got.XDG) != 2 || got.XDG[0] != "share/icons" || got.XDG[1] != "share/fonts" {
		t.Errorf("xdg = %#v", got.XDG)
	}

	wantUnits := map[string]bool{
		"nxf-gui.daily-syncthing.service": false,
		"nxf-gui.daily-terminal.service":  true,
	}
	if len(got.Units) != 2 {
		t.Fatalf("units = %#v", got.Units)
	}
	for _, u := range got.Units {
		auto, ok := wantUnits[u.Name]
		if !ok {
			t.Errorf("unexpected unit %q", u.Name)
			continue
		}
		if u.AutoStart != auto {
			t.Errorf("unit %s autoStart = %v, want %v", u.Name, u.AutoStart, auto)
		}
	}

	media := parsed.Profiles[1]
	if media.Bins == nil || media.Desktop == nil || media.XDG == nil || media.Units == nil {
		t.Errorf("media empty slices must be [] not null: bins=%#v desktop=%#v xdg=%#v units=%#v",
			media.Bins, media.Desktop, media.XDG, media.Units)
	}
	if media.Activate != nil || media.Deactivate != nil {
		t.Errorf("media hooks = activate %#v deactivate %#v, want null", media.Activate, media.Deactivate)
	}

	filtered, err := collectProfiles([]string{"media"})
	if err != nil {
		t.Fatalf("filter: %v", err)
	}
	if len(filtered) != 1 || filtered[0].Name != "media" {
		t.Errorf("filter = %#v", filtered)
	}
	if _, err := collectProfiles([]string{"missing"}); err == nil {
		t.Error("filter missing: expected error")
	}

	prefixed, err := collectProfiles([]string{"gui"})
	if err != nil {
		t.Fatalf("prefix filter: %v", err)
	}
	if len(prefixed) != 1 || prefixed[0].Name != "gui.daily" {
		t.Errorf("prefix gui = %#v", prefixed)
	}
}

func TestFilterProfilesOrderAndPrefix(t *testing.T) {
	infos := []profileInfo{
		{Name: "dev.default"},
		{Name: "gui.admintools"},
		{Name: "gui.daily"},
		{Name: "media"},
		{Name: "terminal.admintools"},
	}
	got, err := filterProfiles(infos, []string{"media", "gui", "terminal.admintools"})
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, p := range got {
		names = append(names, p.Name)
	}
	want := []string{"media", "gui.admintools", "gui.daily", "terminal.admintools"}
	if strings.Join(names, ",") != strings.Join(want, ",") {
		t.Errorf("got %v, want %v", names, want)
	}
	if _, err := filterProfiles(infos, []string{"nope"}); err == nil {
		t.Error("expected unknown profile error")
	}
}

func TestCompactTable(t *testing.T) {
	got := compactTable([]profileInfo{
		{Name: "gui.daily", Bins: []string{"a", "b"}, Desktop: []string{"x.desktop"}, XDG: []string{"share/icons"}},
		{Name: "media", Units: []unitInfo{{Name: "u.service", AutoStart: true}}},
	})
	if !strings.Contains(got, "NAME") || !strings.Contains(got, "BINS") {
		t.Fatalf("missing header: %s", got)
	}
	if !strings.Contains(got, "gui.daily") || !strings.Contains(got, "media") {
		t.Fatalf("missing rows: %s", got)
	}
	if strings.Contains(got, "HOOKS") {
		t.Fatalf("HOOKS column should be omitted when empty: %s", got)
	}

	act := "/nix/store/act"
	got = compactTable([]profileInfo{{Name: "media", Activate: &act}})
	if !strings.Contains(got, "HOOKS") || !strings.Contains(got, "activate") {
		t.Fatalf("HOOKS should appear when a profile has a hook: %s", got)
	}
}

func TestVerboseMeta(t *testing.T) {
	locked := "git+ssh://git.example.net/nixfiles?dir=nix&ref=v2&rev=b87f6a4077512ad335a7e746966669a253a52ca2"
	p := profileInfo{
		Name:     "gui.daily",
		Flake:    "flake:vbargl",
		Locked:   locked,
		Priority: 5,
		Store:    "/nix/store/abc-gui.daily",
		Bins:     []string{".ghostty-wrapped", "ghostty", "keepassxc"},
		Desktop:  []string{"com.mitchellh.ghostty.desktop"},
		XDG:      []string{"share/icons", "share/fonts"},
	}
	got := verboseBlock(p, 12)
	if strings.Contains(got, "prio") {
		t.Errorf("default prio 5 should be omitted:\n%s", got)
	}
	if !strings.Contains(got, "flake:vbargl (b87f6a4)") {
		t.Errorf("want flake + short rev:\n%s", got)
	}
	if !strings.Contains(got, "store") || !strings.Contains(got, "/nix/store/abc-gui.daily") {
		t.Errorf("want labeled store path:\n%s", got)
	}
	if strings.Contains(got, ".ghostty-wrapped") {
		t.Errorf("wrapped bins should be hidden:\n%s", got)
	}
	if !strings.Contains(got, "ghostty") || !strings.Contains(got, "keepassxc") {
		t.Errorf("want visible bins:\n%s", got)
	}
	if !strings.Contains(got, "com.mitchellh.ghostty") || strings.Contains(got, "com.mitchellh.ghostty.desktop") {
		t.Errorf("desktop should drop .desktop suffix:\n%s", got)
	}

	p.Priority = 3
	got = verboseBlock(p, 12)
	if !strings.Contains(got, "prio 3") {
		t.Errorf("non-default prio should show:\n%s", got)
	}
}

func TestCollectProfilesJSONEmpty(t *testing.T) {
	t.Setenv("NXF_PROFILE", t.TempDir())
	t.Setenv("XDG_STATE_HOME", t.TempDir())
	orig := listElements
	listElements = func() ([]nixutil.Element, error) { return nil, nil }
	t.Cleanup(func() { listElements = orig })

	infos, err := collectProfiles(nil)
	if err != nil {
		t.Fatalf("collectProfiles: %v", err)
	}
	raw, err := json.MarshalIndent(profilesJSON{Profiles: infos}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	var parsed map[string][]json.RawMessage
	if err := json.Unmarshal(raw, &parsed); err != nil {
		t.Fatal(err)
	}
	if len(parsed["profiles"]) != 0 {
		t.Errorf("empty JSON = %s", raw)
	}
}
