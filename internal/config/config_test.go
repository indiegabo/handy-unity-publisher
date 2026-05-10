package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLoadUsesDefaultDatabasePathInsideDataDir(t *testing.T) {
	t.Setenv("APP_ENV", "test")
	t.Setenv("HTTP_ADDR", ":9999")
	t.Setenv("DATA_DIR", "/srv/handy")
	t.Setenv("APP_DB_PATH", "")
	t.Setenv("REDIS_ADDR", "")
	t.Setenv("REDIS_DB", "")
	t.Setenv("LOG_LEVEL", "debug")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	want := filepath.Join("/srv/handy", "app.db")
	if cfg.DBPath() != want {
		t.Fatalf("DBPath() = %q, want %q", cfg.DBPath(), want)
	}

	if cfg.RedisAddr != "127.0.0.1:6379" {
		t.Fatalf("RedisAddr = %q", cfg.RedisAddr)
	}

	if cfg.RedisDB != 0 {
		t.Fatalf("RedisDB = %d", cfg.RedisDB)
	}

	if cfg.HostDataDir != "/srv/handy" {
		t.Fatalf("HostDataDir = %q", cfg.HostDataDir)
	}

	if cfg.PipelinesDir != "pipelines" {
		t.Fatalf("PipelinesDir = %q", cfg.PipelinesDir)
	}
}

func TestLoadUsesExplicitDatabasePathOverride(t *testing.T) {
	t.Setenv("DATA_DIR", "/srv/handy")
	t.Setenv("APP_DB_PATH", "/var/lib/handy/custom.db")
	t.Setenv("HOST_DATA_DIR", "")
	t.Setenv("REDIS_ADDR", "redis.internal:6380")
	t.Setenv("REDIS_DB", "4")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.DBPath() != "/var/lib/handy/custom.db" {
		t.Fatalf("DBPath() = %q", cfg.DBPath())
	}

	if cfg.RedisAddr != "redis.internal:6380" {
		t.Fatalf("RedisAddr = %q", cfg.RedisAddr)
	}

	if cfg.RedisDB != 4 {
		t.Fatalf("RedisDB = %d", cfg.RedisDB)
	}

	if cfg.HostDataDir != "/srv/handy" {
		t.Fatalf("HostDataDir = %q", cfg.HostDataDir)
	}

	if cfg.HostWorkspacesDir() != "/srv/handy/workspaces" {
		t.Fatalf("HostWorkspacesDir() = %q", cfg.HostWorkspacesDir())
	}

	required := cfg.RequiredDirs()
	if !containsPath(required, "/var/lib/handy") {
		t.Fatalf("RequiredDirs() = %v, want parent directory for APP_DB_PATH", required)
	}
	if !containsPath(required, "/srv/handy") {
		t.Fatalf("RequiredDirs() = %v, want data dir preserved for runtime storage", required)
	}
}

func TestLoadUsesAppPortFallbackWhenHTTPAddrIsUnset(t *testing.T) {
	t.Setenv("HTTP_ADDR", "")
	t.Setenv("APP_PORT", "9095")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.HTTPAddr != ":9095" {
		t.Fatalf("HTTPAddr = %q", cfg.HTTPAddr)
	}
}

func TestLoadPrefersExplicitHTTPAddrOverAppPort(t *testing.T) {
	t.Setenv("APP_PORT", "9095")
	t.Setenv("HTTP_ADDR", "127.0.0.1:9191")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.HTTPAddr != "127.0.0.1:9191" {
		t.Fatalf("HTTPAddr = %q", cfg.HTTPAddr)
	}
}

func TestLoadRejectsInvalidAppPort(t *testing.T) {
	t.Setenv("APP_PORT", "not-a-port")
	t.Setenv("HTTP_ADDR", "")

	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want invalid APP_PORT error")
	}
}

func TestLoadRejectsInvalidRedisDB(t *testing.T) {
	t.Setenv("REDIS_DB", "workers")

	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want invalid REDIS_DB error")
	}
}

func TestLoadUsesExplicitHostDataDirOverride(t *testing.T) {
	t.Setenv("DATA_DIR", "/srv/runtime-data")
	t.Setenv("HOST_DATA_DIR", "/mnt/host-data")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.HostDataDir != "/mnt/host-data" {
		t.Fatalf("HostDataDir = %q", cfg.HostDataDir)
	}

	if cfg.HostArtifactsDir() != "/mnt/host-data/artifacts" {
		t.Fatalf("HostArtifactsDir() = %q", cfg.HostArtifactsDir())
	}
}

func TestLoadUsesExplicitPipelinesDirOverride(t *testing.T) {
	t.Setenv("PIPELINES_DIR", "/workspace/pipelines")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.PipelinesDir != "/workspace/pipelines" {
		t.Fatalf("PipelinesDir = %q", cfg.PipelinesDir)
	}
}

func TestLoadReadsOptionalConfigFile(t *testing.T) {
	configPath := filepath.Join(t.TempDir(), "config.json")
	if err := os.WriteFile(configPath, []byte(`{
		"env": "production",
		"http_addr": ":9090",
		"data_dir": "/srv/from-file",
		"redis_addr": "redis.internal:6381",
		"redis_db": 7,
		"log_level": "warn"
	}`), 0o644); err != nil {
		t.Fatalf("write config file: %v", err)
	}

	t.Setenv("APP_CONFIG_PATH", configPath)
	t.Setenv("APP_PORT", "")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.ConfigPath != configPath {
		t.Fatalf("ConfigPath = %q", cfg.ConfigPath)
	}
	if cfg.Env != "production" {
		t.Fatalf("Env = %q", cfg.Env)
	}
	if cfg.HTTPAddr != ":9090" {
		t.Fatalf("HTTPAddr = %q", cfg.HTTPAddr)
	}
	if cfg.DBPath() != "/srv/from-file/app.db" {
		t.Fatalf("DBPath() = %q", cfg.DBPath())
	}
	if cfg.RedisAddr != "redis.internal:6381" {
		t.Fatalf("RedisAddr = %q", cfg.RedisAddr)
	}
	if cfg.RedisDB != 7 {
		t.Fatalf("RedisDB = %d", cfg.RedisDB)
	}
	if cfg.LogLevel != "warn" {
		t.Fatalf("LogLevel = %q", cfg.LogLevel)
	}
}

func TestLoadEnvironmentOverridesConfigFile(t *testing.T) {
	configPath := filepath.Join(t.TempDir(), "config.json")
	if err := os.WriteFile(configPath, []byte(`{
		"data_dir": "/srv/from-file",
		"database_path": "/srv/from-file/app.db",
		"host_data_dir": "/srv/from-file",
		"redis_addr": "redis.internal:6381",
		"redis_db": 7
	}`), 0o644); err != nil {
		t.Fatalf("write config file: %v", err)
	}

	t.Setenv("APP_CONFIG_PATH", configPath)
	t.Setenv("APP_PORT", "")
	t.Setenv("DATA_DIR", "/srv/from-env")
	t.Setenv("APP_DB_PATH", "/srv/from-env/custom.db")
	t.Setenv("HOST_DATA_DIR", "/mnt/host-env")
	t.Setenv("REDIS_ADDR", "redis.env:6379")
	t.Setenv("REDIS_DB", "9")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	if cfg.DataDir != "/srv/from-env" {
		t.Fatalf("DataDir = %q", cfg.DataDir)
	}
	if cfg.DBPath() != "/srv/from-env/custom.db" {
		t.Fatalf("DBPath() = %q", cfg.DBPath())
	}
	if cfg.HostDataDir != "/mnt/host-env" {
		t.Fatalf("HostDataDir = %q", cfg.HostDataDir)
	}
	if cfg.RedisAddr != "redis.env:6379" {
		t.Fatalf("RedisAddr = %q", cfg.RedisAddr)
	}
	if cfg.RedisDB != 9 {
		t.Fatalf("RedisDB = %d", cfg.RedisDB)
	}
}

func containsPath(paths []string, want string) bool {
	for _, path := range paths {
		if path == want {
			return true
		}
	}

	return false
}
