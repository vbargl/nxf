{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.nxf;
in
{
  options.programs.nxf = {
    enable = lib.mkEnableOption "nxf, a helper for nix profile bundles and NixOS generations";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.nxf or (throw "programs.nxf.package is not set: import nxf.nixosModules.default from the nxf flake, or set this option to nxf.packages.\${system}.nxf");
      defaultText = lib.literalExpression "pkgs.nxf";
      description = "The nxf package to install.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
