package profile

import (
	"errors"
	"strings"
	"testing"

	"github.com/vbargl/nxf/internal/apply"
)

func fakeSystem(sys string, err error) func() (string, error) {
	return func() (string, error) { return sys, err }
}

func TestExpandProfileRef(t *testing.T) {
	cases := []struct {
		name         string
		ref          string
		wantExpanded string
		wantName     string
		wantErr      bool
	}{
		{
			name:         "simple name with no dots",
			ref:          "vbargl2#media",
			wantExpanded: `vbargl2#profileConfigurations.x86_64-linux."media"`,
			wantName:     "media",
		},
		{
			// Regression: nix's CLI attribute-path syntax splits on every
			// unquoted ".", so a profile name that is itself dot-joined
			// (see flake.nix's nameFor) must be quoted as a single segment
			// or nix looks for a nested "dev"."default" pair instead of the
			// flat "dev.default" key that actually exists.
			name:         "dot-containing name is quoted as one segment",
			ref:          "vbargl2#dev.default",
			wantExpanded: `vbargl2#profileConfigurations.x86_64-linux."dev.default"`,
			wantName:     "dev.default",
		},
		{
			name:         "multi-segment dotted name",
			ref:          "vbargl2#terminal.admintools",
			wantExpanded: `vbargl2#profileConfigurations.x86_64-linux."terminal.admintools"`,
			wantName:     "terminal.admintools",
		},
		{
			name:         "already-expanded ref passes through unchanged, name unquoted",
			ref:          `vbargl2#profileConfigurations.x86_64-linux."dev.default"`,
			wantExpanded: `vbargl2#profileConfigurations.x86_64-linux."dev.default"`,
			wantName:     "dev.default",
		},
		{
			name:         "already-expanded ref with an unquoted, non-dotted name",
			ref:          "vbargl2#profileConfigurations.x86_64-linux.gaming",
			wantExpanded: "vbargl2#profileConfigurations.x86_64-linux.gaming",
			wantName:     "gaming",
		},
		{
			name:         "flake ref with a path, not just a registry name",
			ref:          ".#gui.daily",
			wantExpanded: `.#profileConfigurations.x86_64-linux."gui.daily"`,
			wantName:     "gui.daily",
		},
		{
			name:    "missing # separator is an error",
			ref:     "vbargl2",
			wantErr: true,
		},
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			expanded, name, err := expandProfileRef(c.ref, fakeSystem("x86_64-linux", nil))
			if c.wantErr {
				if err == nil {
					t.Fatalf("expandProfileRef(%q): expected an error, got none", c.ref)
				}
				return
			}
			if err != nil {
				t.Fatalf("expandProfileRef(%q): unexpected error: %v", c.ref, err)
			}
			if expanded != c.wantExpanded {
				t.Errorf("expandProfileRef(%q) expanded = %q, want %q", c.ref, expanded, c.wantExpanded)
			}
			if name != c.wantName {
				t.Errorf("expandProfileRef(%q) name = %q, want %q", c.ref, name, c.wantName)
			}
		})
	}
}

func TestExpandProfileRefPropagatesSystemError(t *testing.T) {
	wantErr := errors.New("no nix on PATH")
	_, _, err := expandProfileRef("vbargl2#media", fakeSystem("", wantErr))
	if !errors.Is(err, wantErr) {
		t.Fatalf("expandProfileRef: expected system error to propagate, got %v", err)
	}
}

func TestSplitConvenienceRef(t *testing.T) {
	cases := []struct {
		ref       string
		wantFlake string
		wantName  string
		wantOK    bool
	}{
		{ref: "vbargl2#media", wantFlake: "vbargl2", wantName: "media", wantOK: true},
		{ref: "vbargl2#dev.default", wantFlake: "vbargl2", wantName: "dev.default", wantOK: true},
		{ref: ".#gui.daily", wantFlake: ".", wantName: "gui.daily", wantOK: true},
		// Already fully-qualified - might target an explicit, non-current
		// system, so it can't be safely folded into a batched build.
		{ref: `vbargl2#profileConfigurations.x86_64-linux."dev.default"`, wantOK: false},
		{ref: "vbargl2#profileConfigurations.x86_64-linux.gaming", wantOK: false},
		{ref: "vbargl2", wantOK: false},
	}
	for _, c := range cases {
		flake, name, ok := splitConvenienceRef(c.ref)
		if ok != c.wantOK {
			t.Errorf("splitConvenienceRef(%q) ok = %v, want %v", c.ref, ok, c.wantOK)
			continue
		}
		if !ok {
			continue
		}
		if flake != c.wantFlake || name != c.wantName {
			t.Errorf("splitConvenienceRef(%q) = (%q, %q), want (%q, %q)", c.ref, flake, name, c.wantFlake, c.wantName)
		}
	}
}

func TestGroupByFlake(t *testing.T) {
	g := groupByFlake([]parsedRef{
		{flake: "flake:vbargl", name: "gui.daily"},
		{flake: "flake:vbargl", name: "media"},
		{flake: "other", name: "x"},
	})
	if len(g) != 2 || len(g[0]) != 2 || g[0][0].name != "gui.daily" || g[0][1].name != "media" {
		t.Fatalf("same-flake group = %#v", g)
	}
	if g[1][0].name != "x" {
		t.Fatalf("second group = %#v", g[1])
	}
}

func TestFormatPlanEntry(t *testing.T) {
	got := formatPlanEntry("gui.daily", 12, "")
	if !strings.Contains(got, "gui.daily") || !strings.Contains(got, "no changes") || strings.Contains(got, "\n  ") {
		t.Errorf("unchanged = %q", got)
	}
	got = formatPlanEntry("media", 5, "<<< old\n>>> new\n")
	if !strings.HasPrefix(got, "media\n") {
		t.Errorf("changed name line = %q", got)
	}
	if !strings.Contains(got, "  <<< old\n") || !strings.Contains(got, "  >>> new\n") {
		t.Errorf("changed body not indented: %q", got)
	}
}

func TestUpgradeRequiresAllXorNames(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())

	if err := Upgrade(nil, false, apply.Options{}); err == nil {
		t.Error("Upgrade(no names, --all not set): expected an error, got none")
	}
	if err := Upgrade([]string{"media"}, true, apply.Options{}); err == nil {
		t.Error("Upgrade(names given, --all set): expected an error, got none")
	}
}

func TestUpgradeErrorsOnUnknownName(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())
	t.Setenv("NXF_PROFILE", t.TempDir())

	err := Upgrade([]string{"never-added"}, false, apply.Options{Approve: true})
	if err == nil || !strings.Contains(err.Error(), "never-added") {
		t.Fatalf("Upgrade(unknown name): expected an error mentioning it, got %v", err)
	}
}

func TestUpgradeAllWithNothingRecordedIsANoop(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())
	t.Setenv("NXF_PROFILE", t.TempDir())

	if err := Upgrade(nil, true, apply.Options{Approve: true}); err != nil {
		t.Fatalf("Upgrade(--all, nothing recorded): unexpected error: %v", err)
	}
}

func TestExpandProfileRefUnquotedNestedName(t *testing.T) {
	expanded, name, err := expandProfileRef(
		"vbargl2#profileConfigurations.x86_64-linux.gui.daily",
		fakeSystem("x86_64-linux", nil),
	)
	if err != nil {
		t.Fatal(err)
	}
	if name != "gui.daily" {
		t.Errorf("name = %q, want gui.daily (not the last segment 'daily')", name)
	}
	if expanded != "vbargl2#profileConfigurations.x86_64-linux.gui.daily" {
		t.Errorf("expanded = %q", expanded)
	}
}
