package profilecmd

import (
	"encoding/json"
	"os"
	"path/filepath"

	"github.com/vbargl/nxf/internal/paths"
)

// loadRefs reads the name -> flake mapping nxf records for every profile
// installed via a convenience ref (<flake>#<name>), so `nxf profile upgrade`
// knows what to rebuild without the caller having to repeat the ref.
func loadRefs() (map[string]string, error) {
	path, err := paths.ProfileRefsFile()
	if err != nil {
		return nil, err
	}
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return map[string]string{}, nil
	}
	if err != nil {
		return nil, err
	}
	refs := map[string]string{}
	if err := json.Unmarshal(data, &refs); err != nil {
		return nil, err
	}
	return refs, nil
}

func saveRefs(refs map[string]string) error {
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
	return os.WriteFile(path, data, 0o644)
}

// rememberRef records that name was installed from flake, for a later `nxf
// profile upgrade` to rebuild from. Best-effort: a failure here shouldn't
// fail the add itself, since the profile was already successfully installed.
func rememberRef(name, flake string) {
	refs, err := loadRefs()
	if err != nil {
		refs = map[string]string{}
	}
	refs[name] = flake
	_ = saveRefs(refs)
}

// forgetRef drops name's recorded ref (see rememberRef), best-effort for the
// same reason - the profile was already successfully removed.
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
