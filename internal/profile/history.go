package profile

import (
	"fmt"
	"time"

	"github.com/vbargl/nxf/internal/apply"
	"github.com/vbargl/nxf/internal/gens"
	"github.com/vbargl/nxf/internal/paths"
	"github.com/vbargl/nxf/internal/reconcile"
	"github.com/vbargl/nxf/internal/ui"
)

func profileLink() (string, error) {
	return paths.NixProfileLink()
}

// Generations prints user-profile generations, newest first.
func Generations() error {
	link, err := profileLink()
	if err != nil {
		return err
	}
	list, err := gens.List(link)
	if err != nil {
		return err
	}
	if len(list) == 0 {
		fmt.Println("no profile generations")
		return nil
	}
	for _, g := range list {
		mark := " "
		if g.Current {
			mark = ui.CurrentMarker()
		}
		fmt.Printf("  %4d  %s  %s\n", g.Number, g.Time.Format("2006-01-02 15:04"), mark)
	}
	return nil
}

// Rollback switches the user profile to generation `to`, or the previous
// generation when to is nil, then reconciles units.
func Rollback(to *int, opts apply.Options) error {
	return withLock(func() error {
		link, err := profileLink()
		if err != nil {
			return err
		}
		list, err := gens.List(link)
		if err != nil {
			return err
		}
		cur := gens.CurrentNumber(list)
		target := 0
		if to != nil {
			target = *to
		} else {
			for _, g := range list {
				if g.Number < cur {
					target = g.Number
					break
				}
			}
			if target == 0 {
				return fmt.Errorf("no previous generation to roll back to")
			}
		}
		found := false
		for _, g := range list {
			if g.Number == target {
				found = true
				break
			}
		}
		if !found {
			return fmt.Errorf("generation %d does not exist", target)
		}

		fmt.Printf("  current %d -> %d\n", cur, target)
		if opts.DryRun {
			fmt.Println("Dry run: not applying.")
			return nil
		}
		if err := opts.Confirm(fmt.Sprintf("roll the user profile back to generation %d", target)); err != nil {
			return err
		}
		ui.Phase("activate")
		if err := gens.SwitchTo(link, target, false); err != nil {
			return err
		}
		if err := reconcile.Run(); err != nil {
			return err
		}
		ui.OK("activate")
		return nil
	})
}

// CleanSpec is the CLI view of gens.Spec plus parsed duration strings.
type CleanSpec struct {
	Keep      int
	KeepSince string
	OlderThan string
	Delete    []int
}

func (s CleanSpec) Spec() (gens.Spec, error) {
	out := gens.Spec{Keep: s.Keep, DeleteNums: s.Delete}
	var err error
	if s.KeepSince != "" {
		out.KeepSince, err = gens.ParseDuration(s.KeepSince)
		if err != nil {
			return gens.Spec{}, err
		}
	}
	if s.OlderThan != "" {
		out.OlderThan, err = gens.ParseDuration(s.OlderThan)
		if err != nil {
			return gens.Spec{}, err
		}
	}
	return out, nil
}

// Clean deletes selected user-profile generations.
func Clean(spec CleanSpec, opts apply.Options) error {
	return withLock(func() error {
		link, err := profileLink()
		if err != nil {
			return err
		}
		return cleanProfile(link, spec, opts, false)
	})
}

func cleanProfile(link string, spec CleanSpec, opts apply.Options, sudo bool) error {
	parsed, err := spec.Spec()
	if err != nil {
		return err
	}
	list, err := gens.List(link)
	if err != nil {
		return err
	}
	del, err := gens.SelectDelete(list, parsed, time.Now())
	if err != nil {
		return err
	}
	if len(del) == 0 {
		fmt.Println("nothing to delete")
		return nil
	}
	ui.Phase("plan")
	var nums []int
	for _, g := range del {
		nums = append(nums, g.Number)
		fmt.Printf("  %s  delete generation %d  (%s)\n", ui.Bullet(), g.Number, g.Time.Format("2006-01-02 15:04"))
	}
	ui.OK("plan")
	if opts.DryRun {
		fmt.Println("Dry run: not applying.")
		return nil
	}
	if err := opts.Confirm(fmt.Sprintf("delete %d generation(s)", len(nums))); err != nil {
		return err
	}
	ui.Phase("activate")
	if err := gens.Delete(link, nums, sudo); err != nil {
		return err
	}
	ui.OK("activate")
	return nil
}
