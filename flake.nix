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
    };
}
