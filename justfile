build:
    CGO_ENABLED=0 go vet ./...
    CGO_ENABLED=0 go build -o nxf ./cmd/nxf

test-unit:
    CGO_ENABLED=0 go test ./...

# Black-box tests against a local incus VM (see test/vm-test.sh).
# Not run in CI: requires the nxf-vm-test VM and scratch flakes outside this repo.
test-integration:
    ./test/vm-test.sh
