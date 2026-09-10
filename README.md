# nxf

Manages user-level `nix profile` bundles (systemd user units of any type,
activation/deactivation scripts, desktop entries) and NixOS system
generations.

```
nxf profile add/remove/upgrade/list/build/sync/generations/rollback/clean
nxf os switch/boot/test/build/generations/rollback/clean
```

Mutating commands build, print a plan, then ask for `yes` (terraform-style)
before applying. Pass `--approve` to skip the prompt, `--dry-run` to stop
after the plan. Unattended runs without `--approve` are refused.

`nxf os` is Linux/NixOS only (`nixos-rebuild`). Profile reconcile talks to
`systemctl --user`; without a user session it still installs unit files and
warns instead of failing.

## Install

Standalone (puts `nxf` on PATH via your user profile):

```
nix profile add github:vbargl/nxf
```

NixOS system package, from a flake:

```nix
{
  inputs.nxf.url = "github:vbargl/nxf";

  outputs = { nixpkgs, nxf, ... }: {
    nixosConfigurations.saber = nixpkgs.lib.nixosSystem {
      modules = [
        nxf.nixosModules.default
        { programs.nxf.enable = true; }
      ];
    };
  };
}
```

Or add `nxf.packages.${system}.nxf` to `environment.systemPackages` yourself.

## Using `mkProfile` from another flake

```nix
{
  inputs.nxf.url = "github:vbargl/nxf";

  outputs = { self, nixpkgs, nxf, ... }:
  let
    pkgs = nixpkgs.legacyPackages.x86_64-linux;
    mkProfile = nxf.lib.mkProfile { inherit pkgs; lib = nixpkgs.lib; };
  in {
    packages.x86_64-linux.nxf = nxf.packages.x86_64-linux.nxf;

    profileConfigurations.x86_64-linux."gui.daily" = mkProfile "gui.daily" {
      description = "Daily GUI tools";
      packages = [ pkgs.vlc ];
      priority = 4; # lower wins nix profile file collisions
      systemdUnits.greeter = {
        unit = {
          Unit.Description = "Greeter";
          Service.ExecStart = "${pkgs.hello}/bin/hello";
          Install.WantedBy = [ "default.target" ]; # lists are fine
        };
      };
      systemdUnits."backup.timer" = {
        unit = {
          Unit.Description = "Backup";
          Timer.OnCalendar = "daily";
          Install.WantedBy = "timers.target";
        };
      };
      activate = pkgs.writeShellScript "gui-daily-activate" ''
        mkdir -p "$HOME/.config/myapp"
      '';
      deactivate = pkgs.writeShellScript "gui-daily-deactivate" ''
        rm -f "$HOME/.config/myapp/managed"
      '';
    };
  };
}
```

Install with `nxf profile add --approve .#gui.daily` (expands to
`profileConfigurations.<currentSystem>."gui.daily"`). Dotted names such as
`gui.daily` are one attribute, not a nested path.

nxf installs the built store path (not the flake attr), so `nix profile
list` shows `gui.daily` / `terminal.daily` — the derivation name, which
equals the nxf profile name.

### Contract

`mkProfile` writes a derivation named `<name>` containing:

| Path | Purpose |
|---|---|
| `bin/` etc. | `packages` merged onto PATH |
| `share/nxf/profiles/<name>/nxf.json` | reconcile manifest |
| `etc/nxf/hooks/<name>/activationHook.sh` | run when the profile is new or the hook changes |
| `etc/nxf/hooks/<name>/deactivationHook.sh` | run on remove, and before a changed activation hook |

`nxf.json`:

```json
{
  "name": "gui.daily",
  "units": { "greeter.service": "/nix/store/...service", "backup.timer": "/nix/store/...timer" },
  "manualUnits": [],
  "priority": 4
}
```

- `units`: `<unit>.<type>` → store path. Types: service (default), timer,
  socket, path, target, slice, mount, automount, swap, scope. The attr name
  may include the type (`"backup.timer"`) or set `type = "timer"`.
- `WantedBy` (and other INI values) may be a string or a list of strings.
- `manualUnits`: install the file but do not `enable --now` / restart.
- `activate` / `deactivate` on `mkProfile` install the hook files above.
  While the profile is installed, nxf execs the path in the profile (so `$0`
  is `.../hooks/<name>/activationHook.sh`). After remove, deactivation runs
  from the recorded store path — bake `name` into the script in Nix if you
  need it there. Units from the manifest are always stopped/unlinked on
  teardown.
- `priority`: default `nix profile add --priority` (CLI `--priority` wins).
- Derivation `name` must equal the nxf profile name (`gui.daily`).
- Profile names: letters, digits, `.`, `_`, `-`; must start with a letter or
  digit; no `/`, no `..`.

### Commands

```
nxf profile add [--dry-run] [--refresh] [--priority N] <flake>#<name>...
nxf profile remove [--dry-run] [--approve] <name>...
nxf profile upgrade [--all] [--refresh] [--dry-run] [name...]
nxf profile list [-v] [name...]          # names filter; "gui" matches gui.daily
nxf profile sync
nxf profile generations
nxf profile rollback [--to N] [--dry-run] [--approve]
nxf profile clean --keep 3 | --keep-since 3d | --older-than 1w | --delete 10,11

nxf os switch|boot|test|build [--dry-run] [--approve] [host]
nxf os generations
nxf os rollback [--to N] [--dry-run] [--approve]
nxf os clean --keep 3 | --keep-since 3d | --older-than 1w | --delete 10,11
```

`list` is a table (NAME / BINS / UNITS / DESKTOP / XDG, plus HOOKS if any).
`-v` prints each profile as a card: name, flake (short rev), non-default
priority, then labeled store/bins/desktop/xdg/units. `nxf os list` is a
generation table; `>` marks the current generation. `--json` dumps the same
data.

Phases on mutate: **evaluate / build** → **plan** (nvd diff) → confirm →
**activate**.

Cleaning never deletes the current generation. Durations: `30m`, `2h`, `3d`,
`1w`.

### Environment

| Variable | Meaning |
|---|---|
| `NXF_FLAKE` | Default flake for `nxf os` (else `.`) |
| `NXF_NO_NOM=1` | Skip nix-output-monitor's tree view |
| `NO_COLOR` | ASCII markers instead of emoji |

State lives under `$XDG_STATE_HOME/nxf/` (`applied/`, `refs.json`,
`desktop-entries.json`, `lock`).

## Development

```
just build            # go vet + go build
just test-unit        # go test ./...
just test-integration # throwaway nix profile via NXF_PROFILE (also run in CI)
just test-vm          # local incus VM; not run in CI
nix build .#nxf       # also runs go test via buildGoModule
nix flake check
```

See [CHANGELOG.md](CHANGELOG.md) for 0.7.0 breaking changes (`--approve`).
