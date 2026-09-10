package cmd

import (
	"testing"
)

func TestAddHasRefreshFlag(t *testing.T) {
	root := New()
	cmd, _, err := root.Find([]string{"profile", "add"})
	if err != nil {
		t.Fatal(err)
	}
	if cmd.Flags().Lookup("refresh") == nil {
		t.Fatal("profile add: missing --refresh")
	}
}

func TestJSONFlagOnListCommands(t *testing.T) {
	root := New()
	have := [][]string{
		{"profile", "list"},
		{"profile", "generations"},
		{"os", "generations"},
	}
	for _, path := range have {
		cmd, _, err := root.Find(path)
		if err != nil {
			t.Fatalf("Find %v: %v", path, err)
		}
		if cmd.Flags().Lookup("json") == nil {
			t.Errorf("%v: missing --json flag", path)
		}
	}

	absent := [][]string{
		{"profile", "add"},
		{"profile", "remove"},
		{"profile", "upgrade"},
		{"profile", "rollback"},
		{"profile", "clean"},
		{"os", "switch"},
		{"os", "rollback"},
		{"os", "clean"},
	}
	for _, path := range absent {
		cmd, _, err := root.Find(path)
		if err != nil {
			t.Fatalf("Find %v: %v", path, err)
		}
		if cmd.Flags().Lookup("json") != nil {
			t.Errorf("%v: unexpected --json flag", path)
		}
	}
}
