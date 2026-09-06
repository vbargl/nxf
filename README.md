# nxf

Manages user-level `nix profile` bundles (systemd user units, activation
scripts) and NixOS system generations for a nix flake repo of your own. See
`main.go` and `internal/` for the actual command surface (`nxf profile
add/remove/upgrade/list/build`, `nxf os switch/boot/test/build`).

## Using it from another flake

```nix
{
  inputs.nxf.url = "github:vbargl/nxf";

  outputs = { self, nixpkgs, nxf, ... }: {
    # e.g. in a NixOS module:
    # environment.systemPackages = [ nxf.packages.${system}.nxf ];
  };
}
```

## Development

```
just build          # go vet + go build
just test-unit       # go test ./...
just test-integration # black-box tests against an incus VM (see vm-test.sh)
nix build .#nxf
```
