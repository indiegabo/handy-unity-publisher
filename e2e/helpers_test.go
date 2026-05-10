package e2e_test

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func repositoryRoot(t *testing.T) string {
	t.Helper()

	wd, err := os.Getwd()
	if err != nil {
		t.Fatalf("get working directory: %v", err)
	}

	return filepath.Dir(wd)
}

func runHGBAndDecodeWithEnv(
	t *testing.T,
	repoRoot string,
	dataDir string,
	extraEnv map[string]string,
	dst any,
	args ...string,
) {
	t.Helper()

	cmd := exec.Command("go", append([]string{"run", "./cmd/hgb"}, args...)...)
	cmd.Dir = repoRoot
	env := append(
		os.Environ(),
		"DATA_DIR="+dataDir,
		"APP_DB_PATH="+filepath.Join(dataDir, "app.db"),
	)
	for key, value := range extraEnv {
		env = append(env, key+"="+value)
	}
	cmd.Env = env

	output, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("run hgb %v: %v\n%s", args, err, string(output))
	}

	if err := json.Unmarshal(output, dst); err != nil {
		t.Fatalf("decode hgb output for %v: %v\n%s", args, err, string(output))
	}
}

func runGit(t *testing.T, repoDir string, args ...string) string {
	t.Helper()

	command := exec.Command("git", args...)
	command.Dir = repoDir
	output, err := command.CombinedOutput()
	if err != nil {
		t.Fatalf("run git %v: %v\n%s", args, err, string(output))
	}

	return strings.TrimSpace(string(output))
}
