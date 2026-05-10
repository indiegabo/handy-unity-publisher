// Command hub provides the operator-facing HTTP client for a running
// handy-unity-bulder service.
package main

import (
	"os"

	"github.com/indiegabo/handy-unity-bulder/internal/hubcli"
)

// main hands the current process streams to the HTTP-backed operator CLI and
// exits with the returned status code.
func main() {
	os.Exit(hubcli.RunWithIO(os.Args[1:], os.Stdin, os.Stdout, os.Stderr))
}