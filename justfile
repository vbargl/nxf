# Run all tests.
test *args:
    nix develop --command cargo test {{ args }}

# Check formatting and lints.
lint:
    nix develop --command cargo fmt --check
    nix develop --command nixfmt --check flake.nix nix/*.nix
    nix develop --command cargo clippy --all-targets -- --deny warnings

# Everything CI checks.
check: lint test
    nix flake check

# Format the Rust and Nix sources.
format:
    nix develop --command cargo fmt
    nix fmt

# Renew nix/nxf.nix from the options of the module.
generate:
    cp --no-preserve=mode "$(nix build .#initModule --no-link --print-out-paths)" nix/nxf.nix
