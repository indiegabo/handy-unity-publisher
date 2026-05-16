//! Resolves typed runtime configuration from environment variables and
//! host-specific application directory conventions.

#![forbid(unsafe_code)]

use std::env;
use std::io;
use std::path::PathBuf;

/// Names the runtime process within diagnostics and supervision metadata.
pub const RUNTIME_NAME: &str = "handy-games-publisher-runtime";

/// Names the persistent application directory used under host-specific roots.
pub const PRODUCT_DIRECTORY_NAME: &str = "HandyGamesPublisher";

/// Overrides the default runtime root directory when set.
pub const RUNTIME_ROOT_ENV: &str = "HANDY_GAMES_PUBLISHER_RUNTIME_ROOT";

/// Overrides the runtime log level when set.
pub const LOG_LEVEL_ENV: &str = "HANDY_GAMES_PUBLISHER_LOG_LEVEL";

/// Overrides the heartbeat cadence of the runtime work loop when set.
pub const HEARTBEAT_INTERVAL_MILLIS_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_HEARTBEAT_INTERVAL_MILLIS";

/// Overrides the scheduler sleep between worker loop iterations when set.
pub const WORKER_LOOP_INTERVAL_MILLIS_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_WORKER_LOOP_INTERVAL_MILLIS";

/// Limits the number of heartbeats emitted by the runtime work loop when set.
pub const MAX_HEARTBEATS_ENV: &str = "HANDY_GAMES_PUBLISHER_RUNTIME_MAX_HEARTBEATS";

/// Forces a recoverable crash after the configured heartbeat count when set.
pub const CRASH_AFTER_HEARTBEATS_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_CRASH_AFTER_HEARTBEATS";

/// Limits recoverable crash injection to the first N supervision attempts.
pub const CRASH_ATTEMPTS_ENV: &str = "HANDY_GAMES_PUBLISHER_RUNTIME_CRASH_ATTEMPTS";

/// Exposes the current supervision attempt to the child runtime process.
pub const SUPERVISION_ATTEMPT_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_SUPERVISION_ATTEMPT";

/// Overrides the number of supervisor restarts allowed after recoverable exits.
pub const SUPERVISION_MAX_RESTARTS_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_MAX_RESTARTS";

/// Overrides the restart backoff between recoverable supervisor retries.
pub const SUPERVISION_BACKOFF_MILLIS_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_RESTART_BACKOFF_MILLIS";

/// Overrides the recoverable exit code used by the runtime supervisor.
pub const SUPERVISION_RECOVERABLE_EXIT_CODE_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_RECOVERABLE_EXIT_CODE";

/// Overrides the number of build runs the local host may claim at once.
pub const MAX_CONCURRENT_BUILD_RUNS_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_MAX_CONCURRENT_BUILD_RUNS";

/// Overrides the number of publish runs the local host may claim at once.
pub const MAX_CONCURRENT_PUBLISH_RUNS_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_MAX_CONCURRENT_PUBLISH_RUNS";

/// Overrides the number of active release lanes allowed per repository.
pub const MAX_ACTIVE_RELEASES_PER_REPOSITORY_ENV: &str =
    "HANDY_GAMES_PUBLISHER_RUNTIME_MAX_ACTIVE_RELEASES_PER_REPOSITORY";

/// Describes the host platform used for application directory selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Windows,
    MacOS,
    Linux,
}

impl HostPlatform {
    /// Returns the host platform of the current process.
    pub fn current() -> Self {
        match env::consts::OS {
            "windows" => Self::Windows,
            "macos" => Self::MacOS,
            _ => Self::Linux,
        }
    }

    /// Returns the stable label used in runtime diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOS => "macos",
            Self::Linux => "linux",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct HostEnvironment {
    local_app_data: Option<PathBuf>,
    app_data: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    home_dir: Option<PathBuf>,
}

impl HostEnvironment {
    fn current() -> Self {
        Self {
            local_app_data: env::var_os("LOCALAPPDATA").map(PathBuf::from),
            app_data: env::var_os("APPDATA").map(PathBuf::from),
            xdg_data_home: env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            home_dir: env::var_os("HOME")
                .or_else(|| env::var_os("USERPROFILE"))
                .map(PathBuf::from),
        }
    }
}

/// Describes the filesystem roots managed by the bundled runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDirectories {
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub runs_dir: PathBuf,
}

impl RuntimeDirectories {
    /// Builds the directory layout for a single local runtime root.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let data_dir = root.into();

        Self {
            state_dir: data_dir.join("state"),
            logs_dir: data_dir.join("logs"),
            artifacts_dir: data_dir.join("artifacts"),
            runs_dir: data_dir.join("runs"),
            data_dir,
        }
    }

    /// Creates every directory required by the current runtime layout.
    pub fn ensure_exists(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.state_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        std::fs::create_dir_all(&self.artifacts_dir)?;
        std::fs::create_dir_all(&self.runs_dir)?;
        Ok(())
    }
}

/// Configures the long-running runtime work loop used by supervised processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLoopConfig {
    pub worker_loop_interval_millis: u64,
    pub heartbeat_interval_millis: u64,
    pub max_heartbeats: Option<u32>,
    pub crash_after_heartbeats: Option<u32>,
    pub crash_attempts: u32,
}

impl RuntimeLoopConfig {
    fn load() -> io::Result<Self> {
        Ok(Self {
            worker_loop_interval_millis: parse_env_u64(
                WORKER_LOOP_INTERVAL_MILLIS_ENV,
                1_000,
            )?,
            heartbeat_interval_millis: parse_env_u64(
                HEARTBEAT_INTERVAL_MILLIS_ENV,
                5_000,
            )?,
            max_heartbeats: parse_optional_env_u32(MAX_HEARTBEATS_ENV)?,
            crash_after_heartbeats: parse_optional_env_u32(CRASH_AFTER_HEARTBEATS_ENV)?,
            crash_attempts: parse_env_u32(CRASH_ATTEMPTS_ENV, 0)?,
        })
    }

    fn development() -> Self {
        Self {
            worker_loop_interval_millis: 1_000,
            heartbeat_interval_millis: 5_000,
            max_heartbeats: None,
            crash_after_heartbeats: None,
            crash_attempts: 0,
        }
    }
}

/// Configures how the runtime should be restarted after recoverable crashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSupervisionSettings {
    pub max_restarts: u32,
    pub restart_backoff_millis: u64,
    pub recoverable_exit_code: i32,
}

impl RuntimeSupervisionSettings {
    fn load() -> io::Result<Self> {
        Ok(Self {
            max_restarts: parse_env_u32(SUPERVISION_MAX_RESTARTS_ENV, 3)?,
            restart_backoff_millis: parse_env_u64(SUPERVISION_BACKOFF_MILLIS_ENV, 1_000)?,
            recoverable_exit_code: parse_env_i32(
                SUPERVISION_RECOVERABLE_EXIT_CODE_ENV,
                75,
            )?,
        })
    }

    fn development() -> Self {
        Self {
            max_restarts: 3,
            restart_backoff_millis: 1_000,
            recoverable_exit_code: 75,
        }
    }
}

/// Configures host-local concurrency ceilings for runtime work claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConcurrencySettings {
    pub max_concurrent_build_runs: u32,
    pub max_concurrent_publish_runs: u32,
    pub max_active_releases_per_repository: u32,
}

impl RuntimeConcurrencySettings {
    fn load() -> io::Result<Self> {
        Ok(Self {
            max_concurrent_build_runs: parse_env_u32(MAX_CONCURRENT_BUILD_RUNS_ENV, 1)?,
            max_concurrent_publish_runs: parse_env_u32(MAX_CONCURRENT_PUBLISH_RUNS_ENV, 1)?,
            max_active_releases_per_repository: parse_env_u32(
                MAX_ACTIVE_RELEASES_PER_REPOSITORY_ENV,
                1,
            )?,
        })
    }

    pub fn development() -> Self {
        Self {
            max_concurrent_build_runs: 1,
            max_concurrent_publish_runs: 1,
            max_active_releases_per_repository: 1,
        }
    }
}

/// Captures the minimum configuration required to bootstrap the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub runtime_name: &'static str,
    pub runtime_version: &'static str,
    pub platform: HostPlatform,
    pub directories: RuntimeDirectories,
    pub log_level: String,
    pub runtime_loop: RuntimeLoopConfig,
    pub supervision: RuntimeSupervisionSettings,
    pub concurrency: RuntimeConcurrencySettings,
}

impl RuntimeConfig {
    /// Loads configuration from the current host environment.
    pub fn load() -> io::Result<Self> {
        let platform = HostPlatform::current();
        let environment = HostEnvironment::current();
        let root = match env::var_os(RUNTIME_ROOT_ENV) {
            Some(path) => PathBuf::from(path),
            None => resolve_default_root(platform, &environment)?,
        };

        Ok(Self::from_parts(
            RuntimeDirectories::from_root(root),
            platform,
            env::var(LOG_LEVEL_ENV).unwrap_or_else(|_| "info".to_owned()),
            RuntimeLoopConfig::load()?,
            RuntimeSupervisionSettings::load()?,
            RuntimeConcurrencySettings::load()?,
        ))
    }

    /// Returns a deterministic development configuration for early migration work.
    pub fn development() -> Self {
        Self::from_parts(
            RuntimeDirectories::from_root(PathBuf::from("var/runtime")),
            HostPlatform::current(),
            "info".to_owned(),
            RuntimeLoopConfig::development(),
            RuntimeSupervisionSettings::development(),
            RuntimeConcurrencySettings::development(),
        )
    }

    /// Builds a runtime configuration for an explicit root on the current platform.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self::from_parts(
            RuntimeDirectories::from_root(root),
            HostPlatform::current(),
            "info".to_owned(),
            RuntimeLoopConfig::development(),
            RuntimeSupervisionSettings::development(),
            RuntimeConcurrencySettings::development(),
        )
    }

    fn from_parts(
        directories: RuntimeDirectories,
        platform: HostPlatform,
        log_level: String,
        runtime_loop: RuntimeLoopConfig,
        supervision: RuntimeSupervisionSettings,
        concurrency: RuntimeConcurrencySettings,
    ) -> Self {
        Self {
            runtime_name: RUNTIME_NAME,
            runtime_version: env!("CARGO_PKG_VERSION"),
            platform,
            directories,
            log_level,
            runtime_loop,
            supervision,
            concurrency,
        }
    }
}

fn parse_env_u64(variable_name: &str, default_value: u64) -> io::Result<u64> {
    match env::var(variable_name) {
        Ok(value) => value.parse().map_err(|_| invalid_env_error(variable_name, &value)),
        Err(env::VarError::NotPresent) => Ok(default_value),
        Err(env::VarError::NotUnicode(_)) => Err(invalid_env_error(variable_name, "<non-unicode>")),
    }
}

fn parse_env_u32(variable_name: &str, default_value: u32) -> io::Result<u32> {
    match env::var(variable_name) {
        Ok(value) => value.parse().map_err(|_| invalid_env_error(variable_name, &value)),
        Err(env::VarError::NotPresent) => Ok(default_value),
        Err(env::VarError::NotUnicode(_)) => Err(invalid_env_error(variable_name, "<non-unicode>")),
    }
}

fn parse_env_i32(variable_name: &str, default_value: i32) -> io::Result<i32> {
    match env::var(variable_name) {
        Ok(value) => value.parse().map_err(|_| invalid_env_error(variable_name, &value)),
        Err(env::VarError::NotPresent) => Ok(default_value),
        Err(env::VarError::NotUnicode(_)) => Err(invalid_env_error(variable_name, "<non-unicode>")),
    }
}

fn parse_optional_env_u32(variable_name: &str) -> io::Result<Option<u32>> {
    match env::var(variable_name) {
        Ok(value) => value
            .parse()
            .map(Some)
            .map_err(|_| invalid_env_error(variable_name, &value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(invalid_env_error(variable_name, "<non-unicode>")),
    }
}

fn resolve_default_root(
    platform: HostPlatform,
    environment: &HostEnvironment,
) -> io::Result<PathBuf> {
    let base_dir = match platform {
        HostPlatform::Windows => environment
            .local_app_data
            .as_ref()
            .or(environment.app_data.as_ref())
            .cloned()
            .ok_or_else(|| missing_variable_error("LOCALAPPDATA or APPDATA"))?,
        HostPlatform::MacOS => environment
            .home_dir
            .as_ref()
            .map(|home_dir| home_dir.join("Library").join("Application Support"))
            .ok_or_else(|| missing_variable_error("HOME"))?,
        HostPlatform::Linux => environment
            .xdg_data_home
            .as_ref()
            .cloned()
            .or_else(|| {
                environment
                    .home_dir
                    .as_ref()
                    .map(|home_dir| home_dir.join(".local").join("share"))
            })
            .ok_or_else(|| missing_variable_error("XDG_DATA_HOME or HOME"))?,
    };

    Ok(base_dir.join(PRODUCT_DIRECTORY_NAME).join("runtime"))
}

fn missing_variable_error(variable_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("missing host directory environment: {variable_name}"),
    )
}

fn invalid_env_error(variable_name: &str, value: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid value for {variable_name}: {value}"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_default_root, HostEnvironment, HostPlatform, RuntimeConfig,
        RuntimeDirectories,
    };
    use std::path::PathBuf;

    #[test]
    fn from_root_builds_expected_layout() {
        let directories = RuntimeDirectories::from_root(PathBuf::from("/tmp/runtime"));

        assert_eq!(directories.state_dir, PathBuf::from("/tmp/runtime/state"));
        assert_eq!(directories.logs_dir, PathBuf::from("/tmp/runtime/logs"));
        assert_eq!(
            directories.artifacts_dir,
            PathBuf::from("/tmp/runtime/artifacts")
        );
        assert_eq!(
            directories.runs_dir,
            PathBuf::from("/tmp/runtime/runs")
        );
    }

    #[test]
    fn windows_default_root_prefers_local_app_data() {
        let environment = HostEnvironment {
            local_app_data: Some(PathBuf::from("C:/Users/test/AppData/Local")),
            app_data: Some(PathBuf::from("C:/Users/test/AppData/Roaming")),
            xdg_data_home: None,
            home_dir: Some(PathBuf::from("C:/Users/test")),
        };

        let root = resolve_default_root(HostPlatform::Windows, &environment)
            .expect("windows root should resolve");

        assert_eq!(
            root,
            PathBuf::from("C:/Users/test/AppData/Local/HandyGamesPublisher/runtime")
        );
    }

    #[test]
    fn macos_default_root_uses_application_support() {
        let environment = HostEnvironment {
            local_app_data: None,
            app_data: None,
            xdg_data_home: None,
            home_dir: Some(PathBuf::from("/Users/test")),
        };

        let root =
            resolve_default_root(HostPlatform::MacOS, &environment).expect("macOS root");

        assert_eq!(
            root,
            PathBuf::from("/Users/test/Library/Application Support/HandyGamesPublisher/runtime")
        );
    }

    #[test]
    fn linux_default_root_uses_xdg_directory() {
        let environment = HostEnvironment {
            local_app_data: None,
            app_data: None,
            xdg_data_home: Some(PathBuf::from("/home/test/.data")),
            home_dir: Some(PathBuf::from("/home/test")),
        };

        let root = resolve_default_root(HostPlatform::Linux, &environment)
            .expect("linux root should resolve");

        assert_eq!(
            root,
            PathBuf::from("/home/test/.data/HandyGamesPublisher/runtime")
        );
    }

    #[test]
    fn from_root_uses_single_host_concurrency_defaults() {
        let config = RuntimeConfig::from_root(PathBuf::from("/tmp/runtime"));

        assert_eq!(config.concurrency.max_concurrent_build_runs, 1);
        assert_eq!(config.concurrency.max_concurrent_publish_runs, 1);
        assert_eq!(config.concurrency.max_active_releases_per_repository, 1);
    }

    #[test]
    fn from_root_uses_fast_worker_loop_and_slower_heartbeat_defaults() {
        let config = RuntimeConfig::from_root(PathBuf::from("/tmp/runtime"));

        assert_eq!(config.runtime_loop.worker_loop_interval_millis, 1_000);
        assert_eq!(config.runtime_loop.heartbeat_interval_millis, 5_000);
    }
}