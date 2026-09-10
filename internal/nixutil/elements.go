package nixutil

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"

	"github.com/vbargl/nxf/internal/names"
	"github.com/vbargl/nxf/internal/paths"
)

// Element is one `nix profile list --json` entry. For nxf profiles installed
// by store path, Name equals the derivation name (the nxf profile name).
// Older flake-ref installs used the last attr-path segment (`daily`).
type Element struct {
	Name        string
	Active      bool
	AttrPath    string
	OriginalURL string
	LockedURL   string
	Priority    int
	StorePaths  []string
}

type profileListJSON struct {
	Elements map[string]struct {
		Active      bool     `json:"active"`
		AttrPath    string   `json:"attrPath"`
		OriginalURL string   `json:"originalUrl"`
		URL         string   `json:"url"`
		Priority    int      `json:"priority"`
		StorePaths  []string `json:"storePaths"`
	} `json:"elements"`
}

// ParseElements decodes `nix profile list --json` output.
func ParseElements(data []byte) ([]Element, error) {
	var parsed profileListJSON
	if err := json.Unmarshal(data, &parsed); err != nil {
		return nil, err
	}
	var out []Element
	for name, e := range parsed.Elements {
		out = append(out, Element{
			Name:        name,
			Active:      e.Active,
			AttrPath:    e.AttrPath,
			OriginalURL: e.OriginalURL,
			LockedURL:   e.URL,
			Priority:    e.Priority,
			StorePaths:  e.StorePaths,
		})
	}
	return out, nil
}

// ListElements returns every element in the current nix profile.
func ListElements() ([]Element, error) {
	profile, err := paths.NixProfileLink()
	if err != nil {
		return nil, err
	}
	data, err := ProfileListJSON(profile)
	if err != nil {
		if _, statErr := os.Lstat(profile); os.IsNotExist(statErr) {
			return nil, nil
		}
		return nil, err
	}
	return ParseElements(data)
}

// FindElement locates the nix profile element for an nxf profile name by
// the store-path drv name (`gui.daily`, or the legacy `profile-gui.daily`).
// Attr-path matching is a fallback for older flake-ref installs.
func FindElement(elements []Element, profileName string) (Element, bool) {
	want := map[string]bool{
		names.DerivationName(profileName):       true,
		names.LegacyDerivationName(profileName): true,
	}
	for _, e := range elements {
		for _, p := range e.StorePaths {
			if want[storeDrvName(p)] {
				return e, true
			}
		}
	}
	for _, e := range elements {
		if attrPathName(e.AttrPath) == profileName {
			return e, true
		}
	}
	return Element{}, false
}

// storeDrvName is the derivation name in a store path
// (`/nix/store/<hash>-gui.daily` → `gui.daily`).
func storeDrvName(p string) string {
	base := filepath.Base(p)
	i := strings.Index(base, "-")
	if i < 0 {
		return base
	}
	return base[i+1:]
}

// attrPathName extracts the nxf profile name from a flake attr path.
//
//	profileConfigurations.x86_64-linux."gui.daily" -> gui.daily
//	profileConfigurations.x86_64-linux.gui.daily   -> gui.daily (nested, last two if dotted)
//
// Nested vs flat is ambiguous when unquoted. We take everything after
// `profileConfigurations.<system>.` so both `gui.daily` and `"gui.daily"`
// round-trip as `gui.daily`.
func attrPathName(attr string) string {
	const prefix = "profileConfigurations."
	if !strings.HasPrefix(attr, prefix) {
		return ""
	}
	rest := attr[len(prefix):]
	// drop <system>.
	if i := strings.Index(rest, "."); i != -1 {
		rest = rest[i+1:]
	}
	return strings.Trim(rest, `"`)
}

// StorePathFor returns the first store path of the element matching name.
func StorePathFor(elements []Element, profileName string) string {
	e, ok := FindElement(elements, profileName)
	if !ok || len(e.StorePaths) == 0 {
		return ""
	}
	return e.StorePaths[0]
}
