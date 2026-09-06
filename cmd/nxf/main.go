package main

import (
	"fmt"
	"os"

	"github.com/vbargl/nxf/internal/cmd"
)

func main() {
	if err := cmd.New().Execute(); err != nil {
		fmt.Fprintf(os.Stderr, "nxf: error: %v\n", err)
		os.Exit(1)
	}
}
