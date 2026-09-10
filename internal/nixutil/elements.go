package nixutil

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"

	"github.com/vbargl/nxf/internal/names"
	"github.com/vbargl/nxf/internal/paths"
)

// Element is one `nix profile list --json` entry. Name is nix's element key
// (last attr-path segment, so "gui.daily" and "terminal.daily" both become
// "daily" / "daily-1"). Match nxf profile names via StorePaths instead.
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
// matching the derivation suffix `profile-<name>` on a store path. Attr-path
// matching is a fallback for older installs whose drv name wasn't set.
func FindElement(elements []Element, profileName string) (Element, bool) {
	drv := names.DerivationName(profileName)
	for _, e := range elements {
		for _, p := range e.StorePaths {
			base := filepath.Base(p)
			if base == drv || strings.HasSuffix(base, "-"+drv) {
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
