// Package hubcli contains the operator-facing HTTP client commands exposed as
// the `hub` binary.
package hubcli

import (
	"encoding/json"
	"fmt"
	"io"
	"strings"
)

// usage describes the top-level hub command surface.
const usage = "hub manages a running handy-unity-bulder app through its HTTP API.\n\n" +
	"Usage:\n" +
	"  hub help\n" +
	"  hub version\n" +
	"  hub dispatch [--git-commit <sha>] [--rebuild] <repository> <git-tag>\n" +
	"  hub install [--path <target>]\n" +
	"  hub runtime automation\n" +
	"  hub runtime pipelines\n" +
	"  hub db export --path <target>\n" +
	"  hub db import --path <source>\n\n" +
	"Environment:\n" +
	"  HUB_BASE_URL   Base URL for the running app. Defaults to http://127.0.0.1:8080\n" +
	"  HUB_COMPOSE_PROJECT   Compose project name used by `hub db import` runtime restarts.\n\n" +
	"The `hub` binary talks to the app over HTTP. If the app is not reachable,\n" +
	"hub will ask you to start it with docker compose first.\n"

// printJSON encodes one value to stdout using the hub CLI indentation style.
func printJSON(stdout io.Writer, value any) error {
	encoder := json.NewEncoder(stdout)
	encoder.SetIndent("", "  ")
	return encoder.Encode(value)
}

// Run executes the hub command without stdin access.
func Run(args []string, stdout, stderr io.Writer) int {
	return RunWithIO(args, nil, stdout, stderr)
}

// RunWithIO executes the hub command with stdin support for interactive flows.
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
		_, _ = fmt.Fprintln(stdout, "hub v0")
		return 0
	case "dispatch":
		return runDispatch(args[1:], stdout, stderr)
	case "install":
		return runInstall(args[1:], stdout, stderr)
	case "runtime":
		return runRuntime(args[1:], stdout, stderr)
	case "db", "database":
		return runDatabase(args[1:], stdout, stderr)
	default:
		_, _ = fmt.Fprintf(stderr, "unknown command %q\n\n", args[0])
		_, _ = fmt.Fprint(stderr, usage)
		return 1
	}
}
