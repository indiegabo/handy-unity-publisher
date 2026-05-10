package hubcli

import (
	"context"
	"fmt"
	"io"
	"strings"
)

// runRuntime executes runtime inspection commands exposed by the hub client.
func runRuntime(args []string, stdout, stderr io.Writer) int {
	if len(args) == 0 {
		_, _ = fmt.Fprintln(stderr, "hub runtime requires a subcommand: pipelines or automation")
		return 1
	}

	ctx := context.Background()
	apiClient := newClient()
	if err := apiClient.ensureAvailable(ctx); err != nil {
		_, _ = fmt.Fprintf(stderr, "%v\n", err)
		return 1
	}

	switch strings.ToLower(args[0]) {
	case "pipelines":
		report, err := apiClient.runtimePipelines(ctx)
		if err != nil {
			_, _ = fmt.Fprintf(stderr, "%v\n", err)
			return 1
		}
		if err := printJSON(stdout, report); err != nil {
			_, _ = fmt.Fprintf(stderr, "encode runtime pipelines: %v\n", err)
			return 1
		}
		return 0
	case "automation":
		report, err := apiClient.runtimeAutomation(ctx)
		if err != nil {
			_, _ = fmt.Fprintf(stderr, "%v\n", err)
			return 1
		}
		if err := printJSON(stdout, report); err != nil {
			_, _ = fmt.Fprintf(stderr, "encode runtime automation: %v\n", err)
			return 1
		}
		return 0
	default:
		_, _ = fmt.Fprintf(stderr, "unknown hub runtime subcommand %q\n", args[0])
		return 1
	}
}
