{ pkgs, lib }:

# Builds a `profileConfigurations.<name>` derivation from a profile
# definition. The result is meant to be installed with
# `nix profile add <flake>#profileConfigurations.<system>.<name>` (or
# `nxf profile add <flake>#<name>`): it carries the profile's packages
# (so they land on PATH like any other `nix profile` entry) plus a
# manifest describing systemd user units and activation scripts,
# embedded at a name-namespaced path so multiple installed profiles
# never collide once `nix profile` merges their outputs together.
#
# nxf discovers installed profiles by walking
# `~/.nix-profile/share/nxf/profiles/*/nxf.json` and reconciles systemd
# user units / hooks by comparing store paths, not by talking to this
# builder directly.
#
# Hooks (optional) land at a fixed path so scripts can see their profile
# name from $0 without an env var:
#   etc/nxf/hooks/<name>/activationHook.sh
#   etc/nxf/hooks/<name>/deactivationHook.sh
#
# JSON schema of nxf.json:
#   {
#     "name": "<flat dotted name>",
#     "units": { "<unit>.<type>": "/nix/store/...unit", ... },
#     "manualUnits": ["<unit>.<type>", ...],
#     "priority": <int> | null
#   }
#
# The derivation `name` MUST equal the nxf profile name (`gui.daily`, not
# `profile-gui.daily`). nxf installs the built store path (not the flake
# attr), so `nix profile list` uses that drv name as the element key.
# Flake-ref installs would name both `gui.daily` and `terminal.daily` as
# `daily` / `daily-1` (last attr-path segment).
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
  #     type = "service"; # service|timer|socket|path|target|slice|mount|automount|swap|scope
  #                       # default: suffix of the attr name, else "service"
  #     unit = {
  #       Unit.Description = "...";
  #       Service.ExecStart = "...";
  #       # Lists are joined with spaces, so WantedBy = [ "default.target" ]
  #       # works (lib.generators.toINI's default would stringify the list).
  #       Install.WantedBy = [ "default.target" ];
  #     };
  #   }
  # The attr name may already include the type (`"backup.timer"`).
  systemdUnits ? { },
  # Optional shell script (a package/derivation, e.g. from pkgs.writeShellScript)
  # run once whenever this profile is newly added or its store path changes.
  activate ? null,
  # Optional matching teardown script, run when the profile is removed or
  # when the activate script's store path changes (old deactivate, then new
  # activate). Units listed in the manifest are always stopped/disabled on
  # teardown regardless of this hook.
  deactivate ? null,
  # Optional nix profile priority (lower = wins file collisions). nxf passes
  # this to `nix profile add --priority` unless the CLI flag overrides it.
  priority ? null,
}:
assert lib.assertMsg (
  builtins.match "[a-zA-Z0-9][a-zA-Z0-9._-]*" name != null && !(lib.hasInfix ".." name)
) "nxf: profile name ${lib.strings.escapeNixString name} is invalid (letters, digits, '.', '_', '-'; no leading '.'; no '..'; no '/')";
let
  knownTypes = [
    "service"
    "timer"
    "socket"
    "path"
    "target"
    "slice"
    "mount"
    "automount"
    "swap"
    "scope"
  ];

  unitTypeOf =
    unitName: def:
    if def.type or null != null then
      def.type
    else
      let
        matched = lib.filter (t: lib.hasSuffix ".${t}" unitName) knownTypes;
      in
      if matched != [ ] then lib.head matched else "service";

  unitBaseName =
    unitName: def:
    let
      t = unitTypeOf unitName def;
    in
    if lib.hasSuffix ".${t}" unitName then lib.removeSuffix ".${t}" unitName else unitName;

  unitKey =
    unitName: def:
    let
      t = unitTypeOf unitName def;
      base = unitBaseName unitName def;
    in
    "${base}.${t}";

  toINI = lib.generators.toINI {
    mkKeyValue = lib.generators.mkKeyValueDefault {
      mkValueString =
        v:
        if lib.isList v then
          lib.concatStringsSep " " (map (lib.generators.mkValueStringDefault { }) v)
        else
          lib.generators.mkValueStringDefault { } v;
    } "=";
  };

  enabledUnits = lib.filterAttrs (_: def: def.enabled or true) systemdUnits;

  unitFile =
    unitName: def:
    let
      t = unitTypeOf unitName def;
      base = unitBaseName unitName def;
    in
    pkgs.writeText "nxf-${name}-${base}.${t}" (toINI def.unit);

  units = lib.listToAttrs (
    lib.mapAttrsToList (unitName: def: {
      name = unitKey unitName def;
      value = "${unitFile unitName def}";
    }) enabledUnits
  );

  manualUnits = lib.mapAttrsToList (unitName: def: unitKey unitName def) (
    lib.filterAttrs (_: def: !(def.autoStart or true)) enabledUnits
  );

  nxfJson = pkgs.writeText "nxf-${name}-nxf.json" (
    builtins.toJSON {
      inherit name;
      inherit units;
      inherit manualUnits;
      inherit priority;
    }
  );
in
pkgs.symlinkJoin {
  inherit name;
  meta.description = description;
  paths = packages ++ [
    (pkgs.runCommand "nxf-${name}-dir" { } ''
      mkdir -p $out/share/nxf/profiles/${name}
      ln -s ${nxfJson} $out/share/nxf/profiles/${name}/nxf.json
      ${lib.optionalString (activate != null) ''
        mkdir -p $out/etc/nxf/hooks/${name}
        ln -s ${activate} $out/etc/nxf/hooks/${name}/activationHook.sh
      ''}
      ${lib.optionalString (deactivate != null) ''
        mkdir -p $out/etc/nxf/hooks/${name}
        ln -s ${deactivate} $out/etc/nxf/hooks/${name}/deactivationHook.sh
      ''}
    '')
  ];
}
