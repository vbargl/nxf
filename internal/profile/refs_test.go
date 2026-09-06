package profile

import "testing"

func TestRefsRoundTrip(t *testing.T) {
	t.Setenv("XDG_STATE_HOME", t.TempDir())

	refs, err := loadRefs()
	if err != nil {
		t.Fatalf("loadRefs (no file yet): unexpected error: %v", err)
	}
	if len(refs) != 0 {
		t.Fatalf("loadRefs (no file yet) = %#v, want empty", refs)
	}

	rememberRef("dev.default", "vbargl2")
	rememberRef("media", "vbargl2")

	refs, err = loadRefs()
	if err != nil {
		t.Fatalf("loadRefs: unexpected error: %v", err)
	}
	want := map[string]string{"dev.default": "vbargl2", "media": "vbargl2"}
	for name, flake := range want {
		if refs[name] != flake {
			t.Errorf("loadRefs()[%q] = %q, want %q", name, refs[name], flake)
		}
	}

	// Re-remembering under a different flake overwrites, it doesn't duplicate.
	rememberRef("media", "other-flake")
	refs, _ = loadRefs()
	if refs["media"] != "other-flake" {
		t.Errorf("loadRefs()[media] = %q, want overwritten %q", refs["media"], "other-flake")
	}

	forgetRef("dev.default")
	refs, _ = loadRefs()
	if _, ok := refs["dev.default"]; ok {
		t.Errorf("loadRefs() still has dev.default after forgetRef")
	}
	if refs["media"] != "other-flake" {
		t.Errorf("forgetRef(dev.default) affected an unrelated entry: %#v", refs)
	}

	// forgetRef on a name that was never remembered is a no-op, not an error.
	forgetRef("never-added")
}
