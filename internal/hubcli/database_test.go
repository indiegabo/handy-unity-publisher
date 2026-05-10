package hubcli

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRunDatabaseExportWritesSnapshotFile(t *testing.T) {
	server := newHubTestServer(t)
	server.databaseSnapshot = []byte("sqlite-export-bytes")
	t.Setenv("HUB_BASE_URL", server.URL)

	exportPath := filepath.Join(t.TempDir(), "exported-hub.db")
	var stdout bytes.Buffer
	var stderr bytes.Buffer

	exitCode := Run([]string{"db", "export", "--path", exportPath}, &stdout, &stderr)
	if exitCode != 0 {
		t.Fatalf("expected db export to succeed, got exit code %d: %s", exitCode, stderr.String())
	}

	exported, err := os.ReadFile(exportPath)
	if err != nil {
		t.Fatalf("ReadFile(exportPath) error = %v", err)
	}
	if string(exported) != "sqlite-export-bytes" {
		t.Fatalf("expected exported snapshot bytes, got %q", string(exported))
	}
	if !strings.Contains(stdout.String(), "Exported database snapshot") {
		t.Fatalf("expected export success message, got %q", stdout.String())
	}
}

func TestRunDatabaseImportUploadsSnapshotAndRestartsRuntime(t *testing.T) {
	server := newHubTestServer(t)
	t.Setenv("HUB_BASE_URL", server.URL)

	importPath := filepath.Join(t.TempDir(), "import-hub.db")
	if err := os.WriteFile(importPath, []byte("sqlite-import-bytes"), 0o644); err != nil {
		t.Fatalf("WriteFile(importPath) error = %v", err)
	}

	runtime := &fakeRuntimeManager{}
	oldFactory := newRuntimeManager
	newRuntimeManager = func() runtimeManager { return runtime }
	t.Cleanup(func() { newRuntimeManager = oldFactory })

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := Run([]string{"db", "import", "--path", importPath}, &stdout, &stderr)
	if exitCode != 0 {
		t.Fatalf("expected db import to succeed, got exit code %d: %s", exitCode, stderr.String())
	}

	if string(server.importedDatabase) != "sqlite-import-bytes" {
		t.Fatalf("expected imported snapshot bytes, got %q", string(server.importedDatabase))
	}
	if runtime.restartCalls != 1 {
		t.Fatalf("expected one runtime restart, got %d", runtime.restartCalls)
	}
	if !strings.Contains(stdout.String(), "Imported database snapshot") {
		t.Fatalf("expected import success message, got %q", stdout.String())
	}
}

type fakeRuntimeManager struct {
	restartCalls int
	restartErr   error
}

func (m *fakeRuntimeManager) RestartRuntime(context.Context) error {
	m.restartCalls++
	return m.restartErr
}
