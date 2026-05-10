// Package config loads and validates runtime configuration.
package config

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

// Config contains the runtime settings shared across the process.
type Config struct {
	ConfigPath    string `json:"config_path,omitempty"`
	Env           string `json:"env"`
	HTTPAddr      string `json:"http_addr"`
	DataDir       string `json:"data_dir"`
	HostDataDir   string `json:"host_data_dir"`
	PipelinesDir  string `json:"pipelines_dir"`
	DatabasePath  string `json:"database_path"`
	RedisAddr     string `json:"redis_addr"`
	RedisUsername string `json:"redis_username,omitempty"`
	RedisPassword string `json:"-"`
	RedisDB       int    `json:"redis_db"`
	LogLevel      string `json:"log_level"`
}

// fileConfig mirrors the optional JSON configuration file used to override
// defaults before environment variables are applied.
type fileConfig struct {
	Env           *string `json:"env"`
	HTTPAddr      *string `json:"http_addr"`
	DataDir       *string `json:"data_dir"`
	HostDataDir   *string `json:"host_data_dir"`
	PipelinesDir  *string `json:"pipelines_dir"`
	DatabasePath  *string `json:"database_path"`
	RedisAddr     *string `json:"redis_addr"`
	RedisUsername *string `json:"redis_username"`
	RedisPassword *string `json:"redis_password"`
	RedisDB       *int    `json:"redis_db"`
	LogLevel      *string `json:"log_level"`
}

// Load reads the runtime configuration from defaults, an optional JSON file,
// and environment variables in that precedence order.
func Load() (Config, error) {
	cfg := Config{
		Env:          "development",
		HTTPAddr:     ":8080",
		DataDir:      "/data",
		PipelinesDir: "pipelines",
		RedisAddr:    "127.0.0.1:6379",
		RedisDB:      0,
		LogLevel:     "info",
	}

	configPath := strings.TrimSpace(os.Getenv("APP_CONFIG_PATH"))
	if configPath != "" {
		fileValues, err := loadFileConfig(configPath)
		if err != nil {
			return Config{}, err
		}

		cfg.ConfigPath = configPath
		applyFileConfig(&cfg, fileValues)
	}

	if err := applyEnvConfig(&cfg); err != nil {
		return Config{}, err
	}

	if strings.TrimSpace(cfg.HostDataDir) == "" {
		cfg.HostDataDir = cfg.DataDir
	}
	if strings.TrimSpace(cfg.DatabasePath) == "" {
		cfg.DatabasePath = filepath.Join(cfg.DataDir, "app.db")
	}

	if strings.TrimSpace(cfg.HTTPAddr) == "" {
		return Config{}, fmt.Errorf("HTTP_ADDR must not be empty")
	}

	if strings.TrimSpace(cfg.DataDir) == "" {
		return Config{}, fmt.Errorf("DATA_DIR must not be empty")
	}

	if strings.TrimSpace(cfg.HostDataDir) == "" {
		return Config{}, fmt.Errorf("HOST_DATA_DIR must not be empty")
	}

	if strings.TrimSpace(cfg.DatabasePath) == "" {
		return Config{}, fmt.Errorf("APP_DB_PATH must not be empty")
	}

	if strings.TrimSpace(cfg.RedisAddr) == "" {
		return Config{}, fmt.Errorf("REDIS_ADDR must not be empty")
	}

	return cfg, nil
}

// loadFileConfig reads and decodes the optional JSON configuration file.
func loadFileConfig(path string) (fileConfig, error) {
	contents, err := os.ReadFile(path)
	if err != nil {
		return fileConfig{}, fmt.Errorf("read APP_CONFIG_PATH %q: %w", path, err)
	}

	var cfg fileConfig
	if err := json.Unmarshal(contents, &cfg); err != nil {
		return fileConfig{}, fmt.Errorf("decode APP_CONFIG_PATH %q: %w", path, err)
	}

	return cfg, nil
}

// applyFileConfig copies non-nil JSON file values into the effective runtime
// configuration.
func applyFileConfig(cfg *Config, values fileConfig) {
	if values.Env != nil {
		cfg.Env = strings.TrimSpace(*values.Env)
	}
	if values.HTTPAddr != nil {
		cfg.HTTPAddr = strings.TrimSpace(*values.HTTPAddr)
	}
	if values.DataDir != nil {
		cfg.DataDir = strings.TrimSpace(*values.DataDir)
	}
	if values.HostDataDir != nil {
		cfg.HostDataDir = strings.TrimSpace(*values.HostDataDir)
	}
	if values.PipelinesDir != nil {
		cfg.PipelinesDir = strings.TrimSpace(*values.PipelinesDir)
	}
	if values.DatabasePath != nil {
		cfg.DatabasePath = strings.TrimSpace(*values.DatabasePath)
	}
	if values.RedisAddr != nil {
		cfg.RedisAddr = strings.TrimSpace(*values.RedisAddr)
	}
	if values.RedisUsername != nil {
		cfg.RedisUsername = strings.TrimSpace(*values.RedisUsername)
	}
	if values.RedisPassword != nil {
		cfg.RedisPassword = *values.RedisPassword
	}
	if values.RedisDB != nil {
		cfg.RedisDB = *values.RedisDB
	}
	if values.LogLevel != nil {
		cfg.LogLevel = strings.TrimSpace(*values.LogLevel)
	}
}

// applyEnvConfig overrides the effective configuration with supported
// environment variables.
func applyEnvConfig(cfg *Config) error {
	if value := strings.TrimSpace(os.Getenv("APP_ENV")); value != "" {
		cfg.Env = value
	}
	httpAddr, err := httpAddrFromPortEnv("APP_PORT")
	if err != nil {
		return err
	}
	if httpAddr != "" {
		cfg.HTTPAddr = httpAddr
	}
	if value := strings.TrimSpace(os.Getenv("HTTP_ADDR")); value != "" {
		cfg.HTTPAddr = value
	}
	if value := strings.TrimSpace(os.Getenv("DATA_DIR")); value != "" {
		cfg.DataDir = value
	}
	if value := strings.TrimSpace(os.Getenv("HOST_DATA_DIR")); value != "" {
		cfg.HostDataDir = value
	}
	if value := strings.TrimSpace(os.Getenv("PIPELINES_DIR")); value != "" {
		cfg.PipelinesDir = value
	}
	if value := strings.TrimSpace(os.Getenv("APP_DB_PATH")); value != "" {
		cfg.DatabasePath = value
	}
	if value := strings.TrimSpace(os.Getenv("REDIS_ADDR")); value != "" {
		cfg.RedisAddr = value
	}
	if value := strings.TrimSpace(os.Getenv("REDIS_USERNAME")); value != "" {
		cfg.RedisUsername = value
	}
	if value := os.Getenv("REDIS_PASSWORD"); strings.TrimSpace(value) != "" {
		cfg.RedisPassword = value
	}

	redisDB, err := atoiEnv("REDIS_DB", cfg.RedisDB)
	if err != nil {
		return err
	}
	cfg.RedisDB = redisDB

	if value := strings.TrimSpace(os.Getenv("LOG_LEVEL")); value != "" {
		cfg.LogLevel = value
	}

	return nil
}

// httpAddrFromPortEnv converts one environment-provided TCP port into the
// `:port` address shape used by the HTTP server.
func httpAddrFromPortEnv(key string) (string, error) {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return "", nil
	}

	parsed, err := strconv.Atoi(value)
	if err != nil {
		return "", fmt.Errorf("%s must be an integer between 1 and 65535: %w", key, err)
	}
	if parsed <= 0 || parsed > 65535 {
		return "", fmt.Errorf("%s must be an integer between 1 and 65535", key)
	}

	return fmt.Sprintf(":%d", parsed), nil
}

// SLogLevel converts the configured log level to slog's level type.
func (c Config) SLogLevel() slog.Level {
	switch strings.ToLower(strings.TrimSpace(c.LogLevel)) {
	case "debug":
		return slog.LevelDebug
	case "warn", "warning":
		return slog.LevelWarn
	case "error":
		return slog.LevelError
	default:
		return slog.LevelInfo
	}
}

// DBPath returns the SQLite database path used by the runtime.
func (c Config) DBPath() string {
	return c.DatabasePath
}

// LogsDir returns the persistent logs directory for runtime and build output.
func (c Config) LogsDir() string {
	return filepath.Join(c.DataDir, "logs")
}

// HostLogsDir returns the host-visible logs directory used for mounted paths
// when the app talks to the host Docker daemon through the Docker socket.
func (c Config) HostLogsDir() string {
	return filepath.Join(c.effectiveHostDataDir(), "logs")
}

// ArtifactsDir returns the persistent artifact directory.
func (c Config) ArtifactsDir() string {
	return filepath.Join(c.DataDir, "artifacts")
}

// HostArtifactsDir returns the host-visible artifact directory.
func (c Config) HostArtifactsDir() string {
	return filepath.Join(c.effectiveHostDataDir(), "artifacts")
}

// WorkspacesDir returns the persistent workspace directory.
func (c Config) WorkspacesDir() string {
	return filepath.Join(c.DataDir, "workspaces")
}

// HostWorkspacesDir returns the host-visible workspace directory.
func (c Config) HostWorkspacesDir() string {
	return filepath.Join(c.effectiveHostDataDir(), "workspaces")
}

// RequiredDirs returns the directories that must exist before runtime starts.
func (c Config) RequiredDirs() []string {
	return uniqueDirs(
		c.DataDir,
		filepath.Dir(c.DBPath()),
		c.LogsDir(),
		c.ArtifactsDir(),
		c.WorkspacesDir(),
	)
}

// uniqueDirs removes empty values and duplicates from a directory list while
// preserving the first occurrence order.
func uniqueDirs(paths ...string) []string {
	seen := make(map[string]struct{}, len(paths))
	unique := make([]string, 0, len(paths))

	for _, path := range paths {
		clean := filepath.Clean(strings.TrimSpace(path))
		if clean == "" {
			continue
		}

		if _, ok := seen[clean]; ok {
			continue
		}

		seen[clean] = struct{}{}
		unique = append(unique, clean)
	}

	return unique
}

// effectiveHostDataDir returns the host-visible data directory used for bind
// mounts and falls back to the runtime data directory when unset.
func (c Config) effectiveHostDataDir() string {
	hostDataDir := strings.TrimSpace(c.HostDataDir)
	if hostDataDir != "" {
		return hostDataDir
	}

	return c.DataDir
}

// getenv returns the trimmed environment value or the provided fallback when
// the variable is unset or blank.
func getenv(key, fallback string) string {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}

	return value
}

// atoiEnv parses one integer environment variable and falls back when it is
// unset.
func atoiEnv(key string, fallback int) (int, error) {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback, nil
	}

	parsed, err := strconv.Atoi(value)
	if err != nil {
		return 0, fmt.Errorf("%s must be an integer: %w", key, err)
	}

	return parsed, nil
}
