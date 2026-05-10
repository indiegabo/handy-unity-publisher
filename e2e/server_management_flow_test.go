package e2e_test

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"testing"
	"time"

	miniredis "github.com/alicebob/miniredis/v2"
	"github.com/indiegabo/handy-unity-bulder/internal/pipelines"
)

func TestServerDeclarativePipelineSyncPersistsAcrossRestart(t *testing.T) {
	repoRoot := repositoryRoot(t)
	serverBinary := buildServerBinary(t, repoRoot)
	dataDir := t.TempDir()
	pipelinesDir := t.TempDir()
	redisServer := miniredis.RunT(t)
	t.Cleanup(redisServer.Close)

	writePipelineManifest(t, filepath.Join(pipelinesDir, "alpha.yml"), `apiVersion: handy.unity.builder/v1alpha1
kind: Pipeline
metadata:
  name: alpha
spec:
  repository:
    url: https://example.com/org/alpha.git
    defaultBranch: main
    enabled: true
    pollingIntervalSeconds: 300
  build:
    targets:
      - name: linux64
        enabled: true
        platform: StandaloneLinux64
        buildMethod: Builder.BuildLinux64
        output:
          kind: archive
          path: Builds/Linux64/game.zip
  publish:
    targets:
      - name: filesystem-release
        enabled: true
        kind: filesystem
        config:
          root_path: /exports/releases
  bindings:
    - buildTarget: linux64
      publishTarget: filesystem-release
      enabled: true
      options: {}
`)
	writePipelineManifest(t, filepath.Join(pipelinesDir, "broken.yml"), `apiVersion: handy.unity.builder/v1alpha1
kind: Pipeline
metadata:
  name: broken
spec:
  repository:
    url: ""
`)

	serverConfigPath := filepath.Join(t.TempDir(), "server-config.json")
	serverAddr := freeTCPAddress(t)
	if err := os.WriteFile(
		serverConfigPath,
		[]byte(fmt.Sprintf(`{
			"http_addr": %q,
			"data_dir": %q,
			"host_data_dir": %q,
			"pipelines_dir": %q,
			"redis_addr": %q,
			"log_level": "debug"
		}`,
			serverAddr,
			dataDir,
			dataDir,
			pipelinesDir,
			redisServer.Addr(),
		)),
		0o644,
	); err != nil {
		t.Fatalf("write server config: %v", err)
	}

	server := startServerProcess(t, repoRoot, serverBinary, map[string]string{
		"APP_CONFIG_PATH": serverConfigPath,
	})
	defer server.Stop(t)

	client := &http.Client{Timeout: 5 * time.Second}
	baseURL := server.BaseURL()

	report := fetchRuntimePipelines(t, client, baseURL)
	assertPipelineReport(t, report)

	server.Stop(t)
	server = startServerProcess(t, repoRoot, serverBinary, map[string]string{
		"APP_CONFIG_PATH": serverConfigPath,
	})
	defer server.Stop(t)

	report = fetchRuntimePipelines(t, client, server.BaseURL())
	assertPipelineReport(t, report)

	if _, err := os.Stat(filepath.Join(dataDir, "app.db")); err != nil {
		t.Fatalf("expected runtime database to exist after startup: %v", err)
	}
}

func fetchRuntimePipelines(t *testing.T, client *http.Client, baseURL string) pipelines.ApplyReport {
	t.Helper()

	var report pipelines.ApplyReport
	httpJSON(t, client, http.MethodGet, baseURL+"/api/v1/runtime/pipelines", nil, http.StatusOK, &report)
	return report
}

func assertPipelineReport(t *testing.T, report pipelines.ApplyReport) {
	t.Helper()

	if len(report.Pipelines) != 2 {
		t.Fatalf("expected two pipeline statuses, got %#v", report.Pipelines)
	}

	var applied pipelines.ApplyStatus
	var skipped pipelines.ApplyStatus
	for _, status := range report.Pipelines {
		if status.Applied {
			applied = status
		} else {
			skipped = status
		}
	}

	if applied.PipelineName != "alpha" {
		t.Fatalf("expected applied pipeline alpha, got %#v", applied)
	}
	if skipped.Path == "" || skipped.Error == "" {
		t.Fatalf("expected one skipped invalid manifest, got %#v", skipped)
	}
}

type runningServer struct {
	addr   string
	cmd    *exec.Cmd
	output *bytes.Buffer
	done   chan error
	once   sync.Once
}

func startServerProcess(
	t *testing.T,
	repoRoot string,
	serverBinary string,
	extraEnv map[string]string,
) *runningServer {
	t.Helper()

	cmd := exec.Command(serverBinary)
	cmd.Dir = repoRoot
	output := &bytes.Buffer{}
	cmd.Stdout = output
	cmd.Stderr = output

	addr := readHTTPAddrFromConfig(t, extraEnv["APP_CONFIG_PATH"])
	cmd.Env = append(os.Environ(), formatEnv(extraEnv)...)

	if err := cmd.Start(); err != nil {
		t.Fatalf("start server process: %v", err)
	}

	server := &runningServer{
		addr:   addr,
		cmd:    cmd,
		output: output,
		done:   make(chan error, 1),
	}

	go func() {
		server.done <- cmd.Wait()
	}()

	server.waitUntilHealthy(t)
	return server
}

func buildServerBinary(t *testing.T, repoRoot string) string {
	t.Helper()

	binaryPath := filepath.Join(t.TempDir(), "hgb-server")
	cmd := exec.Command("go", "build", "-o", binaryPath, "./cmd/server")
	cmd.Dir = repoRoot
	output, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("build server binary: %v\n%s", err, string(output))
	}

	return binaryPath
}

func (s *runningServer) BaseURL() string {
	return "http://" + s.addr
}

func (s *runningServer) Stop(t *testing.T) {
	t.Helper()

	s.once.Do(func() {
		if s.cmd.Process != nil {
			err := s.cmd.Process.Signal(syscall.SIGTERM)
			if err != nil && !errors.Is(err, os.ErrProcessDone) {
				t.Fatalf("signal server process: %v\n%s", err, s.output.String())
			}
		}

		select {
		case err := <-s.done:
			if err != nil && !isExpectedServerExit(err) {
				t.Fatalf("stop server process: %v\n%s", err, s.output.String())
			}
		case <-time.After(10 * time.Second):
			if s.cmd.Process != nil {
				_ = s.cmd.Process.Kill()
			}
			select {
			case err := <-s.done:
				if err != nil && !isExpectedServerExit(err) {
					t.Fatalf("force stop server process: %v\n%s", err, s.output.String())
				}
			case <-time.After(5 * time.Second):
				t.Fatalf("timed out forcing server process to stop\n%s", s.output.String())
			}

			t.Fatalf("timed out stopping server process\n%s", s.output.String())
		}
	})
}

func (s *runningServer) waitUntilHealthy(t *testing.T) {
	t.Helper()

	client := &http.Client{Timeout: time.Second}
	deadline := time.Now().Add(20 * time.Second)
	for time.Now().Before(deadline) {
		select {
		case err := <-s.done:
			t.Fatalf("server exited before becoming healthy: %v\n%s", err, s.output.String())
		default:
		}

		response, err := client.Get(s.BaseURL() + "/healthz")
		if err == nil {
			_ = response.Body.Close()
			if response.StatusCode == http.StatusOK {
				return
			}
		}

		time.Sleep(100 * time.Millisecond)
	}

	t.Fatalf("server did not become healthy\n%s", s.output.String())
}

func httpJSON[T any](
	t *testing.T,
	client *http.Client,
	method string,
	url string,
	payload any,
	status int,
	dst *T,
) {
	t.Helper()

	var body io.Reader
	if payload != nil {
		encoded, err := json.Marshal(payload)
		if err != nil {
			t.Fatalf("marshal request payload for %s %s: %v", method, url, err)
		}
		body = bytes.NewReader(encoded)
	}

	request, err := http.NewRequest(method, url, body)
	if err != nil {
		t.Fatalf("create request %s %s: %v", method, url, err)
	}
	request.Header.Set("Content-Type", "application/json")

	response, err := client.Do(request)
	if err != nil {
		t.Fatalf("perform request %s %s: %v", method, url, err)
	}
	defer response.Body.Close()

	contents, err := io.ReadAll(response.Body)
	if err != nil {
		t.Fatalf("read response body for %s %s: %v", method, url, err)
	}

	if response.StatusCode != status {
		t.Fatalf(
			"expected status %d for %s %s, got %d: %s",
			status,
			method,
			url,
			response.StatusCode,
			string(contents),
		)
	}

	if dst == nil || len(contents) == 0 {
		return
	}

	if err := json.Unmarshal(contents, dst); err != nil {
		t.Fatalf("decode response body for %s %s: %v\n%s", method, url, err, string(contents))
	}
}

func freeTCPAddress(t *testing.T) string {
	t.Helper()

	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("allocate tcp address: %v", err)
	}
	defer listener.Close()

	return listener.Addr().String()
}

func formatEnv(values map[string]string) []string {
	formatted := make([]string, 0, len(values))
	for key, value := range values {
		formatted = append(formatted, key+"="+value)
	}

	return formatted
}

func readHTTPAddrFromConfig(t *testing.T, path string) string {
	t.Helper()

	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read server config %q: %v", path, err)
	}

	var config struct {
		HTTPAddr string `json:"http_addr"`
	}
	if err := json.Unmarshal(contents, &config); err != nil {
		t.Fatalf("decode server config %q: %v", path, err)
	}

	if strings.TrimSpace(config.HTTPAddr) == "" {
		t.Fatalf("server config %q does not define http_addr", path)
	}

	return config.HTTPAddr
}

func isExpectedServerExit(err error) bool {
	if err == nil {
		return true
	}

	message := err.Error()
	return strings.Contains(message, "killed") || strings.Contains(message, "terminated")
}

func writePipelineManifest(t *testing.T, path string, contents string) {
	t.Helper()

	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("write pipeline manifest %q: %v", path, err)
	}
}
