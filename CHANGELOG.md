# Changelog

## 0.8.3

- `nxf profile list <name...>` filters by exact name or dotted prefix
  (`gui` matches `gui.daily` and `gui.admintools`).

## 0.8.2

- Dotted profile refs try a nested Nix attr path if the quoted name is
  missing (`terminal.admintools.extra` → `.terminal.admintools.extra`, not
  `."terminal.admintools.extra"`).

## 0.8.1

- `nxf profile upgrade --all` builds every profile from the same flake in
  one `nix-fast-build` (parallel). Plan is one line per unchanged profile,
  or the name plus an indented nvd diff. Add/upgrade no longer prompt for
  `yes` (rollback still exists). `nxf profile add --refresh` works; a bare
  profile name reuses the recorded flake URL.

## 0.8.0

- `nix profile list` names nxf profiles `gui.daily` / `terminal.daily`
  (install the built store path; derivation name equals the nxf name).
  Older `profile-<name>` / `daily` / `daily-1` installs still match.
- `nxf profile list` is a table; `-v` is a card (flake, short rev, labeled
  store, bins/desktop/xdg in two columns). `nxf os list` is a generation
  table; `>` marks the current generation.
- `--json` on `profile list`, `profile generations`, and `os generations`.

## 0.7.0

Breaking CLI: mutating commands (`profile add/remove/upgrade/rollback/clean`,
`os switch/test/boot/rollback/clean`) print a plan and then ask for `yes`
before applying. Unattended runs must pass `--approve`. `--dry-run` stops
after the plan.

### Added
- `deactivate` hook on `mkProfile`; run on profile remove and before a
  changed activate script. Units from the manifest are always torn down.
  Hooks are files at `etc/nxf/hooks/<name>/{activation,deactivation}Hook.sh`
  (no `NXF_PROFILE_NAME` env var).
- systemd unit types besides `.service` (timer, socket, path, …).
- `--priority` / `mkProfile.priority` for nix profile file collisions.
- `nxf profile list -v`, generations, rollback, clean (`--keep`,
  `--keep-since`, `--older-than`, `--delete`).
- Same generation/rollback/clean surface on `nxf os` (`os list` aliases
  `os generations`).
- NixOS module: `nxf.nixosModules.default` + `programs.nxf.enable`.
- Profile name sanitization. (`NXFP_*` renamed to `NXF_*`; `NXF_PROFILE`
  remains a test-only override of `~/.nix-profile`.)
- In-repo `test/flake.nix` + `test/integration.sh` (run in CI).

### Changed
- `nxf profile list` / `remove` / `upgrade` identify profiles by the
  derivation suffix `profile-<name>`, not nix's element key (so
  `gui.daily` stays `gui.daily` even when `nix profile list` shows
  `daily` / `daily-1`).
- Upgrade records the resolved original flake URL, not the relative path
  you typed. Re-`add` once if a 0.6.x install was added from `.`.
- `WantedBy` (and other unit INI values) may be a list.
- GitHub Actions pins installer/checkout SHAs and runs the integration
  suite. `nix flake check` is the unit+mkProfile gate (`nix build` is
  the same derivation; no longer duplicated).

### Fixed
- `nxf profile remove` of a name that was never added now fails.
- Multi-profile add is one transaction; a failed activate rolls the nix
  profile back.
