// Package version holds nxf's version string, set at build time via
// ldflags from the version declared in nix/packages/nxf/package.nix.
package version

// Version defaults to "dev" for `go build`/`go run` outside the nix package.
var Version = "dev"
