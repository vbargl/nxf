package profilecmd

import (
	"errors"
	"strings"
	"testing"
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

func TestCmdUpgradeRequiresAllXorNames(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())

	if err := cmdUpgrade(nil, false, false); err == nil {
		t.Error("cmdUpgrade(no names, --all not set): expected an error, got none")
	}
	if err := cmdUpgrade([]string{"media"}, true, false); err == nil {
		t.Error("cmdUpgrade(names given, --all set): expected an error, got none")
	}
}

func TestCmdUpgradeErrorsOnUnknownName(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())

	err := cmdUpgrade([]string{"never-added"}, false, false)
	if err == nil || !strings.Contains(err.Error(), "never-added") {
		t.Fatalf("cmdUpgrade(unknown name): expected an error mentioning it, got %v", err)
	}
}

func TestCmdUpgradeAllWithNothingRecordedIsANoop(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())

	if err := cmdUpgrade(nil, true, false); err != nil {
		t.Fatalf("cmdUpgrade(--all, nothing recorded): unexpected error: %v", err)
	}
}

func TestProfileElementName(t *testing.T) {
	cases := map[string]string{
		"media":       "profile-media",
		"dev.default": "profile-dev.default",
	}
	for name, want := range cases {
		if got := profileElementName(name); got != want {
			t.Errorf("profileElementName(%q) = %q, want %q", name, got, want)
		}
	}
}
