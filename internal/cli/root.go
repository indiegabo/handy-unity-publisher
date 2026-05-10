// Package cli contains operator-facing command entrypoints.
package cli

import (
	"encoding/json"
	"fmt"
	"io"
	"strings"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/version"
)

// usage describes the top-level hgb command surface.
const usage = `hgb manages the handy-unity-bulder service.

Usage:
	hgb
  hgb help
  hgb version
  hgb config
	hgb releases dispatch manual --repository-id <id> --git-tag <tag>
	hgb releases plan --release-run-id <id>
`

// Run executes the top-level CLI command.
func Run(args []string, stdout, stderr io.Writer) int {
	return RunWithIO(args, nil, stdout, stderr)
}

// RunWithIO executes the top-level CLI command.
func RunWithIO(args []string, stdin io.Reader, stdout, stderr io.Writer) int {
	_ = stdin

	if len(args) == 0 {
		_, _ = fmt.Fprint(stdout, usage)
		return 0
	}

	switch strings.ToLower(args[0]) {
	case "help", "-h", "--help":
		_, _ = fmt.Fprint(stdout, usage)
		return 0
	case "version":
		_, _ = fmt.Fprintln(stdout, version.String())
		return 0
	case "config":
		return printConfig(stdout, stderr)
	case "releases":
		return runReleases(args[1:], stdout, stderr)
	default:
		_, _ = fmt.Fprintf(stderr, "unknown command %q\n\n", args[0])
		_, _ = fmt.Fprint(stderr, usage)
		return 1
	}
}

// printConfig renders the effective runtime configuration as indented JSON.
func printConfig(stdout, stderr io.Writer) int {
	cfg, err := config.Load()
	if err != nil {
		_, _ = fmt.Fprintf(stderr, "load config: %v\n", err)
		return 1
	}

	encoder := json.NewEncoder(stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(cfg); err != nil {
		_, _ = fmt.Fprintf(stderr, "encode config: %v\n", err)
		return 1
	}

	return 0
}

// printJSON encodes one value to stdout using the CLI-wide indentation style.
func printJSON(stdout io.Writer, value any) error {
	encoder := json.NewEncoder(stdout)
	encoder.SetIndent("", "  ")
	return encoder.Encode(value)
}
