// Package names validates nxf profile names. These names are used as
// directory entries, applied-state filenames, systemd unit prefixes, and
// nix derivation suffixes (profile-<name>), so they must be path-safe.
package names

import (
	"fmt"
	"strings"
	"unicode"
)

// Check reports whether name is a legal nxf profile name.
// Allowed: letters, digits, '.', '_' and '-', starting with a letter or
// digit, no "..", no path separators. Dotted names like "gui.daily" are the
// common case.
func Check(name string) error {
	if name == "" {
		return fmt.Errorf("profile name is empty")
	}
	if len(name) > 100 {
		return fmt.Errorf("profile name %q is too long (max 100 characters)", name)
	}
	if strings.Contains(name, "..") {
		return fmt.Errorf("profile name %q must not contain \"..\"", name)
	}
	if strings.ContainsAny(name, "/\\") || strings.ContainsRune(name, 0) {
		return fmt.Errorf("profile name %q contains a path separator", name)
	}
	runes := []rune(name)
	if !unicode.IsLetter(runes[0]) && !unicode.IsDigit(runes[0]) {
		return fmt.Errorf("profile name %q must start with a letter or digit", name)
	}
	for _, r := range runes {
		if unicode.IsLetter(r) || unicode.IsDigit(r) || r == '.' || r == '_' || r == '-' {
			continue
		}
		return fmt.Errorf("profile name %q contains invalid character %q (allowed: letters, digits, '.', '_', '-')", name, string(r))
	}
	return nil
}

// DerivationName is the nix derivation `name` mkProfile writes, and the
// suffix of the resulting store path (`…-profile-gui.daily`). nix profile
// element *keys* are a different string (nix takes the last attr-path
// segment), so this is for matching store paths, not for `nix profile remove`.
func DerivationName(profileName string) string {
	return "profile-" + profileName
}
