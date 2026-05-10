package hubcli

import (
	"context"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// defaultComposeProject and databaseTransferTimeout define the runtime restart
// fallback project name and the transfer budget for database export/import.
const (
	defaultComposeProject   = "handy-unity-builder"
	databaseTransferTimeout = 5 * time.Minute
)

// newRuntimeManager builds the runtime restart helper used after database
// imports.
var newRuntimeManager = func() runtimeManager {
	return &dockerRuntimeManager{
		projectName: composeProjectName(),
		dockerBin:   "docker",
		runner:      execCommandRunner{},
	}
}

// runtimeManager coordinates disruptive runtime operations required by
// database import, such as reopening all processes against the new file.
type runtimeManager interface {
	RestartRuntime(ctx context.Context) error
}

// commandRunner executes one host command and returns combined stdout/stderr.
type commandRunner interface {
	CombinedOutput(ctx context.Context, name string, args ...string) ([]byte, error)
}

// execCommandRunner executes host commands through os/exec.
type execCommandRunner struct{}

// CombinedOutput runs one host command and returns the combined output.
func (execCommandRunner) CombinedOutput(
	ctx context.Context,
	name string,
	args ...string,
) ([]byte, error) {
	return exec.CommandContext(ctx, name, args...).CombinedOutput()
}

// dockerRuntimeManager restarts the Compose-managed server and workers through
// Docker labels instead of depending on the current working directory.
type dockerRuntimeManager struct {
	projectName string
	dockerBin   string
	runner      commandRunner
}

// runDatabase executes one hub database command.
func runDatabase(args []string, stdout, stderr io.Writer) int {
	if len(args) == 0 {
		_, _ = fmt.Fprintln(stderr, "hub db requires a subcommand: export or import")
		return 1
	}

	switch strings.ToLower(args[0]) {
	case "export":
		return runDatabaseExport(args[1:], stdout, stderr)
	case "import":
		return runDatabaseImport(args[1:], stdout, stderr)
	default:
		_, _ = fmt.Fprintf(stderr, "unknown hub db subcommand %q\n", args[0])
		return 1
	}
}

// runDatabaseExport downloads one consistent SQLite snapshot from the runtime.
func runDatabaseExport(args []string, stdout, stderr io.Writer) int {
	flagSet := flag.NewFlagSet("hub db export", flag.ContinueOnError)
	flagSet.SetOutput(stderr)
	pathFlag := flagSet.String("path", "hub.db", "target snapshot path")
	if err := flagSet.Parse(args); err != nil {
		return 1
	}
	if flagSet.NArg() != 0 {
		_, _ = fmt.Fprintln(stderr, "hub db export does not accept positional arguments")
		return 1
	}

	targetPath := filepath.Clean(strings.TrimSpace(*pathFlag))
	if targetPath == "" || targetPath == "." {
		_, _ = fmt.Fprintln(stderr, "hub db export requires --path to point at a file")
		return 1
	}
	if err := os.MkdirAll(filepath.Dir(targetPath), 0o755); err != nil {
		_, _ = fmt.Fprintf(stderr, "create export directory: %v\n", err)
		return 1
	}

	ctx, cancel := context.WithTimeout(context.Background(), databaseTransferTimeout)
	defer cancel()

	apiClient := newClient()
	if err := apiClient.ensureAvailable(ctx); err != nil {
		_, _ = fmt.Fprintf(stderr, "%v\n", err)
		return 1
	}

	outputFile, err := os.Create(targetPath)
	if err != nil {
		_, _ = fmt.Fprintf(stderr, "create export file: %v\n", err)
		return 1
	}
	defer outputFile.Close()

	if err := apiClient.exportDatabase(ctx, outputFile); err != nil {
		_, _ = fmt.Fprintf(stderr, "%v\n", err)
		return 1
	}
	if err := outputFile.Sync(); err != nil {
		_, _ = fmt.Fprintf(stderr, "sync export file: %v\n", err)
		return 1
	}

	_, _ = fmt.Fprintf(stdout, "Exported database snapshot to %s\n", targetPath)
	return 0
}

// runDatabaseImport uploads one SQLite snapshot and restarts the runtime so
// all processes reopen the imported database file.
func runDatabaseImport(args []string, stdout, stderr io.Writer) int {
	flagSet := flag.NewFlagSet("hub db import", flag.ContinueOnError)
	flagSet.SetOutput(stderr)
	pathFlag := flagSet.String("path", "", "source snapshot path")
	if err := flagSet.Parse(args); err != nil {
		return 1
	}
	if flagSet.NArg() != 0 {
		_, _ = fmt.Fprintln(stderr, "hub db import does not accept positional arguments")
		return 1
	}

	sourcePath := filepath.Clean(strings.TrimSpace(*pathFlag))
	if sourcePath == "" || sourcePath == "." {
		_, _ = fmt.Fprintln(stderr, "hub db import requires --path to point at an existing SQLite file")
		return 1
	}

	inputFile, err := os.Open(sourcePath)
	if err != nil {
		_, _ = fmt.Fprintf(stderr, "open import file: %v\n", err)
		return 1
	}
	defer inputFile.Close()

	ctx, cancel := context.WithTimeout(context.Background(), databaseTransferTimeout)
	defer cancel()

	apiClient := newClient()
	if err := apiClient.ensureAvailable(ctx); err != nil {
		_, _ = fmt.Fprintf(stderr, "%v\n", err)
		return 1
	}

	if err := apiClient.importDatabase(ctx, inputFile); err != nil {
		_, _ = fmt.Fprintf(stderr, "%v\n", err)
		return 1
	}

	if err := newRuntimeManager().RestartRuntime(ctx); err != nil {
		_, _ = fmt.Fprintf(
			stderr,
			"database snapshot uploaded, but runtime restart failed: %v\n",
			err,
		)
		return 1
	}

	_, _ = fmt.Fprintf(
		stdout,
		"Imported database snapshot from %s and restarted the runtime containers.\n",
		sourcePath,
	)
	return 0
}

// RestartRuntime restarts the Compose-managed server and workers so they all
// reopen the newly imported SQLite file.
func (m *dockerRuntimeManager) RestartRuntime(ctx context.Context) error {
	containerIDs, err := m.runtimeContainerIDs(ctx)
	if err != nil {
		return err
	}

	output, err := m.runner.CombinedOutput(
		ctx,
		m.dockerBin,
		append([]string{"restart"}, containerIDs...)...,
	)
	if err != nil {
		return fmt.Errorf(
			"restart runtime containers for Compose project %q: %w: %s",
			m.projectName,
			err,
			strings.TrimSpace(string(output)),
		)
	}

	return nil
}

// runtimeContainerIDs resolves the runtime containers for one Compose project.
func (m *dockerRuntimeManager) runtimeContainerIDs(ctx context.Context) ([]string, error) {
	serviceNames := []string{
		"unity-build-api",
		"unity-build-worker",
		"artifact-publish-worker",
	}
	seen := make(map[string]struct{}, len(serviceNames))
	containerIDs := make([]string, 0, len(serviceNames))

	for _, service := range serviceNames {
		serviceIDs, err := m.listServiceContainerIDs(ctx, service)
		if err != nil {
			return nil, err
		}
		for _, id := range serviceIDs {
			if _, ok := seen[id]; ok {
				continue
			}
			seen[id] = struct{}{}
			containerIDs = append(containerIDs, id)
		}
	}

	if len(containerIDs) == 0 {
		return nil, fmt.Errorf(
			"no runtime containers found for Compose project %q; set HUB_COMPOSE_PROJECT if the stack uses a different project name",
			m.projectName,
		)
	}

	return containerIDs, nil
}

// listServiceContainerIDs resolves one Compose service to one or more container ids.
func (m *dockerRuntimeManager) listServiceContainerIDs(
	ctx context.Context,
	service string,
) ([]string, error) {
	if strings.TrimSpace(m.projectName) == "" {
		return nil, fmt.Errorf("compose project name must not be empty")
	}

	output, err := m.runner.CombinedOutput(
		ctx,
		m.dockerBin,
		"ps",
		"-aq",
		"--filter",
		fmt.Sprintf("label=com.docker.compose.project=%s", m.projectName),
		"--filter",
		fmt.Sprintf("label=com.docker.compose.service=%s", service),
	)
	if err != nil {
		return nil, fmt.Errorf(
			"resolve Compose service %q for project %q: %w: %s",
			service,
			m.projectName,
			err,
			strings.TrimSpace(string(output)),
		)
	}

	containerIDs := strings.Fields(strings.TrimSpace(string(output)))
	if len(containerIDs) == 0 {
		return nil, fmt.Errorf(
			"no container found for Compose project %q service %q; set HUB_COMPOSE_PROJECT if needed",
			m.projectName,
			service,
		)
	}

	return containerIDs, nil
}

// composeProjectName returns the Compose project name used for runtime restarts.
func composeProjectName() string {
	projectName := strings.TrimSpace(os.Getenv("HUB_COMPOSE_PROJECT"))
	if projectName == "" {
		return defaultComposeProject
	}

	return projectName
}
