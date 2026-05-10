// Command hgb provides operator-facing entrypoints for the service.
package main

import (
	"os"

	"github.com/indiegabo/handy-unity-bulder/internal/cli"
)

// main hands the current process streams to the operator CLI and exits with
// the returned status code.
func main() {
	os.Exit(cli.RunWithIO(os.Args[1:], os.Stdin, os.Stdout, os.Stderr))
}