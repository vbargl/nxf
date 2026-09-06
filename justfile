build:
    CGO_ENABLED=0 go vet ./...
    CGO_ENABLED=0 go build -o nxf ./cmd/nxf

test-unit:
    CGO_ENABLED=0 go test ./...

test-integration:
    ./test/vm-test.sh
