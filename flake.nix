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
        initModule = import ./nix/init-module.nix { inherit self nixpkgs pkgs; };
      });

      nixosModules.default = import ./nix/module.nix self;

      checks = each (pkgs: {
        package = self.packages.${pkgs.stdenv.hostPlatform.system}.nxf;
        module = import ./nix/module-check.nix { inherit self nixpkgs pkgs; };
        shared-records = import ./nix/records-test.nix { inherit self pkgs; };
        # nix/nxf.nix is generated; `just generate` renews it.
        init-module =
          pkgs.runCommand "nxf-init-module-check" { }
            "diff --unified ${./nix/nxf.nix} ${
              self.packages.${pkgs.stdenv.hostPlatform.system}.initModule
            } && touch $out";
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
            # The tests run these; the host's may be missing or behave differently.
            # `.out` keeps their headers out of the shell.
            nix.out
            openssh.out
            util-linux
            just
            nixfmt
          ];
        };
      });

      formatter = each (pkgs: pkgs.nixfmt-tree);
    };
}
