# nxf

Manages user-level `nix profile` bundles (systemd user units, activation
scripts, desktop entries) and NixOS system generations for a flake repo of
your own.

```
nxf profile add/remove/upgrade/list/build/sync
nxf os switch/boot/test/build
```

`nxf os` is Linux/NixOS only (`nixos-rebuild`). Profile reconcile talks to
`systemctl --user`; without a user session it still installs unit files and
warns instead of failing.

## Using it from another flake

```nix
{
  inputs.nxf.url = "github:vbargl/nxf";

  outputs = { self, nixpkgs, nxf, ... }:
  let
    pkgs = nixpkgs.legacyPackages.x86_64-linux;
    mkProfile = nxf.lib.mkProfile { inherit pkgs; lib = nixpkgs.lib; };
  in {
    packages.x86_64-linux.nxf = nxf.packages.x86_64-linux.nxf;

    profileConfigurations.x86_64-linux.media = mkProfile "media" {
      description = "Media playback";
      packages = [ pkgs.vlc ];
    };
  };
}
```

Install with `nxf profile add .#media` (expands to
`profileConfigurations.<currentSystem>."media"`). Dotted names such as
`dev.default` are one attribute, not a nested path.

### Contract

`mkProfile` writes a derivation named `profile-<name>` containing:

| Path | Purpose |
|---|---|
| `bin/` etc. | `packages` merged onto PATH |
| `share/nxf/profiles/<name>/nxf.json` | reconcile manifest |

`nxf.json`:

```json
{
  "name": "dev.default",
  "units": { "ssh-agent": "/nix/store/...service" },
  "manualUnits": [],
  "activate": null
}
```

- `units`: unit name → store path of a `.service` file. `WantedBy` in the
  unit INI must be a **string** (`"default.target"`), not a list (`lib.generators.toINI`).
- `manualUnits`: install the file but do not `enable --now` / restart.
- `activate`: optional script run when the store path is new or changed.
  There is no deactivate hook.
- Derivation `name` must stay `profile-<name>`: `nxf profile remove`
  matches `nix profile` elements against that identifier.

### Environment

| Variable | Meaning |
|---|---|
| `NXF_FLAKE` | Default flake for `nxf os` (else `.`) |
| `NXF_NO_NOM=1` | Skip nix-output-monitor's tree view |
| `NXFP_PROFILE` | Override `~/.nix-profile` |
| `NXFP_PROFILE_NAME` | Set on activation scripts |

## Development

```
just build            # go vet + go build
just test-unit        # go test ./...
just test-integration # local incus VM; not run in CI
nix build .#nxf       # also runs go test via buildGoModule
nix flake check
```
