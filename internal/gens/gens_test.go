package gens

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestListAndSelect(t *testing.T) {
	dir := t.TempDir()
	now := time.Date(2026, 9, 10, 12, 0, 0, 0, time.UTC)
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
	mk(1, false, now.Add(-10*24*time.Hour))
	mk(2, false, now.Add(-2*24*time.Hour))
	mk(3, true, now)

	userLink := filepath.Join(dir, "user-link")
	if err := os.Symlink(filepath.Join(dir, "profile"), userLink); err != nil {
		t.Fatal(err)
	}

	got, err := List(userLink)
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(got) != 3 {
		t.Fatalf("List = %#v, want 3", got)
	}
	if !got[0].Current || got[0].Number != 3 {
		t.Errorf("newest current = %#v", got[0])
	}

	del, err := SelectDelete(got, Spec{Keep: 2}, now)
	if err != nil {
		t.Fatal(err)
	}
	if len(del) != 1 || del[0].Number != 1 {
		t.Errorf("keep 2 deleted %#v, want gen 1", del)
	}

	if _, err = SelectDelete(got, Spec{DeleteNums: []int{3}}, now); err == nil {
		t.Error("expected error deleting current")
	}

	del, err = SelectDelete(got, Spec{DeleteNums: []int{1}}, now)
	if err != nil || len(del) != 1 {
		t.Errorf("delete 1: %v %#v", err, del)
	}
}

func TestSelectDeleteByAge(t *testing.T) {
	now := time.Date(2026, 9, 10, 12, 0, 0, 0, time.UTC)
	list := []Generation{
		{Number: 3, Time: now, Current: true},
		{Number: 2, Time: now.Add(-2 * 24 * time.Hour)},
		{Number: 1, Time: now.Add(-10 * 24 * time.Hour)},
	}
	del, err := SelectDelete(list, Spec{KeepSince: 3 * 24 * time.Hour}, now)
	if err != nil {
		t.Fatal(err)
	}
	if len(del) != 1 || del[0].Number != 1 {
		t.Errorf("keep-since 3d deleted %#v, want gen 1", del)
	}
	del, err = SelectDelete(list, Spec{OlderThan: 7 * 24 * time.Hour}, now)
	if err != nil {
		t.Fatal(err)
	}
	if len(del) != 1 || del[0].Number != 1 {
		t.Errorf("older-than 7d deleted %#v, want gen 1", del)
	}
}

func TestParseDuration(t *testing.T) {
	d, err := ParseDuration("3d")
	if err != nil || d != 3*24*time.Hour {
		t.Errorf("3d = %v %v", d, err)
	}
	d, err = ParseDuration("1w")
	if err != nil || d != 7*24*time.Hour {
		t.Errorf("1w = %v %v", d, err)
	}
	if _, err := ParseDuration("0d"); err == nil {
		t.Error("0d should fail")
	}
	if _, err := ParseDuration("3"); err == nil {
		t.Error("3 should fail")
	}
}

func TestSelectDeleteRequiresExactlyOne(t *testing.T) {
	if _, err := SelectDelete(nil, Spec{}, time.Now()); err == nil {
		t.Error("empty spec should fail")
	}
	if _, err := SelectDelete(nil, Spec{Keep: 1, OlderThan: time.Hour}, time.Now()); err == nil {
		t.Error("two fields should fail")
	}
}

func itoa(n int) string {
	return []string{"0", "1", "2", "3", "4", "5"}[n]
}
