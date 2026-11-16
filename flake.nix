{
  description = "nxf keeps a NixOS machine's local flake updated";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      each = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = each (pkgs: rec {
        nxf = pkgs.callPackage ./nix/package.nix { };
        default = nxf;
      });

      nixosModules.default = import ./nix/module.nix self;

      checks = each (pkgs: {
        package = self.packages.${pkgs.stdenv.hostPlatform.system}.nxf;
        module = import ./nix/module-check.nix { inherit self nixpkgs pkgs; };
      });

      devShells = each (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            clippy
            rustc
            rustfmt
            rust-analyzer
            dix
            git
            nh
            util-linux
            just
          ];
        };
      });

      formatter = each (pkgs: pkgs.nixfmt-tree);
    };
}
