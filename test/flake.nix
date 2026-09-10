{
  description = "In-repo nxf profiles used by test/integration.sh (and CI)";

  inputs.nxf.url = "path:..";
  inputs.nixpkgs.follows = "nxf/nixpkgs";

  outputs =
    { nixpkgs, nxf, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems f;
    in
    {
      profileConfigurations = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          mkProfile = nxf.lib.mkProfile {
            inherit pkgs;
            lib = nixpkgs.lib;
          };
        in
        {
          pkgs-only = mkProfile "pkgs-only" {
            description = "plain packages";
            packages = [ pkgs.hello ];
          };

          with-unit = mkProfile "with-unit" {
            description = "one systemd user unit";
            systemdUnits.greeter = {
              unit = {
                Unit.Description = "nxf integration greeter";
                Service.ExecStart = "${pkgs.hello}/bin/hello";
                Install.WantedBy = [ "default.target" ];
              };
            };
          };

          with-hooks = mkProfile "with-hooks" {
            description = "activate/deactivate hooks";
            activate = pkgs.writeShellScript "nxf-test-activate" ''
              echo "activate $(basename "$(dirname "$0")")" >> "$HOME/hooks.log"
            '';
            deactivate = pkgs.writeShellScript "nxf-test-deactivate" ''
              echo "deactivate with-hooks" >> "$HOME/hooks.log"
            '';
          };

          "gui.daily" = mkProfile "gui.daily" {
            description = "dotted name";
          };
        }
      );
    };
}
