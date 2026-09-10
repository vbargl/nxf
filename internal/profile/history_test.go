package profile

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/vbargl/nxf/internal/gens"
)

func TestGenerationsJSON(t *testing.T) {
	dir := t.TempDir()
	now := time.Date(2026, 9, 9, 12, 35, 51, 0, time.UTC)
	mk := func(n int, current bool, mtime time.Time) {
		store := filepath.Join(dir, "store-"+itoa(n))
		if err := os.Mkdir(store, 0o755); err != nil {
			t.Fatal(err)
		}
		link := filepath.Join(dir, "profile-"+itoa(n)+"-link")
		if err := os.Symlink(store, link); err != nil {
			t.Fatal(err)
		}
		if err := os.Chtimes(link, mtime, mtime); err != nil {
			t.Fatal(err)
		}
		if current {
			if err := os.Symlink("profile-"+itoa(n)+"-link", filepath.Join(dir, "profile")); err != nil {
				t.Fatal(err)
			}
		}
	}
	mk(1, false, now.Add(-24*time.Hour))
	mk(2, true, now)

	user := filepath.Join(dir, "user")
	if err := os.Symlink(filepath.Join(dir, "profile"), user); err != nil {
		t.Fatal(err)
	}
	t.Setenv("NXF_PROFILE", user)

	list, err := gens.List(user)
	if err != nil {
		t.Fatalf("gens.List: %v", err)
	}
	raw, err := json.MarshalIndent(struct {
		Generations []gens.JSONGeneration `json:"generations"`
	}{gens.JSONList(list)}, "", "  ")
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
		t.Fatalf("len = %d, want 2\n%s", len(parsed.Generations), raw)
	}
	if parsed.Generations[0].Number != 2 || !parsed.Generations[0].Current {
		t.Errorf("newest = %+v", parsed.Generations[0])
	}
	if parsed.Generations[1].Number != 1 || parsed.Generations[1].Current {
		t.Errorf("older = %+v", parsed.Generations[1])
	}
	for _, g := range parsed.Generations {
		if _, err := time.Parse(time.RFC3339, g.Time); err != nil {
			t.Errorf("time %q is not RFC3339: %v", g.Time, err)
		}
		if g.NixosVersion != "" || g.KernelVersion != "" {
			t.Errorf("user generations should omit os fields: %+v", g)
		}
	}
	if containsJSONKey(raw, "nixosVersion") || containsJSONKey(raw, "kernelVersion") {
		t.Errorf("user JSON should omit empty os fields:\n%s", raw)
	}
}

func containsJSONKey(raw []byte, key string) bool {
	var obj map[string][]map[string]any
	if err := json.Unmarshal(raw, &obj); err != nil {
		return false
	}
	for _, g := range obj["generations"] {
		if _, ok := g[key]; ok {
			return true
		}
	}
	return false
}

func itoa(n int) string {
	return []string{"0", "1", "2", "3", "4", "5"}[n]
}
