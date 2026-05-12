//! Owns runtime bootstrap, health persistence, supervision contracts,
//! structured lifecycle logging, and shutdown metadata.

#![forbid(unsafe_code)]

pub mod concurrency;

use runtime_config::{RuntimeConfig, RuntimeSupervisionSettings};
use runtime_store::{
    initialize_database, recover_runtime_state, DatabaseBootstrapReport,
    RuntimeRecoveryReport, StorageLayout,
    RECOVERY_INTERRUPTION_KIND_REQUESTED, RECOVERY_INTERRUPTION_KIND_SYSTEM,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

/// Versions the local supervision contract consumed by the desktop shell.
pub const SUPERVISION_PROTOCOL_VERSION: u32 = 1;

/// Names the log event emitted by heartbeat updates.
pub const RUNTIME_HEARTBEAT_EVENT: &str = "runtime.heartbeat";

/// Describes how the runtime should be restarted after recoverable exits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeRestartPolicy {
    pub max_restarts: u32,
    pub restart_backoff_millis: u64,
    pub recoverable_exit_code: i32,
}

impl RuntimeRestartPolicy {
    /// Builds a serializable restart policy from runtime configuration settings.
    pub fn from_settings(settings: &RuntimeSupervisionSettings) -> Self {
        Self {
            max_restarts: settings.max_restarts,
            restart_backoff_millis: settings.restart_backoff_millis,
            recoverable_exit_code: settings.recoverable_exit_code,
        }
    }

    /// Returns whether the supervisor should restart after the given exit code.
    pub fn should_restart(&self, exit_code: Option<i32>, completed_restarts: u32) -> bool {
        matches!(exit_code, Some(code) if code == self.recoverable_exit_code)
            && completed_restarts < self.max_restarts
    }
}

/// Describes the coarse-grained state of the runtime supervisor loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSupervisorStatus {
    Starting,
    Running,
    Restarting,
    Completed,
    Failed,
}

impl RuntimeSupervisorStatus {
    /// Returns the stable label used in supervisor diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Restarting => "restarting",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

/// Describes the coarse-grained health state of the bundled runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Bootstrapping,
    Healthy,
    ShuttingDown,
    Stopped,
    Unhealthy,
}

impl RuntimeStatus {
    /// Returns the stable label used by the bundled runtime.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bootstrapping => "bootstrapping",
            Self::Healthy => "healthy",
            Self::ShuttingDown => "shutting_down",
            Self::Stopped => "stopped",
            Self::Unhealthy => "unhealthy",
        }
    }
}

/// Persists the operator-facing health snapshot of the bundled runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHealthReport {
    pub runtime_name: String,
    pub runtime_version: String,
    pub platform: String,
    pub log_level: String,
    pub status: RuntimeStatus,
    pub process_id: u32,
    pub started_at_unix: u64,
    pub updated_at_unix: u64,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub health_report_path: PathBuf,
    pub log_file_path: PathBuf,
}

impl RuntimeHealthReport {
    /// Builds a health snapshot for the current runtime state.
    pub fn new(
        config: &RuntimeConfig,
        storage: &StorageLayout,
        status: RuntimeStatus,
        process_id: u32,
        started_at_unix: u64,
        updated_at_unix: u64,
    ) -> Self {
        Self {
            runtime_name: config.runtime_name.to_owned(),
            runtime_version: config.runtime_version.to_owned(),
            platform: config.platform.as_str().to_owned(),
            log_level: config.log_level.clone(),
            status,
            process_id,
            started_at_unix,
            updated_at_unix,
            data_dir: config.directories.data_dir.clone(),
            database_path: storage.database_path.clone(),
            health_report_path: storage.health_report_path.clone(),
            log_file_path: storage.runtime_log_path.clone(),
        }
    }

    /// Returns a copy of the health snapshot with a new lifecycle status.
    pub fn with_status(&self, status: RuntimeStatus, updated_at_unix: u64) -> Self {
        let mut next = self.clone();
        next.status = status;
        next.updated_at_unix = updated_at_unix;
        next
    }

    /// Serializes the health snapshot for CLI diagnostics.
    pub fn to_json_pretty(&self) -> io::Result<String> {
        serde_json::to_string_pretty(self).map_err(json_error)
    }
}

/// Describes how the desktop shell can supervise the bundled runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSupervisionContract {
    pub protocol_version: u32,
    pub runtime_name: String,
    pub runtime_version: String,
    pub runtime_command: PathBuf,
    pub health_report_path: PathBuf,
    pub log_file_path: PathBuf,
    pub supervisor_state_path: PathBuf,
    pub serve_arguments: Vec<String>,
    pub bootstrap_arguments: Vec<String>,
    pub shutdown_arguments: Vec<String>,
    pub health_arguments: Vec<String>,
    pub status_arguments: Vec<String>,
    pub restart_policy: RuntimeRestartPolicy,
}

impl RuntimeSupervisionContract {
    /// Builds the local supervision contract for the current runtime binary.
    pub fn new(
        config: &RuntimeConfig,
        storage: &StorageLayout,
        runtime_command: &Path,
        restart_policy: RuntimeRestartPolicy,
    ) -> Self {
        Self {
            protocol_version: SUPERVISION_PROTOCOL_VERSION,
            runtime_name: config.runtime_name.to_owned(),
            runtime_version: config.runtime_version.to_owned(),
            runtime_command: runtime_command.to_path_buf(),
            health_report_path: storage.health_report_path.clone(),
            log_file_path: storage.runtime_log_path.clone(),
            supervisor_state_path: storage.supervisor_state_path.clone(),
            serve_arguments: vec!["serve".to_owned()],
            bootstrap_arguments: vec!["bootstrap".to_owned()],
            shutdown_arguments: vec!["shutdown".to_owned()],
            health_arguments: vec!["health".to_owned()],
            status_arguments: vec!["status".to_owned()],
            restart_policy,
        }
    }

    /// Serializes the supervision contract for CLI diagnostics.
    pub fn to_json_pretty(&self) -> io::Result<String> {
        serde_json::to_string_pretty(self).map_err(json_error)
    }
}

/// Couples the persisted health report with the supervision contract used by the shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeStateSnapshot {
    pub database_bootstrap: DatabaseBootstrapReport,
    pub recovery_report: RuntimeRecoveryReport,
    pub health_report: RuntimeHealthReport,
    pub supervision_contract: RuntimeSupervisionContract,
}

impl RuntimeStateSnapshot {
    /// Serializes the runtime snapshot for bootstrap diagnostics.
    pub fn to_json_pretty(&self) -> io::Result<String> {
        serde_json::to_string_pretty(self).map_err(json_error)
    }
}

/// Persists the state of the runtime supervisor loop for shell diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSupervisorSnapshot {
    pub runtime_name: String,
    pub runtime_version: String,
    pub supervisor_process_id: u32,
    pub active_child_process_id: Option<u32>,
    pub attempt: u32,
    pub restart_count: u32,
    pub last_exit_code: Option<i32>,
    pub status: RuntimeSupervisorStatus,
    pub updated_at_unix: u64,
    pub message: String,
}

impl RuntimeSupervisorSnapshot {
    /// Builds a supervisor snapshot for the current supervision loop state.
    pub fn new(
        config: &RuntimeConfig,
        supervisor_process_id: u32,
        active_child_process_id: Option<u32>,
        attempt: u32,
        restart_count: u32,
        last_exit_code: Option<i32>,
        status: RuntimeSupervisorStatus,
        message: impl Into<String>,
    ) -> io::Result<Self> {
        Ok(Self {
            runtime_name: config.runtime_name.to_owned(),
            runtime_version: config.runtime_version.to_owned(),
            supervisor_process_id,
            active_child_process_id,
            attempt,
            restart_count,
            last_exit_code,
            status,
            updated_at_unix: unix_timestamp()?,
            message: message.into(),
        })
    }

    /// Serializes the supervisor snapshot for CLI diagnostics.
    pub fn to_json_pretty(&self) -> io::Result<String> {
        serde_json::to_string_pretty(self).map_err(json_error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct StructuredLogEvent {
    level: &'static str,
    event: &'static str,
    runtime_name: String,
    runtime_version: String,
    status: RuntimeStatus,
    process_id: u32,
    timestamp_unix: u64,
    message: String,
}

struct RuntimeRecoveryInterruptionContext {
    kind: &'static str,
    message: &'static str,
}

/// Bootstraps the local runtime metadata and writes the initial health contract.
pub fn bootstrap_runtime(
    config: &RuntimeConfig,
    storage: &StorageLayout,
    runtime_command: &Path,
    restart_policy: RuntimeRestartPolicy,
) -> io::Result<RuntimeStateSnapshot> {
    config.directories.ensure_exists()?;
    let recovery_context = recovery_interruption_context(storage);
    let database_bootstrap = initialize_database(storage)?;
    let recovery_report = recover_runtime_state(
        storage,
        recovery_context.kind,
        recovery_context.message,
    )?;

    let supervision_contract =
        RuntimeSupervisionContract::new(config, storage, runtime_command, restart_policy);
    write_json_file(
        &storage.supervision_contract_path,
        &supervision_contract,
    )?;

    let started_at_unix = unix_timestamp()?;
    let process_id = process::id();
    let bootstrapping = RuntimeHealthReport::new(
        config,
        storage,
        RuntimeStatus::Bootstrapping,
        process_id,
        started_at_unix,
        started_at_unix,
    );
    persist_health_report(storage, &bootstrapping)?;
    emit_log(
        &storage.runtime_log_path,
        StructuredLogEvent {
            level: "info",
            event: "runtime.bootstrap.started",
            runtime_name: bootstrapping.runtime_name.clone(),
            runtime_version: bootstrapping.runtime_version.clone(),
            status: bootstrapping.status,
            process_id,
            timestamp_unix: bootstrapping.updated_at_unix,
            message: format!(
                "runtime directories ready at {} and sqlite opened at {}",
                config.directories.data_dir.display(),
                database_bootstrap.database_path.display()
            ),
        },
    )?;

    let healthy = bootstrapping.with_status(RuntimeStatus::Healthy, unix_timestamp()?);
    persist_health_report(storage, &healthy)?;
    emit_log(
        &storage.runtime_log_path,
        StructuredLogEvent {
            level: "info",
            event: "runtime.bootstrap.completed",
            runtime_name: healthy.runtime_name.clone(),
            runtime_version: healthy.runtime_version.clone(),
            status: healthy.status,
            process_id,
            timestamp_unix: healthy.updated_at_unix,
            message: format!(
                "runtime health report written to {} after applying {} migrations",
                storage.health_report_path.display(),
                database_bootstrap.applied_migrations.len()
            ),
        },
    )?;

    Ok(RuntimeStateSnapshot {
        database_bootstrap,
        recovery_report,
        health_report: healthy,
        supervision_contract,
    })
}

fn recovery_interruption_context(storage: &StorageLayout) -> RuntimeRecoveryInterruptionContext {
    match read_health_report(&storage.health_report_path) {
        Ok(report)
            if matches!(report.status, RuntimeStatus::ShuttingDown | RuntimeStatus::Stopped) =>
        {
            RuntimeRecoveryInterruptionContext {
                kind: RECOVERY_INTERRUPTION_KIND_REQUESTED,
                message: "build attempt interrupted after a requested runtime shutdown",
            }
        }
        Ok(_) | Err(_) => RuntimeRecoveryInterruptionContext {
            kind: RECOVERY_INTERRUPTION_KIND_SYSTEM,
            message: "build attempt interrupted after an unexpected runtime interruption",
        },
    }
}

/// Marks the runtime as stopped in the persisted lifecycle metadata.
pub fn shutdown_runtime(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> io::Result<RuntimeHealthReport> {
    config.directories.ensure_exists()?;

    let current = match read_health_report(&storage.health_report_path) {
        Ok(report) => report,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let now = unix_timestamp()?;
            RuntimeHealthReport::new(
                config,
                storage,
                RuntimeStatus::Unhealthy,
                process::id(),
                now,
                now,
            )
        }
        Err(error) => return Err(error),
    };

    let shutting_down = current.with_status(RuntimeStatus::ShuttingDown, unix_timestamp()?);
    persist_health_report(storage, &shutting_down)?;
    emit_log(
        &storage.runtime_log_path,
        StructuredLogEvent {
            level: "info",
            event: "runtime.shutdown.started",
            runtime_name: shutting_down.runtime_name.clone(),
            runtime_version: shutting_down.runtime_version.clone(),
            status: shutting_down.status,
            process_id: shutting_down.process_id,
            timestamp_unix: shutting_down.updated_at_unix,
            message: "runtime shutdown marker persisted".to_owned(),
        },
    )?;

    let stopped = shutting_down.with_status(RuntimeStatus::Stopped, unix_timestamp()?);
    persist_health_report(storage, &stopped)?;
    emit_log(
        &storage.runtime_log_path,
        StructuredLogEvent {
            level: "info",
            event: "runtime.shutdown.completed",
            runtime_name: stopped.runtime_name.clone(),
            runtime_version: stopped.runtime_version.clone(),
            status: stopped.status,
            process_id: stopped.process_id,
            timestamp_unix: stopped.updated_at_unix,
            message: "runtime stopped cleanly".to_owned(),
        },
    )?;

    Ok(stopped)
}

/// Reads the last persisted runtime health report from disk.
pub fn read_health_report(path: &Path) -> io::Result<RuntimeHealthReport> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(json_error)
}

/// Reads the persisted supervision contract from disk.
pub fn read_supervision_contract(path: &Path) -> io::Result<RuntimeSupervisionContract> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(json_error)
}

/// Reads the persisted supervisor snapshot from disk.
pub fn read_supervisor_snapshot(path: &Path) -> io::Result<RuntimeSupervisorSnapshot> {
    let content = fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(json_error)
}

/// Updates the persisted runtime health report and appends a matching log event.
pub fn update_runtime_health(
    storage: &StorageLayout,
    report: &RuntimeHealthReport,
    status: RuntimeStatus,
    event: &'static str,
    message: impl Into<String>,
) -> io::Result<RuntimeHealthReport> {
    let message = message.into();
    let next = report.with_status(status, unix_timestamp()?);
    persist_health_report(storage, &next)?;
    emit_log(
        &storage.runtime_log_path,
        StructuredLogEvent {
            level: log_level_for_status(status),
            event,
            runtime_name: next.runtime_name.clone(),
            runtime_version: next.runtime_version.clone(),
            status: next.status,
            process_id: next.process_id,
            timestamp_unix: next.updated_at_unix,
            message,
        },
    )?;
    Ok(next)
}

/// Persists the runtime supervisor snapshot and appends a structured log event.
pub fn write_supervisor_snapshot(
    storage: &StorageLayout,
    snapshot: &RuntimeSupervisorSnapshot,
) -> io::Result<()> {
    write_json_file(&storage.supervisor_state_path, snapshot)?;
    emit_log(
        &storage.runtime_log_path,
        StructuredLogEvent {
            level: log_level_for_supervisor_status(snapshot.status),
            event: "runtime.supervisor.state",
            runtime_name: snapshot.runtime_name.clone(),
            runtime_version: snapshot.runtime_version.clone(),
            status: match snapshot.status {
                RuntimeSupervisorStatus::Failed => RuntimeStatus::Unhealthy,
                RuntimeSupervisorStatus::Completed => RuntimeStatus::Stopped,
                _ => RuntimeStatus::Healthy,
            },
            process_id: snapshot.supervisor_process_id,
            timestamp_unix: snapshot.updated_at_unix,
            message: snapshot.message.clone(),
        },
    )
}

fn persist_health_report(storage: &StorageLayout, report: &RuntimeHealthReport) -> io::Result<()> {
    write_json_file(&storage.health_report_path, report)
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_vec_pretty(value).map_err(json_error)?;
    fs::write(path, content)
}

fn emit_log(path: &Path, event: StructuredLogEvent) -> io::Result<()> {
    let line = serde_json::to_string(&event).map_err(json_error)?;
    println!("{line}");

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

fn log_level_for_status(status: RuntimeStatus) -> &'static str {
    match status {
        RuntimeStatus::Unhealthy => "error",
        RuntimeStatus::ShuttingDown | RuntimeStatus::Stopped => "info",
        RuntimeStatus::Bootstrapping | RuntimeStatus::Healthy => "info",
    }
}

fn log_level_for_supervisor_status(status: RuntimeSupervisorStatus) -> &'static str {
    match status {
        RuntimeSupervisorStatus::Failed => "error",
        RuntimeSupervisorStatus::Restarting => "warn",
        RuntimeSupervisorStatus::Starting
        | RuntimeSupervisorStatus::Running
        | RuntimeSupervisorStatus::Completed => "info",
    }
}

fn json_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn unix_timestamp() -> io::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_runtime, read_health_report, read_supervision_contract,
        read_supervisor_snapshot, recovery_interruption_context, shutdown_runtime,
        write_supervisor_snapshot, RuntimeRestartPolicy, RuntimeStatus,
        RuntimeSupervisorSnapshot, RuntimeSupervisorStatus,
        SUPERVISION_PROTOCOL_VERSION,
    };
    use runtime_config::RuntimeConfig;
    use runtime_store::{
        StorageLayout, RECOVERY_INTERRUPTION_KIND_REQUESTED,
        RECOVERY_INTERRUPTION_KIND_SYSTEM,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn bootstrap_writes_contract_report_and_log() {
        let root = test_root("bootstrap");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);

        let snapshot = bootstrap_runtime(
            &config,
            &storage,
            Path::new("hup-runtime"),
            RuntimeRestartPolicy::from_settings(&config.supervision),
        )
        .expect("bootstrap should succeed");

        assert!(snapshot.database_bootstrap.database_path.exists());
        assert_eq!(
            snapshot.database_bootstrap.applied_migrations,
            vec![
                "0001_runtime_metadata.sql",
                "0002_pipeline_definitions.sql",
                "0003_execution_runs.sql",
                "0004_local_coordination.sql",
                "0005_host_native_runner_defaults.sql",
                "0006_repository_source_configuration.sql",
                "0007_repository_path_model_cleanup.sql",
                "0008_build_run_stage_tracking.sql",
            ]
        );
        assert_eq!(snapshot.health_report.status, RuntimeStatus::Healthy);
        assert!(snapshot.recovery_report.is_empty());
        assert_eq!(
            snapshot.supervision_contract.protocol_version,
            SUPERVISION_PROTOCOL_VERSION
        );
        assert_eq!(snapshot.supervision_contract.serve_arguments, vec!["serve"]);
        assert!(storage.health_report_path.exists());
        assert!(storage.supervision_contract_path.exists());
        assert!(storage.runtime_log_path.exists());

        let persisted_report =
            read_health_report(&storage.health_report_path).expect("health report should load");
        let persisted_contract = read_supervision_contract(&storage.supervision_contract_path)
            .expect("contract should load");

        assert_eq!(persisted_report.status, RuntimeStatus::Healthy);
        assert_eq!(persisted_contract.runtime_name, config.runtime_name);
        assert!(storage.database_path.exists());

        fs::remove_dir_all(root).expect("temporary runtime directory should be removable");
    }

    #[test]
    fn shutdown_marks_runtime_as_stopped() {
        let root = test_root("shutdown");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);

        bootstrap_runtime(
            &config,
            &storage,
            Path::new("hup-runtime"),
            RuntimeRestartPolicy::from_settings(&config.supervision),
        )
        .expect("bootstrap should succeed");
        let report = shutdown_runtime(&config, &storage).expect("shutdown should succeed");

        assert_eq!(report.status, RuntimeStatus::Stopped);
        assert_eq!(
            read_health_report(&storage.health_report_path)
                .expect("health report should exist after shutdown")
                .status,
            RuntimeStatus::Stopped
        );

        fs::remove_dir_all(root).expect("temporary runtime directory should be removable");
    }

    #[test]
    fn recovery_interruption_context_uses_requested_shutdown_after_stopped_health_report() {
        let root = test_root("recovery-requested");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);

        bootstrap_runtime(
            &config,
            &storage,
            Path::new("hup-runtime"),
            RuntimeRestartPolicy::from_settings(&config.supervision),
        )
        .expect("bootstrap should succeed");
        shutdown_runtime(&config, &storage).expect("shutdown should succeed");

        let context = recovery_interruption_context(&storage);
        assert_eq!(context.kind, RECOVERY_INTERRUPTION_KIND_REQUESTED);

        fs::remove_dir_all(root).expect("temporary runtime directory should be removable");
    }

    #[test]
    fn recovery_interruption_context_defaults_to_system_without_health_report() {
        let root = test_root("recovery-system-default");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);

        let context = recovery_interruption_context(&storage);
        assert_eq!(context.kind, RECOVERY_INTERRUPTION_KIND_SYSTEM);

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn restart_policy_restarts_only_recoverable_exit_codes_within_budget() {
        let policy = RuntimeRestartPolicy {
            max_restarts: 2,
            restart_backoff_millis: 250,
            recoverable_exit_code: 75,
        };

        assert!(policy.should_restart(Some(75), 0));
        assert!(policy.should_restart(Some(75), 1));
        assert!(!policy.should_restart(Some(75), 2));
        assert!(!policy.should_restart(Some(1), 0));
        assert!(!policy.should_restart(None, 0));
    }

    #[test]
    fn supervisor_snapshot_is_persisted() {
        let root = test_root("supervisor-snapshot");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        config
            .directories
            .ensure_exists()
            .expect("directories should be created");

        let snapshot = RuntimeSupervisorSnapshot::new(
            &config,
            11,
            Some(22),
            2,
            1,
            Some(75),
            RuntimeSupervisorStatus::Restarting,
            "retrying runtime after recoverable exit",
        )
        .expect("snapshot should be created");
        write_supervisor_snapshot(&storage, &snapshot)
            .expect("supervisor snapshot should be persisted");

        assert_eq!(
            read_supervisor_snapshot(&storage.supervisor_state_path)
                .expect("persisted snapshot should load")
                .status,
            RuntimeSupervisorStatus::Restarting
        );

        fs::remove_dir_all(root).expect("temporary runtime directory should be removable");
    }

    fn test_root(label: &str) -> PathBuf {
        let unix_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "handy-unity-publisher-runtime-core-{label}-{}-{unix_time}",
            std::process::id()
        ))
    }
}
