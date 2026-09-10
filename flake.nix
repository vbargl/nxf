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
    in
    {
      # Consumer flakes call this as:
      #   mkProfile = inputs.nxf.lib.mkProfile { inherit pkgs lib; };
      #   mkProfile "dev.default" { packages = [ pkgs.git ]; }
      lib.mkProfile = import ./nix/mkProfile.nix;

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
      # test/vm-test.sh is local-only (needs a dedicated incus VM).
      checks = forAllSystems (system: {
        nxf = self.packages.${system}.nxf;
      });
    };
}
