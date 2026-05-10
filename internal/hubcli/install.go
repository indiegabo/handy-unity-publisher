package hubcli

import (
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// runInstall copies the current hub binary to the requested user-local path.
func runInstall(args []string, stdout, stderr io.Writer) int {
	flagSet := flag.NewFlagSet("hub install", flag.ContinueOnError)
	flagSet.SetOutput(stderr)
	pathFlag := flagSet.String("path", defaultInstallPath(), "target installation path")
	if err := flagSet.Parse(args); err != nil {
		return 1
	}

	if flagSet.NArg() != 0 {
		_, _ = fmt.Fprintln(stderr, "hub install does not accept positional arguments")
		return 1
	}

	execPath, err := os.Executable()
	if err != nil {
		_, _ = fmt.Fprintf(stderr, "resolve current executable: %v\n", err)
		return 1
	}

	payload, err := os.ReadFile(execPath)
	if err != nil {
		_, _ = fmt.Fprintf(stderr, "read current executable: %v\n", err)
		return 1
	}

	target := filepath.Clean(*pathFlag)
	if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
		_, _ = fmt.Fprintf(stderr, "create install directory: %v\n", err)
		return 1
	}

	if err := installBinary(target, payload); err != nil {
		_, _ = fmt.Fprintf(stderr, "write installed hub binary: %v\n", err)
		return 1
	}

	_, _ = fmt.Fprintf(stdout, "Installed hub to %s\n", target)
	_, _ = fmt.Fprintln(
		stdout,
		"hub install copies the currently running binary. Use `go run ./cmd/hub install` from the repository root to install the latest source version.",
	)
	if !pathContains(filepath.Dir(target)) {
		_, _ = fmt.Fprintf(stdout, "Add %s to PATH to run `hub` globally.\n", filepath.Dir(target))
	}

	return 0
}

// installBinary writes one executable payload atomically so replacing a live
// binary does not truncate the target on failure.
func installBinary(target string, payload []byte) error {
	tempFile, err := os.CreateTemp(filepath.Dir(target), ".hub-install-*")
	if err != nil {
		return err
	}
	tempPath := tempFile.Name()
	defer os.Remove(tempPath)

	if _, err := tempFile.Write(payload); err != nil {
		_ = tempFile.Close()
		return err
	}

	if err := tempFile.Chmod(0o755); err != nil {
		_ = tempFile.Close()
		return err
	}

	if err := tempFile.Close(); err != nil {
		return err
	}

	return os.Rename(tempPath, target)
}

// defaultInstallPath chooses the first conventional hub binary location that
// is already visible on PATH, falling back to the primary local bin path.
func defaultInstallPath() string {
	home := userHomeDir()
	candidates := []string{
		filepath.Join(home, ".local", "bin", "hub"),
		filepath.Join(home, ".local", "go", "bin", "hub"),
		filepath.Join(home, "go", "bin", "hub"),
	}

	for _, candidate := range candidates {
		if pathContains(filepath.Dir(candidate)) {
			return candidate
		}
	}

	return candidates[0]
}

// userHomeDir resolves the current user's home directory and falls back to the
// current directory when the environment is incomplete.
func userHomeDir() string {
	home, err := os.UserHomeDir()
	if err != nil || strings.TrimSpace(home) == "" {
		return "."
	}

	return home
}

// pathContains reports whether the provided directory is already present in
// PATH after path cleaning.
func pathContains(dir string) bool {
	for _, entry := range filepath.SplitList(os.Getenv("PATH")) {
		if filepath.Clean(entry) == filepath.Clean(dir) {
			return true
		}
	}

	return false
}
