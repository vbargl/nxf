build:
    CGO_ENABLED=0 go vet ./...
    CGO_ENABLED=0 go build -o nxf ./cmd/nxf

test-unit:
    CGO_ENABLED=0 go test ./...

# Throwaway nix profile; this is what CI runs.
test-integration:
    ./test/integration.sh

# Black-box tests against a local incus VM (see test/vm-test.sh).
# Not run in CI: requires the nxf-vm-test VM and scratch flakes outside this repo.
test-vm:
    ./test/vm-test.sh
