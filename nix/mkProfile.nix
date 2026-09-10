{ pkgs, lib }:

# Builds a `profileConfigurations.<name>` derivation from a profile
# definition. The result is meant to be installed with
# `nix profile add <flake>#profileConfigurations.<system>.<name>` (or
# `nxf profile add <flake>#<name>`): it carries the profile's packages
# (so they land on PATH like any other `nix profile` entry) plus a
# manifest describing systemd user units and an activation script,
# embedded at a name-namespaced path so multiple installed profiles
# never collide once `nix profile` merges their outputs together.
#
# nxf discovers installed profiles by walking
# `~/.nix-profile/share/nxf/profiles/*/nxf.json` and reconciles systemd
# user units / runs activation scripts by comparing store paths, not by
# talking to this builder directly.
#
# JSON schema of nxf.json:
#   {
#     "name": "<flat dotted name>",
#     "units": { "<unit>": "/nix/store/...service" },
#     "manualUnits": ["<unit>", ...],
#     "activate": "/nix/store/...-script" | null
#   }
#
# The derivation `name` MUST be `profile-<name>`: `nxf profile remove`
# matches `nix profile` elements against that identifier.
name:
{
  # One-line summary shown by `nix flake show` / `nix eval .#profileConfigurations.<system>.<name>.meta.description`.
  description ? "nxf profile: ${name}",
  packages ? [ ],
  # Attrset of unit name -> unit definition, e.g.:
  #   {
  #     enabled = true;   # install this unit at all (default true)
  #     autoStart = true; # enable --now on install, restart on change; false
  #                       # means only install the unit file and leave
  #                       # enable/start/restart to the user (default true)
  #     unit = {
  #       Unit.Description = "...";
  #       Service.ExecStart = "...";
  #       # lib.generators.toINI stringifies values: WantedBy must be a
  #       # string ("default.target"), not a list.
  #       Install.WantedBy = "default.target";
  #     };
  #   }
  systemdUnits ? { },
  # Optional shell script (a package/derivation, e.g. from pkgs.writeShellScript)
  # run once whenever this profile is newly added or its store path changes.
  # There is no matching deactivate hook today.
  activate ? null,
}:
let
  enabledUnits = lib.filterAttrs (_: def: def.enabled or true) systemdUnits;

  unitFile =
    unitName: def:
    pkgs.writeText "nxf-${name}-${unitName}.service" (lib.generators.toINI { } def.unit);

  units = lib.mapAttrs unitFile enabledUnits;

  manualUnits = lib.attrNames (lib.filterAttrs (_: def: !(def.autoStart or true)) enabledUnits);

  nxfJson = pkgs.writeText "nxf-${name}-nxf.json" (
    builtins.toJSON {
      inherit name;
      units = lib.mapAttrs (_: drv: "${drv}") units;
      inherit manualUnits;
      activate = if activate == null then null else "${activate}";
    }
  );
in
pkgs.symlinkJoin {
  name = "profile-${name}";
  meta.description = description;
  paths = packages ++ [
    (pkgs.runCommand "nxf-${name}-dir" { } ''
      mkdir -p $out/share/nxf/profiles/${name}
      ln -s ${nxfJson} $out/share/nxf/profiles/${name}/nxf.json
    '')
  ];
}
