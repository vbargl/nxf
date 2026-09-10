// Package names validates nxf profile names. These names are used as
// directory entries, applied-state filenames, systemd unit prefixes, and
// the nix derivation name (so `nix profile list` shows `gui.daily`).
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

// DerivationName is the current mkProfile derivation name (equal to the
// nxf profile name, so `nix profile add <store-path>` names the element
// `gui.daily`).
func DerivationName(profileName string) string {
	return profileName
}

// LegacyDerivationName is the 0.6/0.7 prefix form (`profile-gui.daily`)
// still present in already-installed profiles.
func LegacyDerivationName(profileName string) string {
	return "profile-" + profileName
}
