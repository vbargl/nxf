{
  lib,
  buildGoModule,
  nvd,
  nix-output-monitor,
  nix-fast-build,
}:

buildGoModule rec {
  pname = "nxf";
  # Bump on every change to this repo (semver: patch for fixes, minor for
  # new commands/flags, major for breaking CLI changes).
  version = "0.5.1";

  src = ../.;
  # go.mod has a real dependency (spf13/cobra), so this must be a real
  # vendor hash rather than null - update it whenever go.mod/go.sum change.
  vendorHash = "sha256-7K17JaXFsjf163g5PXCb5ng2gYdotnZ2IDKk8KFjNj0=";

  # Embed nvd's, nom's, and nix-fast-build's store paths so `nxf os`/`nxf
  # profile` diffing and build-progress display don't depend on any of them
  # being on the caller's PATH.
  ldflags = [
    "-X github.com/vbargl/nxf/internal/nixutil.NvdPath=${nvd}/bin/nvd"
    "-X github.com/vbargl/nxf/internal/nixutil.NomPath=${nix-output-monitor}/bin/nom"
    "-X github.com/vbargl/nxf/internal/nixutil.NixFastBuildPath=${nix-fast-build}/bin/nix-fast-build"
    "-X github.com/vbargl/nxf/internal/version.Version=${version}"
  ];

  meta = {
    description = "Manages nix profiles (systemd user units, activation scripts) and NixOS system generations";
    mainProgram = "nxf";
    license = lib.licenses.mit;
  };
}
