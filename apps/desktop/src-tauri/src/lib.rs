//! Implements the Tauri desktop shell bindings that supervise the bundled
//! runtime and expose operator-facing diagnostics to the UI.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use runtime_config::{HostPlatform, RuntimeConfig};
use runtime_core::{
    read_health_report, read_supervision_contract, read_supervisor_snapshot,
    RuntimeHealthReport, RuntimeRestartPolicy, RuntimeSupervisorSnapshot,
    RuntimeSupervisorStatus,
};
use runtime_git::{KIND_GIT_HTTP_BASIC, KIND_GIT_HTTP_BEARER};
use runtime_runner::{
    default_unity_discovery_root_paths, diagnose_host_native_runner_config,
    inspect_host_capability_profile, HostCapabilityProfile,
    HostNativeRunnerDiagnostics, RunnerFamily,
};
use runtime_store::{
    ArtifactInspectionRecord, AutomationSnapshot, BuildHistoryRecord,
    ReleaseAutomationStatus, UpsertCredentialRecordInput,
    initialize_database, list_artifact_inspection_records,
    list_build_history_records,
    list_build_target_runtime_settings, list_credential_records,
    list_publish_target_runtime_settings, LocalCoordinator, StorageLayout,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, RunEvent};

const RUNTIME_PACKAGE_NAME: &str = "runtime-bin";
const RUNTIME_BINARY_NAME: &str = "hup-runtime";
const DEFAULT_RUNTIME_LOG_LINE_LIMIT: usize = 100;
const MAX_RUNTIME_LOG_LINE_LIMIT: usize = 500;
const SECRET_STORAGE_MODEL_INLINE_SQLITE: &str = "sqlite-inline-config-json";
const RUNTIME_STARTUP_PROBE_MILLIS: u64 = 150;
const RUNTIME_SHUTDOWN_WAIT_POLL_MILLIS: u64 = 100;
const RUNTIME_SHUTDOWN_WAIT_POLLS: usize = 20;
const BUILD_EXECUTION_RETAINED_DIR_NAME: &str = "retained";
const BUILD_EXECUTION_REPORT_FILE_NAME: &str = "execution-report.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeLaunchAction {
    Supervise,
    Shutdown,
}

impl RuntimeLaunchAction {
    const fn as_arg(self) -> &'static str {
        match self {
            Self::Supervise => "supervise",
            Self::Shutdown => "shutdown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeCommandPlan {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    inherit_stdio: bool,
}

impl RuntimeCommandPlan {
    fn into_command(self) -> Command {
        let mut command = Command::new(self.program);
        command.args(self.args);
        command.stdin(Stdio::null());
        if let Some(current_dir) = self.current_dir {
            command.current_dir(current_dir);
        }
        if !self.inherit_stdio {
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
        }
        command
    }
}

#[derive(Default)]
struct RuntimeProcessState {
    child: Mutex<Option<Child>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RuntimeDirectorySettings {
    data_dir: PathBuf,
    state_dir: PathBuf,
    logs_dir: PathBuf,
    artifacts_dir: PathBuf,
    runs_dir: PathBuf,
    database_path: PathBuf,
    health_report_path: PathBuf,
    supervision_contract_path: PathBuf,
    supervisor_state_path: PathBuf,
    runtime_log_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct UnityDiscoveryRootSetting {
    path: PathBuf,
    exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct UnityBuildTargetRunnerSettings {
    build_target_id: i64,
    repository_id: i64,
    repository_name: String,
    target_name: String,
    platform: String,
    runner_type: String,
    build_method: Option<String>,
    enabled: bool,
    diagnostic_status: String,
    diagnostic_message: String,
    host_native_diagnostics: Option<HostNativeRunnerDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct UnityRunnerSettings {
    platform: String,
    supported_runner_families: Vec<String>,
    discovery_roots: Vec<UnityDiscoveryRootSetting>,
    capability_profile: HostCapabilityProfile,
    build_targets: Vec<UnityBuildTargetRunnerSettings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SecretBindingReference {
    binding_kind: String,
    binding_id: i64,
    binding_name: String,
    repository_id: i64,
    repository_name: String,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositorySecretBindingSetting {
    repository_id: i64,
    repository_name: String,
    credentials_id: Option<i64>,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PublishTargetSecretBindingSetting {
    publish_target_id: i64,
    repository_id: i64,
    repository_name: String,
    publish_target_name: String,
    publish_target_kind: String,
    credentials_id: Option<i64>,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CredentialConfigSummary {
    status: String,
    message: String,
    top_level_keys: Vec<String>,
    missing_required_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SecretCredentialSetting {
    credential_id: i64,
    name: String,
    kind: String,
    created_at: String,
    updated_at: String,
    storage_model: String,
    config_summary: CredentialConfigSummary,
    bindings: Vec<SecretBindingReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SecretSettings {
    storage_model: String,
    supported_credential_kinds: Vec<String>,
    warnings: Vec<String>,
    credentials: Vec<SecretCredentialSetting>,
    repository_bindings: Vec<RepositorySecretBindingSetting>,
    publish_target_bindings: Vec<PublishTargetSecretBindingSetting>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SaveSecretCredentialInput {
    credential_id: Option<i64>,
    name: String,
    kind: String,
    config_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdateRepositorySecretBindingInput {
    repository_id: i64,
    credentials_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdatePublishTargetSecretBindingInput {
    publish_target_id: i64,
    credentials_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RuntimeLifecycleCommandSettings {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RuntimeLifecycleSettings {
    startup_command: RuntimeLifecycleCommandSettings,
    shutdown_command: RuntimeLifecycleCommandSettings,
    shell_launches_supervisor_on_startup: bool,
    shell_requests_shutdown_on_exit: bool,
    shell_force_kills_after_timeout: bool,
    shell_relaunches_supervisor_on_restart: bool,
    runtime_supervisor_owns_crash_recovery: bool,
    shutdown_grace_period_millis: u64,
    restart_policy: RuntimeRestartPolicy,
    crash_recovery_status: String,
    supervisor_snapshot: Option<RuntimeSupervisorSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryCredentialReference {
    credential_id: i64,
    name: String,
    kind: String,
    config_status: String,
    config_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryPublishTargetInspection {
    publish_target_id: i64,
    name: String,
    kind: String,
    enabled: bool,
    credentials: Option<RepositoryCredentialReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryInspectionEntry {
    repository_id: i64,
    repository_name: String,
    repo_url: String,
    enabled: bool,
    polling_interval_seconds: i64,
    last_seen_tag: Option<String>,
    enabled_build_target_count: i64,
    credentials: Option<RepositoryCredentialReference>,
    build_targets: Vec<UnityBuildTargetRunnerSettings>,
    publish_targets: Vec<RepositoryPublishTargetInspection>,
    pending_release_count: i64,
    queued_build_runs: i64,
    running_build_runs: i64,
    queued_publish_runs: i64,
    running_publish_runs: i64,
    release_queue: Vec<ReleaseAutomationStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryInspectionSettings {
    generated_at: String,
    repositories: Vec<RepositoryInspectionEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct BuildExecutionReportPayload {
    build_run_id: i64,
    workspace_path: Option<PathBuf>,
    report_path: Option<PathBuf>,
    exists: bool,
    report: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct BuildExecutionRetentionPurgeReport {
    build_run_id: i64,
    workspace_path: Option<PathBuf>,
    retained_dir_path: Option<PathBuf>,
    removed_paths: Vec<PathBuf>,
    workspace_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ApplicationVersionInfo {
    product_name: String,
    app_version: String,
}

/// Runs the desktop shell and supervises the bundled local runtime process.
pub fn run() {
    let app = tauri::Builder::default()
        .manage(RuntimeProcessState::default())
        .invoke_handler(tauri::generate_handler![
            application_version,
            runtime_health,
            runtime_logs,
            runtime_directories,
            runtime_lifecycle_settings,
            release_status,
            repository_inspection,
            build_history,
            artifact_inspection,
            build_execution_report,
            purge_build_execution_retention,
            secret_settings,
            save_secret_credential,
            update_repository_secret_binding,
            update_publish_target_secret_binding,
            unity_runner_settings,
        ])
        .setup(|app| {
            launch_runtime_process(app)
                .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("desktop shell failed to build");

    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            stop_runtime_process(app_handle);
        }
    });
}

#[tauri::command]
fn application_version(app_handle: AppHandle) -> Result<ApplicationVersionInfo, String> {
    let package = app_handle.package_info();
    Ok(ApplicationVersionInfo {
        product_name: package.name.clone(),
        app_version: package.version.to_string(),
    })
}

#[tauri::command]
fn runtime_health() -> Result<RuntimeHealthReport, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_runtime_health_report(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn runtime_logs(line_limit: Option<usize>) -> Result<Vec<String>, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_runtime_log_lines(&config, normalize_runtime_log_line_limit(line_limit))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn runtime_directories() -> Result<RuntimeDirectorySettings, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_runtime_directory_settings(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn runtime_lifecycle_settings() -> Result<RuntimeLifecycleSettings, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_runtime_lifecycle_settings(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn release_status() -> Result<AutomationSnapshot, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_release_status(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn repository_inspection() -> Result<RepositoryInspectionSettings, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_repository_inspection(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn build_history() -> Result<Vec<BuildHistoryRecord>, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_build_history(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn artifact_inspection() -> Result<Vec<ArtifactInspectionRecord>, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_artifact_inspection(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn build_execution_report(build_run_id: i64) -> Result<BuildExecutionReportPayload, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_build_execution_report(&config, build_run_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn purge_build_execution_retention(
    build_run_id: i64,
) -> Result<BuildExecutionRetentionPurgeReport, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    purge_build_execution_retention_files(&config, build_run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn secret_settings() -> Result<SecretSettings, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_secret_settings(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_secret_credential(input: SaveSecretCredentialInput) -> Result<(), String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    persist_secret_credential(&config, input).map_err(|error| error.to_string())
}

#[tauri::command]
fn update_repository_secret_binding(
    input: UpdateRepositorySecretBindingInput,
) -> Result<(), String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    persist_repository_secret_binding(&config, input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn update_publish_target_secret_binding(
    input: UpdatePublishTargetSecretBindingInput,
) -> Result<(), String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    persist_publish_target_secret_binding(&config, input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn unity_runner_settings() -> Result<UnityRunnerSettings, String> {
    let config = RuntimeConfig::load().map_err(|error| error.to_string())?;
    load_unity_runner_settings(&config).map_err(|error| error.to_string())
}

fn launch_runtime_process<R: tauri::Runtime>(app: &tauri::App<R>) -> io::Result<()> {
    let plan = current_runtime_command_plan(RuntimeLaunchAction::Supervise)?;
    let mut child = plan.into_command().spawn()?;

    thread::sleep(Duration::from_millis(RUNTIME_STARTUP_PROBE_MILLIS));
    if let Some(status) = child.try_wait()? {
        return Err(io::Error::other(format!(
            "runtime process exited during shell startup with status {status}"
        )));
    }

    let state = app.state::<RuntimeProcessState>();
    let mut guard = state
        .child
        .lock()
        .map_err(|_| io::Error::other("runtime process mutex is poisoned"))?;
    *guard = Some(child);
    Ok(())
}

fn stop_runtime_process<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    let state = app_handle.state::<RuntimeProcessState>();
    let child = match state.child.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => None,
    };
    let Some(mut child) = child else {
        return;
    };

    let _ = request_runtime_shutdown();

    for _ in 0..RUNTIME_SHUTDOWN_WAIT_POLLS {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(
                RUNTIME_SHUTDOWN_WAIT_POLL_MILLIS,
            )),
            Err(_) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();
}

fn request_runtime_shutdown() -> io::Result<()> {
    let plan = current_runtime_command_plan(RuntimeLaunchAction::Shutdown)?;
    let status = plan.into_command().status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "runtime shutdown command exited with status {status}"
        )))
    }
}

fn load_runtime_health_report(config: &RuntimeConfig) -> io::Result<RuntimeHealthReport> {
    let storage = StorageLayout::from_directories(&config.directories);
    read_health_report(&storage.health_report_path)
}

fn load_runtime_log_lines(
    config: &RuntimeConfig,
    line_limit: usize,
) -> io::Result<Vec<String>> {
    let storage = StorageLayout::from_directories(&config.directories);
    let content = fs::read_to_string(&storage.runtime_log_path)?;
    let mut lines = VecDeque::with_capacity(line_limit);

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        if lines.len() == line_limit {
            lines.pop_front();
        }
        lines.push_back(line.to_owned());
    }

    Ok(lines.into_iter().collect())
}

fn normalize_runtime_log_line_limit(line_limit: Option<usize>) -> usize {
    line_limit
        .unwrap_or(DEFAULT_RUNTIME_LOG_LINE_LIMIT)
        .clamp(1, MAX_RUNTIME_LOG_LINE_LIMIT)
}

fn load_runtime_directory_settings(
    config: &RuntimeConfig,
) -> io::Result<RuntimeDirectorySettings> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);

    Ok(RuntimeDirectorySettings {
        data_dir: config.directories.data_dir.clone(),
        state_dir: config.directories.state_dir.clone(),
        logs_dir: config.directories.logs_dir.clone(),
        artifacts_dir: config.directories.artifacts_dir.clone(),
        runs_dir: config.directories.runs_dir.clone(),
        database_path: storage.database_path,
        health_report_path: storage.health_report_path,
        supervision_contract_path: storage.supervision_contract_path,
        supervisor_state_path: storage.supervisor_state_path,
        runtime_log_path: storage.runtime_log_path,
    })
}

fn load_runtime_lifecycle_settings(
    config: &RuntimeConfig,
) -> io::Result<RuntimeLifecycleSettings> {
    let storage = StorageLayout::from_directories(&config.directories);
    let startup_command = current_runtime_command_plan(RuntimeLaunchAction::Supervise)?;
    let shutdown_command = current_runtime_command_plan(RuntimeLaunchAction::Shutdown)?;
    let restart_policy = load_runtime_restart_policy(config, &storage)?;
    let supervisor_snapshot = match read_supervisor_snapshot(&storage.supervisor_state_path) {
        Ok(snapshot) => Some(snapshot),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    Ok(RuntimeLifecycleSettings {
        startup_command: startup_command.into(),
        shutdown_command: shutdown_command.into(),
        shell_launches_supervisor_on_startup: true,
        shell_requests_shutdown_on_exit: true,
        shell_force_kills_after_timeout: true,
        shell_relaunches_supervisor_on_restart: true,
        runtime_supervisor_owns_crash_recovery: true,
        shutdown_grace_period_millis: runtime_shutdown_grace_period_millis(),
        restart_policy,
        crash_recovery_status: runtime_crash_recovery_status(supervisor_snapshot.as_ref()),
        supervisor_snapshot,
    })
}

fn load_release_status(config: &RuntimeConfig) -> io::Result<AutomationSnapshot> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(AutomationSnapshot {
            generated_at: String::new(),
            queue_messages: Vec::new(),
            coordination_leases: Vec::new(),
            repositories: Vec::new(),
        });
    }

    LocalCoordinator::new(&storage).automation_snapshot()
}

fn load_repository_inspection(
    config: &RuntimeConfig,
) -> io::Result<RepositoryInspectionSettings> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(RepositoryInspectionSettings {
            generated_at: String::new(),
            repositories: Vec::new(),
        });
    }

    let release_status = load_release_status(config)?;
    let unity_settings = load_unity_runner_settings(config)?;
    let secret_settings = load_secret_settings(config)?;
    let generated_at = release_status.generated_at.clone();
    let release_status_by_repository = release_status
        .repositories
        .into_iter()
        .map(|repository| (repository.repository_id, repository))
        .collect::<HashMap<_, _>>();
    let credential_by_id = secret_settings
        .credentials
        .into_iter()
        .map(|credential| {
            (
                credential.credential_id,
                RepositoryCredentialReference {
                    credential_id: credential.credential_id,
                    name: credential.name,
                    kind: credential.kind,
                    config_status: credential.config_summary.status,
                    config_message: credential.config_summary.message,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let mut build_targets_by_repository = HashMap::<
        i64,
        Vec<UnityBuildTargetRunnerSettings>,
    >::new();
    for target in unity_settings.build_targets {
        build_targets_by_repository
            .entry(target.repository_id)
            .or_default()
            .push(target);
    }

    let mut publish_targets_by_repository =
        HashMap::<i64, Vec<RepositoryPublishTargetInspection>>::new();
    for target in secret_settings.publish_target_bindings {
        publish_targets_by_repository
            .entry(target.repository_id)
            .or_default()
            .push(RepositoryPublishTargetInspection {
                publish_target_id: target.publish_target_id,
                name: target.publish_target_name,
                kind: target.publish_target_kind,
                enabled: target.enabled,
                credentials: clone_credential_reference(
                    &credential_by_id,
                    target.credentials_id,
                ),
            });
    }

    let repositories = LocalCoordinator::new(&storage)
        .list_polling_repositories()?
        .into_iter()
        .map(|repository| {
            let release_status = release_status_by_repository.get(&repository.id);

            RepositoryInspectionEntry {
                repository_id: repository.id,
                repository_name: repository.name,
                repo_url: repository.repo_url,
                enabled: repository.enabled,
                polling_interval_seconds: repository.polling_interval_seconds,
                last_seen_tag: repository.last_seen_tag,
                enabled_build_target_count: repository.enabled_build_target_count,
                credentials: clone_credential_reference(
                    &credential_by_id,
                    repository.credentials_id,
                ),
                build_targets: build_targets_by_repository
                    .remove(&repository.id)
                    .unwrap_or_default(),
                publish_targets: publish_targets_by_repository
                    .remove(&repository.id)
                    .unwrap_or_default(),
                pending_release_count: release_status
                    .map(|status| status.pending_release_count)
                    .unwrap_or(0),
                queued_build_runs: release_status
                    .map(|status| status.queued_build_runs)
                    .unwrap_or(0),
                running_build_runs: release_status
                    .map(|status| status.running_build_runs)
                    .unwrap_or(0),
                queued_publish_runs: release_status
                    .map(|status| status.queued_publish_runs)
                    .unwrap_or(0),
                running_publish_runs: release_status
                    .map(|status| status.running_publish_runs)
                    .unwrap_or(0),
                release_queue: release_status
                    .map(|status| status.release_queue.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();

    Ok(RepositoryInspectionSettings {
        generated_at,
        repositories,
    })
}

fn load_build_history(config: &RuntimeConfig) -> io::Result<Vec<BuildHistoryRecord>> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(Vec::new());
    }

    list_build_history_records(&storage)
}

fn load_artifact_inspection(
    config: &RuntimeConfig,
) -> io::Result<Vec<ArtifactInspectionRecord>> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(Vec::new());
    }

    list_artifact_inspection_records(&storage)
}

fn build_execution_retained_dir(workspace_path: &Path) -> PathBuf {
    workspace_path.join(BUILD_EXECUTION_RETAINED_DIR_NAME)
}

fn build_execution_report_path(workspace_path: &Path) -> PathBuf {
    build_execution_retained_dir(workspace_path).join(BUILD_EXECUTION_REPORT_FILE_NAME)
}

fn load_build_execution_report(
    config: &RuntimeConfig,
    build_run_id: i64,
) -> io::Result<BuildExecutionReportPayload> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(BuildExecutionReportPayload {
            build_run_id,
            workspace_path: None,
            report_path: None,
            exists: false,
            report: None,
        });
    }

    let build_run = LocalCoordinator::new(&storage).get_build_run_record(build_run_id)?;
    let workspace_path = build_run
        .workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let report_path = workspace_path.as_deref().map(build_execution_report_path);
    let report = match report_path.as_ref() {
        Some(path) if path.is_file() => Some(
            serde_json::from_slice(&fs::read(path)?)
                .map_err(io::Error::other)?,
        ),
        _ => None,
    };

    Ok(BuildExecutionReportPayload {
        build_run_id,
        workspace_path,
        report_path,
        exists: report.is_some(),
        report,
    })
}

fn purge_build_execution_retention_files(
    config: &RuntimeConfig,
    build_run_id: i64,
) -> io::Result<BuildExecutionRetentionPurgeReport> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(BuildExecutionRetentionPurgeReport {
            build_run_id,
            workspace_path: None,
            retained_dir_path: None,
            removed_paths: Vec::new(),
            workspace_removed: false,
        });
    }

    let build_run = LocalCoordinator::new(&storage).get_build_run_record(build_run_id)?;
    let workspace_path = build_run
        .workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let retained_dir_path = workspace_path.as_deref().map(build_execution_retained_dir);
    let mut removed_paths = Vec::new();

    if let Some(retained_dir_path) = retained_dir_path.as_ref() {
        if retained_dir_path.exists() {
            fs::remove_dir_all(retained_dir_path)?;
            removed_paths.push(retained_dir_path.clone());
        }
    }

    let mut workspace_removed = false;
    if let Some(workspace_path) = workspace_path.as_ref() {
        if workspace_path.is_dir() {
            let is_empty = fs::read_dir(workspace_path)?.next().is_none();
            if is_empty {
                fs::remove_dir(workspace_path)?;
                removed_paths.push(workspace_path.clone());
                workspace_removed = true;
            }
        }
    }

    Ok(BuildExecutionRetentionPurgeReport {
        build_run_id,
        workspace_path,
        retained_dir_path,
        removed_paths,
        workspace_removed,
    })
}

fn clone_credential_reference(
    credential_by_id: &HashMap<i64, RepositoryCredentialReference>,
    credential_id: Option<i64>,
) -> Option<RepositoryCredentialReference> {
    credential_id.and_then(|id| credential_by_id.get(&id).cloned())
}

fn writable_secret_storage(config: &RuntimeConfig) -> io::Result<StorageLayout> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        initialize_database(&storage)?;
    }

    Ok(storage)
}

fn load_secret_settings(config: &RuntimeConfig) -> io::Result<SecretSettings> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(SecretSettings {
            storage_model: String::from(SECRET_STORAGE_MODEL_INLINE_SQLITE),
            supported_credential_kinds: supported_credential_kinds(),
            warnings: secret_settings_warnings(),
            credentials: Vec::new(),
            repository_bindings: Vec::new(),
            publish_target_bindings: Vec::new(),
        });
    }

    let credentials = list_credential_records(&storage)?;
    let repository_bindings = LocalCoordinator::new(&storage)
        .list_polling_repositories()?
        .into_iter()
        .map(|repository| RepositorySecretBindingSetting {
            repository_id: repository.id,
            repository_name: repository.name,
            credentials_id: repository.credentials_id,
            enabled: repository.enabled,
        })
        .collect::<Vec<_>>();
    let publish_target_bindings = list_publish_target_runtime_settings(&storage)?
        .into_iter()
        .map(|target| PublishTargetSecretBindingSetting {
            publish_target_id: target.id,
            repository_id: target.repository_id,
            repository_name: target.repository_name,
            publish_target_name: target.name,
            publish_target_kind: target.kind,
            credentials_id: target.credentials_id,
            enabled: target.enabled,
        })
        .collect::<Vec<_>>();
    let credential_settings = credentials
        .into_iter()
        .map(|credential| {
            let bindings = credential_binding_references(
                credential.id,
                &repository_bindings,
                &publish_target_bindings,
            );

            SecretCredentialSetting {
                credential_id: credential.id,
                name: credential.name,
                kind: credential.kind.clone(),
                created_at: credential.created_at,
                updated_at: credential.updated_at,
                storage_model: String::from(SECRET_STORAGE_MODEL_INLINE_SQLITE),
                config_summary: summarize_credential_config(
                    &credential.kind,
                    &credential.config_json,
                ),
                bindings,
            }
        })
        .collect();

    Ok(SecretSettings {
        storage_model: String::from(SECRET_STORAGE_MODEL_INLINE_SQLITE),
        supported_credential_kinds: supported_credential_kinds(),
        warnings: secret_settings_warnings(),
        credentials: credential_settings,
        repository_bindings,
        publish_target_bindings,
    })
}

fn persist_secret_credential(
    config: &RuntimeConfig,
    input: SaveSecretCredentialInput,
) -> io::Result<()> {
    if !credential_kind_supported(&input.kind) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "credentials kind {:?} is not translated by the current runtime",
                input.kind
            ),
        ));
    }

    let storage = writable_secret_storage(config)?;
    LocalCoordinator::new(&storage).upsert_credential_record(
        UpsertCredentialRecordInput {
            credential_id: input.credential_id,
            name: input.name,
            kind: input.kind,
            config_json: input.config_json,
        },
    )?;

    Ok(())
}

fn persist_repository_secret_binding(
    config: &RuntimeConfig,
    input: UpdateRepositorySecretBindingInput,
) -> io::Result<()> {
    let storage = writable_secret_storage(config)?;
    LocalCoordinator::new(&storage).update_repository_credentials_binding(
        input.repository_id,
        input.credentials_id,
    )
}

fn persist_publish_target_secret_binding(
    config: &RuntimeConfig,
    input: UpdatePublishTargetSecretBindingInput,
) -> io::Result<()> {
    let storage = writable_secret_storage(config)?;
    LocalCoordinator::new(&storage).update_publish_target_credentials_binding(
        input.publish_target_id,
        input.credentials_id,
    )
}

fn credential_binding_references(
    credential_id: i64,
    repository_bindings: &[RepositorySecretBindingSetting],
    publish_target_bindings: &[PublishTargetSecretBindingSetting],
) -> Vec<SecretBindingReference> {
    let mut bindings = Vec::new();

    for repository in repository_bindings {
        if repository.credentials_id != Some(credential_id) {
            continue;
        }

        bindings.push(SecretBindingReference {
            binding_kind: String::from("repository"),
            binding_id: repository.repository_id,
            binding_name: repository.repository_name.clone(),
            repository_id: repository.repository_id,
            repository_name: repository.repository_name.clone(),
            enabled: repository.enabled,
        });
    }

    for publish_target in publish_target_bindings {
        if publish_target.credentials_id != Some(credential_id) {
            continue;
        }

        bindings.push(SecretBindingReference {
            binding_kind: String::from("publish_target"),
            binding_id: publish_target.publish_target_id,
            binding_name: publish_target.publish_target_name.clone(),
            repository_id: publish_target.repository_id,
            repository_name: publish_target.repository_name.clone(),
            enabled: publish_target.enabled,
        });
    }

    bindings
}

fn summarize_credential_config(kind: &str, config_json: &str) -> CredentialConfigSummary {
    let expected_keys = expected_credential_keys(kind);
    let parsed = match serde_json::from_str::<serde_json::Value>(config_json.trim()) {
        Ok(value) => value,
        Err(error) => {
            return CredentialConfigSummary {
                status: String::from("invalid_config_json"),
                message: format!(
                    "stored credential config_json is not valid JSON: {error}"
                ),
                top_level_keys: Vec::new(),
                missing_required_keys: expected_keys,
            }
        }
    };
    let Some(object) = parsed.as_object() else {
        return CredentialConfigSummary {
            status: String::from("invalid_config_shape"),
            message: String::from(
                "stored credential config_json must decode to a JSON object",
            ),
            top_level_keys: Vec::new(),
            missing_required_keys: expected_keys,
        };
    };

    let mut top_level_keys = object.keys().cloned().collect::<Vec<_>>();
    top_level_keys.sort();
    let missing_required_keys = expected_keys
        .iter()
        .filter(|key| !object.contains_key(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if !credential_kind_supported(kind) {
        return CredentialConfigSummary {
            status: String::from("unsupported_kind"),
            message: format!(
                "credentials kind {:?} is not translated by the current runtime",
                kind
            ),
            top_level_keys,
            missing_required_keys,
        };
    }

    if !missing_required_keys.is_empty() {
        return CredentialConfigSummary {
            status: String::from("incomplete_config"),
            message: format!(
                "stored credential config_json is missing required keys: {}",
                missing_required_keys.join(", ")
            ),
            top_level_keys,
            missing_required_keys,
        };
    }

    CredentialConfigSummary {
        status: String::from("ready"),
        message: String::from(
            "stored credential config_json decodes successfully and required keys are present",
        ),
        top_level_keys,
        missing_required_keys,
    }
}

fn supported_credential_kinds() -> Vec<String> {
    vec![
        String::from(KIND_GIT_HTTP_BASIC),
        String::from(KIND_GIT_HTTP_BEARER),
    ]
}

fn credential_kind_supported(kind: &str) -> bool {
    matches!(kind.trim(), KIND_GIT_HTTP_BASIC | KIND_GIT_HTTP_BEARER)
}

fn expected_credential_keys(kind: &str) -> Vec<String> {
    match kind.trim() {
        KIND_GIT_HTTP_BASIC => vec![String::from("password"), String::from("username")],
        KIND_GIT_HTTP_BEARER => vec![String::from("token")],
        _ => Vec::new(),
    }
}

fn secret_settings_warnings() -> Vec<String> {
    vec![
        String::from(
            "credentials are currently stored inline in SQLite credentials.config_json",
        ),
        String::from(
            "manifest sync resolves env and file sources before persistence, so SQLite may already contain materialized secret values",
        ),
        String::from(
            "OS-native secret storage and durable secret references are not implemented yet",
        ),
    ]
}

fn load_unity_runner_settings(config: &RuntimeConfig) -> io::Result<UnityRunnerSettings> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    let build_targets = if storage.database_path.is_file() {
        list_build_target_runtime_settings(&storage)?
            .into_iter()
            .map(map_build_target_runner_settings)
            .collect()
    } else {
        Vec::new()
    };
    let capability_profile = inspect_host_capability_profile(config.platform);
    let mut supported_runner_families = vec![String::from(RunnerFamily::HostNative.label())];
    if let Some(selected_runner_family) = capability_profile
        .runner_selection
        .selected_runner_family
        .clone()
    {
        if !supported_runner_families.contains(&selected_runner_family) {
            supported_runner_families.push(selected_runner_family);
        }
    }

    Ok(UnityRunnerSettings {
        platform: String::from(config.platform.as_str()),
        supported_runner_families,
        discovery_roots: default_unity_discovery_roots(config.platform),
        capability_profile,
        build_targets,
    })
}

fn default_unity_discovery_roots(
    platform: HostPlatform,
) -> Vec<UnityDiscoveryRootSetting> {
    default_unity_discovery_root_paths(platform)
        .into_iter()
        .map(|path| UnityDiscoveryRootSetting {
            exists: path.is_dir(),
            path,
        })
        .collect()
}

fn map_build_target_runner_settings(
    target: runtime_store::BuildTargetRuntimeSettingsRecord,
) -> UnityBuildTargetRunnerSettings {
    let diagnostic = if target.runner_type == RunnerFamily::HostNative.label() {
        Some(diagnose_host_native_runner_config(&target.config_json))
    } else {
        None
    };
    let (diagnostic_status, diagnostic_message) = match diagnostic.as_ref() {
        Some(diagnostic) => (
            diagnostic.status.clone(),
            diagnostic.message.clone(),
        ),
        None => (
            String::from("unsupported_runner_type"),
            format!(
                "runner type {:?} is not supported by the Tauri runtime",
                target.runner_type
            ),
        ),
    };

    UnityBuildTargetRunnerSettings {
        build_target_id: target.id,
        repository_id: target.repository_id,
        repository_name: target.repository_name,
        target_name: target.name,
        platform: target.platform,
        runner_type: target.runner_type,
        build_method: target.build_method,
        enabled: target.enabled,
        diagnostic_status,
        diagnostic_message,
        host_native_diagnostics: diagnostic,
    }
}

impl From<RuntimeCommandPlan> for RuntimeLifecycleCommandSettings {
    fn from(plan: RuntimeCommandPlan) -> Self {
        Self {
            program: plan.program,
            args: plan.args,
            current_dir: plan.current_dir,
        }
    }
}

fn load_runtime_restart_policy(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> io::Result<RuntimeRestartPolicy> {
    match read_supervision_contract(&storage.supervision_contract_path) {
        Ok(contract) => Ok(contract.restart_policy),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Ok(RuntimeRestartPolicy::from_settings(&config.supervision))
        }
        Err(error) => Err(error),
    }
}

fn runtime_shutdown_grace_period_millis() -> u64 {
    RUNTIME_SHUTDOWN_WAIT_POLL_MILLIS * RUNTIME_SHUTDOWN_WAIT_POLLS as u64
}

fn runtime_crash_recovery_status(
    snapshot: Option<&RuntimeSupervisorSnapshot>,
) -> String {
    match snapshot.map(|snapshot| snapshot.status) {
        Some(RuntimeSupervisorStatus::Starting | RuntimeSupervisorStatus::Running) => {
            String::from("supervisor_running")
        }
        Some(RuntimeSupervisorStatus::Restarting) => String::from("recovering"),
        Some(RuntimeSupervisorStatus::Completed) => String::from("runtime_stopped_cleanly"),
        Some(RuntimeSupervisorStatus::Failed) => String::from("restart_policy_exhausted"),
        None => String::from("not_started"),
    }
}

fn current_runtime_command_plan(action: RuntimeLaunchAction) -> io::Result<RuntimeCommandPlan> {
    if cfg!(debug_assertions) {
        return Ok(development_runtime_command_plan(
            &workspace_root(),
            &development_cargo_path(),
            action,
        ));
    }

    packaged_runtime_command_plan(&std::env::current_exe()?, action)
}

fn development_runtime_command_plan(
    workspace_root: &Path,
    cargo_path: &Path,
    action: RuntimeLaunchAction,
) -> RuntimeCommandPlan {
    RuntimeCommandPlan {
        program: cargo_path.to_path_buf(),
        args: vec![
            String::from("run"),
            String::from("-p"),
            String::from(RUNTIME_PACKAGE_NAME),
            String::from("--bin"),
            String::from(RUNTIME_BINARY_NAME),
            String::from("--"),
            String::from(action.as_arg()),
        ],
        current_dir: Some(workspace_root.to_path_buf()),
        inherit_stdio: true,
    }
}

fn packaged_runtime_command_plan(
    current_executable: &Path,
    action: RuntimeLaunchAction,
) -> io::Result<RuntimeCommandPlan> {
    let parent = current_executable.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::NotFound,
            format!(
                "desktop shell executable {} has no parent directory",
                current_executable.display()
            ),
        )
    })?;

    let program = parent.join(runtime_binary_file_name());
    if !program.is_file() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "packaged runtime executable was not found at {}",
                program.display()
            ),
        ));
    }

    Ok(RuntimeCommandPlan {
        program,
        args: vec![String::from(action.as_arg())],
        current_dir: None,
        inherit_stdio: false,
    })
}

fn development_cargo_path() -> PathBuf {
    option_env!("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn runtime_binary_file_name() -> String {
    format!("{RUNTIME_BINARY_NAME}{}", std::env::consts::EXE_SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::{
        load_artifact_inspection,
        load_build_execution_report,
        load_build_history,
        load_repository_inspection,
        load_release_status,
        development_runtime_command_plan, load_runtime_directory_settings,
        load_runtime_health_report, load_runtime_lifecycle_settings,
        load_runtime_log_lines,
        persist_publish_target_secret_binding,
        persist_repository_secret_binding,
        persist_secret_credential,
        purge_build_execution_retention_files,
        load_secret_settings,
        load_unity_runner_settings,
        normalize_runtime_log_line_limit, packaged_runtime_command_plan,
        runtime_binary_file_name, RuntimeLaunchAction, RUNTIME_BINARY_NAME,
        SaveSecretCredentialInput, UpdatePublishTargetSecretBindingInput,
        UpdateRepositorySecretBindingInput,
    };
    use runtime_config::RuntimeConfig;
    use runtime_core::{
        bootstrap_runtime, write_supervisor_snapshot, RuntimeRestartPolicy,
        RuntimeStatus, RuntimeSupervisorSnapshot, RuntimeSupervisorStatus,
    };
    use runtime_store::{
        initialize_database, open_connection, LocalCoordinator,
        ManualReleaseDispatchInput, StorageLayout,
    };
    use rusqlite::params;
    use std::path::{Path, PathBuf};

    #[test]
    fn development_runtime_command_plan_uses_workspace_cargo_run() {
        let plan = development_runtime_command_plan(
            Path::new("C:/repo"),
            Path::new("C:/tools/cargo.exe"),
            RuntimeLaunchAction::Supervise,
        );

        assert_eq!(plan.program, PathBuf::from("C:/tools/cargo.exe"));
        assert_eq!(plan.current_dir, Some(PathBuf::from("C:/repo")));
        assert_eq!(
            plan.args,
            vec![
                "run",
                "-p",
                "runtime-bin",
                "--bin",
                "hup-runtime",
                "--",
                "supervise",
            ]
        );
        assert!(plan.inherit_stdio);
    }

    #[test]
    fn packaged_runtime_command_plan_uses_sibling_runtime_binary() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-plan-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }
        std::fs::create_dir_all(&root).expect("temp directory should create");

        let desktop_path = root.join(format!("HUP{}", std::env::consts::EXE_SUFFIX));
        let runtime_path = root.join(runtime_binary_file_name());
        std::fs::write(&desktop_path, b"desktop").expect("desktop binary placeholder should write");
        std::fs::write(&runtime_path, b"runtime").expect("runtime binary placeholder should write");

        let plan = packaged_runtime_command_plan(&desktop_path, RuntimeLaunchAction::Shutdown)
            .expect("packaged runtime plan should resolve sibling runtime binary");

        assert_eq!(plan.program, runtime_path);
        assert_eq!(plan.args, vec!["shutdown"]);
        assert!(!plan.inherit_stdio);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_runtime_health_report_reads_persisted_runtime_snapshot() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-health-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        bootstrap_runtime(
            &config,
            &storage,
            Path::new(RUNTIME_BINARY_NAME),
            RuntimeRestartPolicy::from_settings(&config.supervision),
        )
        .expect("runtime bootstrap should persist health metadata");

        let report = load_runtime_health_report(&config)
            .expect("desktop shell health loader should read persisted report");

        assert_eq!(report.status, RuntimeStatus::Healthy);
        assert_eq!(report.health_report_path, storage.health_report_path);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_runtime_log_lines_reads_recent_runtime_events() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-logs-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        bootstrap_runtime(
            &config,
            &storage,
            Path::new(RUNTIME_BINARY_NAME),
            RuntimeRestartPolicy::from_settings(&config.supervision),
        )
        .expect("runtime bootstrap should persist log metadata");

        let logs = load_runtime_log_lines(&config, 2)
            .expect("desktop shell log loader should read persisted log lines");

        assert_eq!(logs.len(), 2);
        assert!(logs[0].contains("\"event\":\"runtime.bootstrap.started\""));
        assert!(logs[1].contains("\"event\":\"runtime.bootstrap.completed\""));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn normalize_runtime_log_line_limit_clamps_requested_window() {
        assert_eq!(normalize_runtime_log_line_limit(None), 100);
        assert_eq!(normalize_runtime_log_line_limit(Some(0)), 1);
        assert_eq!(normalize_runtime_log_line_limit(Some(999)), 500);
    }

    #[test]
    fn load_runtime_directory_settings_resolves_runtime_paths() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-directories-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let settings = load_runtime_directory_settings(&config)
            .expect("desktop shell directory loader should resolve runtime paths");

        assert_eq!(settings.data_dir, root);
        assert_eq!(settings.state_dir, settings.data_dir.join("state"));
        assert_eq!(settings.logs_dir, settings.data_dir.join("logs"));
        assert_eq!(settings.artifacts_dir, settings.data_dir.join("artifacts"));
        assert_eq!(settings.runs_dir, settings.data_dir.join("runs"));
        assert_eq!(settings.database_path, settings.state_dir.join("runtime.db"));
        assert_eq!(
            settings.health_report_path,
            settings.state_dir.join("health.json")
        );
        assert_eq!(
            settings.supervision_contract_path,
            settings.state_dir.join("supervision.json")
        );
        assert_eq!(
            settings.supervisor_state_path,
            settings.state_dir.join("supervisor-state.json")
        );
        assert_eq!(settings.runtime_log_path, settings.logs_dir.join("runtime.jsonl"));
        assert!(settings.data_dir.is_dir());
        assert!(settings.state_dir.is_dir());
        assert!(settings.logs_dir.is_dir());
        assert!(settings.artifacts_dir.is_dir());
        assert!(settings.runs_dir.is_dir());

        std::fs::remove_dir_all(&settings.data_dir)
            .expect("temp directory should be removable");
    }

    #[test]
    fn load_runtime_lifecycle_settings_reports_supervisor_recovery_policy() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-lifecycle-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        let restart_policy = RuntimeRestartPolicy::from_settings(&config.supervision);
        bootstrap_runtime(
            &config,
            &storage,
            Path::new(RUNTIME_BINARY_NAME),
            restart_policy.clone(),
        )
        .expect("runtime bootstrap should persist lifecycle metadata");

        let snapshot = RuntimeSupervisorSnapshot::new(
            &config,
            4100,
            Some(4200),
            2,
            1,
            Some(restart_policy.recoverable_exit_code),
            RuntimeSupervisorStatus::Restarting,
            "recoverable exit detected, retry in progress",
        )
        .expect("supervisor snapshot should build");
        write_supervisor_snapshot(&storage, &snapshot)
            .expect("supervisor snapshot should persist");

        let settings = load_runtime_lifecycle_settings(&config)
            .expect("runtime lifecycle settings should load");

        assert!(settings.shell_launches_supervisor_on_startup);
        assert!(settings.shell_requests_shutdown_on_exit);
        assert!(settings.shell_force_kills_after_timeout);
        assert!(settings.shell_relaunches_supervisor_on_restart);
        assert!(settings.runtime_supervisor_owns_crash_recovery);
        assert_eq!(settings.shutdown_grace_period_millis, 2_000);
        assert_eq!(settings.restart_policy, restart_policy);
        assert_eq!(settings.startup_command.args.last(), Some(&String::from("supervise")));
        assert_eq!(settings.shutdown_command.args.last(), Some(&String::from("shutdown")));
        assert_eq!(settings.crash_recovery_status, "recovering");
        assert_eq!(settings.supervisor_snapshot, Some(snapshot));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_release_status_reports_pending_release_backlog() {
        let root = std::env::temp_dir().join("desktop-shell-release-status-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url) VALUES (?, ?)",
                params!["release-status-repo", "https://example.com/release-status.git"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "windows",
                    "host-native",
                    "CI.Build.Perform",
                    "{}",
                ],
            )
            .expect("build target should insert");
        drop(connection);

        LocalCoordinator::new(&storage)
            .dispatch_manual_release(ManualReleaseDispatchInput {
                repository_id,
                git_tag: String::from("v9.0.0"),
                git_commit: String::from("deadbeef"),
                requested_via: String::from("desktop-shell-test"),
            })
            .expect("manual release dispatch should succeed");

        let snapshot = load_release_status(&config)
            .expect("release status should load queued automation snapshot");

        assert!(!snapshot.generated_at.is_empty());
        assert_eq!(snapshot.repositories.len(), 1);
        assert_eq!(snapshot.repositories[0].repository_name, "release-status-repo");
        assert_eq!(snapshot.repositories[0].enabled_build_target_count, 1);
        assert_eq!(snapshot.repositories[0].pending_release_count, 1);
        assert_eq!(snapshot.repositories[0].release_queue.len(), 1);
        assert_eq!(snapshot.repositories[0].release_queue[0].git_tag, "v9.0.0");

        let release_queue = snapshot
            .queue_messages
            .iter()
            .find(|queue| queue.queue_name == "release-runs")
            .expect("release queue snapshot should exist");
        assert_eq!(release_queue.ready_count, 1);
        assert_eq!(release_queue.leased_count, 0);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_repository_inspection_reports_repository_config_and_backlog() {
        let root = std::env::temp_dir().join("desktop-shell-repository-inspection-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "origin-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("repository credentials should insert");
        let repository_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "publish-bearer",
                    "git-http-bearer",
                    r#"{"token":"top-secret-token"}"#,
                ],
            )
            .expect("publish credentials should insert");
        let publish_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO repositories (
                    name,
                    repo_url,
                    credentials_id,
                    polling_interval_seconds
                )
                VALUES (?, ?, ?, ?)
                ",
                params![
                    "repo-inspection",
                    "https://example.com/repo-inspection.git",
                    repository_credentials_id,
                    120_i64,
                ],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "windows",
                    "host-native",
                    "CI.Build.Perform",
                    "{}",
                ],
            )
            .expect("build target should insert");
        connection
            .execute(
                "
                INSERT INTO publish_targets (
                    repository_id,
                    name,
                    kind,
                    credentials_id
                )
                VALUES (?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "filesystem-release",
                    "filesystem",
                    publish_credentials_id,
                ],
            )
            .expect("publish target should insert");
        drop(connection);

        LocalCoordinator::new(&storage)
            .dispatch_manual_release(ManualReleaseDispatchInput {
                repository_id,
                git_tag: String::from("v10.0.0"),
                git_commit: String::from("feedface"),
                requested_via: String::from("desktop-shell-test"),
            })
            .expect("manual release dispatch should succeed");

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should aggregate repository metadata");

        assert!(!inspection.generated_at.is_empty());
        assert_eq!(inspection.repositories.len(), 1);

        let repository = &inspection.repositories[0];
        assert_eq!(repository.repository_name, "repo-inspection");
        assert_eq!(repository.repo_url, "https://example.com/repo-inspection.git");
        assert_eq!(repository.polling_interval_seconds, 120);
        assert_eq!(repository.enabled_build_target_count, 1);
        assert_eq!(repository.build_targets.len(), 1);
        assert_eq!(repository.build_targets[0].target_name, "windows-player");
        assert_eq!(repository.publish_targets.len(), 1);
        assert_eq!(repository.publish_targets[0].name, "filesystem-release");
        assert_eq!(repository.pending_release_count, 1);
        assert_eq!(repository.release_queue.len(), 1);
        assert_eq!(repository.release_queue[0].git_tag, "v10.0.0");

        let repository_credentials = repository
            .credentials
            .as_ref()
            .expect("repository credentials should resolve");
        assert_eq!(repository_credentials.name, "origin-basic");
        assert_eq!(repository_credentials.kind, "git-http-basic");
        assert_eq!(repository_credentials.config_status, "ready");

        let publish_credentials = repository.publish_targets[0]
            .credentials
            .as_ref()
            .expect("publish target credentials should resolve");
        assert_eq!(publish_credentials.name, "publish-bearer");
        assert_eq!(publish_credentials.kind, "git-http-bearer");

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_build_history_reports_recent_build_activity() {
        let root = std::env::temp_dir().join("desktop-shell-build-history-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url) VALUES (?, ?)",
                params!["build-history-repo", "https://example.com/build-history.git"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "windows",
                    "host-native",
                    "CI.Build.Perform",
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repository_id, "filesystem-release", "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    unity_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v11.0.0",
                    "deadbeef",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    unity_version,
                    image_ref,
                    status,
                    workspace_path,
                    log_path,
                    artifact_root_path,
                    started_at,
                    finished_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "2022.3.20f1",
                    "host-native",
                    "succeeded",
                    "C:/runs/build-history",
                    "C:/logs/build-history.log",
                    "C:/artifacts/build-history",
                    "2026-01-11T10:00:00Z",
                    "2026-01-11T10:05:00Z",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO artifacts (build_run_id, name, kind, path) VALUES (?, ?, ?, ?)",
                params![build_run_id, "history.zip", "archive", "artifacts/history.zip"],
            )
            .expect("artifact should insert");
        let artifact_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO publish_runs (
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    "queued",
                ],
            )
            .expect("publish run should insert");
        drop(connection);

        let builds = load_build_history(&config)
            .expect("build history should load recent build activity");

        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].build_run_id, build_run_id);
        assert_eq!(builds[0].release_run_id, release_run_id);
        assert_eq!(builds[0].repository_name, "build-history-repo");
        assert_eq!(builds[0].git_tag, "v11.0.0");
        assert_eq!(builds[0].build_target_name, "windows-player");
        assert_eq!(builds[0].status, "succeeded");
        assert_eq!(builds[0].artifact_count, 1);
        assert_eq!(builds[0].publish_run_count, 1);
        assert_eq!(
            builds[0].log_path.as_deref(),
            Some("C:/logs/build-history.log")
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_build_execution_report_reads_retained_json_report() {
        let root = std::env::temp_dir().join("desktop-shell-build-execution-report-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let workspace_path = config.directories.runs_dir.join("build-run-report-sample");
        let retained_dir = workspace_path.join("retained");
        std::fs::create_dir_all(&retained_dir).expect("retained directory should create");
        let report_path = retained_dir.join("execution-report.json");
        std::fs::write(
            &report_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "cleanup_policy": "retain-zipped-logs-json-report",
                "build_run": {
                    "status": "succeeded"
                },
                "cleanup": {
                    "status": "completed"
                }
            }))
            .expect("report json should serialize"),
        )
        .expect("report file should write");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url) VALUES (?, ?)",
                params!["report-repo", "https://example.com/report.git"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "windows",
                    "host-native",
                    "CI.Build.Perform",
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    unity_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v16.0.0",
                    "deadbeef",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    status,
                    workspace_path,
                    artifact_root_path,
                    finished_at
                )
                VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "succeeded",
                    workspace_path.display().to_string(),
                    "C:/artifacts/build-run-report-sample",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        drop(connection);

        let payload = load_build_execution_report(&config, build_run_id)
            .expect("build execution report should load");

        assert_eq!(payload.build_run_id, build_run_id);
        assert_eq!(payload.workspace_path.as_deref(), Some(workspace_path.as_path()));
        assert_eq!(payload.report_path.as_deref(), Some(report_path.as_path()));
        assert!(payload.exists);
        assert_eq!(
            payload
                .report
                .as_ref()
                .and_then(|report| report.get("cleanup"))
                .and_then(|cleanup| cleanup.get("status"))
                .and_then(|status| status.as_str()),
            Some("completed")
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn purge_build_execution_retention_removes_retained_files_and_empty_workspace() {
        let root = std::env::temp_dir().join("desktop-shell-build-execution-purge-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let workspace_path = config.directories.runs_dir.join("build-run-purge-sample");
        let retained_dir = workspace_path.join("retained");
        std::fs::create_dir_all(&retained_dir).expect("retained directory should create");
        let report_path = retained_dir.join("execution-report.json");
        std::fs::write(&report_path, "{}").expect("report file should write");
        let archive_path = retained_dir.join("execution-logs.zip");
        std::fs::write(&archive_path, "zip").expect("log archive should write");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url) VALUES (?, ?)",
                params!["purge-repo", "https://example.com/purge.git"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "windows",
                    "host-native",
                    "CI.Build.Perform",
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    unity_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v17.0.0",
                    "deadbeef",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    status,
                    workspace_path,
                    artifact_root_path,
                    finished_at
                )
                VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "succeeded",
                    workspace_path.display().to_string(),
                    "C:/artifacts/build-run-purge-sample",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        drop(connection);

        let purge_report = purge_build_execution_retention_files(&config, build_run_id)
            .expect("retention purge should succeed");

        assert_eq!(purge_report.build_run_id, build_run_id);
        assert_eq!(purge_report.workspace_path.as_deref(), Some(workspace_path.as_path()));
        assert_eq!(purge_report.retained_dir_path.as_deref(), Some(retained_dir.as_path()));
        assert!(purge_report.workspace_removed);
        assert_eq!(
            purge_report.removed_paths,
            vec![retained_dir.clone(), workspace_path.clone()]
        );
        assert!(!report_path.exists());
        assert!(!archive_path.exists());
        assert!(!retained_dir.exists());
        assert!(!workspace_path.exists());

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_artifact_inspection_reports_recent_artifact_activity() {
        let root = std::env::temp_dir().join("desktop-shell-artifact-inspection-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url) VALUES (?, ?)",
                params![
                    "artifact-inspection-repo",
                    "https://example.com/artifact-inspection.git"
                ],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "windows",
                    "host-native",
                    "CI.Build.Perform",
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repository_id, "filesystem-release", "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    unity_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v12.0.0",
                    "feedface",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    unity_version,
                    image_ref,
                    status,
                    artifact_root_path
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "2022.3.20f1",
                    "host-native",
                    "succeeded",
                    "C:/artifacts/artifact-inspection",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO artifacts (
                    build_run_id,
                    name,
                    kind,
                    path,
                    size_bytes,
                    checksum_sha256
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    build_run_id,
                    "artifact-inspection.zip",
                    "archive",
                    "builds/windows/artifact-inspection.zip",
                    8192_i64,
                    "deadbeef",
                ],
            )
            .expect("artifact should insert");
        let artifact_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO publish_runs (
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    "succeeded",
                ],
            )
            .expect("publish run should insert");
        drop(connection);

        let artifacts = load_artifact_inspection(&config)
            .expect("artifact inspection should load recent artifact activity");

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_id, artifact_id);
        assert_eq!(artifacts[0].build_run_id, build_run_id);
        assert_eq!(artifacts[0].release_run_id, release_run_id);
        assert_eq!(artifacts[0].repository_name, "artifact-inspection-repo");
        assert_eq!(artifacts[0].git_tag, "v12.0.0");
        assert_eq!(artifacts[0].build_target_name, "windows-player");
        assert_eq!(artifacts[0].artifact_name, "artifact-inspection.zip");
        assert_eq!(artifacts[0].artifact_kind, "archive");
        assert_eq!(artifacts[0].publish_run_count, 1);
        assert_eq!(artifacts[0].succeeded_publish_runs, 1);
        assert_eq!(artifacts[0].size_bytes, Some(8192));
        assert_eq!(
            artifacts[0].artifact_root_path.as_deref(),
            Some("C:/artifacts/artifact-inspection")
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_unity_runner_settings_reports_discovery_roots_and_target_diagnostics() {
        let root = std::env::temp_dir().join("desktop-shell-unity-runner-settings-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let unity_executable_path = root.join(format!("unity{}", std::env::consts::EXE_SUFFIX));
        std::fs::create_dir_all(&root).expect("test root should create");
        std::fs::write(&unity_executable_path, b"unity")
            .expect("fake unity executable should write");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url) VALUES (?, ?)",
                params!["unity-settings-repo", "https://example.com/unity-settings.git"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "windows",
                    "host-native",
                    "CI.Build.Perform",
                    serde_json::json!({
                        "unity_executable_path": unity_executable_path.display().to_string(),
                        "additional_arguments": ["-silent-crashes"],
                        "environment": {
                            "UNITY_LICENSE": "redacted"
                        }
                    })
                    .to_string(),
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        drop(connection);

        let settings = load_unity_runner_settings(&config)
            .expect("unity runner settings should load");

        assert_eq!(settings.platform, config.platform.as_str());
        assert_eq!(settings.supported_runner_families[0], "host-native");
        assert!(!settings.discovery_roots.is_empty());
        assert_eq!(settings.capability_profile.platform, config.platform.as_str());
        assert!(!settings.capability_profile.architecture.is_empty());
        assert!(!settings.capability_profile.packaging_mode.is_empty());
        assert!(!settings.capability_profile.runner_selection.status.is_empty());
        assert_eq!(settings.build_targets.len(), 1);
        assert_eq!(settings.build_targets[0].build_target_id, build_target_id);
        assert_eq!(settings.build_targets[0].repository_id, repository_id);
        assert_eq!(settings.build_targets[0].repository_name, "unity-settings-repo");
        assert_eq!(settings.build_targets[0].target_name, "windows-player");
        assert_eq!(settings.build_targets[0].platform, "windows");
        assert_eq!(settings.build_targets[0].runner_type, "host-native");
        assert_eq!(
            settings.build_targets[0].build_method.as_deref(),
            Some("CI.Build.Perform")
        );
        assert_eq!(settings.build_targets[0].diagnostic_status, "ready");
        assert!(settings.build_targets[0].host_native_diagnostics.is_some());
        let diagnostics = settings.build_targets[0]
            .host_native_diagnostics
            .as_ref()
            .expect("host-native diagnostics should exist");
        assert_eq!(
            diagnostics.unity_executable_path.as_deref(),
            Some(unity_executable_path.display().to_string().as_str())
        );
        assert!(diagnostics.unity_executable_exists);
        assert!(diagnostics.unity_executable_is_file);
        assert_eq!(diagnostics.additional_argument_count, 1);
        assert_eq!(diagnostics.environment_variable_count, 1);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_secret_settings_reports_redacted_credentials_and_bindings() {
        let root = std::env::temp_dir().join("desktop-shell-secret-settings-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "origin-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("git basic credentials should insert");
        let repository_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "publish-bearer",
                    "git-http-bearer",
                    r#"{"token":"top-secret-token"}"#,
                ],
            )
            .expect("git bearer credentials should insert");
        let publish_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, credentials_id) VALUES (?, ?, ?)",
                params![
                    "revolutions",
                    "https://example.com/revolutions.git",
                    repository_credentials_id,
                ],
            )
            .expect("bound repository should insert");
        let bound_repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url) VALUES (?, ?)",
                params!["workers", "https://example.com/workers.git"],
            )
            .expect("unbound repository should insert");
        connection
            .execute(
                "
                INSERT INTO publish_targets (repository_id, name, kind, credentials_id)
                VALUES (?, ?, ?, ?)
                ",
                params![
                    bound_repository_id,
                    "filesystem-release",
                    "filesystem",
                    publish_credentials_id,
                ],
            )
            .expect("bound publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        drop(connection);

        let settings = load_secret_settings(&config)
            .expect("secret settings should load");

        assert_eq!(settings.storage_model, "sqlite-inline-config-json");
        assert_eq!(
            settings.supported_credential_kinds,
            vec!["git-http-basic", "git-http-bearer"]
        );
        assert_eq!(settings.credentials.len(), 2);
        assert_eq!(settings.repository_bindings.len(), 2);
        assert_eq!(settings.publish_target_bindings.len(), 1);
        assert_eq!(settings.credentials[0].name, "origin-basic");
        assert_eq!(settings.credentials[0].config_summary.status, "ready");
        assert_eq!(
            settings.credentials[0].config_summary.top_level_keys,
            vec!["password", "username"]
        );
        assert_eq!(settings.credentials[0].bindings.len(), 1);
        assert_eq!(settings.credentials[0].bindings[0].binding_kind, "repository");
        assert_eq!(settings.credentials[0].bindings[0].binding_id, bound_repository_id);
        assert_eq!(settings.credentials[1].name, "publish-bearer");
        assert_eq!(settings.credentials[1].config_summary.top_level_keys, vec!["token"]);
        assert_eq!(settings.credentials[1].bindings.len(), 1);
        assert_eq!(settings.credentials[1].bindings[0].binding_kind, "publish_target");
        assert_eq!(settings.credentials[1].bindings[0].binding_id, publish_target_id);
        assert_eq!(settings.repository_bindings[0].repository_name, "revolutions");
        assert_eq!(
            settings.repository_bindings[0].credentials_id,
            Some(repository_credentials_id)
        );
        assert_eq!(settings.repository_bindings[1].repository_name, "workers");
        assert_eq!(settings.repository_bindings[1].credentials_id, None);
        assert_eq!(
            settings.publish_target_bindings[0].credentials_id,
            Some(publish_credentials_id)
        );

        let encoded = serde_json::to_string(&settings)
            .expect("secret settings should serialize without secrets leaking");
        assert!(!encoded.contains("solidarity"));
        assert!(!encoded.contains("top-secret-token"));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_secret_credential_creates_redacted_secret_settings() {
        let root = std::env::temp_dir().join("desktop-shell-secret-save-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        persist_secret_credential(
            &config,
            SaveSecretCredentialInput {
                credential_id: None,
                name: String::from("origin-basic"),
                kind: String::from("git-http-basic"),
                config_json: String::from(
                    r#"{"username":"worker","password":"solidarity"}"#,
                ),
            },
        )
        .expect("credential should persist");

        let settings = load_secret_settings(&config)
            .expect("secret settings should load persisted credential");

        assert_eq!(settings.credentials.len(), 1);
        assert_eq!(settings.credentials[0].name, "origin-basic");
        assert_eq!(settings.credentials[0].kind, "git-http-basic");
        assert_eq!(settings.credentials[0].config_summary.status, "ready");
        let encoded = serde_json::to_string(&settings)
            .expect("secret settings should serialize without secret values");
        assert!(!encoded.contains("solidarity"));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_secret_bindings_updates_repository_and_publish_target_settings() {
        let root = std::env::temp_dir().join("desktop-shell-secret-binding-save-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "origin-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("credentials row should insert");
        let credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url) VALUES (?, ?)",
                params!["revolutions", "https://example.com/revolutions.git"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repository_id, "filesystem-release", "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        drop(connection);

        persist_repository_secret_binding(
            &config,
            UpdateRepositorySecretBindingInput {
                repository_id,
                credentials_id: Some(credentials_id),
            },
        )
        .expect("repository binding should persist");
        persist_publish_target_secret_binding(
            &config,
            UpdatePublishTargetSecretBindingInput {
                publish_target_id,
                credentials_id: Some(credentials_id),
            },
        )
        .expect("publish target binding should persist");

        let settings = load_secret_settings(&config)
            .expect("secret settings should reflect updated bindings");

        assert_eq!(settings.repository_bindings.len(), 1);
        assert_eq!(settings.repository_bindings[0].credentials_id, Some(credentials_id));
        assert_eq!(settings.publish_target_bindings.len(), 1);
        assert_eq!(
            settings.publish_target_bindings[0].credentials_id,
            Some(credentials_id)
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }
}