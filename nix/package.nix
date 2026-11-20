{
  lib,
  rustPlatform,
  makeWrapper,
  coreutils,
  dix,
  git,
  nh,
  nix,
  systemd,
}:

let
  manifest = (lib.importTOML ../Cargo.toml).package;
in
rustPlatform.buildRustPackage {
  pname = manifest.name;
  inherit (manifest) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../nix/nxf.nix
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  # The tests need a Nix daemon and Git; they run from the dev shell and in CI.
  doCheck = false;

  postInstall = ''
    wrapProgram $out/bin/nxf --suffix PATH : ${
      lib.makeBinPath [
        coreutils
        dix
        git
        nh
        nix
        systemd
      ]
    }
  '';

  meta = {
    inherit (manifest) description;
    homepage = manifest.repository;
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "nxf";
    platforms = lib.platforms.linux;
  };
}
