package oscmd

import (
	"os/exec"
	"reflect"
	"testing"
)

func TestRunNixosRebuildArgs(t *testing.T) {
	var got struct {
		name string
		args []string
	}
	orig := execCommand
	execCommand = func(name string, args ...string) *exec.Cmd {
		got.name, got.args = name, args
		return exec.Command("true")
	}
	t.Cleanup(func() { execCommand = orig })

	if err := runNixosRebuild("switch", ".#saber"); err != nil {
		t.Fatalf("runNixosRebuild: unexpected error: %v", err)
	}
	if got.name != "nixos-rebuild" {
		t.Errorf("runNixosRebuild command = %q, want %q", got.name, "nixos-rebuild")
	}
	want := []string{"switch", "--flake", ".#saber"}
	if !reflect.DeepEqual(got.args, want) {
		t.Errorf("runNixosRebuild args = %v, want %v", got.args, want)
	}
}
