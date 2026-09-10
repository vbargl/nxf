package names

import "testing"

func TestCheck(t *testing.T) {
	ok := []string{"media", "gui.daily", "terminal.admintools", "dev.default", "a", "A1_2-3.4"}
	for _, name := range ok {
		if err := Check(name); err != nil {
			t.Errorf("Check(%q): unexpected error: %v", name, err)
		}
	}

	bad := map[string]string{
		"":                        "empty",
		".":                       "start",
		".hidden":                 "start",
		"foo/bar":                 "separator",
		`foo\bar`:                 "separator",
		"foo..bar":                "..",
		"gui daily":               "invalid",
		"gui:daily":               "invalid",
		"../etc":                  "separator",
		string(make([]byte, 101)): "too long",
	}
	for name, kind := range bad {
		if err := Check(name); err == nil {
			t.Errorf("Check(%q): expected an error (%s)", name, kind)
		}
	}
}

func TestDerivationName(t *testing.T) {
	if got := DerivationName("gui.daily"); got != "gui.daily" {
		t.Errorf("DerivationName(gui.daily) = %q", got)
	}
	if got := LegacyDerivationName("gui.daily"); got != "profile-gui.daily" {
		t.Errorf("LegacyDerivationName(gui.daily) = %q", got)
	}
}
