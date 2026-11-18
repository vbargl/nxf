# Generates nxf.nix, the module file `nxf init` writes: automatic updates
# enabled, and every other option commented out with its default.
{
  self,
  nixpkgs,
  pkgs,
}:

let
  inherit (nixpkgs) lib;

  options =
    (nixpkgs.lib.nixosSystem {
      inherit (pkgs.stdenv.hostPlatform) system;
      modules = [ self.nixosModules.default ];
    }).options.services.nxf;

  # The order of the file. Every option must be listed, so a new one cannot
  # be forgotten here.
  order = [
    "schedule"
    "randomizedDelay"
    "fixedDelay"
    "checkHook"
    "healthChecks"
    "healthTimeout"
    "healthInterval"
    "failureHook"
    "hookTimeout"
    "gitIdentity"
    "gitCredentialsFile"
    "nixConfigFile"
    "strategy"
    "inputs"
    "timeout"
  ];
  listed = lib.sort lib.lessThan ([ "enable" ] ++ order);
  declared = lib.attrNames options.autoUpdate;

  value =
    option:
    if option ? defaultText then
      option.defaultText.text
    else
      lib.generators.toPretty { multiline = false; } option.default;

  comment =
    indent: text:
    lib.concatMapStringsSep "\n" (line: lib.trimWith { end = true; } "${indent}# ${line}") (
      lib.splitString "\n" (lib.trim text)
    );

  entry =
    indent: name: option:
    ''
      ${comment indent option.description}
      ${indent}# ${name} = ${value option};
    '';
in
assert lib.assertMsg (listed == declared) "nix/init-module.nix must list the options ${toString declared}";
pkgs.writeText "nxf.nix" ''
  # Automatic updates of this machine flake with nxf.
  #
  # The options are those of nxf.nixosModules.default, which the flake must
  # import next to this file. Each is shown with its default; uncomment a
  # line to change it.
  { config, ... }:
  {
  ${entry "  " "services.nxf.flake" options.flake}
    services.nxf.autoUpdate = {
      enable = true;

  ${lib.concatMapStringsSep "\n" (name: entry "    " name options.autoUpdate.${name}) order}  };
  }
''
