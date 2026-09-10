{
  description = "nxf: manages nix profiles (systemd user units, activation scripts) and NixOS system generations";

  inputs.nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems f;
      lib = nixpkgs.lib;
    in
    {
      # Consumer flakes call this as:
      #   mkProfile = inputs.nxf.lib.mkProfile { inherit pkgs lib; };
      #   mkProfile "gui.daily" { packages = [ pkgs.git ]; }
      lib.mkProfile = import ./nix/mkProfile.nix;

      overlays.default = final: prev: {
        nxf = self.packages.${final.stdenv.hostPlatform.system}.nxf;
      };

      nixosModules.default =
        { pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          programs.nxf.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.nxf;
        };
      nixosModules.nxf = self.nixosModules.default;

      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          nxf = pkgs.callPackage ./nix/package.nix { };
          default = self.packages.${system}.nxf;
        }
      );

      # buildGoModule runs `go test ./...` during the build; this check
      # is that package, so `nix flake check` covers the unit suite.
      # mkProfileEval builds a tiny profile so the Nix contract stays honest.
      # test/vm-test.sh is local-only (needs a dedicated incus VM).
      checks = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          mkProfile = self.lib.mkProfile { inherit pkgs lib; };
        in
        {
          nxf = self.packages.${system}.nxf;
          mkProfile = mkProfile "gui.daily" {
            description = "nxf check profile";
            packages = [ pkgs.hello ];
            systemdUnits.greeter = {
              unit = {
                Unit.Description = "greeter";
                Service.ExecStart = "${pkgs.hello}/bin/hello";
                Install.WantedBy = [ "default.target" ];
              };
            };
            systemdUnits."backup.timer" = {
              unit = {
                Unit.Description = "backup";
                Timer.OnCalendar = "daily";
                Install.WantedBy = "timers.target";
              };
            };
            activate = pkgs.writeShellScript "nxf-check-activate" "true";
            deactivate = pkgs.writeShellScript "nxf-check-deactivate" "true";
            priority = 4;
          };
        }
      );
    };
}
