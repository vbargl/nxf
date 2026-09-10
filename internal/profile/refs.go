package profile

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/vbargl/nxf/internal/fsutil"
	"github.com/vbargl/nxf/internal/nixutil"
	"github.com/vbargl/nxf/internal/paths"
)

// Ref is the flake identity of an installed profile, recorded so upgrade
// can rebuild from the *resolved* original URL (registry/path as nix saw
// it) rather than whatever relative path the user happened to type.
type Ref struct {
	OriginalURL string `json:"originalUrl"`
	LockedURL   string `json:"lockedUrl,omitempty"`
	AttrPath    string `json:"attrPath,omitempty"`
	Installable string `json:"installable"`
}

func loadRefs() (map[string]Ref, error) {
	path, err := paths.ProfileRefsFile()
	if err != nil {
		return nil, err
	}
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return map[string]Ref{}, nil
	}
	if err != nil {
		return nil, err
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return nil, err
	}
	out := map[string]Ref{}
	for k, v := range raw {
		if len(v) > 0 && v[0] == '"' {
			var s string
			if err := json.Unmarshal(v, &s); err != nil {
				return nil, err
			}
			out[k] = Ref{OriginalURL: s, Installable: s + "#" + k}
			continue
		}
		var r Ref
		if err := json.Unmarshal(v, &r); err != nil {
			return nil, err
		}
		out[k] = r
	}
	return out, nil
}

func saveRefs(refs map[string]Ref) error {
	path, err := paths.ProfileRefsFile()
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(refs, "", "  ")
	if err != nil {
		return err
	}
	return fsutil.WriteFileAtomic(path, data, 0o644)
}

func rememberRef(name string, ref Ref) {
	refs, err := loadRefs()
	if err != nil {
		refs = map[string]Ref{}
	}
	refs[name] = ref
	_ = saveRefs(refs)
}

func forgetRef(name string) {
	refs, err := loadRefs()
	if err != nil {
		return
	}
	if _, ok := refs[name]; !ok {
		return
	}
	delete(refs, name)
	_ = saveRefs(refs)
}

func isRelativeFlake(flake string) bool {
	return flake == "" || flake == "." || strings.HasPrefix(flake, "./") || strings.HasPrefix(flake, "../")
}

// rememberAfterAdd records the resolved original/locked URLs nix stored for
// name, using the fragment from the installable we actually passed so
// dotted names stay quoted.
func rememberAfterAdd(name, expanded string) {
	_, frag, _ := strings.Cut(expanded, "#")
	ref := Ref{Installable: expanded, AttrPath: frag}
	if els, err := nixutil.ListElements(); err == nil {
		if e, ok := nixutil.FindElement(els, name); ok {
			ref.OriginalURL = e.OriginalURL
			ref.LockedURL = e.LockedURL
			if e.AttrPath != "" {
				ref.AttrPath = e.AttrPath
			}
			if e.OriginalURL != "" && frag != "" {
				ref.Installable = e.OriginalURL + "#" + frag
			}
		}
	}
	if ref.OriginalURL == "" {
		if flake, _, ok := splitConvenienceRef(expanded); ok {
			ref.OriginalURL = flake
		} else if flake, _, found := strings.Cut(expanded, "#"); found {
			ref.OriginalURL = flake
		}
	}
	rememberRef(name, ref)
}

func upgradeInstallable(name string, stored Ref) (string, error) {
	orig := stored.OriginalURL
	if isRelativeFlake(orig) {
		if flake, _, _ := strings.Cut(stored.Installable, "#"); !isRelativeFlake(flake) {
			orig = flake
		}
	}
	if isRelativeFlake(orig) {
		return "", fmt.Errorf("profile %q has no resolved flake URL recorded (was added from a relative path) - add it again from a registry name or absolute flake ref", name)
	}
	system, err := nixutil.CurrentSystem()
	if err != nil {
		return "", err
	}
	return fmt.Sprintf("%s#profileConfigurations.%s.%q", orig, system, name), nil
}
