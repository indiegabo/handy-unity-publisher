use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use runtime_config::{RuntimeConfig, SUPERVISION_ATTEMPT_ENV};
use runtime_git::{
    git_auth_options_from_credentials, GitAuthOptions, GitTag, GitTagListRequest, GitTagLister,
    GitWorkspaceSyncRefRequest, GitWorkspaceSyncer,
};
use runtime_manifests::sync_directory as sync_manifest_directory;
use runtime_publish::{
    ExecutionPlan as PublishExecutionPlan,
    ExecutionProcessor as PublishExecutionProcessor,
    Processor as PublishProcessor,
    resolve_destination_path as resolve_publish_destination_path,
};
use runtime_core::{
    bootstrap_runtime, read_health_report, read_supervision_contract, shutdown_runtime,
    update_runtime_health, write_supervisor_snapshot, RuntimeRestartPolicy,
    RuntimeSupervisionContract, RuntimeSupervisorSnapshot, RuntimeSupervisorStatus,
    RuntimeStatus, RUNTIME_HEARTBEAT_EVENT,
};
use runtime_runner::{
    discover_artifacts, resolve_final_artifact_output_path, ExecutionPlan,
    ExecutionProcessOutcome, ExecutionProcessor, ExecutionProgress,
    ExecutionProgressReporter, ExecutionResult,
    inspect_host_capability_profile, resolve_host_native_execution_plan,
    next_workspace_attempt_token, HostCapabilityProfile, HostNativeUnityExecutor, RunnerFamily,
    WorkspacePreparationInput, WorkspacePreparer,
};
use runtime_store::lifecycle::PublishStatus;
use runtime_store::{
    ArtifactRecord,
    initialize_database, BuildDispatchJob, BuildExecutionPlan as StoredBuildExecutionPlan,
    BuildRunRecord, BuildRunStageRecord, CancelBuildRunInput, CompleteBuildRunInput,
    CompleteBuildRunStageInput, CreateArtifactRecordInput, FailBuildRunInput,
    FailBuildRunStageInput, HeartbeatBuildRunStageInput, LocalCoordinator,
    InterruptedBuildRecoveryRecord,
    ManualReleaseDispatchInput, PollingRepositoryRecord, PublishDispatchJob,
    PublishExecutionPlan as StoredPublishExecutionPlan, PublishRunRecord,
    RepositoryCheckoutRecord,
    QueueDispatchOutcome, RepositoryPollDispatchInput, StartBuildRunInput,
    StartBuildRunStageInput, StartPublishRunInput, StorageLayout, CompletePublishRunInput,
    FailPublishRunInput,
    RuntimeRecoveryReport,
    RECOVERY_INTERRUPTION_KIND_REQUESTED, RECOVERY_INTERRUPTION_KIND_SYSTEM,
    open_connection,
};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const RELEASE_PLANNER_WORKER_NAME: &str = "runtime-release-planner";
const RELEASE_QUEUE_LEASE_TTL: Duration = Duration::from_secs(30);
const BUILD_STAGER_WORKER_NAME: &str = "runtime-build-stager";
const BUILD_QUEUE_LEASE_TTL: Duration = Duration::from_secs(30);
const PUBLISH_WORKER_NAME: &str = "runtime-publish-worker";
const PUBLISH_QUEUE_LEASE_TTL: Duration = Duration::from_secs(30);
const POLL_STATUS_SKIPPED_DISABLED: &str = "skipped_disabled";
const POLL_STATUS_SKIPPED_NO_ENABLED_BUILD_TARGETS: &str = "skipped_no_enabled_build_targets";
const POLL_STATUS_SKIPPED_ACTIVE_RELEASE_BACKLOG: &str = "skipped_active_release_backlog";
const POLL_STATUS_NO_TAGS: &str = "no_tags";
const POLL_STATUS_UNCHANGED: &str = "unchanged";
const POLL_STATUS_QUEUED: &str = "queued";
const POLL_STATUS_ALREADY_SEEN: &str = "already_seen";
const POLL_STATUS_BUILD_IN_PROGRESS: &str = "build_in_progress";
const POLL_STATUS_ERROR: &str = "error";
const POLL_OBSERVED_VIA: &str = "runtime-bin";
const DEFAULT_REVOLUTIONS_PROJECT_PAT_ENV: &str = "REVOLUTIONS_PROJECT_PAT";
const BUILD_EXECUTION_REPORT_SCHEMA_VERSION: u32 = 2;
const BUILD_EXECUTION_CLEANUP_POLICY: &str = "retain-zipped-logs-json-report";
const BUILD_EXECUTION_CLEANUP_PENDING: &str = "pending";
const BUILD_EXECUTION_CLEANUP_COMPLETED: &str = "completed";
const BUILD_EXECUTION_CLEANUP_FAILED: &str = "failed";
const BUILD_EXECUTION_CLEANUP_TRIGGER_TERMINAL_STATE: &str = "terminal_state";
const BUILD_EXECUTION_CLEANUP_TRIGGER_REQUESTED_INTERRUPTION: &str = "requested_interruption";
const BUILD_EXECUTION_CLEANUP_TRIGGER_SYSTEM_INTERRUPTION: &str = "system_interruption";
const BUILD_EXECUTION_RETAINED_DIR_NAME: &str = "retained";
const BUILD_EXECUTION_REPORT_FILE_NAME: &str = "execution-report.json";
const BUILD_EXECUTION_LOG_ARCHIVE_FILE_NAME: &str = "execution-logs.zip";
const UNITY_NON_SHIPPABLE_ARCHIVE_PATH_SUFFIXES: &[&str] = &[
    "_DoNotShip",
    "_BackUpThisFolder_ButDontShipItWithYourGame",
];
const UNITY_MACOS_OPTIONAL_ARCHIVE_PATH_SUFFIXES: &[&str] = &[".dSYM"];
const UNITY_WINDOWS_OPTIONAL_ARCHIVE_FILE_SUFFIXES: &[&str] = &[".pdb"];
const UNITY_WEBGL_OPTIONAL_ARCHIVE_FILE_SUFFIXES: &[&str] = &[".symbols.json"];
const QUEUE_LEASE_RENEWER_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MIN_QUEUE_LEASE_RENEW_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BuildProcessStage {
    ValidateContext,
    CheckoutRepository,
    UnityBuild,
    PackageArtifact,
    RegisterArtifacts,
}

impl BuildProcessStage {
    const fn key(self) -> &'static str {
        match self {
            Self::ValidateContext => "validate-build-context",
            Self::CheckoutRepository => "checkout-repository",
            Self::UnityBuild => "unity-build",
            Self::PackageArtifact => "package-artifact",
            Self::RegisterArtifacts => "register-artifacts",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::ValidateContext => "Validate Build Context",
            Self::CheckoutRepository => "Checkout Repository",
            Self::UnityBuild => "Execute Unity Build",
            Self::PackageArtifact => "Package Artifact",
            Self::RegisterArtifacts => "Register Artifacts",
        }
    }

    const fn writes_runtime_log(self) -> bool {
        !matches!(self, Self::UnityBuild)
    }
}

#[derive(Debug, Default)]
struct BuildRunStageSequence {
    ordered_stages: Vec<BuildProcessStage>,
}

impl BuildRunStageSequence {
    fn execution_index(&mut self, stage: BuildProcessStage) -> usize {
        if let Some(index) = self
            .ordered_stages
            .iter()
            .position(|candidate| *candidate == stage)
        {
            return index + 1;
        }

        self.ordered_stages.push(stage);
        self.ordered_stages.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildRunStageExecution {
    position: i64,
    log_path: PathBuf,
}

struct BuildRunStageTracker<'a> {
    coordinator: &'a LocalCoordinator,
    build_run_id: i64,
    workspace_path: PathBuf,
    artifact_root_path: PathBuf,
    stage_sequence: Rc<RefCell<BuildRunStageSequence>>,
}

impl<'a> BuildRunStageTracker<'a> {
    fn new(
        coordinator: &'a LocalCoordinator,
        build_run_id: i64,
        workspace_path: impl Into<PathBuf>,
        artifact_root_path: impl Into<PathBuf>,
        stage_sequence: Rc<RefCell<BuildRunStageSequence>>,
    ) -> io::Result<Self> {
        let tracker = Self {
            coordinator,
            build_run_id,
            workspace_path: workspace_path.into(),
            artifact_root_path: artifact_root_path.into(),
            stage_sequence,
        };
        fs::create_dir_all(tracker.logs_dir())?;
        Ok(tracker)
    }

    fn logs_dir(&self) -> PathBuf {
        self.workspace_path.join("logs")
    }

    fn stage_log_path(&self, stage: BuildProcessStage) -> PathBuf {
        self.stage_execution(stage).log_path
    }

    fn stage_execution(&self, stage: BuildProcessStage) -> BuildRunStageExecution {
        let execution_index = self.stage_sequence.borrow_mut().execution_index(stage);

        BuildRunStageExecution {
            position: execution_index as i64,
            log_path: self
                .logs_dir()
                .join(format!("{execution_index:02}-{}.log", stage.key())),
        }
    }

    fn start_stage(&self, stage: BuildProcessStage, message: &str) -> io::Result<()> {
        let execution = self.stage_execution(stage);
        self.write_stage_message(stage, &execution.log_path, message)?;
        self.coordinator.start_build_run_stage(
            self.build_run_id,
            StartBuildRunStageInput {
                position: execution.position,
                step_key: String::from(stage.key()),
                step_label: String::from(stage.label()),
                step_log_path: execution.log_path.display().to_string(),
                workspace_path: self.workspace_path.display().to_string(),
                log_path: execution.log_path.display().to_string(),
                artifact_root_path: self.artifact_root_path.display().to_string(),
                message: message.to_owned(),
            },
        )?;
        Ok(())
    }

    fn heartbeat_stage(&self, stage: BuildProcessStage, message: &str) -> io::Result<()> {
        let execution = self.stage_execution(stage);
        self.write_stage_message(stage, &execution.log_path, message)?;
        self.coordinator.heartbeat_build_run_stage(
            self.build_run_id,
            HeartbeatBuildRunStageInput {
                step_key: String::from(stage.key()),
                step_label: String::from(stage.label()),
                step_log_path: execution.log_path.display().to_string(),
                workspace_path: self.workspace_path.display().to_string(),
                log_path: execution.log_path.display().to_string(),
                artifact_root_path: self.artifact_root_path.display().to_string(),
                message: message.to_owned(),
            },
        )?;
        Ok(())
    }

    fn complete_stage(&self, stage: BuildProcessStage, message: &str) -> io::Result<()> {
        let execution = self.stage_execution(stage);
        self.write_stage_message(stage, &execution.log_path, message)?;
        self.coordinator.complete_build_run_stage(
            self.build_run_id,
            CompleteBuildRunStageInput {
                step_key: String::from(stage.key()),
                step_label: String::from(stage.label()),
                step_log_path: execution.log_path.display().to_string(),
                workspace_path: self.workspace_path.display().to_string(),
                log_path: execution.log_path.display().to_string(),
                artifact_root_path: self.artifact_root_path.display().to_string(),
                message: message.to_owned(),
            },
        )?;
        Ok(())
    }

    fn fail_stage(&self, stage: BuildProcessStage, error_message: &str) -> io::Result<()> {
        let execution = self.stage_execution(stage);
        self.write_stage_message(stage, &execution.log_path, error_message)?;
        self.coordinator.fail_build_run_stage(
            self.build_run_id,
            FailBuildRunStageInput {
                step_key: String::from(stage.key()),
                step_label: String::from(stage.label()),
                step_log_path: execution.log_path.display().to_string(),
                workspace_path: self.workspace_path.display().to_string(),
                log_path: execution.log_path.display().to_string(),
                artifact_root_path: self.artifact_root_path.display().to_string(),
                error_message: error_message.to_owned(),
            },
        )?;
        Ok(())
    }

    fn write_stage_message(
        &self,
        stage: BuildProcessStage,
        path: &Path,
        message: &str,
    ) -> io::Result<()> {
        if !stage.writes_runtime_log() {
            return Ok(());
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(
            file,
            "[{}] {}",
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            message,
        )?;
        Ok(())
    }
}

struct BuildStageHeartbeatReporter<'a, 'b> {
    tracker: &'a BuildRunStageTracker<'b>,
    stage: BuildProcessStage,
    error: Option<io::Error>,
}

impl<'a, 'b> BuildStageHeartbeatReporter<'a, 'b> {
    fn new(tracker: &'a BuildRunStageTracker<'b>, stage: BuildProcessStage) -> Self {
        Self {
            tracker,
            stage,
            error: None,
        }
    }

    fn take_error(&mut self) -> Option<io::Error> {
        self.error.take()
    }
}

impl ExecutionProgressReporter for BuildStageHeartbeatReporter<'_, '_> {
    fn heartbeat(&mut self, progress: ExecutionProgress) {
        if self.error.is_some() {
            return;
        }

        if let Err(error) = self.tracker.heartbeat_stage(self.stage, &progress.message) {
            self.error = Some(error);
        }
    }
}

fn queue_lease_renew_interval(lease_ttl: Duration) -> Duration {
    let ttl_millis = lease_ttl.as_millis();
    let minimum_millis = MIN_QUEUE_LEASE_RENEW_INTERVAL.as_millis();
    let interval_millis = (ttl_millis / 3).max(minimum_millis);

    Duration::from_millis(interval_millis.min(u64::MAX as u128) as u64)
}

fn store_queue_lease_renewer_error(slot: &Arc<Mutex<Option<String>>>, message: String) {
    if let Ok(mut slot) = slot.lock() {
        if slot.is_none() {
            *slot = Some(message);
        }
    }
}

struct QueueLeaseRenewer {
    stop_signal: Arc<AtomicBool>,
    error_message: Arc<Mutex<Option<String>>>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl QueueLeaseRenewer {
    fn spawn(
        coordinator: LocalCoordinator,
        message_id: i64,
        lease_token: String,
        lease_ttl: Duration,
        context: &str,
    ) -> Self {
        let stop_signal = Arc::new(AtomicBool::new(false));
        let error_message = Arc::new(Mutex::new(None));
        let renew_interval = queue_lease_renew_interval(lease_ttl);
        let context = context.to_owned();
        let thread_stop_signal = Arc::clone(&stop_signal);
        let thread_error_message = Arc::clone(&error_message);
        let join_handle = thread::spawn(move || {
            let mut last_renewed_at = std::time::Instant::now();

            loop {
                if thread_stop_signal.load(Ordering::Acquire) {
                    break;
                }

                thread::sleep(QUEUE_LEASE_RENEWER_POLL_INTERVAL);
                if thread_stop_signal.load(Ordering::Acquire) {
                    break;
                }
                if last_renewed_at.elapsed() < renew_interval {
                    continue;
                }

                match coordinator.renew_message_lease(message_id, &lease_token, lease_ttl) {
                    Ok(true) => {
                        last_renewed_at = std::time::Instant::now();
                    }
                    Ok(false) => {
                        if thread_stop_signal.load(Ordering::Acquire) {
                            break;
                        }

                        store_queue_lease_renewer_error(
                            &thread_error_message,
                            format!(
                                "{context} {message_id} lost its lease before work completed",
                            ),
                        );
                        break;
                    }
                    Err(error) => {
                        if thread_stop_signal.load(Ordering::Acquire) {
                            break;
                        }

                        store_queue_lease_renewer_error(
                            &thread_error_message,
                            format!(
                                "renew {context} {message_id} lease: {error}",
                            ),
                        );
                        break;
                    }
                }
            }
        });

        Self {
            stop_signal,
            error_message,
            join_handle: Some(join_handle),
        }
    }

    fn stop(&self) {
        self.stop_signal.store(true, Ordering::Release);
    }

    fn finish(mut self) -> io::Result<()> {
        self.stop();
        if let Some(join_handle) = self.join_handle.take() {
            join_handle
                .join()
                .map_err(|_| io::Error::other("queue lease renewer thread panicked"))?;
        }

        match self.error_message.lock() {
            Ok(mut error_message) => match error_message.take() {
                Some(message) => Err(io::Error::other(message)),
                None => Ok(()),
            },
            Err(_) => Err(io::Error::other(
                "queue lease renewer error state lock was poisoned",
            )),
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("runtime command failed: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let command = RuntimeCommand::from_args(&arguments);
    let config = RuntimeConfig::load()?;
    let storage = StorageLayout::from_directories(&config.directories);

    match command {
        RuntimeCommand::Bootstrap => {
            let executable = env::current_exe()?;
            let snapshot = bootstrap_runtime(
                &config,
                &storage,
                &executable,
                RuntimeRestartPolicy::from_settings(&config.supervision),
            )?;
            println!("{}", snapshot.to_json_pretty()?);
        }
        RuntimeCommand::Serve => {
            serve_runtime(&config, &storage)?;
        }
        RuntimeCommand::Supervise => {
            supervise_runtime(&config, &storage)?;
        }
        RuntimeCommand::Shutdown => {
            let report = shutdown_runtime(&config, &storage)?;
            println!("{}", report.to_json_pretty()?);
        }
        RuntimeCommand::Health => {
            let report = read_health_report(&storage.health_report_path)?;
            println!("{}", report.to_json_pretty()?);
        }
        RuntimeCommand::Contract => {
            let contract = match read_supervision_contract(&storage.supervision_contract_path) {
                Ok(contract) => contract,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    RuntimeSupervisionContract::new(
                        &config,
                        &storage,
                        &env::current_exe()?,
                        RuntimeRestartPolicy::from_settings(&config.supervision),
                    )
                }
                Err(error) => return Err(Box::new(error)),
            };
            println!("{}", contract.to_json_pretty()?);
        }
        RuntimeCommand::Status => {
            print_status(&config, &storage);
        }
        RuntimeCommand::Automation => {
            println!("{}", run_automation_command(&arguments[1..], &storage)?);
        }
        RuntimeCommand::Registrations => {
            println!("{}", run_registrations_command(&arguments[1..], &config, &storage)?);
        }
        RuntimeCommand::Manifests => {
            println!("{}", run_manifests_command(&arguments[1..], &storage)?);
        }
        RuntimeCommand::Releases => {
            println!("{}", run_releases_command(&arguments[1..], &storage)?);
        }
        RuntimeCommand::Builds => {
            println!("{}", run_builds_command(&arguments[1..], &config, &storage)?);
        }
        RuntimeCommand::Publishes => {
            println!("{}", run_publishes_command(&arguments[1..], &config, &storage)?);
        }
        RuntimeCommand::Help => print_help(),
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeCommand {
    Bootstrap,
    Serve,
    Supervise,
    Shutdown,
    Health,
    Contract,
    Status,
    Automation,
    Registrations,
    Manifests,
    Releases,
    Builds,
    Publishes,
    Help,
}

impl RuntimeCommand {
    fn from_args(arguments: &[String]) -> Self {
        match arguments.first().map(String::as_str) {
            Some("bootstrap") => Self::Bootstrap,
            Some("serve") => Self::Serve,
            Some("supervise") => Self::Supervise,
            Some("shutdown") => Self::Shutdown,
            Some("health") => Self::Health,
            Some("contract") => Self::Contract,
            Some("status") | Some("paths") => Self::Status,
            Some("automation") => Self::Automation,
            Some("registrations") => Self::Registrations,
            Some("manifests") => Self::Manifests,
            Some("releases") => Self::Releases,
            Some("builds") => Self::Builds,
            Some("publishes") => Self::Publishes,
            Some("help") | Some("--help") | Some("-h") | None => Self::Help,
            Some(_) => Self::Help,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManualReleaseDispatchCommand {
    repository_id: i64,
    git_tag: String,
    git_commit: String,
    requested_via: String,
    rebuild: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestSyncCommand {
    manifest_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SeedRevolutionsRegistrationCommand {
    project_pat_env: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistrationCheckoutCommand {
    repository_id: i64,
    git_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistrationImportRuntimeDbCommand {
    source_db_path: PathBuf,
    repository_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PublishInspectScope {
    BuildRun(i64),
    PublishRun(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishInspectCommand {
    scope: PublishInspectScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReleasePlanCommand {
    release_run_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PublishedOutputInspectionReport {
    requested_build_run_id: Option<i64>,
    requested_publish_run_id: Option<i64>,
    publish_runs: Vec<PublishedOutputDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PublishedOutputDiagnostic {
    publish_run_id: i64,
    build_run_id: i64,
    release_run_id: i64,
    publish_target_id: i64,
    artifact_id: Option<i64>,
    status: String,
    destination_ref: Option<String>,
    expected_destination_ref: Option<String>,
    publish_target_name: Option<String>,
    publish_target_kind: Option<String>,
    artifact_name: Option<String>,
    artifact_path: Option<String>,
    source_path: Option<String>,
    destination_exists: bool,
    destination_is_file: bool,
    destination_size_bytes: Option<u64>,
    destination_error: Option<String>,
    expected_destination_error: Option<String>,
    plan_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PublishedDestinationStatus {
    exists: bool,
    is_file: bool,
    size_bytes: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BuildExecutionReport {
    schema_version: u32,
    cleanup_policy: String,
    build_plan: StoredBuildExecutionPlan,
    build_run: BuildRunRecord,
    stages: Vec<BuildRunStageRecord>,
    artifacts: Vec<ArtifactRecord>,
    publish_runs: Vec<BuildExecutionPublishSnapshot>,
    attempts: Vec<BuildExecutionAttemptSnapshot>,
    cleanup: BuildExecutionCleanupSnapshot,
    interruption: Option<BuildExecutionInterruptionSnapshot>,
    retained_files: Vec<BuildExecutionRetainedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BuildExecutionPublishSnapshot {
    record: PublishRunRecord,
    execution_plan: Option<StoredPublishExecutionPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BuildExecutionAttemptSnapshot {
    workspace_path: String,
    is_final_workspace: bool,
    removed_after_cleanup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BuildExecutionCleanupSnapshot {
    status: String,
    trigger: String,
    workspace_path: String,
    workspace_bytes_before: u64,
    workspace_bytes_after: u64,
    removed_attempt_count: usize,
    error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BuildExecutionInterruptionSnapshot {
    kind: String,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BuildExecutionRetainedFile {
    role: String,
    path: String,
    source_path: Option<String>,
    content_type: String,
    content_encoding: Option<String>,
    size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AutomationPollReport {
    repositories: Vec<RepositoryPollResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RepositoryPollResult {
    repository_id: i64,
    repository_name: String,
    status: String,
    error: Option<String>,
    last_seen_tag_before: Option<String>,
    last_seen_tag_after: Option<String>,
    discovered_tags: Vec<GitTag>,
    queued_release_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct RepositoryPollSchedule {
    next_poll_at_by_repository: HashMap<i64, SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistrationSeedReport {
    registration_name: String,
    repository_id: i64,
    build_target_count: i64,
    workspace_root_override: Option<String>,
    artifacts_root_override: Option<String>,
    project_pat_env: String,
    seed_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistrationCheckoutReport {
    repository_id: i64,
    repository_name: String,
    source_mode: String,
    workspace_strategy: String,
    git_ref: String,
    git_ref_source: String,
    workspace_root_path: String,
    checkout_path: String,
    head_commit: String,
}

struct ResolvedBuildContext {
    plan: StoredBuildExecutionPlan,
    preparation: WorkspacePreparationInput,
}

struct ResolvedPublishContext {
    plan: StoredPublishExecutionPlan,
}

fn print_status(config: &RuntimeConfig, storage: &StorageLayout) {
    println!("runtime: {} {}", config.runtime_name, config.runtime_version);
    println!("platform: {}", config.platform.as_str());
    println!("log level: {}", config.log_level);
    println!("data root: {}", config.directories.data_dir.display());
    println!("state root: {}", config.directories.state_dir.display());
    println!("logs root: {}", config.directories.logs_dir.display());
    println!("artifacts root: {}", config.directories.artifacts_dir.display());
    println!("runs root: {}", config.directories.runs_dir.display());
    println!("database path: {}", storage.database_path.display());
    println!("health report: {}", storage.health_report_path.display());
    println!(
        "supervision contract: {}",
        storage.supervision_contract_path.display()
    );
    println!("supervisor state: {}", storage.supervisor_state_path.display());
    println!("runtime log: {}", storage.runtime_log_path.display());
    println!(
        "heartbeat interval: {} ms",
        config.runtime_loop.heartbeat_interval_millis
    );
    println!("max heartbeats: {:?}", config.runtime_loop.max_heartbeats);
    println!(
        "crash after heartbeats: {:?}",
        config.runtime_loop.crash_after_heartbeats
    );
    println!("crash attempts: {}", config.runtime_loop.crash_attempts);
    println!("max restarts: {}", config.supervision.max_restarts);
    println!(
        "restart backoff: {} ms",
        config.supervision.restart_backoff_millis
    );
    println!(
        "recoverable exit code: {}",
        config.supervision.recoverable_exit_code
    );
    println!(
        "max concurrent build runs: {}",
        config.concurrency.max_concurrent_build_runs
    );
    println!(
        "max concurrent publish runs: {}",
        config.concurrency.max_concurrent_publish_runs
    );
    println!(
        "max active releases per repository: {}",
        config.concurrency.max_active_releases_per_repository
    );
}

fn print_help() {
    println!("handy-unity-builder runtime scaffold");
    println!();
    println!("Commands:");
    println!("  bootstrap  create app directories and write health + supervision metadata");
    println!("  serve      run the local runtime work loop with heartbeat updates");
    println!("  supervise  run the runtime under a restart policy for recoverable exits");
    println!("  shutdown   mark the persisted runtime state as stopped");
    println!("  health     print the last persisted health report as JSON");
    println!("  contract   print the shell-to-runtime supervision contract as JSON");
    println!("  automation inspect or poll runtime automation state");
    println!("  registrations seed or materialize direct repository registrations");
    println!("  manifests  load pipeline manifests and sync them into SQLite");
    println!("  builds     manually stage or execute queued build work");
    println!("  publishes  manually execute queued publish work and inspect outputs");
    println!("  releases   manage manual release intake and local release planning");
    println!("  status     print the resolved runtime directories and store paths");
    println!("  help       print this command summary");
}

fn run_automation_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if arguments.is_empty() || is_help_request(arguments) {
        return Ok(automation_usage().to_owned());
    }

    match arguments[0].as_str() {
        "inspect" => run_automation_inspect_command(&arguments[1..], storage),
        "poll-once" => run_automation_poll_once_command(&arguments[1..], storage),
        command => Err(cli_usage_error(format!(
            "unknown automation command {command:?}\n\n{}",
            automation_usage()
        ))
        .into()),
    }
}

fn run_registrations_command(
    arguments: &[String],
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if arguments.is_empty() || is_help_request(arguments) {
        return Ok(registrations_usage().to_owned());
    }

    match arguments[0].as_str() {
        "checkout" => run_registration_checkout_command(&arguments[1..], config, storage),
        "import-runtime-db" => {
            run_registration_import_runtime_db_command(&arguments[1..], storage)
        }
        "seed-revolutions" => {
            run_seed_revolutions_registration_command(&arguments[1..], storage)
        }
        command => Err(cli_usage_error(format!(
            "unknown registrations command {command:?}\n\n{}",
            registrations_usage()
        ))
        .into()),
    }
}

fn run_manifests_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if arguments.is_empty() || is_help_request(arguments) {
        return Ok(manifests_usage().to_owned());
    }

    match arguments[0].as_str() {
        "sync" => run_manifest_sync_command(&arguments[1..], storage),
        command => Err(cli_usage_error(format!(
            "unknown manifests command {command:?}\n\n{}",
            manifests_usage()
        ))
        .into()),
    }
}

fn run_automation_inspect_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(automation_inspect_usage().to_owned());
    }
    if !arguments.is_empty() {
        return Err(cli_usage_error(format!(
            "automation inspect does not accept positional arguments\n\n{}",
            automation_inspect_usage()
        ))
        .into());
    }

    initialize_database(storage)?;
    let coordinator = LocalCoordinator::new(storage);
    let snapshot = coordinator.automation_snapshot()?;

    serde_json::to_string_pretty(&snapshot).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_automation_poll_once_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(automation_poll_once_usage().to_owned());
    }
    if !arguments.is_empty() {
        return Err(cli_usage_error(format!(
            "automation poll-once does not accept positional arguments\n\n{}",
            automation_poll_once_usage()
        ))
        .into());
    }

    initialize_database(storage)?;
    let coordinator = LocalCoordinator::new(storage);
    let report = run_repository_poll_cycle(&coordinator, None)?;

    serde_json::to_string_pretty(&report).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_builds_command(
    arguments: &[String],
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if arguments.is_empty() || is_help_request(arguments) {
        return Ok(builds_usage().to_owned());
    }

    match arguments[0].as_str() {
        "stage-next" => run_build_stage_next_command(&arguments[1..], config, storage),
        "run-next" => run_build_run_next_command(&arguments[1..], config, storage),
        command => Err(cli_usage_error(format!(
            "unknown builds command {command:?}\n\n{}",
            builds_usage()
        ))
        .into()),
    }
}

fn run_releases_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if arguments.is_empty() || is_help_request(arguments) {
        return Ok(releases_usage().to_owned());
    }

    match arguments[0].as_str() {
        "dispatch" => run_release_dispatch_command(&arguments[1..], storage),
        "plan" => run_release_plan_command(&arguments[1..], storage),
        command => Err(cli_usage_error(format!(
            "unknown releases command {command:?}\n\n{}",
            releases_usage()
        ))
        .into()),
    }
}

fn run_publishes_command(
    arguments: &[String],
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if arguments.is_empty() || is_help_request(arguments) {
        return Ok(publishes_usage().to_owned());
    }

    match arguments[0].as_str() {
        "run-next" => run_publish_run_next_command(&arguments[1..], config, storage),
        "inspect" => run_publish_inspect_command(&arguments[1..], storage),
        command => Err(cli_usage_error(format!(
            "unknown publishes command {command:?}\n\n{}",
            publishes_usage()
        ))
        .into()),
    }
}

fn run_release_dispatch_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if arguments.is_empty() || is_help_request(arguments) {
        return Ok(release_dispatch_usage().to_owned());
    }

    match arguments[0].as_str() {
        "manual" => run_manual_release_dispatch_command(&arguments[1..], storage),
        command => Err(cli_usage_error(format!(
            "unknown releases dispatch command {command:?}\n\n{}",
            release_dispatch_usage()
        ))
        .into()),
    }
}

fn run_manual_release_dispatch_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(manual_release_dispatch_usage().to_owned());
    }

    let command = parse_manual_release_dispatch_command(arguments)?;
    initialize_database(storage)?;
    let coordinator = LocalCoordinator::new(storage);
    let input = ManualReleaseDispatchInput {
        repository_id: command.repository_id,
        git_tag: command.git_tag,
        git_commit: command.git_commit,
        requested_via: command.requested_via,
    };
    let record = if command.rebuild {
        coordinator.dispatch_manual_release_rebuild(input)?
    } else {
        coordinator.dispatch_manual_release(input)?
    };

    serde_json::to_string_pretty(&record).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_release_plan_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(release_plan_usage().to_owned());
    }

    let command = parse_release_plan_command(arguments)?;
    initialize_database(storage)?;
    let coordinator = LocalCoordinator::new(storage);
    let runs = coordinator.plan_release_builds(command.release_run_id)?;

    serde_json::to_string_pretty(&runs).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_manifest_sync_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(manifest_sync_usage().to_owned());
    }

    let command = parse_manifest_sync_command(arguments)?;
    initialize_database(storage)?;
    let report = sync_manifest_directory(&storage.database_path, &command.manifest_dir)?;

    serde_json::to_string_pretty(&report).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_seed_revolutions_registration_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(registrations_seed_revolutions_usage().to_owned());
    }

    let command = parse_seed_revolutions_registration_command(arguments)?;
    initialize_database(storage)?;

    let project_pat = env::var(&command.project_pat_env).map_err(|error| {
        Box::new(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "registrations seed-revolutions requires {} to be set: {error}",
                command.project_pat_env
            ),
        )) as Box<dyn Error>
    })?;
    let project_pat = require_cli_value(&project_pat, "project pat env value")?;
    let seed_path = revolutions_managed_repository_seed_path();
    let seed_sql = std::fs::read_to_string(&seed_path)?;
    let seed_sql = seed_sql.replace(
        "__REVOLUTIONS_PROJECT_PAT__",
        &escape_sql_literal(&project_pat),
    );

    let connection = open_connection(&storage.database_path)?;
    connection.execute_batch(&seed_sql).map_err(|error| {
        Box::new(io::Error::other(format!(
            "apply Revolutions registration seed {:?}: {error}",
            seed_path.display()
        ))) as Box<dyn Error>
    })?;

    let (repository_id, workspace_root_override, artifacts_root_override): (
        i64,
        Option<String>,
        Option<String>,
    ) = connection
        .query_row(
            "
            SELECT id,
                   workspace_root_override,
                   artifacts_root_override
            FROM repositories
            WHERE name = 'Revolutions'
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|error| Box::new(io::Error::other(format!(
            "load seeded Revolutions repository: {error}"
        ))) as Box<dyn Error>)?;
    let build_target_count: i64 = connection
        .query_row(
            "SELECT COUNT(1) FROM build_targets WHERE repository_id = ? AND enabled = 1",
            [repository_id],
            |row| row.get(0),
        )
        .map_err(|error| Box::new(io::Error::other(format!(
            "count seeded Revolutions build targets: {error}"
        ))) as Box<dyn Error>)?;

    serde_json::to_string_pretty(&RegistrationSeedReport {
        registration_name: String::from("Revolutions"),
        repository_id,
        build_target_count,
        workspace_root_override,
        artifacts_root_override,
        project_pat_env: command.project_pat_env,
        seed_path: seed_path.display().to_string(),
    })
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_registration_checkout_command(
    arguments: &[String],
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(registrations_checkout_usage().to_owned());
    }

    let command = parse_registration_checkout_command(arguments)?;
    initialize_database(storage)?;

    let coordinator = LocalCoordinator::new(storage);
    let repository = coordinator.get_repository_checkout_record(command.repository_id)?;
    if repository.source_mode != "managed_repository" {
        return Err(Box::new(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "registrations checkout only supports source_mode managed_repository; repository {} uses {}",
                repository.id, repository.source_mode
            ),
        )));
    }
    if repository.workspace_strategy != "managed_checkout" {
        return Err(Box::new(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "registrations checkout only supports workspace_strategy managed_checkout; repository {} uses {}",
                repository.id, repository.workspace_strategy
            ),
        )));
    }

    let repository_url = repository.repo_url.clone().ok_or_else(|| {
        Box::new(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "repository {} is missing repo_url required for managed checkout",
                repository.id
            ),
        )) as Box<dyn Error>
    })?;
    let (git_ref, git_ref_source) =
        resolve_registration_checkout_ref(&repository, command.git_ref)?;
    let workspace_root_path = resolve_registration_checkout_workspace_root(config, &repository);
    let checkout_path = workspace_root_path.join("checkout");
    let git_auth = resolve_repository_git_auth(&coordinator, repository.credentials_id)?;

    GitWorkspaceSyncer::new().sync_ref(&GitWorkspaceSyncRefRequest {
        repository_url,
        workspace_path: checkout_path.clone(),
        git_ref: git_ref.clone(),
        auth: git_auth,
    })?;

    let head_commit = read_checked_out_head_commit(&checkout_path)?;

    serde_json::to_string_pretty(&RegistrationCheckoutReport {
        repository_id: repository.id,
        repository_name: repository.name.clone(),
        source_mode: repository.source_mode.clone(),
        workspace_strategy: repository.workspace_strategy.clone(),
        git_ref,
        git_ref_source,
        workspace_root_path: workspace_root_path.display().to_string(),
        checkout_path: checkout_path.display().to_string(),
        head_commit,
    })
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_registration_import_runtime_db_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(registrations_import_runtime_db_usage().to_owned());
    }

    let command = parse_registration_import_runtime_db_command(arguments)?;
    initialize_database(storage)?;

    let coordinator = LocalCoordinator::new(storage);
    let report = coordinator.import_repository_registration_from_database(
        &command.source_db_path,
        &command.repository_name,
    )?;

    serde_json::to_string_pretty(&report).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_build_stage_next_command(
    arguments: &[String],
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(build_stage_next_usage().to_owned());
    }
    if !arguments.is_empty() {
        return Err(cli_usage_error(format!(
            "builds stage-next does not accept positional arguments\n\n{}",
            build_stage_next_usage()
        ))
        .into());
    }

    initialize_database(storage)?;
    let coordinator = LocalCoordinator::new(storage);
    let Some(message) = coordinator.claim_next_build_job(
        BUILD_STAGER_WORKER_NAME,
        Duration::ZERO,
        BUILD_QUEUE_LEASE_TTL,
        &config.concurrency,
    )? else {
        return Ok(String::from("null"));
    };
    let lease_renewer = QueueLeaseRenewer::spawn(
        coordinator.clone(),
        message.id,
        message.lease_token.clone(),
        BUILD_QUEUE_LEASE_TTL,
        "build queue message",
    );

    let staged_result = stage_claimed_build_job(&coordinator, config, &message.payload)
        .or_else(|error| {
            coordinator
                .release_message(message.id, &message.lease_token)
                .map_err(|release_error| {
                    Box::new(io::Error::other(format!(
                        "release claimed build message {} after error {error}: {release_error}",
                        message.id
                    ))) as Box<dyn Error>
                })
                .and_then(|_| Err(Box::new(error) as Box<dyn Error>))
        });

    lease_renewer.stop();
    let staged = match staged_result {
        Ok(staged) => staged,
        Err(error) => {
            if let Err(lease_error) = lease_renewer.finish() {
                eprintln!("queue lease renewer stopped with error after build staging failure: {lease_error}");
            }
            return Err(error);
        }
    };

    let acknowledged = coordinator.acknowledge_message(message.id, &message.lease_token)?;
    let renewer_result = lease_renewer.finish();
    if !acknowledged {
        renewer_result?;
        return Err(Box::new(io::Error::other(format!(
            "build queue message {} could not be acknowledged",
            message.id
        ))));
    }
    renewer_result?;

    serde_json::to_string_pretty(&staged).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_build_run_next_command(
    arguments: &[String],
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(build_run_next_usage().to_owned());
    }
    if !arguments.is_empty() {
        return Err(cli_usage_error(format!(
            "builds run-next does not accept positional arguments\n\n{}",
            build_run_next_usage()
        ))
        .into());
    }

    initialize_database(storage)?;
    let coordinator = LocalCoordinator::new(storage);
    let Some(message) = coordinator.claim_next_build_job(
        BUILD_STAGER_WORKER_NAME,
        Duration::ZERO,
        BUILD_QUEUE_LEASE_TTL,
        &config.concurrency,
    )? else {
        return Ok(String::from("null"));
    };
    let lease_renewer = QueueLeaseRenewer::spawn(
        coordinator.clone(),
        message.id,
        message.lease_token.clone(),
        BUILD_QUEUE_LEASE_TTL,
        "build queue message",
    );
    let record_result = (|| -> Result<BuildRunRecord, Box<dyn Error>> {
        let resolved = match resolve_claimed_build_context(&coordinator, &message.payload) {
            Ok(resolved) => resolved,
            Err(error) => {
                release_claimed_build_message(
                    &coordinator,
                    message.id,
                    &message.lease_token,
                    &error,
                )?;
                return Err(Box::new(error));
            }
        };

        let planned = match WorkspacePreparer::new(&config.directories).plan(&resolved.preparation) {
            Ok(planned) => planned,
            Err(error) => {
                release_claimed_build_message(
                    &coordinator,
                    message.id,
                    &message.lease_token,
                    &error,
                )?;
                return Err(Box::new(error));
            }
        };

        let stage_sequence = Rc::new(RefCell::new(BuildRunStageSequence::default()));
        let validation_tracker = BuildRunStageTracker::new(
            &coordinator,
            resolved.plan.build_run_id,
            planned.root_path.clone(),
            planned.artifact_root_path.clone(),
            stage_sequence.clone(),
        )?;
        let validation_log_path =
            validation_tracker.stage_log_path(BuildProcessStage::ValidateContext);
        let mut attempt_roots = vec![planned.root_path.clone()];

        coordinator.start_build_run(
            resolved.plan.build_run_id,
            StartBuildRunInput {
                workspace_path: planned.root_path.display().to_string(),
                log_path: validation_log_path.display().to_string(),
                artifact_root_path: planned.artifact_root_path.display().to_string(),
            },
        )?;
        validation_tracker.start_stage(
            BuildProcessStage::ValidateContext,
            &format!(
                "Validating build context for repository '{}' tag '{}' target '{}' ({}) using Unity {}.",
                resolved.plan.repository_name,
                resolved.plan.git_tag,
                resolved.plan.target_name,
                resolved.plan.platform,
                resolved.plan.unity_version,
            ),
        )?;

        let runner_plan = match resolve_runtime_build_execution_plan(config, &resolved.plan) {
            Ok(plan) => {
                validation_tracker.complete_stage(
                    BuildProcessStage::ValidateContext,
                    &format!(
                        "Resolved host-native runner '{}' and build method '{}'.",
                        plan.runner_type,
                        plan.build_method,
                    ),
                )?;
                plan
            }
            Err(error) => {
                validation_tracker.fail_stage(
                    BuildProcessStage::ValidateContext,
                    &error.to_string(),
                )?;
                let record = coordinator.fail_build_run(
                    resolved.plan.build_run_id,
                    FailBuildRunInput {
                        workspace_path: planned.root_path.display().to_string(),
                        log_path: validation_log_path.display().to_string(),
                        artifact_root_path: planned.artifact_root_path.display().to_string(),
                        error_message: error.to_string(),
                    },
                )?;
                run_build_cleanup(&coordinator, record.id, &attempt_roots);
                return Ok(record);
            }
        };

        let processor =
            ExecutionProcessor::new(&config.directories, HostNativeUnityExecutor::new());
        process_build_run_with_retry(
            &coordinator,
            &config.directories,
            &processor,
            &runner_plan,
            &resolved.preparation,
            resolved.plan.build_run_id,
            stage_sequence,
            &mut attempt_roots,
        )
        .map_err(|error| Box::new(error) as Box<dyn Error>)
    })();

    lease_renewer.stop();
    let record = match record_result {
        Ok(record) => record,
        Err(error) => {
            if let Err(lease_error) = lease_renewer.finish() {
                eprintln!("queue lease renewer stopped with error after build run failure: {lease_error}");
            }
            return Err(error);
        }
    };

    let acknowledged = coordinator.acknowledge_message(message.id, &message.lease_token)?;
    let renewer_result = lease_renewer.finish();
    if !acknowledged {
        renewer_result?;
        return Err(Box::new(io::Error::other(format!(
            "build queue message {} could not be acknowledged",
            message.id
        ))));
    }
    renewer_result?;

    serde_json::to_string_pretty(&record).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_publish_run_next_command(
    arguments: &[String],
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(publish_run_next_usage().to_owned());
    }
    if !arguments.is_empty() {
        return Err(cli_usage_error(format!(
            "publishes run-next does not accept positional arguments\n\n{}",
            publish_run_next_usage()
        ))
        .into());
    }

    initialize_database(storage)?;
    let coordinator = LocalCoordinator::new(storage);
    let Some(message) = coordinator.claim_next_publish_job(
        PUBLISH_WORKER_NAME,
        Duration::ZERO,
        PUBLISH_QUEUE_LEASE_TTL,
        &config.concurrency,
    )? else {
        return Ok(String::from("null"));
    };
    let lease_renewer = QueueLeaseRenewer::spawn(
        coordinator.clone(),
        message.id,
        message.lease_token.clone(),
        PUBLISH_QUEUE_LEASE_TTL,
        "publish queue message",
    );
    let record_result = (|| -> Result<PublishRunRecord, Box<dyn Error>> {
        let resolved = match resolve_claimed_publish_context(&coordinator, &message.payload) {
            Ok(resolved) => resolved,
            Err(error) => {
                release_claimed_publish_message(
                    &coordinator,
                    message.id,
                    &message.lease_token,
                    &error,
                )?;
                return Err(Box::new(error));
            }
        };
        let publish_plan = match publish_execution_plan(&resolved.plan) {
            Ok(plan) => plan,
            Err(error) => {
                release_claimed_publish_message(
                    &coordinator,
                    message.id,
                    &message.lease_token,
                    &error,
                )?;
                return Err(Box::new(error));
            }
        };

        coordinator.start_publish_run(
            resolved.plan.publish_run_id,
            StartPublishRunInput::default(),
        )?;

        let processor = PublishExecutionProcessor::new();
        let record = match processor.process(&publish_plan) {
            Ok(result) => coordinator.complete_publish_run(
                resolved.plan.publish_run_id,
                CompletePublishRunInput {
                    destination_ref: result.destination_ref,
                },
            )?,
            Err(error) => coordinator.fail_publish_run(
                resolved.plan.publish_run_id,
                FailPublishRunInput {
                    destination_ref: String::new(),
                    error_message: error.to_string(),
                },
            )?,
        };
        synchronize_build_execution_report_from_publish(&coordinator, &record);
        Ok(record)
    })();

    lease_renewer.stop();
    let record = match record_result {
        Ok(record) => record,
        Err(error) => {
            if let Err(lease_error) = lease_renewer.finish() {
                eprintln!("queue lease renewer stopped with error after publish failure: {lease_error}");
            }
            return Err(error);
        }
    };

    let acknowledged = coordinator.acknowledge_message(message.id, &message.lease_token)?;
    let renewer_result = lease_renewer.finish();
    if !acknowledged {
        renewer_result?;
        return Err(Box::new(io::Error::other(format!(
            "publish queue message {} could not be acknowledged",
            message.id
        ))));
    }
    renewer_result?;

    serde_json::to_string_pretty(&record).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_publish_inspect_command(
    arguments: &[String],
    storage: &StorageLayout,
) -> Result<String, Box<dyn Error>> {
    if is_help_request(arguments) {
        return Ok(publish_inspect_usage().to_owned());
    }

    let command = parse_publish_inspect_command(arguments)?;
    initialize_database(storage)?;
    let coordinator = LocalCoordinator::new(storage);
    let report = inspect_published_outputs(&coordinator, &command)?;

    serde_json::to_string_pretty(&report).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn stage_claimed_build_job(
    coordinator: &LocalCoordinator,
    config: &RuntimeConfig,
    payload: &[u8],
) -> io::Result<BuildRunRecord> {
    let resolved = resolve_claimed_build_context(coordinator, payload)?;
    let preparer = WorkspacePreparer::new(&config.directories);
    let planned = preparer.plan(&resolved.preparation)?;
    let started = coordinator.start_build_run(
        resolved.plan.build_run_id,
        StartBuildRunInput {
            workspace_path: planned.root_path.display().to_string(),
            log_path: planned.log_path.display().to_string(),
            artifact_root_path: planned.artifact_root_path.display().to_string(),
        },
    )?;

    match preparer.prepare(&resolved.preparation) {
        Ok(_) => Ok(started),
        Err(error) => coordinator.fail_build_run(
            resolved.plan.build_run_id,
            FailBuildRunInput {
                workspace_path: planned.root_path.display().to_string(),
                log_path: planned.log_path.display().to_string(),
                artifact_root_path: planned.artifact_root_path.display().to_string(),
                error_message: error.to_string(),
            },
        ),
    }
}

fn complete_successful_build_run(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    runner_plan: &ExecutionPlan,
    result: &ExecutionResult,
    tracker: &BuildRunStageTracker<'_>,
) -> io::Result<BuildRunRecord> {
    if output_requires_runtime_archive(runner_plan) {
        tracker.start_stage(
            BuildProcessStage::PackageArtifact,
            "Packaging Unity output into a runtime-owned zip archive.",
        )?;
        if let Err(error) = package_build_output(runner_plan, result) {
            tracker.fail_stage(BuildProcessStage::PackageArtifact, &error.to_string())?;
            return Err(error);
        }
        tracker.complete_stage(
            BuildProcessStage::PackageArtifact,
            "Runtime archive packaging completed.",
        )?;
    }

    tracker.start_stage(
        BuildProcessStage::RegisterArtifacts,
        "Discovering artifacts, registering them, and dispatching publish work.",
    )?;
    register_artifacts_and_dispatch_publish_runs(
        coordinator,
        build_run_id,
        &result.artifact_root_path,
    )
    .map_err(|error| {
        let _ = tracker.fail_stage(BuildProcessStage::RegisterArtifacts, &error.to_string());
        error
    })?;
    tracker.complete_stage(
        BuildProcessStage::RegisterArtifacts,
        "Artifacts registered and downstream publish work dispatched.",
    )?;

    coordinator.complete_build_run(
        build_run_id,
        CompleteBuildRunInput {
            workspace_path: result.workspace_path.display().to_string(),
            log_path: result.log_path.display().to_string(),
            artifact_root_path: result.artifact_root_path.display().to_string(),
        },
    )
}

fn output_requires_runtime_archive(plan: &ExecutionPlan) -> bool {
    plan.output_kind
        .as_deref()
        .is_some_and(|output_kind| output_kind.eq_ignore_ascii_case("archive"))
}

fn package_build_output(plan: &ExecutionPlan, result: &ExecutionResult) -> io::Result<()> {
    let source_directory = &result.output_path;
    if !source_directory.is_dir() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "expected Unity archive source directory at {:?}",
                source_directory.display()
            ),
        ));
    }

    let artifact_path = resolve_final_artifact_output_path(plan, &result.artifact_root_path)?;
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if artifact_path.exists() {
        let metadata = fs::metadata(&artifact_path)?;
        if metadata.is_dir() {
            fs::remove_dir_all(&artifact_path)?;
        } else {
            fs::remove_file(&artifact_path)?;
        }
    }

    let file = fs::File::create(&artifact_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    add_build_output_directory_to_zip(
        &mut zip,
        source_directory,
        source_directory,
        options,
        plan,
    )?;
    zip.finish().map_err(io::Error::other)?;

    Ok(())
}

fn add_build_output_directory_to_zip<W>(
    zip: &mut ZipWriter<W>,
    root: &Path,
    current: &Path,
    options: SimpleFileOptions,
    plan: &ExecutionPlan,
) -> io::Result<()>
where
    W: io::Write + io::Seek,
{
    add_build_output_directory_to_zip_with_prefix(
        zip,
        root,
        current,
        options,
        plan,
        None,
    )
}

fn add_build_output_directory_to_zip_with_prefix<W>(
    zip: &mut ZipWriter<W>,
    root: &Path,
    current: &Path,
    options: SimpleFileOptions,
    plan: &ExecutionPlan,
    archive_prefix: Option<&str>,
) -> io::Result<()>
where
    W: io::Write + io::Seek,
{
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let relative_path = path.strip_prefix(root).map_err(io::Error::other)?;
        if should_exclude_build_output_archive_path(plan, relative_path) {
            continue;
        }

        let relative = relative_path.to_string_lossy().replace('\\', "/");
        let archive_relative = match archive_prefix {
            Some(prefix) if !relative.is_empty() => format!("{prefix}/{relative}"),
            Some(prefix) => prefix.to_owned(),
            None => relative,
        };
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !archive_relative.is_empty() {
                zip.add_directory(format!("{archive_relative}/"), options)
                    .map_err(io::Error::other)?;
            }
            add_build_output_directory_to_zip_with_prefix(
                zip,
                root,
                &path,
                options,
                plan,
                archive_prefix,
            )?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        zip.start_file(archive_relative, options)
            .map_err(io::Error::other)?;
        let mut source = fs::File::open(&path)?;
        io::copy(&mut source, zip)?;
    }

    Ok(())
}

fn should_exclude_build_output_archive_path(plan: &ExecutionPlan, relative_path: &Path) -> bool {
    if archive_path_has_non_shippable_segment(relative_path) {
        return true;
    }

    archive_path_is_optional_debug_symbol(plan, relative_path)
}

fn archive_path_has_non_shippable_segment(relative_path: &Path) -> bool {
    relative_path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|segment| {
            has_any_suffix_case_insensitive(
                segment,
                UNITY_NON_SHIPPABLE_ARCHIVE_PATH_SUFFIXES,
            )
        })
}

fn archive_path_is_optional_debug_symbol(
    plan: &ExecutionPlan,
    relative_path: &Path,
) -> bool {
    let Some(file_name) = relative_path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    match plan.platform.trim().to_ascii_lowercase().as_str() {
        "macos" => has_any_suffix_case_insensitive(
            file_name,
            UNITY_MACOS_OPTIONAL_ARCHIVE_PATH_SUFFIXES,
        ),
        "windows" => has_any_suffix_case_insensitive(
            file_name,
            UNITY_WINDOWS_OPTIONAL_ARCHIVE_FILE_SUFFIXES,
        ),
        "webgl" => has_any_suffix_case_insensitive(
            file_name,
            UNITY_WEBGL_OPTIONAL_ARCHIVE_FILE_SUFFIXES,
        ),
        _ => false,
    }
}

fn has_any_suffix_case_insensitive(value: &str, suffixes: &[&str]) -> bool {
    let normalized = value.to_ascii_lowercase();
    suffixes.iter().any(|suffix| {
        normalized.ends_with(&suffix.to_ascii_lowercase())
    })
}

fn add_directory_to_zip_with_prefix<W>(
    zip: &mut ZipWriter<W>,
    root: &Path,
    current: &Path,
    options: SimpleFileOptions,
    archive_prefix: Option<&str>,
) -> io::Result<()>
where
    W: io::Write + io::Seek,
{
    let mut entries = fs::read_dir(current)?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .replace('\\', "/");
        let archive_relative = match archive_prefix {
            Some(prefix) if !relative.is_empty() => format!("{prefix}/{relative}"),
            Some(prefix) => prefix.to_owned(),
            None => relative,
        };
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !archive_relative.is_empty() {
                zip.add_directory(format!("{archive_relative}/"), options)
                    .map_err(io::Error::other)?;
            }
            add_directory_to_zip_with_prefix(zip, root, &path, options, archive_prefix)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        zip.start_file(archive_relative, options)
            .map_err(io::Error::other)?;
        let mut source = fs::File::open(&path)?;
        io::copy(&mut source, zip)?;
    }

    Ok(())
}

fn build_execution_retained_dir(workspace_path: &Path) -> PathBuf {
    workspace_path.join(BUILD_EXECUTION_RETAINED_DIR_NAME)
}

fn build_execution_report_path(workspace_path: &Path) -> PathBuf {
    build_execution_retained_dir(workspace_path).join(BUILD_EXECUTION_REPORT_FILE_NAME)
}

fn build_execution_logs_archive_path(workspace_path: &Path) -> PathBuf {
    build_execution_retained_dir(workspace_path).join(BUILD_EXECUTION_LOG_ARCHIVE_FILE_NAME)
}

fn push_attempt_root(attempt_roots: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !attempt_roots.iter().any(|existing| existing == &candidate) {
        attempt_roots.push(candidate);
    }
}

fn append_cleanup_error(current: &mut Option<String>, error: impl Into<String>) {
    let error = error.into();
    match current {
        Some(existing) => {
            existing.push_str("; ");
            existing.push_str(&error);
        }
        None => *current = Some(error),
    }
}

fn directory_size_bytes(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }

    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        total = total.saturating_add(directory_size_bytes(&entry.path())?);
    }

    Ok(total)
}

fn total_workspace_size_bytes(paths: &[PathBuf]) -> io::Result<u64> {
    let mut total = 0_u64;
    for path in paths {
        total = total.saturating_add(directory_size_bytes(path)?);
    }

    Ok(total)
}

fn archive_build_run_logs(
    attempt_roots: &[PathBuf],
    final_workspace_path: &Path,
) -> io::Result<Option<BuildExecutionRetainedFile>> {
    let log_roots = attempt_roots
        .iter()
        .filter_map(|attempt_root| {
            let logs_dir = attempt_root.join("logs");
            logs_dir.is_dir().then_some((attempt_root, logs_dir))
        })
        .collect::<Vec<_>>();
    if log_roots.is_empty() {
        return Ok(None);
    }

    let retained_dir = build_execution_retained_dir(final_workspace_path);
    fs::create_dir_all(&retained_dir)?;

    let archive_path = build_execution_logs_archive_path(final_workspace_path);
    if archive_path.exists() {
        fs::remove_file(&archive_path)?;
    }

    let file = fs::File::create(&archive_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    for (attempt_root, logs_dir) in log_roots {
        let prefix = attempt_root
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("attempt");
        add_directory_to_zip_with_prefix(
            &mut zip,
            attempt_root,
            &logs_dir,
            options,
            Some(prefix),
        )?;
    }

    zip.finish().map_err(io::Error::other)?;
    let size_bytes = fs::metadata(&archive_path).ok().map(|metadata| metadata.len());

    Ok(Some(BuildExecutionRetainedFile {
        role: String::from("logs-archive"),
        path: archive_path.display().to_string(),
        source_path: None,
        content_type: String::from("application/zip"),
        content_encoding: None,
        size_bytes,
    }))
}

fn prune_build_run_workspaces(
    attempt_roots: &[PathBuf],
    final_workspace_path: &Path,
) -> io::Result<usize> {
    let mut removed_attempt_count = 0_usize;

    for attempt_root in attempt_roots {
        if attempt_root == final_workspace_path {
            if !attempt_root.is_dir() {
                continue;
            }

            for entry in fs::read_dir(attempt_root)? {
                let entry = entry?;
                if entry.file_name() == BUILD_EXECUTION_RETAINED_DIR_NAME {
                    continue;
                }

                let path = entry.path();
                if path.is_dir() {
                    fs::remove_dir_all(path)?;
                } else if path.exists() {
                    fs::remove_file(path)?;
                }
            }

            continue;
        }

        if !attempt_root.exists() {
            continue;
        }

        if attempt_root.is_dir() {
            fs::remove_dir_all(attempt_root)?;
        } else {
            fs::remove_file(attempt_root)?;
        }
        removed_attempt_count += 1;
    }

    Ok(removed_attempt_count)
}

fn collect_build_execution_retained_files(
    workspace_path: &Path,
) -> io::Result<Vec<BuildExecutionRetainedFile>> {
    let archive_path = build_execution_logs_archive_path(workspace_path);
    if !archive_path.is_file() {
        return Ok(Vec::new());
    }

    Ok(vec![BuildExecutionRetainedFile {
        role: String::from("logs-archive"),
        path: archive_path.display().to_string(),
        source_path: None,
        content_type: String::from("application/zip"),
        content_encoding: None,
        size_bytes: Some(fs::metadata(&archive_path)?.len()),
    }])
}

fn load_build_execution_report(report_path: &Path) -> io::Result<Option<BuildExecutionReport>> {
    match fs::read(report_path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .map_err(io::Error::other),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn collect_build_execution_publish_snapshots(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
) -> io::Result<Vec<BuildExecutionPublishSnapshot>> {
    let publish_runs = coordinator.list_publish_runs_by_build_run(build_run_id)?;
    let mut snapshots = Vec::with_capacity(publish_runs.len());

    for record in publish_runs {
        let execution_plan = coordinator.get_publish_execution_plan(record.id).ok();
        snapshots.push(BuildExecutionPublishSnapshot {
            record,
            execution_plan,
        });
    }

    Ok(snapshots)
}

fn default_build_execution_cleanup_snapshot(workspace_path: &Path) -> BuildExecutionCleanupSnapshot {
    BuildExecutionCleanupSnapshot {
        status: String::from(BUILD_EXECUTION_CLEANUP_PENDING),
        trigger: String::from(BUILD_EXECUTION_CLEANUP_TRIGGER_TERMINAL_STATE),
        workspace_path: workspace_path.display().to_string(),
        workspace_bytes_before: 0,
        workspace_bytes_after: 0,
        removed_attempt_count: 0,
        error_message: None,
    }
}

fn synchronize_build_execution_report(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    attempts_override: Option<Vec<BuildExecutionAttemptSnapshot>>,
    cleanup_override: Option<BuildExecutionCleanupSnapshot>,
) -> io::Result<()> {
    let build_run = coordinator.get_build_run_record(build_run_id)?;
    let Some(workspace_path) = build_run
        .workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return Ok(());
    };

    synchronize_build_execution_report_for_workspace(
        coordinator,
        build_run_id,
        &workspace_path,
        attempts_override,
        cleanup_override,
        None,
    )
}

fn synchronize_build_execution_report_for_workspace(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    workspace_path: &Path,
    attempts_override: Option<Vec<BuildExecutionAttemptSnapshot>>,
    cleanup_override: Option<BuildExecutionCleanupSnapshot>,
    interruption_override: Option<BuildExecutionInterruptionSnapshot>,
) -> io::Result<()> {
    let report_path = build_execution_report_path(workspace_path);
    let existing = load_build_execution_report(&report_path)?;
    let build_run = coordinator.get_build_run_record(build_run_id)?;
    let build_plan = coordinator.get_build_execution_plan(build_run_id)?;
    let stages = coordinator.list_build_run_stages(build_run_id)?;
    let artifacts = coordinator.list_artifacts_by_build_run(build_run_id)?;
    let publish_runs = collect_build_execution_publish_snapshots(coordinator, build_run_id)?;
    let retained_files = collect_build_execution_retained_files(workspace_path)?;

    let attempts = attempts_override
        .or_else(|| existing.as_ref().map(|report| report.attempts.clone()))
        .unwrap_or_default();
    let cleanup = cleanup_override
        .or_else(|| existing.as_ref().map(|report| report.cleanup.clone()))
        .unwrap_or_else(|| default_build_execution_cleanup_snapshot(workspace_path));
    let interruption = interruption_override
        .or_else(|| existing.as_ref().and_then(|report| report.interruption.clone()));

    let report = BuildExecutionReport {
        schema_version: BUILD_EXECUTION_REPORT_SCHEMA_VERSION,
        cleanup_policy: String::from(BUILD_EXECUTION_CLEANUP_POLICY),
        build_plan,
        build_run,
        stages,
        artifacts,
        publish_runs,
        attempts,
        cleanup,
        interruption,
        retained_files,
    };

    fs::create_dir_all(build_execution_retained_dir(workspace_path))?;
    fs::write(
        report_path,
        serde_json::to_vec_pretty(&report).map_err(io::Error::other)?,
    )?;

    Ok(())
}

fn run_build_cleanup(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    attempt_roots: &[PathBuf],
) {
    let build_run = match coordinator.get_build_run_record(build_run_id) {
        Ok(build_run) => build_run,
        Err(error) => {
            eprintln!(
                "runtime cleanup could not reload build run {}: {}",
                build_run_id, error
            );
            return;
        }
    };
    let Some(final_workspace_path) = build_run
        .workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return;
    };

    let mut all_attempt_roots = Vec::new();
    for attempt_root in attempt_roots {
        push_attempt_root(&mut all_attempt_roots, attempt_root.clone());
    }
    push_attempt_root(&mut all_attempt_roots, final_workspace_path.clone());

    let workspace_bytes_before = match total_workspace_size_bytes(&all_attempt_roots) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "runtime cleanup could not size build run {} before pruning: {}",
                build_run_id, error
            );
            0
        }
    };

    let mut cleanup_error = None;
    if let Err(error) = archive_build_run_logs(&all_attempt_roots, &final_workspace_path) {
        append_cleanup_error(&mut cleanup_error, error.to_string());
    }
    let removed_attempt_count = match prune_build_run_workspaces(&all_attempt_roots, &final_workspace_path) {
        Ok(count) => count,
        Err(error) => {
            append_cleanup_error(&mut cleanup_error, error.to_string());
            0
        }
    };

    let workspace_bytes_after = match total_workspace_size_bytes(&all_attempt_roots) {
        Ok(bytes) => bytes,
        Err(error) => {
            append_cleanup_error(&mut cleanup_error, error.to_string());
            0
        }
    };

    let attempts = all_attempt_roots
        .iter()
        .map(|attempt_root| BuildExecutionAttemptSnapshot {
            workspace_path: attempt_root.display().to_string(),
            is_final_workspace: attempt_root == &final_workspace_path,
            removed_after_cleanup: attempt_root != &final_workspace_path && !attempt_root.exists(),
        })
        .collect::<Vec<_>>();
    let cleanup = BuildExecutionCleanupSnapshot {
        status: String::from(if cleanup_error.is_some() {
            BUILD_EXECUTION_CLEANUP_FAILED
        } else {
            BUILD_EXECUTION_CLEANUP_COMPLETED
        }),
        trigger: String::from(BUILD_EXECUTION_CLEANUP_TRIGGER_TERMINAL_STATE),
        workspace_path: final_workspace_path.display().to_string(),
        workspace_bytes_before,
        workspace_bytes_after,
        removed_attempt_count,
        error_message: cleanup_error,
    };

    if let Err(error) = synchronize_build_execution_report(
        coordinator,
        build_run_id,
        Some(attempts),
        Some(cleanup),
    ) {
        eprintln!(
            "runtime cleanup could not persist build execution report for {}: {}",
            build_run_id, error
        );
    }
}

fn recover_interrupted_build_attempts(
    coordinator: &LocalCoordinator,
    recovery_report: &RuntimeRecoveryReport,
) {
    for interrupted_build in &recovery_report.interrupted_builds {
        run_interrupted_build_cleanup(coordinator, interrupted_build);
    }
}

fn run_interrupted_build_cleanup(
    coordinator: &LocalCoordinator,
    interrupted_build: &InterruptedBuildRecoveryRecord,
) {
    let workspace_path = PathBuf::from(interrupted_build.workspace_path.trim());
    let attempt_roots = discover_build_run_attempt_roots(
        interrupted_build.build_run_id,
        &workspace_path,
    )
    .unwrap_or_else(|error| {
        eprintln!(
            "runtime recovery could not enumerate interrupted build attempts for {}: {}",
            interrupted_build.build_run_id, error
        );
        vec![workspace_path.clone()]
    });
    let workspace_bytes_before = match total_workspace_size_bytes(&attempt_roots) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "runtime recovery could not size interrupted build run {} before pruning: {}",
                interrupted_build.build_run_id, error
            );
            0
        }
    };

    let mut cleanup_error = None;
    if let Err(error) = archive_build_run_logs(&attempt_roots, &workspace_path) {
        append_cleanup_error(&mut cleanup_error, error.to_string());
    }
    let removed_attempt_count =
        match prune_build_run_workspaces(&attempt_roots, &workspace_path) {
            Ok(count) => count,
            Err(error) => {
                append_cleanup_error(&mut cleanup_error, error.to_string());
                0
            }
        };
    let workspace_bytes_after = match total_workspace_size_bytes(&attempt_roots) {
        Ok(bytes) => bytes,
        Err(error) => {
            append_cleanup_error(&mut cleanup_error, error.to_string());
            0
        }
    };

    let attempts = attempt_roots
        .iter()
        .map(|attempt_root| BuildExecutionAttemptSnapshot {
            workspace_path: attempt_root.display().to_string(),
            is_final_workspace: attempt_root == &workspace_path,
            removed_after_cleanup: attempt_root != &workspace_path && !attempt_root.exists(),
        })
        .collect::<Vec<_>>();
    let cleanup = BuildExecutionCleanupSnapshot {
        status: String::from(if cleanup_error.is_some() {
            BUILD_EXECUTION_CLEANUP_FAILED
        } else {
            BUILD_EXECUTION_CLEANUP_COMPLETED
        }),
        trigger: String::from(cleanup_trigger_for_interruption_kind(
            &interrupted_build.interruption_kind,
        )),
        workspace_path: workspace_path.display().to_string(),
        workspace_bytes_before,
        workspace_bytes_after,
        removed_attempt_count,
        error_message: cleanup_error,
    };
    let interruption = BuildExecutionInterruptionSnapshot {
        kind: interrupted_build.interruption_kind.clone(),
        message: interrupted_build.interruption_message.clone(),
    };

    if let Err(error) = synchronize_build_execution_report_for_workspace(
        coordinator,
        interrupted_build.build_run_id,
        &workspace_path,
        Some(attempts),
        Some(cleanup),
        Some(interruption),
    ) {
        eprintln!(
            "runtime recovery could not persist interrupted build execution report for {}: {}",
            interrupted_build.build_run_id, error
        );
    }
}

fn cleanup_trigger_for_interruption_kind(interruption_kind: &str) -> &'static str {
    match interruption_kind {
        RECOVERY_INTERRUPTION_KIND_REQUESTED => {
            BUILD_EXECUTION_CLEANUP_TRIGGER_REQUESTED_INTERRUPTION
        }
        RECOVERY_INTERRUPTION_KIND_SYSTEM => BUILD_EXECUTION_CLEANUP_TRIGGER_SYSTEM_INTERRUPTION,
        _ => BUILD_EXECUTION_CLEANUP_TRIGGER_SYSTEM_INTERRUPTION,
    }
}

fn discover_build_run_attempt_roots(
    build_run_id: i64,
    workspace_path: &Path,
) -> io::Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    let prefix = format!("build-run-{build_run_id}-attempt-");

    if let Some(parent) = workspace_path.parent() {
        if parent.is_dir() {
            let mut entries = fs::read_dir(parent)?
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if name.starts_with(&prefix) {
                    push_attempt_root(&mut roots, path);
                }
            }
        }
    }

    push_attempt_root(&mut roots, workspace_path.to_path_buf());
    Ok(roots)
}

fn synchronize_build_execution_report_from_publish(
    coordinator: &LocalCoordinator,
    publish_run: &PublishRunRecord,
) {
    if let Err(error) = synchronize_build_execution_report(
        coordinator,
        publish_run.build_run_id,
        None,
        None,
    ) {
        eprintln!(
            "runtime cleanup could not refresh build execution report for publish run {}: {}",
            publish_run.id, error
        );
    }
}

fn process_build_run_with_retry(
    coordinator: &LocalCoordinator,
    directories: &runtime_config::RuntimeDirectories,
    processor: &ExecutionProcessor<HostNativeUnityExecutor>,
    runner_plan: &ExecutionPlan,
    preparation: &WorkspacePreparationInput,
    build_run_id: i64,
    stage_sequence: Rc<RefCell<BuildRunStageSequence>>,
    attempt_roots: &mut Vec<PathBuf>,
) -> io::Result<BuildRunRecord> {
    let mut current_preparation = preparation.clone();
    let mut retry_available = true;

    loop {
        let planned = WorkspacePreparer::new(directories).plan(&current_preparation)?;
        push_attempt_root(attempt_roots, planned.root_path.clone());
        let tracker = BuildRunStageTracker::new(
            coordinator,
            build_run_id,
            planned.root_path.clone(),
            planned.artifact_root_path.clone(),
            stage_sequence.clone(),
        )?;
        let checkout_log_path = tracker.stage_log_path(BuildProcessStage::CheckoutRepository);

        tracker.start_stage(
            BuildProcessStage::CheckoutRepository,
            &format!(
                "Checking out repository '{}' at tag '{}' into '{}'.",
                current_preparation.repository_url,
                current_preparation.git_tag,
                planned.source_path.display(),
            ),
        )?;

        let workspace = match processor.prepare_workspace(&current_preparation) {
            Ok(workspace) => {
                tracker.complete_stage(
                    BuildProcessStage::CheckoutRepository,
                    &format!(
                        "Repository checkout completed at '{}'.",
                        workspace.source_path.display(),
                    ),
                )?;
                workspace
            }
            Err(error) => {
                tracker.fail_stage(BuildProcessStage::CheckoutRepository, &error.to_string())?;
                let record = coordinator.fail_build_run(
                    build_run_id,
                    FailBuildRunInput {
                        workspace_path: planned.root_path.display().to_string(),
                        log_path: checkout_log_path.display().to_string(),
                        artifact_root_path: planned.artifact_root_path.display().to_string(),
                        error_message: error.to_string(),
                    },
                )?;
                run_build_cleanup(coordinator, record.id, attempt_roots);
                return Ok(record);
            }
        };

        let unity_log_path = tracker.stage_log_path(BuildProcessStage::UnityBuild);
        let mut workspace = workspace;
        workspace.log_path = unity_log_path.clone();

        tracker.start_stage(
            BuildProcessStage::UnityBuild,
            &format!(
                "Launching Unity build method '{}' for target '{}'.",
                runner_plan.build_method,
                runner_plan.platform,
            ),
        )?;

        let mut reporter = BuildStageHeartbeatReporter::new(&tracker, BuildProcessStage::UnityBuild);
        let execute_outcome = processor.execute_prepared(runner_plan, workspace, &mut reporter);
        if let Some(error) = reporter.take_error() {
            tracker.fail_stage(BuildProcessStage::UnityBuild, &error.to_string())?;
            let record = coordinator.fail_build_run(
                build_run_id,
                FailBuildRunInput {
                    workspace_path: planned.root_path.display().to_string(),
                    log_path: unity_log_path.display().to_string(),
                    artifact_root_path: planned.artifact_root_path.display().to_string(),
                    error_message: error.to_string(),
                },
            )?;
            run_build_cleanup(coordinator, record.id, attempt_roots);
            return Ok(record);
        }

        match execute_outcome {
            Ok(ExecutionProcessOutcome { result, error }) => match error {
                Some(error)
                    if retry_available
                        && should_retry_in_fresh_workspace(&result.log_path)? =>
                {
                    tracker.fail_stage(
                        BuildProcessStage::UnityBuild,
                        &format!(
                            "{} Retrying once with a fresh workspace.",
                            error,
                        ),
                    )?;
                    retry_available = false;
                    current_preparation.attempt_token = next_workspace_attempt_token()?;
                    continue;
                }
                Some(error) => {
                    tracker.fail_stage(BuildProcessStage::UnityBuild, &error.to_string())?;
                    let record = persist_host_native_failure(
                        coordinator,
                        build_run_id,
                        &result,
                        &error,
                    )?;
                    run_build_cleanup(coordinator, record.id, attempt_roots);
                    return Ok(record);
                }
                None => {
                    tracker.complete_stage(
                        BuildProcessStage::UnityBuild,
                        &format!(
                            "Unity build completed with raw output at '{}'.",
                            result.output_path.display(),
                        ),
                    )?;
                    let record = complete_successful_build_run(
                        coordinator,
                        build_run_id,
                        runner_plan,
                        &result,
                        &tracker,
                    )
                    .or_else(|error| {
                        coordinator.fail_build_run(
                            build_run_id,
                            FailBuildRunInput {
                                workspace_path: result.workspace_path.display().to_string(),
                                log_path: result.log_path.display().to_string(),
                                artifact_root_path: result.artifact_root_path.display().to_string(),
                                error_message: error.to_string(),
                            },
                        )
                    })?;
                    run_build_cleanup(coordinator, record.id, attempt_roots);
                    return Ok(record);
                }
            },
            Err(error)
                if retry_available && should_retry_in_fresh_workspace(&unity_log_path)? =>
            {
                tracker.fail_stage(
                    BuildProcessStage::UnityBuild,
                    &format!(
                        "{} Retrying once with a fresh workspace.",
                        error,
                    ),
                )?;
                retry_available = false;
                current_preparation.attempt_token = next_workspace_attempt_token()?;
                continue;
            }
            Err(error) => {
                tracker.fail_stage(BuildProcessStage::UnityBuild, &error.to_string())?;
                let record = coordinator.fail_build_run(
                    build_run_id,
                    FailBuildRunInput {
                        workspace_path: planned.root_path.display().to_string(),
                        log_path: unity_log_path.display().to_string(),
                        artifact_root_path: planned.artifact_root_path.display().to_string(),
                        error_message: error.to_string(),
                    },
                )?;
                run_build_cleanup(coordinator, record.id, attempt_roots);
                return Ok(record);
            }
        }
    }
}

fn should_retry_in_fresh_workspace(log_path: &Path) -> io::Result<bool> {
    let contents = match fs::read_to_string(log_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let normalized = contents.to_ascii_lowercase();

    Ok(normalized.contains("packagecache")
        && normalized.contains("rename")
        && (normalized.contains("eperm")
            || normalized.contains("operation not permitted")))
}

fn register_artifacts_and_dispatch_publish_runs(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    artifact_root_path: &Path,
) -> io::Result<()> {
    let artifacts = discover_artifacts(artifact_root_path)?;
    let inputs = artifacts
        .into_iter()
        .map(|artifact| CreateArtifactRecordInput {
            name: artifact.name,
            kind: artifact.kind,
            path: artifact.path,
            size_bytes: artifact.size_bytes,
            checksum_sha256: artifact.checksum_sha256,
        })
        .collect::<Vec<_>>();
    coordinator.replace_build_artifacts(build_run_id, inputs)?;

    for run in coordinator.plan_build_publish_runs(build_run_id)? {
        if run.status != PublishStatus::Queued.as_str() {
            continue;
        }

        match coordinator.dispatch_publish_run(run.id)? {
            QueueDispatchOutcome::Enqueued | QueueDispatchOutcome::AlreadyClaimed => {}
            QueueDispatchOutcome::InProgress => {
                return Err(io::Error::new(
                    ErrorKind::WouldBlock,
                    format!("publish run {} dispatch is already in progress", run.id),
                ));
            }
        }
    }

    Ok(())
}

fn resolve_claimed_build_context(
    coordinator: &LocalCoordinator,
    payload: &[u8],
) -> io::Result<ResolvedBuildContext> {
    let job: BuildDispatchJob = serde_json::from_slice(payload)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    let plan = coordinator.get_build_execution_plan(job.build_run_id)?;
    let git_auth = match plan.repository_credentials_id {
        Some(credentials_id) => {
            let credentials = coordinator.get_credential_record(credentials_id)?;
            git_auth_options_from_credentials(&credentials.kind, &credentials.config_json)?
        }
        None => GitAuthOptions::default(),
    };

    Ok(ResolvedBuildContext {
        preparation: WorkspacePreparationInput {
            build_run_id: plan.build_run_id,
            attempt_token: next_workspace_attempt_token()?,
            repository_name: plan.repository_name.clone(),
            repository_url: plan.repository_url.clone(),
            git_auth,
            git_tag: plan.git_tag.clone(),
            workspace_root_override: plan.workspace_root_override.clone(),
            artifacts_root_override: plan.artifacts_root_override.clone(),
        },
        plan,
    })
}

fn resolve_claimed_publish_context(
    coordinator: &LocalCoordinator,
    payload: &[u8],
) -> io::Result<ResolvedPublishContext> {
    let job: PublishDispatchJob = serde_json::from_slice(payload)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    let plan = coordinator.get_publish_execution_plan(job.publish_run_id)?;

    Ok(ResolvedPublishContext { plan })
}

fn runner_execution_plan(plan: &StoredBuildExecutionPlan) -> io::Result<ExecutionPlan> {
    let build_method = require_cli_value(
        plan.build_method.as_deref().unwrap_or_default(),
        "build method",
    )?;
    if plan.timeout_seconds <= 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "build run {} has invalid timeout_seconds {}",
                plan.build_run_id, plan.timeout_seconds
            ),
        ));
    }

    Ok(ExecutionPlan {
        build_run_id: plan.build_run_id,
        release_run_id: plan.release_run_id,
        build_target_id: plan.build_target_id,
        repository_name: plan.repository_name.clone(),
        repository_url: plan.repository_url.clone(),
        git_tag: plan.git_tag.clone(),
        target_name: plan.target_name.clone(),
        platform: plan.platform.clone(),
        runner_type: plan.runner_type.clone(),
        build_method,
        output_kind: plan.output_kind.clone(),
        output_path_template: plan.output_path_template.clone(),
        unity_version: plan.unity_version.clone(),
        config_json: plan.config_json.clone(),
        timeout_seconds: plan.timeout_seconds,
    })
}

fn resolve_runtime_build_execution_plan(
    config: &RuntimeConfig,
    plan: &StoredBuildExecutionPlan,
) -> io::Result<ExecutionPlan> {
    let capability_profile = inspect_host_capability_profile(config.platform);
    resolve_runtime_build_execution_plan_with_profile(plan, &capability_profile)
}

fn resolve_runtime_build_execution_plan_with_profile(
    plan: &StoredBuildExecutionPlan,
    capability_profile: &HostCapabilityProfile,
) -> io::Result<ExecutionPlan> {
    let runner_plan = runner_execution_plan(plan)?;
    if runner_plan.runner_type.trim() != RunnerFamily::HostNative.label() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "build run {} uses unsupported runner_type {:?} for builds run-next",
                runner_plan.build_run_id, runner_plan.runner_type
            ),
        ));
    }

    resolve_host_native_execution_plan(&runner_plan, capability_profile)
}

fn publish_execution_plan(plan: &StoredPublishExecutionPlan) -> io::Result<PublishExecutionPlan> {
    Ok(PublishExecutionPlan {
        publish_run_id: plan.publish_run_id,
        release_run_id: plan.release_run_id,
        repository_id: plan.repository_id,
        repository_name: plan.repository_name.clone(),
        git_tag: plan.git_tag.clone(),
        build_run_id: plan.build_run_id,
        publish_target_id: plan.publish_target_id,
        publish_target_name: plan.publish_target_name.clone(),
        publish_target_kind: require_cli_value(&plan.publish_target_kind, "publish target kind")?,
        publish_target_config_json: plan.publish_target_config_json.clone(),
        artifact_id: plan.artifact_id,
        artifact_name: plan.artifact_name.clone(),
        artifact_kind: plan.artifact_kind.clone(),
        artifact_path: plan.artifact_path.clone(),
        artifact_root_path: plan.artifact_root_path.clone(),
        source_path: plan.source_path.clone(),
        status: plan.status.clone(),
    })
}

fn inspect_published_outputs(
    coordinator: &LocalCoordinator,
    command: &PublishInspectCommand,
) -> io::Result<PublishedOutputInspectionReport> {
    let (requested_build_run_id, requested_publish_run_id, records) = match command.scope {
        PublishInspectScope::BuildRun(build_run_id) => (
            Some(build_run_id),
            None,
            coordinator.list_publish_runs_by_build_run(build_run_id)?,
        ),
        PublishInspectScope::PublishRun(publish_run_id) => (
            None,
            Some(publish_run_id),
            vec![coordinator.get_publish_run_record(publish_run_id)?],
        ),
    };

    let publish_runs = records
        .iter()
        .map(|record| inspect_publish_run(coordinator, record))
        .collect();

    Ok(PublishedOutputInspectionReport {
        requested_build_run_id,
        requested_publish_run_id,
        publish_runs,
    })
}

fn inspect_publish_run(
    coordinator: &LocalCoordinator,
    record: &runtime_store::PublishRunRecord,
) -> PublishedOutputDiagnostic {
    let destination_status = inspect_persisted_destination(record.destination_ref.as_deref());
    let mut diagnostic = PublishedOutputDiagnostic {
        publish_run_id: record.id,
        build_run_id: record.build_run_id,
        release_run_id: record.release_run_id,
        publish_target_id: record.publish_target_id,
        artifact_id: record.artifact_id,
        status: record.status.clone(),
        destination_ref: record.destination_ref.clone(),
        expected_destination_ref: None,
        publish_target_name: None,
        publish_target_kind: None,
        artifact_name: None,
        artifact_path: None,
        source_path: None,
        destination_exists: destination_status.exists,
        destination_is_file: destination_status.is_file,
        destination_size_bytes: destination_status.size_bytes,
        destination_error: destination_status.error,
        expected_destination_error: None,
        plan_error: None,
    };

    let stored_plan = match coordinator.get_publish_execution_plan(record.id) {
        Ok(plan) => plan,
        Err(error) => {
            diagnostic.plan_error = Some(error.to_string());
            return diagnostic;
        }
    };
    let publish_plan = match publish_execution_plan(&stored_plan) {
        Ok(plan) => plan,
        Err(error) => {
            diagnostic.plan_error = Some(error.to_string());
            return diagnostic;
        }
    };

    diagnostic.publish_target_name = Some(publish_plan.publish_target_name.clone());
    diagnostic.publish_target_kind = Some(publish_plan.publish_target_kind.clone());
    diagnostic.artifact_name = Some(publish_plan.artifact_name.clone());
    diagnostic.artifact_path = Some(publish_plan.artifact_path.clone());
    diagnostic.source_path = Some(publish_plan.source_path.clone());

    match resolve_publish_destination_path(&publish_plan) {
        Ok(path) => {
            diagnostic.expected_destination_ref = Some(path.display().to_string());
        }
        Err(error) => {
            diagnostic.expected_destination_error = Some(error.to_string());
        }
    }

    diagnostic
}

fn inspect_persisted_destination(destination_ref: Option<&str>) -> PublishedDestinationStatus {
    let Some(destination_ref) = destination_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return PublishedDestinationStatus::default();
    };

    match std::fs::metadata(destination_ref) {
        Ok(metadata) => {
            let is_file = metadata.is_file();
            let error = if is_file {
                None
            } else {
                Some(format!(
                    "destination path {:?} is not a regular file",
                    destination_ref
                ))
            };

            PublishedDestinationStatus {
                exists: true,
                is_file,
                size_bytes: metadata.is_file().then_some(metadata.len()),
                error,
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => PublishedDestinationStatus {
            error: Some(format!(
                "destination path {:?} was not found",
                destination_ref
            )),
            ..PublishedDestinationStatus::default()
        },
        Err(error) => PublishedDestinationStatus {
            error: Some(format!(
                "stat destination path {:?}: {}",
                destination_ref, error
            )),
            ..PublishedDestinationStatus::default()
        },
    }
}

fn persist_host_native_failure(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    result: &runtime_runner::ExecutionResult,
    error: &io::Error,
) -> io::Result<BuildRunRecord> {
    if error.kind() == ErrorKind::TimedOut {
        return coordinator.cancel_build_run(
            build_run_id,
            CancelBuildRunInput {
                workspace_path: result.workspace_path.display().to_string(),
                log_path: result.log_path.display().to_string(),
                artifact_root_path: result.artifact_root_path.display().to_string(),
                error_message: error.to_string(),
            },
        );
    }

    coordinator.fail_build_run(
        build_run_id,
        FailBuildRunInput {
            workspace_path: result.workspace_path.display().to_string(),
            log_path: result.log_path.display().to_string(),
            artifact_root_path: result.artifact_root_path.display().to_string(),
            error_message: error.to_string(),
        },
    )
}

fn release_claimed_build_message(
    coordinator: &LocalCoordinator,
    message_id: i64,
    lease_token: &str,
    error: &io::Error,
) -> Result<(), Box<dyn Error>> {
    coordinator
        .release_message(message_id, lease_token)
        .map_err(|release_error| {
            Box::new(io::Error::other(format!(
                "release claimed build message {message_id} after error {error}: {release_error}"
            ))) as Box<dyn Error>
        })?;

    Ok(())
}

fn release_claimed_publish_message(
    coordinator: &LocalCoordinator,
    message_id: i64,
    lease_token: &str,
    error: &io::Error,
) -> Result<(), Box<dyn Error>> {
    coordinator
        .release_message(message_id, lease_token)
        .map_err(|release_error| {
            Box::new(io::Error::other(format!(
                "release claimed publish message {message_id} after error {error}: {release_error}"
            ))) as Box<dyn Error>
        })?;

    Ok(())
}

fn parse_manual_release_dispatch_command(
    arguments: &[String],
) -> io::Result<ManualReleaseDispatchCommand> {
    let mut repository_id = None;
    let mut git_tag = None;
    let mut git_commit = String::new();
    let mut requested_via = String::from("runtime-bin");
    let mut rebuild = false;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--repository-id" => {
                let value = read_flag_value(arguments, index, "--repository-id")?;
                repository_id = Some(parse_positive_i64_flag(&value, "repository-id")?);
                index += 2;
            }
            "--git-tag" => {
                let value = read_flag_value(arguments, index, "--git-tag")?;
                git_tag = Some(require_cli_value(&value, "git-tag")?);
                index += 2;
            }
            "--git-commit" => {
                git_commit = read_flag_value(arguments, index, "--git-commit")?;
                index += 2;
            }
            "--requested-via" => {
                requested_via = require_cli_value(
                    &read_flag_value(arguments, index, "--requested-via")?,
                    "requested-via",
                )?;
                index += 2;
            }
            "--rebuild" => {
                rebuild = true;
                index += 1;
            }
            flag => {
                return Err(cli_usage_error(format!(
                    "unknown releases dispatch manual flag {flag:?}\n\n{}",
                    manual_release_dispatch_usage()
                )));
            }
        }
    }

    Ok(ManualReleaseDispatchCommand {
        repository_id: repository_id.ok_or_else(|| {
            cli_usage_error(format!(
                "missing required --repository-id\n\n{}",
                manual_release_dispatch_usage()
            ))
        })?,
        git_tag: git_tag.ok_or_else(|| {
            cli_usage_error(format!(
                "missing required --git-tag\n\n{}",
                manual_release_dispatch_usage()
            ))
        })?,
        git_commit,
        requested_via,
        rebuild,
    })
}

fn parse_release_plan_command(arguments: &[String]) -> io::Result<ReleasePlanCommand> {
    let mut release_run_id = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--release-run-id" => {
                let value = read_flag_value(arguments, index, "--release-run-id")?;
                release_run_id = Some(parse_positive_i64_flag(&value, "release-run-id")?);
                index += 2;
            }
            flag => {
                return Err(cli_usage_error(format!(
                    "unknown releases plan flag {flag:?}\n\n{}",
                    release_plan_usage()
                )));
            }
        }
    }

    Ok(ReleasePlanCommand {
        release_run_id: release_run_id.ok_or_else(|| {
            cli_usage_error(format!(
                "missing required --release-run-id\n\n{}",
                release_plan_usage()
            ))
        })?,
    })
}

fn parse_manifest_sync_command(arguments: &[String]) -> io::Result<ManifestSyncCommand> {
    let mut manifest_dir = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--dir" => {
                let value = read_flag_value(arguments, index, "--dir")?;
                manifest_dir = Some(PathBuf::from(require_cli_value(&value, "dir")?));
                index += 2;
            }
            flag => {
                return Err(cli_usage_error(format!(
                    "unknown manifests sync flag {flag:?}\n\n{}",
                    manifest_sync_usage()
                )));
            }
        }
    }

    Ok(ManifestSyncCommand {
        manifest_dir: manifest_dir.unwrap_or_else(default_manifest_directory),
    })
}

fn parse_seed_revolutions_registration_command(
    arguments: &[String],
) -> io::Result<SeedRevolutionsRegistrationCommand> {
    let mut project_pat_env = String::from(DEFAULT_REVOLUTIONS_PROJECT_PAT_ENV);
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--project-pat-env" => {
                project_pat_env = require_cli_value(
                    &read_flag_value(arguments, index, arguments[index].as_str())?,
                    "project-pat-env",
                )?;
                index += 2;
            }
            flag => {
                return Err(cli_usage_error(format!(
                    "unknown registrations seed-revolutions flag {flag:?}\n\n{}",
                    registrations_seed_revolutions_usage()
                )));
            }
        }
    }

    Ok(SeedRevolutionsRegistrationCommand { project_pat_env })
}

fn parse_registration_checkout_command(
    arguments: &[String],
) -> io::Result<RegistrationCheckoutCommand> {
    let mut repository_id = None;
    let mut git_ref = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--repository-id" => {
                let value = read_flag_value(arguments, index, "--repository-id")?;
                repository_id = Some(parse_positive_i64_flag(&value, "repository-id")?);
                index += 2;
            }
            "--ref" => {
                let value = read_flag_value(arguments, index, "--ref")?;
                git_ref = Some(require_cli_value(&value, "ref")?);
                index += 2;
            }
            flag => {
                return Err(cli_usage_error(format!(
                    "unknown registrations checkout flag {flag:?}\n\n{}",
                    registrations_checkout_usage()
                )));
            }
        }
    }

    Ok(RegistrationCheckoutCommand {
        repository_id: repository_id.ok_or_else(|| {
            cli_usage_error(format!(
                "missing required --repository-id\n\n{}",
                registrations_checkout_usage()
            ))
        })?,
        git_ref,
    })
}

fn parse_registration_import_runtime_db_command(
    arguments: &[String],
) -> io::Result<RegistrationImportRuntimeDbCommand> {
    let mut source_db_path = None;
    let mut repository_name = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--source-db" => {
                let value = read_flag_value(arguments, index, "--source-db")?;
                source_db_path = Some(PathBuf::from(require_cli_value(&value, "source-db")?));
                index += 2;
            }
            "--repository-name" => {
                let value = read_flag_value(arguments, index, "--repository-name")?;
                repository_name = Some(require_cli_value(&value, "repository-name")?);
                index += 2;
            }
            flag => {
                return Err(cli_usage_error(format!(
                    "unknown registrations import-runtime-db flag {flag:?}\n\n{}",
                    registrations_import_runtime_db_usage()
                )));
            }
        }
    }

    Ok(RegistrationImportRuntimeDbCommand {
        source_db_path: source_db_path.ok_or_else(|| {
            cli_usage_error(format!(
                "missing required --source-db\n\n{}",
                registrations_import_runtime_db_usage()
            ))
        })?,
        repository_name: repository_name.ok_or_else(|| {
            cli_usage_error(format!(
                "missing required --repository-name\n\n{}",
                registrations_import_runtime_db_usage()
            ))
        })?,
    })
}

fn parse_publish_inspect_command(arguments: &[String]) -> io::Result<PublishInspectCommand> {
    let mut build_run_id = None;
    let mut publish_run_id = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--build-run-id" => {
                let value = read_flag_value(arguments, index, "--build-run-id")?;
                build_run_id = Some(parse_positive_i64_flag(&value, "build-run-id")?);
                index += 2;
            }
            "--publish-run-id" => {
                let value = read_flag_value(arguments, index, "--publish-run-id")?;
                publish_run_id = Some(parse_positive_i64_flag(&value, "publish-run-id")?);
                index += 2;
            }
            flag => {
                return Err(cli_usage_error(format!(
                    "unknown publishes inspect flag {flag:?}\n\n{}",
                    publish_inspect_usage()
                )));
            }
        }
    }

    match (build_run_id, publish_run_id) {
        (Some(build_run_id), None) => Ok(PublishInspectCommand {
            scope: PublishInspectScope::BuildRun(build_run_id),
        }),
        (None, Some(publish_run_id)) => Ok(PublishInspectCommand {
            scope: PublishInspectScope::PublishRun(publish_run_id),
        }),
        (None, None) => Err(cli_usage_error(format!(
            "missing required --build-run-id or --publish-run-id\n\n{}",
            publish_inspect_usage()
        ))),
        (Some(_), Some(_)) => Err(cli_usage_error(format!(
            "publishes inspect accepts exactly one of --build-run-id or --publish-run-id\n\n{}",
            publish_inspect_usage()
        ))),
    }
}

fn default_manifest_directory() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("pipelines")
}

fn revolutions_managed_repository_seed_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("revolutions-managed-repository.sql")
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn read_flag_value(arguments: &[String], index: usize, flag: &str) -> io::Result<String> {
    arguments
        .get(index + 1)
        .cloned()
        .ok_or_else(|| cli_usage_error(format!("missing value for {flag}")))
}

fn parse_positive_i64_flag(value: &str, label: &str) -> io::Result<i64> {
    let parsed = value.trim().parse::<i64>().map_err(|error| {
        cli_usage_error(format!("{label} must be a positive integer: {error}"))
    })?;
    if parsed <= 0 {
        return Err(cli_usage_error(format!(
            "{label} must be greater than zero"
        )));
    }

    Ok(parsed)
}

fn require_cli_value(value: &str, label: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(cli_usage_error(format!("{label} must not be empty")));
    }

    Ok(trimmed.to_owned())
}

fn is_help_request(arguments: &[String]) -> bool {
    matches!(arguments.first().map(String::as_str), Some("help") | Some("--help") | Some("-h"))
}

fn cli_usage_error(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message.into())
}

fn releases_usage() -> &'static str {
    "handy-unity-builder runtime releases commands\n\nUsage:\n  releases dispatch manual --repository-id <id> --git-tag <tag> [--git-commit <sha>] [--requested-via <source>] [--rebuild]\n  releases plan --release-run-id <id>\n"
}

fn automation_usage() -> &'static str {
    "handy-unity-builder runtime automation commands\n\nUsage:\n  automation inspect\n  automation poll-once\n"
}

fn registrations_usage() -> &'static str {
    "handy-unity-builder runtime registrations commands\n\nUsage:\n  registrations checkout --repository-id <id> [--ref <git-ref>]\n  registrations import-runtime-db --source-db <path> --repository-name <name>\n  registrations seed-revolutions [--project-pat-env <env>]\n"
}

fn registrations_checkout_usage() -> &'static str {
    "handy-unity-builder runtime registrations checkout\n\nUsage:\n  registrations checkout --repository-id <id> [--ref <git-ref>]\n\nDefaults:\n  --ref defaults to the repository default_branch stored in SQLite\n"
}

fn registrations_import_runtime_db_usage() -> &'static str {
    "handy-unity-builder runtime registrations import-runtime-db\n\nUsage:\n  registrations import-runtime-db --source-db <path> --repository-name <name>\n\nBehavior:\n  imports repository configuration from another runtime.db into the current app database without copying release, build, or publish runs\n"
}

fn manifests_usage() -> &'static str {
    "handy-unity-builder runtime manifests commands\n\nUsage:\n  manifests sync [--dir <path>]\n"
}

fn builds_usage() -> &'static str {
    "handy-unity-builder runtime builds commands\n\nUsage:\n  builds stage-next\n  builds run-next\n"
}

fn publishes_usage() -> &'static str {
    "handy-unity-builder runtime publishes commands\n\nUsage:\n  publishes run-next\n  publishes inspect (--build-run-id <id> | --publish-run-id <id>)\n"
}

fn release_dispatch_usage() -> &'static str {
    "handy-unity-builder runtime release dispatch commands\n\nUsage:\n  releases dispatch manual --repository-id <id> --git-tag <tag> [--git-commit <sha>] [--requested-via <source>] [--rebuild]\n"
}

fn manual_release_dispatch_usage() -> &'static str {
    "handy-unity-builder runtime releases dispatch manual\n\nUsage:\n  releases dispatch manual --repository-id <id> --git-tag <tag> [--git-commit <sha>] [--requested-via <source>] [--rebuild]\n"
}

fn release_plan_usage() -> &'static str {
    "handy-unity-builder runtime releases plan\n\nUsage:\n  releases plan --release-run-id <id>\n"
}

fn automation_inspect_usage() -> &'static str {
    "handy-unity-builder runtime automation inspect\n\nUsage:\n  automation inspect\n"
}

fn automation_poll_once_usage() -> &'static str {
    "handy-unity-builder runtime automation poll-once\n\nUsage:\n  automation poll-once\n"
}

fn registrations_seed_revolutions_usage() -> &'static str {
    "handy-unity-builder runtime registrations seed-revolutions\n\nUsage:\n  registrations seed-revolutions [--project-pat-env <env>]\n\nDefaults:\n  --project-pat-env defaults to REVOLUTIONS_PROJECT_PAT\n"
}

fn manifest_sync_usage() -> &'static str {
    "handy-unity-builder runtime manifests sync\n\nUsage:\n  manifests sync [--dir <path>]\n\nDefaults:\n  --dir defaults to ./pipelines relative to the current working directory\n"
}

fn build_stage_next_usage() -> &'static str {
    "handy-unity-builder runtime builds stage-next\n\nUsage:\n  builds stage-next\n"
}

fn build_run_next_usage() -> &'static str {
    "handy-unity-builder runtime builds run-next\n\nUsage:\n  builds run-next\n"
}

fn publish_run_next_usage() -> &'static str {
    "handy-unity-builder runtime publishes run-next\n\nUsage:\n  publishes run-next\n"
}

fn publish_inspect_usage() -> &'static str {
    "handy-unity-builder runtime publishes inspect\n\nUsage:\n  publishes inspect --build-run-id <id>\n  publishes inspect --publish-run-id <id>\n"
}

fn run_release_planner_cycle(storage: &StorageLayout) -> Result<bool, Box<dyn Error>> {
    let coordinator = LocalCoordinator::new(storage);

    coordinator
        .process_next_release_job(
            RELEASE_PLANNER_WORKER_NAME,
            Duration::ZERO,
            RELEASE_QUEUE_LEASE_TTL,
        )
        .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_build_worker_cycle(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<bool, Box<dyn Error>> {
    Ok(run_build_run_next_command(&[], config, storage)? != "null")
}

fn run_publish_worker_cycle(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<bool, Box<dyn Error>> {
    Ok(run_publish_run_next_command(&[], config, storage)? != "null")
}

impl RepositoryPollSchedule {
    fn is_due(&self, repository_id: i64, now: SystemTime) -> bool {
        self.next_poll_at_by_repository
            .get(&repository_id)
            .is_none_or(|next_poll_at| *next_poll_at <= now)
    }

    fn set_next_poll_at(&mut self, repository_id: i64, now: SystemTime, interval: Duration) {
        self.next_poll_at_by_repository
            .insert(repository_id, now + interval);
    }

    fn delete_repository(&mut self, repository_id: i64) {
        self.next_poll_at_by_repository.remove(&repository_id);
    }

    fn retain_repositories(&mut self, repositories: &HashSet<i64>) {
        self.next_poll_at_by_repository
            .retain(|repository_id, _| repositories.contains(repository_id));
    }
}

fn run_repository_poll_cycle(
    coordinator: &LocalCoordinator,
    mut poll_schedule: Option<&mut RepositoryPollSchedule>,
) -> io::Result<AutomationPollReport> {
    let repositories = coordinator.list_polling_repositories()?;
    let tag_lister = GitTagLister::default();
    let now = SystemTime::now();
    let mut seen_repositories = HashSet::with_capacity(repositories.len());
    let mut results = Vec::new();

    for repository in repositories {
        seen_repositories.insert(repository.id);

        if !repository.enabled {
            if let Some(schedule) = poll_schedule.as_deref_mut() {
                schedule.delete_repository(repository.id);
            }
            results.push(skipped_poll_result(
                &repository,
                POLL_STATUS_SKIPPED_DISABLED,
            ));
            continue;
        }

        if repository.enabled_build_target_count == 0 {
            if let Some(schedule) = poll_schedule.as_deref_mut() {
                schedule.delete_repository(repository.id);
            }
            results.push(skipped_poll_result(
                &repository,
                POLL_STATUS_SKIPPED_NO_ENABLED_BUILD_TARGETS,
            ));
            continue;
        }

        match coordinator.advance_repository_release_queue(repository.id) {
            Ok(true) => {
                results.push(skipped_poll_result(
                    &repository,
                    POLL_STATUS_SKIPPED_ACTIVE_RELEASE_BACKLOG,
                ));
                continue;
            }
            Ok(false) => {}
            Err(error) => {
                results.push(error_poll_result(&repository, error));
                continue;
            }
        }

        if let Some(schedule) = poll_schedule.as_deref_mut() {
            if !schedule.is_due(repository.id, now) {
                continue;
            }
            schedule.set_next_poll_at(
                repository.id,
                now,
                Duration::from_secs(repository.polling_interval_seconds as u64),
            );
        }

        match poll_repository(coordinator, &tag_lister, &repository) {
            Ok(result) => {
                if !result.queued_release_ids.is_empty() {
                    if let Err(error) = coordinator.advance_repository_release_queue(repository.id)
                    {
                        results.push(error_poll_result(&repository, error));
                        continue;
                    }
                }
                results.push(result);
            }
            Err(error) => results.push(error_poll_result(&repository, error)),
        }
    }

    if let Some(schedule) = poll_schedule {
        schedule.retain_repositories(&seen_repositories);
    }

    Ok(AutomationPollReport { repositories: results })
}

fn poll_repository(
    coordinator: &LocalCoordinator,
    tag_lister: &GitTagLister,
    repository: &PollingRepositoryRecord,
) -> io::Result<RepositoryPollResult> {
    let git_auth = resolve_repository_git_auth(coordinator, repository.credentials_id)?;
    let tags = tag_lister.list_tags(&GitTagListRequest {
        repository_url: repository.repo_url.clone(),
        auth: git_auth,
    })?;
    let (selected_tags, status, ok) = select_queued_repository_tags(
        &tags,
        repository.last_seen_tag.as_deref(),
    );
    if !ok {
        return Ok(RepositoryPollResult {
            repository_id: repository.id,
            repository_name: repository.name.clone(),
            status: status.to_owned(),
            error: None,
            last_seen_tag_before: repository.last_seen_tag.clone(),
            last_seen_tag_after: repository.last_seen_tag.clone(),
            discovered_tags: Vec::new(),
            queued_release_ids: Vec::new(),
        });
    }

    let mut queued_release_ids = Vec::new();
    let mut discovered_tags = Vec::new();
    let mut last_seen_tag_after = repository.last_seen_tag.clone();

    for tag in selected_tags {
        match coordinator.dispatch_repository_poll_release(RepositoryPollDispatchInput {
            repository_id: repository.id,
            git_tag: tag.name.clone(),
            git_commit: tag.commit.clone(),
            observed_via: POLL_OBSERVED_VIA.to_owned(),
        }) {
            Ok(release) => {
                coordinator.update_repository_last_seen_tag(repository.id, &tag.name)?;
                last_seen_tag_after = Some(tag.name.clone());
                discovered_tags.push(tag);
                queued_release_ids.push(release.id);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                coordinator.update_repository_last_seen_tag(repository.id, &tag.name)?;
                last_seen_tag_after = Some(tag.name.clone());
                discovered_tags.push(tag);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                let status = if queued_release_ids.is_empty() {
                    POLL_STATUS_BUILD_IN_PROGRESS
                } else {
                    POLL_STATUS_QUEUED
                };
                return Ok(RepositoryPollResult {
                    repository_id: repository.id,
                    repository_name: repository.name.clone(),
                    status: status.to_owned(),
                    error: None,
                    last_seen_tag_before: repository.last_seen_tag.clone(),
                    last_seen_tag_after,
                    discovered_tags,
                    queued_release_ids,
                });
            }
            Err(error) => return Err(error),
        }
    }

    let status = if !queued_release_ids.is_empty() {
        POLL_STATUS_QUEUED
    } else {
        POLL_STATUS_ALREADY_SEEN
    };

    Ok(RepositoryPollResult {
        repository_id: repository.id,
        repository_name: repository.name.clone(),
        status: status.to_owned(),
        error: None,
        last_seen_tag_before: repository.last_seen_tag.clone(),
        last_seen_tag_after,
        discovered_tags,
        queued_release_ids,
    })
}

fn resolve_repository_git_auth(
    coordinator: &LocalCoordinator,
    credentials_id: Option<i64>,
) -> io::Result<GitAuthOptions> {
    let Some(credentials_id) = credentials_id else {
        return Ok(GitAuthOptions::default());
    };

    let credentials = coordinator.get_credential_record(credentials_id)?;
    git_auth_options_from_credentials(&credentials.kind, &credentials.config_json)
}

fn resolve_registration_checkout_ref(
    repository: &RepositoryCheckoutRecord,
    explicit_git_ref: Option<String>,
) -> io::Result<(String, String)> {
    if let Some(git_ref) = explicit_git_ref {
        return Ok((git_ref, String::from("explicit")));
    }

    let default_branch = repository.default_branch.clone().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "repository {} is missing default_branch; pass --ref to registrations checkout",
                repository.id
            ),
        )
    })?;

    Ok((default_branch, String::from("default_branch")))
}

fn resolve_registration_checkout_workspace_root(
    config: &RuntimeConfig,
    repository: &RepositoryCheckoutRecord,
) -> PathBuf {
    repository
        .workspace_root_override
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            config
                .directories
            .data_dir
            .join("repositories")
                .join(format!("repository-{}", repository.id))
        })
}

fn read_checked_out_head_commit(source_path: &Path) -> io::Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(source_path)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "read checked out HEAD from {:?}: exit code {:?}; stderr: {}",
            source_path,
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim(),
        )));
    }

    let head_commit = String::from_utf8_lossy(&output.stdout);
    let trimmed = head_commit.trim();
    if trimmed.is_empty() {
        return Err(io::Error::other(format!(
            "read checked out HEAD from {:?}: git returned an empty commit id",
            source_path,
        )));
    }

    Ok(trimmed.to_owned())
}

fn select_queued_repository_tags(
    tags: &[GitTag],
    last_seen_tag: Option<&str>,
) -> (Vec<GitTag>, &'static str, bool) {
    if tags.is_empty() {
        return (Vec::new(), POLL_STATUS_NO_TAGS, false);
    }

    let normalized_last_seen = last_seen_tag.unwrap_or_default().trim();
    if normalized_last_seen.is_empty() {
        return (tags.to_vec(), "", true);
    }

    for (index, tag) in tags.iter().enumerate() {
        if tag.name != normalized_last_seen {
            continue;
        }

        if index == tags.len() - 1 {
            return (Vec::new(), POLL_STATUS_UNCHANGED, false);
        }

        return (tags[index + 1..].to_vec(), "", true);
    }

    if tags.last().is_some_and(|tag| tag.name == normalized_last_seen) {
        return (Vec::new(), POLL_STATUS_UNCHANGED, false);
    }

    (vec![tags[tags.len() - 1].clone()], "", true)
}

fn skipped_poll_result(
    repository: &PollingRepositoryRecord,
    status: &str,
) -> RepositoryPollResult {
    RepositoryPollResult {
        repository_id: repository.id,
        repository_name: repository.name.clone(),
        status: status.to_owned(),
        error: None,
        last_seen_tag_before: repository.last_seen_tag.clone(),
        last_seen_tag_after: repository.last_seen_tag.clone(),
        discovered_tags: Vec::new(),
        queued_release_ids: Vec::new(),
    }
}

fn error_poll_result(
    repository: &PollingRepositoryRecord,
    error: io::Error,
) -> RepositoryPollResult {
    RepositoryPollResult {
        repository_id: repository.id,
        repository_name: repository.name.clone(),
        status: POLL_STATUS_ERROR.to_owned(),
        error: Some(error.to_string()),
        last_seen_tag_before: repository.last_seen_tag.clone(),
        last_seen_tag_after: repository.last_seen_tag.clone(),
        discovered_tags: Vec::new(),
        queued_release_ids: Vec::new(),
    }
}

fn serve_runtime(config: &RuntimeConfig, storage: &StorageLayout) -> Result<(), Box<dyn Error>> {
    let executable = env::current_exe()?;
    let attempt = current_supervision_attempt();
    let snapshot = bootstrap_runtime(
        config,
        storage,
        &executable,
        RuntimeRestartPolicy::from_settings(&config.supervision),
    )?;
    let coordinator = LocalCoordinator::new(storage);
    recover_interrupted_build_attempts(&coordinator, &snapshot.recovery_report);
    let mut report = snapshot.health_report;
    let mut heartbeat_count = 0_u32;
    let mut poll_schedule = RepositoryPollSchedule::default();

    loop {
        if runtime_stop_requested(storage)? {
            return Ok(());
        }

        let coordinator = LocalCoordinator::new(storage);
        let _ = run_repository_poll_cycle(&coordinator, Some(&mut poll_schedule))?;
        while run_release_planner_cycle(storage)? {}
        while run_build_worker_cycle(config, storage)? {}
        while run_publish_worker_cycle(config, storage)? {}

        if let Some(max_heartbeats) = config.runtime_loop.max_heartbeats {
            if heartbeat_count >= max_heartbeats {
                let stopped = shutdown_runtime(config, storage)?;
                println!("{}", stopped.to_json_pretty()?);
                return Ok(());
            }
        }

        thread::sleep(Duration::from_millis(
            config.runtime_loop.heartbeat_interval_millis,
        ));
        if runtime_stop_requested(storage)? {
            return Ok(());
        }

        heartbeat_count += 1;
        report = update_runtime_health(
            storage,
            &report,
            RuntimeStatus::Healthy,
            RUNTIME_HEARTBEAT_EVENT,
            format!(
                "heartbeat {} on supervision attempt {}",
                heartbeat_count, attempt
            ),
        )?;

        if should_force_recoverable_crash(config, attempt, heartbeat_count) {
            let _ = update_runtime_health(
                storage,
                &report,
                RuntimeStatus::Unhealthy,
                "runtime.crash.recoverable",
                format!(
                    "forcing recoverable crash after {} heartbeats on attempt {}",
                    heartbeat_count, attempt
                ),
            )?;
            process::exit(config.supervision.recoverable_exit_code);
        }
    }
}

fn runtime_stop_requested(storage: &StorageLayout) -> io::Result<bool> {
    match read_health_report(&storage.health_report_path) {
        Ok(report) => Ok(matches!(
            report.status,
            RuntimeStatus::ShuttingDown | RuntimeStatus::Stopped
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn supervise_runtime(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<(), Box<dyn Error>> {
    let executable = env::current_exe()?;
    let restart_policy = RuntimeRestartPolicy::from_settings(&config.supervision);

    let snapshot = bootstrap_runtime(config, storage, &executable, restart_policy.clone())?;
    let coordinator = LocalCoordinator::new(storage);
    recover_interrupted_build_attempts(&coordinator, &snapshot.recovery_report);

    let supervisor_process_id = process::id();
    let mut attempt = 1_u32;
    let mut restart_count = 0_u32;

    loop {
        write_supervisor_snapshot(
            storage,
            &RuntimeSupervisorSnapshot::new(
                config,
                supervisor_process_id,
                None,
                attempt,
                restart_count,
                None,
                RuntimeSupervisorStatus::Starting,
                format!("spawning runtime serve attempt {attempt}"),
            )?,
        )?;

        let mut child = Command::new(&executable)
            .arg("serve")
            .env(SUPERVISION_ATTEMPT_ENV, attempt.to_string())
            .spawn()?;

        write_supervisor_snapshot(
            storage,
            &RuntimeSupervisorSnapshot::new(
                config,
                supervisor_process_id,
                Some(child.id()),
                attempt,
                restart_count,
                None,
                RuntimeSupervisorStatus::Running,
                format!("runtime serve attempt {attempt} running as pid {}", child.id()),
            )?,
        )?;

        let exit_status = child.wait()?;
        let exit_code = exit_status.code();

        if exit_status.success() {
            let snapshot = RuntimeSupervisorSnapshot::new(
                config,
                supervisor_process_id,
                None,
                attempt,
                restart_count,
                exit_code,
                RuntimeSupervisorStatus::Completed,
                format!("runtime serve attempt {attempt} completed cleanly"),
            )?;
            write_supervisor_snapshot(storage, &snapshot)?;
            println!("{}", snapshot.to_json_pretty()?);
            return Ok(());
        }

        if restart_policy.should_restart(exit_code, restart_count) {
            restart_count += 1;
            let snapshot = RuntimeSupervisorSnapshot::new(
                config,
                supervisor_process_id,
                None,
                attempt,
                restart_count,
                exit_code,
                RuntimeSupervisorStatus::Restarting,
                format!(
                    "recoverable exit {:?} detected, restarting after {} ms",
                    exit_code,
                    restart_policy.restart_backoff_millis
                ),
            )?;
            write_supervisor_snapshot(storage, &snapshot)?;
            thread::sleep(Duration::from_millis(
                restart_policy.restart_backoff_millis,
            ));
            attempt += 1;
            continue;
        }

        let snapshot = RuntimeSupervisorSnapshot::new(
            config,
            supervisor_process_id,
            None,
            attempt,
            restart_count,
            exit_code,
            RuntimeSupervisorStatus::Failed,
            format!("runtime serve exited unsuccessfully with code {:?}", exit_code),
        )?;
        write_supervisor_snapshot(storage, &snapshot)?;
        if let Ok(report) = read_health_report(&storage.health_report_path) {
            let _ = update_runtime_health(
                storage,
                &report,
                RuntimeStatus::Unhealthy,
                "runtime.supervisor.failed",
                format!(
                    "supervisor exhausted restart policy after exit code {:?}",
                    exit_code
                ),
            )?;
        }
        return Err(format!("runtime serve exited unsuccessfully with code {:?}", exit_code).into());
    }
}

fn current_supervision_attempt() -> u32 {
    env::var(SUPERVISION_ATTEMPT_ENV)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|attempt| *attempt > 0)
        .unwrap_or(1)
}

fn should_force_recoverable_crash(
    config: &RuntimeConfig,
    attempt: u32,
    heartbeat_count: u32,
) -> bool {
    match config.runtime_loop.crash_after_heartbeats {
        Some(crash_after_heartbeats)
            if config.runtime_loop.crash_attempts >= attempt
                && heartbeat_count >= crash_after_heartbeats =>
        {
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        package_build_output,
        build_execution_logs_archive_path, build_execution_report_path,
        recover_interrupted_build_attempts,
        run_automation_inspect_command, run_automation_poll_once_command,
        parse_manifest_sync_command, parse_manual_release_dispatch_command,
        parse_registration_import_runtime_db_command,
        parse_registration_checkout_command,
        parse_seed_revolutions_registration_command,
        parse_publish_inspect_command,
        QueueLeaseRenewer,
        parse_release_plan_command, run_manifest_sync_command,
        run_registrations_command,
        resolve_runtime_build_execution_plan_with_profile,
        run_build_run_next_command, run_build_stage_next_command,
        run_publish_inspect_command,
        run_manual_release_dispatch_command, run_release_plan_command,
        run_publish_run_next_command, run_release_planner_cycle,
        runtime_stop_requested, select_queued_repository_tags,
        AutomationPollReport, BuildExecutionReport, BuildRunRecord, RegistrationCheckoutReport,
        RegistrationSeedReport,
        PublishedOutputInspectionReport,
    };
    use rusqlite::{params, Connection};
    use runtime_core::shutdown_runtime;
    use runtime_config::{HostPlatform, RuntimeConfig, RuntimeDirectories};
    use runtime_git::GitTag;
    use runtime_manifests::ApplyReport as ManifestApplyReport;
    use runtime_store::{
        ImportedRepositoryRegistrationReport, InterruptedBuildRecoveryRecord,
        LocalCoordinator, RuntimeRecoveryReport, RECOVERY_INTERRUPTION_KIND_REQUESTED,
    };
    use runtime_runner::{
        resolve_final_artifact_output_path, DiscoveredUnityEditor,
        ExecutionPlan as RunnerExecutionPlan, ExecutionResult,
        HostCapabilityProfile, HostToolCapability, RunnerSelectionDiagnostics,
        UnityLicenseDiagnostics,
    };
    use serde_json::json;
    use std::fs;
    use runtime_store::{
        initialize_database, AutomationSnapshot, BuildExecutionPlan,
        PublishRunRecord, ReleaseRunRecord, StorageLayout,
    };
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn load_build_execution_report(workspace_path: &Path) -> BuildExecutionReport {
        let report_path = build_execution_report_path(workspace_path);
        let contents = fs::read(&report_path).expect("build execution report should exist");
        serde_json::from_slice(&contents).expect("build execution report should decode")
    }

    fn archive_entry_names(archive_path: &Path) -> Vec<String> {
        let file = fs::File::open(archive_path).expect("archive should open");
        let mut archive = zip::ZipArchive::new(file).expect("archive should decode");
        let mut names = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index).expect("archive entry should load");
            names.push(entry.name().to_owned());
        }
        names
    }

    fn read_archive_entry(archive_path: &Path, entry_name: &str) -> String {
        let file = fs::File::open(archive_path).expect("archive should open");
        let mut archive = zip::ZipArchive::new(file).expect("archive should decode");
        let mut entry = archive
            .by_name(entry_name)
            .expect("archive entry should exist");
        let mut contents = String::new();
        entry
            .read_to_string(&mut contents)
            .expect("archive entry should read");
        contents
    }

    fn test_archive_execution_plan(
        platform: &str,
        target_name: &str,
    ) -> RunnerExecutionPlan {
        RunnerExecutionPlan {
            build_run_id: 41,
            release_run_id: 11,
            build_target_id: 13,
            repository_name: String::from("revolutions"),
            repository_url: String::from("https://example.com/revolutions.git"),
            git_tag: String::from("v1.0.3"),
            target_name: String::from(target_name),
            platform: String::from(platform),
            runner_type: String::from("host-native"),
            build_method: String::from("Builder.Perform"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(format!("Builds/{target_name}")),
            unity_version: String::from("2021.3.33f1"),
            config_json: String::from("{}"),
            timeout_seconds: 900,
        }
    }

    fn test_archive_execution_result(
        root: &Path,
        artifact_root_path: &Path,
        output_path: &Path,
    ) -> ExecutionResult {
        ExecutionResult {
            workspace_path: root.join("workspace"),
            log_path: root.join("workspace").join("logs").join("unity-build.log"),
            artifact_root_path: artifact_root_path.to_path_buf(),
            output_path: output_path.to_path_buf(),
        }
    }

    #[test]
    fn package_build_output_excludes_unity_marked_non_shippable_directories() {
        let root = test_root("runtime-bin-package-filter-non-shippable-dirs");
        fs::create_dir_all(&root).expect("test root should create");

        let output_root = root.join("unity-output");
        fs::create_dir_all(output_root.join("revolutions_Data/Managed"))
            .expect("player data directory should create");
        fs::create_dir_all(output_root.join("D3D12"))
            .expect("d3d12 directory should create");
        fs::create_dir_all(
            output_root.join("revolutions_BurstDebugInformation_DoNotShip/NativeData"),
        )
        .expect("burst do-not-ship directory should create");
        fs::create_dir_all(
            output_root.join(
                "revolutions_BackUpThisFolder_ButDontShipItWithYourGame/ShaderCache",
            ),
        )
        .expect("backup do-not-ship directory should create");
        fs::write(output_root.join("revolutions.exe"), "player")
            .expect("player executable should write");
        fs::write(output_root.join("UnityPlayer.dll"), "engine")
            .expect("unity player should write");
        fs::write(
            output_root.join("revolutions_Data/Managed/Assembly-CSharp.dll"),
            "managed",
        )
        .expect("managed assembly should write");
        fs::write(output_root.join("D3D12/d3d12core.dll"), "directstorage")
            .expect("d3d12 runtime should write");
        fs::write(
            output_root.join(
                "revolutions_BurstDebugInformation_DoNotShip/NativeData/methods.dbg",
            ),
            "burst-symbols",
        )
        .expect("burst symbols should write");
        fs::write(
            output_root.join(
                "revolutions_BackUpThisFolder_ButDontShipItWithYourGame/ShaderCache/cache.bin",
            ),
            "cache",
        )
        .expect("backup cache should write");

        let artifact_root = root.join("artifact-root");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let plan = test_archive_execution_plan("windows", "windows-player");
        let result = test_archive_execution_result(&root, &artifact_root, &output_root);

        package_build_output(&plan, &result).expect("build output should package");

        let archive_path = resolve_final_artifact_output_path(&plan, &artifact_root)
            .expect("artifact archive path should resolve");
        let names = archive_entry_names(&archive_path);
        assert!(names.iter().any(|name| name == "revolutions.exe"));
        assert!(names.iter().any(|name| name == "UnityPlayer.dll"));
        assert!(names.iter().any(|name| {
            name == "revolutions_Data/Managed/Assembly-CSharp.dll"
        }));
        assert!(names.iter().any(|name| name == "D3D12/d3d12core.dll"));
        assert!(!names.iter().any(|name| name.contains("_DoNotShip")));
        assert!(!names.iter().any(|name| {
            name.contains("_BackUpThisFolder_ButDontShipItWithYourGame")
        }));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn package_build_output_excludes_windows_pdb_files() {
        let root = test_root("runtime-bin-package-filter-windows-pdb");
        fs::create_dir_all(&root).expect("test root should create");

        let output_root = root.join("unity-output");
        fs::create_dir_all(&output_root).expect("unity output should create");
        fs::write(output_root.join("revolutions.exe"), "player")
            .expect("player executable should write");
        fs::write(output_root.join("revolutions.pdb"), "debug-symbols")
            .expect("pdb should write");

        let artifact_root = root.join("artifact-root");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let plan = test_archive_execution_plan("windows", "windows-player");
        let result = test_archive_execution_result(&root, &artifact_root, &output_root);

        package_build_output(&plan, &result).expect("build output should package");

        let archive_path = resolve_final_artifact_output_path(&plan, &artifact_root)
            .expect("artifact archive path should resolve");
        let names = archive_entry_names(&archive_path);
        assert!(names.iter().any(|name| name == "revolutions.exe"));
        assert!(!names.iter().any(|name| name.ends_with(".pdb")));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn package_build_output_excludes_macos_dsym_bundles() {
        let root = test_root("runtime-bin-package-filter-macos-dsym");
        fs::create_dir_all(&root).expect("test root should create");

        let output_root = root.join("unity-output");
        fs::create_dir_all(output_root.join("revolutions.app/Contents/MacOS"))
            .expect("macos app directory should create");
        fs::create_dir_all(
            output_root.join("revolutions.app.dSYM/Contents/Resources/DWARF"),
        )
        .expect("dSYM bundle should create");
        fs::write(
            output_root.join("revolutions.app/Contents/MacOS/revolutions"),
            "player",
        )
        .expect("macos executable should write");
        fs::write(
            output_root.join("revolutions.app/Contents/Info.plist"),
            "plist",
        )
        .expect("macos app metadata should write");
        fs::write(
            output_root.join("revolutions.app.dSYM/Contents/Resources/DWARF/revolutions"),
            "debug-symbols",
        )
        .expect("dSYM payload should write");

        let artifact_root = root.join("artifact-root");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let plan = test_archive_execution_plan("macos", "macos-player");
        let result = test_archive_execution_result(&root, &artifact_root, &output_root);

        package_build_output(&plan, &result).expect("build output should package");

        let archive_path = resolve_final_artifact_output_path(&plan, &artifact_root)
            .expect("artifact archive path should resolve");
        let names = archive_entry_names(&archive_path);
        assert!(
            names.iter().any(|name| name == "revolutions.app/Contents/MacOS/revolutions")
        );
        assert!(
            names.iter().any(|name| name == "revolutions.app/Contents/Info.plist")
        );
        assert!(!names.iter().any(|name| name.contains(".dSYM/")));
        assert!(!names.iter().any(|name| name.ends_with(".dSYM")));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn package_build_output_excludes_webgl_symbols_json_files() {
        let root = test_root("runtime-bin-package-filter-webgl-symbols");
        fs::create_dir_all(&root).expect("test root should create");

        let output_root = root.join("unity-output");
        fs::create_dir_all(output_root.join("Build"))
            .expect("webgl build directory should create");
        fs::create_dir_all(output_root.join("TemplateData"))
            .expect("template data directory should create");
        fs::write(output_root.join("index.html"), "<html></html>")
            .expect("index should write");
        fs::write(output_root.join("TemplateData/style.css"), "body {}")
            .expect("template stylesheet should write");
        fs::write(output_root.join("Build/revolutions.loader.js"), "loader")
            .expect("loader should write");
        fs::write(output_root.join("Build/revolutions.framework.js"), "framework")
            .expect("framework should write");
        fs::write(output_root.join("Build/revolutions.data"), "data")
            .expect("data file should write");
        fs::write(output_root.join("Build/revolutions.wasm"), "wasm")
            .expect("wasm file should write");
        fs::write(
            output_root.join("Build/revolutions.symbols.json"),
            "debug-symbols",
        )
        .expect("symbols json should write");

        let artifact_root = root.join("artifact-root");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let plan = test_archive_execution_plan("webgl", "webgl-player");
        let result = test_archive_execution_result(&root, &artifact_root, &output_root);

        package_build_output(&plan, &result).expect("build output should package");

        let archive_path = resolve_final_artifact_output_path(&plan, &artifact_root)
            .expect("artifact archive path should resolve");
        let names = archive_entry_names(&archive_path);
        assert!(names.iter().any(|name| name == "index.html"));
        assert!(names.iter().any(|name| name == "TemplateData/style.css"));
        assert!(names.iter().any(|name| name == "Build/revolutions.loader.js"));
        assert!(names.iter().any(|name| name == "Build/revolutions.framework.js"));
        assert!(names.iter().any(|name| name == "Build/revolutions.data"));
        assert!(names.iter().any(|name| name == "Build/revolutions.wasm"));
        assert!(!names.iter().any(|name| name.ends_with(".symbols.json")));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn parse_manual_release_dispatch_command_accepts_rebuild() {
        let command = parse_manual_release_dispatch_command(&[
            String::from("--repository-id"),
            String::from("41"),
            String::from("--git-tag"),
            String::from("v1.2.3"),
            String::from("--git-commit"),
            String::from("deadbeef"),
            String::from("--requested-via"),
            String::from("cli"),
            String::from("--rebuild"),
        ])
        .expect("manual dispatch command should parse");

        assert_eq!(command.repository_id, 41);
        assert_eq!(command.git_tag, "v1.2.3");
        assert_eq!(command.git_commit, "deadbeef");
        assert_eq!(command.requested_via, "cli");
        assert!(command.rebuild);
    }

    #[test]
    fn parse_release_plan_command_requires_release_id() {
        let error = parse_release_plan_command(&[])
            .expect_err("release plan command should require a release id");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("missing required --release-run-id"));
    }

    #[test]
    fn parse_manifest_sync_command_accepts_explicit_directory() {
        let command = parse_manifest_sync_command(&[
            String::from("--dir"),
            String::from("custom/pipelines"),
        ])
        .expect("manifest sync command should parse");

        assert_eq!(command.manifest_dir, PathBuf::from("custom/pipelines"));
    }

    #[test]
    fn parse_seed_revolutions_registration_command_accepts_env_override() {
        let command = parse_seed_revolutions_registration_command(&[
            String::from("--project-pat-env"),
            String::from("RUNTIME_BIN_TEST_PAT"),
        ])
        .expect("seed registrations command should parse");

        assert_eq!(command.project_pat_env, "RUNTIME_BIN_TEST_PAT");
    }

    #[test]
    fn parse_registration_checkout_command_accepts_explicit_ref() {
        let command = parse_registration_checkout_command(&[
            String::from("--repository-id"),
            String::from("41"),
            String::from("--ref"),
            String::from("main"),
        ])
        .expect("registration checkout command should parse");

        assert_eq!(command.repository_id, 41);
        assert_eq!(command.git_ref.as_deref(), Some("main"));
    }

    #[test]
    fn parse_registration_import_runtime_db_command_accepts_required_flags() {
        let command = parse_registration_import_runtime_db_command(&[
            String::from("--source-db"),
            String::from("C:/runtime/state/runtime.db"),
            String::from("--repository-name"),
            String::from("Revolutions"),
        ])
        .expect("registration import-runtime-db command should parse");

        assert_eq!(
            command.source_db_path,
            PathBuf::from("C:/runtime/state/runtime.db")
        );
        assert_eq!(command.repository_name, "Revolutions");
    }

    #[test]
    fn runtime_stop_requested_detects_persisted_shutdown_marker() {
        let root = test_root("runtime-bin-stop-requested");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);

        assert!(
            !runtime_stop_requested(&storage).expect("missing report should not request stop")
        );

        shutdown_runtime(&config, &storage).expect("shutdown marker should persist");

        assert!(
            runtime_stop_requested(&storage).expect("shutdown marker should request stop")
        );

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn parse_publish_inspect_command_accepts_publish_run_id() {
        let command = parse_publish_inspect_command(&[
            String::from("--publish-run-id"),
            String::from("17"),
        ])
        .expect("publish inspect command should parse");

        assert!(matches!(
            command.scope,
            super::PublishInspectScope::PublishRun(17)
        ));
    }

    #[test]
    fn manifest_sync_command_outputs_report_and_persists_pipeline_state() {
        std::env::set_var("RUNTIME_BIN_MANIFEST_USER", "git");
        std::env::set_var("RUNTIME_BIN_MANIFEST_TOKEN", "solidarity");

        let root = test_root("runtime-bin-manifest-sync");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let pipelines_dir = root.join("pipelines");
        fs::create_dir_all(&pipelines_dir).expect("pipelines directory should create");
        fs::write(
            pipelines_dir.join("revolutions.yml"),
            concat!(
                "apiVersion: handy.unity.builder/v1alpha1\n",
                "kind: Pipeline\n",
                "metadata:\n",
                "  name: revolutions\n",
                "spec:\n",
                "  repository:\n",
                "    url: https://example.com/org/revolutions.git\n",
                "    credentials: origin\n",
                "  credentials:\n",
                "    - name: origin\n",
                "      kind: git-http-basic\n",
                "      basic:\n",
                "        username:\n",
                "          env: RUNTIME_BIN_MANIFEST_USER\n",
                "        password:\n",
                "          env: RUNTIME_BIN_MANIFEST_TOKEN\n",
                "  build:\n",
                "    targets:\n",
                "      - name: windows64\n",
                "        platform: StandaloneWindows64\n",
                "        buildMethod: Builder.BuildWindows64\n",
                "        output:\n",
                "          kind: archive\n",
                "          path: Builds/Windows64\n",
                "  publish:\n",
                "    targets:\n",
                "      - name: filesystem-release\n",
                "        kind: filesystem\n",
                "        config:\n",
                "          root_path: C:/exports/releases\n",
                "  bindings:\n",
                "    - buildTarget: windows64\n",
                "      publishTarget: filesystem-release\n"
            ),
        )
        .expect("manifest should write");

        let output = run_manifest_sync_command(
            &[
                String::from("--dir"),
                pipelines_dir.display().to_string(),
            ],
            &storage,
        )
        .expect("manifest sync command should succeed");
        let report: ManifestApplyReport =
            serde_json::from_str(&output).expect("manifest sync output should decode");

        assert_eq!(report.pipelines.len(), 1);
        assert!(report.pipelines[0].applied);
        assert_eq!(report.pipelines[0].pipeline_name, "revolutions");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = connection
            .query_row(
                "SELECT id FROM repositories WHERE name = ?",
                ["revolutions"],
                |row| row.get::<_, i64>(0),
            )
            .expect("repository should persist");

        let credential_name: String = connection
            .query_row(
                "SELECT name FROM credentials WHERE id = (SELECT credentials_id FROM repositories WHERE id = ?)",
                [repository_id],
                |row| row.get(0),
            )
            .expect("repository credential should persist");
        assert_eq!(credential_name, "revolutions/origin");

        let build_target_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM build_targets WHERE repository_id = ? AND enabled = 1",
                [repository_id],
                |row| row.get(0),
            )
            .expect("build target count should load");
        assert_eq!(build_target_count, 1);

        let publish_target_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM publish_targets WHERE repository_id = ? AND enabled = 1",
                [repository_id],
                |row| row.get(0),
            )
            .expect("publish target count should load");
        assert_eq!(publish_target_count, 1);

        let binding_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM build_publish_bindings WHERE enabled = 1",
                [],
                |row| row.get(0),
            )
            .expect("binding count should load");
        assert_eq!(binding_count, 1);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn registrations_seed_revolutions_command_applies_sql_seed() {
        let root = test_root("runtime-bin-registrations-seed-revolutions");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        let project_pat_env = "RUNTIME_BIN_TEST_REVOLUTIONS_PROJECT_PAT";
        let project_pat = "solidarity'token";
        std::env::set_var(project_pat_env, project_pat);

        let output = run_registrations_command(
            &[
                String::from("seed-revolutions"),
                String::from("--project-pat-env"),
                String::from(project_pat_env),
            ],
            &config,
            &storage,
        )
        .expect("registrations seed command should succeed");
        let report: RegistrationSeedReport = serde_json::from_str(&output)
            .expect("registration seed output should decode");

        assert_eq!(report.registration_name, "Revolutions");
        assert_eq!(report.build_target_count, 1);
        assert_eq!(report.project_pat_env, project_pat_env);
        assert_eq!(
            report.workspace_root_override.as_deref(),
            Some("D:\\Users\\gabao\\RevolutionsHandyUnityBuilderWorkspace")
        );
        assert_eq!(
            report.artifacts_root_override.as_deref(),
            Some("D:\\Users\\gabao\\Revolutions\\builds-output")
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let credentials_config_json: String = connection
            .query_row(
                "SELECT config_json FROM credentials WHERE name = 'Revolutions/origin'",
                [],
                |row| row.get(0),
            )
            .expect("seeded credentials should load");
        let credentials_config: serde_json::Value = serde_json::from_str(&credentials_config_json)
            .expect("seeded credentials config should decode");
        assert_eq!(credentials_config["username"], "indiegabo");
        assert_eq!(credentials_config["password"], project_pat);
        drop(connection);

        std::env::remove_var(project_pat_env);
        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn registrations_checkout_command_materializes_repository_workspace() {
        let root = test_root("runtime-bin-registrations-checkout");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_path = root.join("fixtures").join("revolutions");
        let repository_url = create_unity_repository_with_tags(
            &repository_path,
            "2022.3.20f1",
            &["v1.0.0"],
        );
        let default_branch = current_git_branch_name(&repository_path);
        let expected_head_commit = current_git_head_commit(&repository_path);
        let workspace_root_override = root.join("managed-checkouts").join("revolutions");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let credentials_id = seed_credentials(
            &connection,
            "revolutions/origin",
            "git-http-basic",
            r#"{"username":"comrade","password":"sickle"}"#,
        );
        let repository_id = seed_repository_with_url_and_credentials(
            &connection,
            "revolutions",
            &repository_url,
            Some(credentials_id),
        );
        connection
            .execute(
                "
                UPDATE repositories
                SET default_branch = ?,
                    workspace_root_override = ?
                WHERE id = ?
                ",
                params![
                    default_branch,
                    workspace_root_override.display().to_string(),
                    repository_id,
                ],
            )
            .expect("repository checkout metadata should update");
        drop(connection);

        let output = run_registrations_command(
            &[
                String::from("checkout"),
                String::from("--repository-id"),
                repository_id.to_string(),
            ],
            &config,
            &storage,
        )
        .expect("registrations checkout command should succeed");
        let report: RegistrationCheckoutReport = serde_json::from_str(&output)
            .expect("registration checkout output should decode");

        assert_eq!(report.repository_id, repository_id);
        assert_eq!(report.repository_name, "revolutions");
        assert_eq!(report.source_mode, "managed_repository");
        assert_eq!(report.workspace_strategy, "managed_checkout");
        assert_eq!(report.git_ref, default_branch);
        assert_eq!(report.git_ref_source, "default_branch");
        assert_eq!(
            PathBuf::from(&report.workspace_root_path),
            workspace_root_override
        );
        assert_eq!(
            PathBuf::from(&report.checkout_path),
            workspace_root_override.join("checkout")
        );
        assert_eq!(report.head_commit, expected_head_commit);
        assert!(workspace_root_override.join("checkout").join(".git").is_dir());
        assert!(workspace_root_override
            .join("checkout")
            .join("ProjectSettings")
            .join("ProjectVersion.txt")
            .is_file());

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn registrations_import_runtime_db_command_imports_repository_configuration() {
        let root = test_root("runtime-bin-registrations-import-runtime-db");
        let target_config = RuntimeConfig::from_root(root.join("target-runtime"));
        let target_storage = StorageLayout::from_directories(&target_config.directories);
        initialize_database(&target_storage).expect("target database bootstrap should succeed");

        let source_directories = RuntimeDirectories::from_root(root.join("source-runtime"));
        let source_storage = StorageLayout::from_directories(&source_directories);
        initialize_database(&source_storage).expect("source database bootstrap should succeed");

        let source_connection = Connection::open(&source_storage.database_path)
            .expect("source connection should open");
        let credentials_id = seed_credentials(
            &source_connection,
            "Revolutions/origin",
            "git-http-basic",
            r#"{"username":"comrade","password":"sickle"}"#,
        );
        let repository_id = seed_repository_with_url_and_credentials(
            &source_connection,
            "Revolutions",
            "https://example.com/revolutions.git",
            Some(credentials_id),
        );
        source_connection
            .execute(
                "
                UPDATE repositories
                SET default_branch = ?,
                    artifacts_root_override = ?,
                    workspace_root_override = ?
                WHERE id = ?
                ",
                params![
                    "main",
                    "D:/build-output",
                    "D:/managed-workspace",
                    repository_id,
                ],
            )
            .expect("source repository overrides should update");
        source_connection
            .execute(
                "
                INSERT INTO trigger_rules (repository_id, name, source, enabled, config_json)
                VALUES (?, ?, ?, ?, ?)
                ",
                params![repository_id, "poll-default", "poll", 1, "{}"],
            )
            .expect("source trigger rule should insert");
        let build_target_id = seed_build_target(
            &source_connection,
            repository_id,
            "windows-player",
            "windows",
        );
        let publish_target_id = seed_publish_target(
            &source_connection,
            repository_id,
            "filesystem-release",
            "filesystem",
        );
        seed_build_publish_binding(&source_connection, build_target_id, publish_target_id);
        source_connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    trigger_source,
                    source_metadata_json,
                    status
                ) VALUES (?, ?, ?, ?, ?)
                ",
                params![repository_id, "v1.0.0", "poll", "{}", "queued"],
            )
            .expect("source release run should insert");
        drop(source_connection);

        let output = run_registrations_command(
            &[
                String::from("import-runtime-db"),
                String::from("--source-db"),
                source_storage.database_path.display().to_string(),
                String::from("--repository-name"),
                String::from("Revolutions"),
            ],
            &target_config,
            &target_storage,
        )
        .expect("registrations import-runtime-db command should succeed");
        let report: ImportedRepositoryRegistrationReport = serde_json::from_str(&output)
            .expect("registration import-runtime-db output should decode");

        assert_eq!(report.repository_name, "Revolutions");
        assert_eq!(report.credential_name.as_deref(), Some("Revolutions/origin"));
        assert_eq!(report.trigger_rule_count, 1);
        assert_eq!(report.build_target_count, 1);
        assert_eq!(report.publish_target_count, 1);
        assert_eq!(report.binding_count, 1);

        let target_connection = Connection::open(&target_storage.database_path)
            .expect("target connection should open");
        let counts: (i64, i64) = target_connection
            .query_row(
                "
                SELECT
                    (SELECT COUNT(1) FROM repositories WHERE name = 'Revolutions'),
                    (SELECT COUNT(1) FROM release_runs)
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("target counts should load");
        assert_eq!(counts.0, 1);
        assert_eq!(counts.1, 0);
        drop(target_connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn automation_inspect_command_outputs_runtime_snapshot_json() {
        let root = test_root("runtime-bin-automation-inspect");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(&connection, "runtime-bin-automation-inspect");
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        seed_build_target(&connection, repository_id, "linux-player", "linux");
        let publish_target_id = seed_publish_target(
            &connection,
            repository_id,
            "filesystem-release",
            "filesystem",
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v15.0.0",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let build_runs = connection
            .prepare(
                "
                SELECT id
                FROM build_runs
                WHERE release_run_id = ?
                ORDER BY build_target_id ASC
                ",
            )
            .expect("build run query should prepare")
            .query_map([release_run_id], |row| row.get::<_, i64>(0))
            .expect("build run query should succeed")
            .collect::<Result<Vec<_>, _>>()
            .expect("build runs should collect");
        assert_eq!(build_runs.len(), 2);

        connection
            .execute(
                "
                UPDATE build_runs
                SET status = ?,
                    workspace_path = ?,
                    log_path = ?,
                    artifact_root_path = ?,
                    started_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    "running",
                    "C:/runtime/runs/repo",
                    "C:/runtime/logs/build.log",
                    "C:/runtime/artifacts/repo",
                    build_runs[1],
                ],
            )
            .expect("second build run should mark running");

        let artifact_id = insert_artifact_record(
            &connection,
            build_runs[0],
            "game.zip",
            "archive",
            "game.zip",
        );
        let publish_run_id = insert_publish_run_record(
            &connection,
            release_run_id,
            build_runs[0],
            publish_target_id,
            artifact_id,
            "queued",
        );
        drop(connection);

        let coordinator = runtime_store::LocalCoordinator::new(&storage);
        let claimed_build_message = coordinator
            .claim_next(
                "build-runs",
                "runtime-bin-build-worker",
                Duration::ZERO,
                Duration::from_secs(30),
            )
            .expect("build queue claim should succeed")
            .expect("one build queue message should be available");
        coordinator
            .dispatch_publish_run(publish_run_id)
            .expect("publish run should dispatch");
        let claimed_publish_message = coordinator
            .claim_next(
                "publish-runs",
                "runtime-bin-publish-worker",
                Duration::ZERO,
                Duration::from_secs(30),
            )
            .expect("publish queue claim should succeed")
            .expect("one publish queue message should be available");
        let lease = coordinator
            .acquire_lock(
                "release-plan:runtime-bin-automation-inspect",
                Duration::from_secs(30),
            )
            .expect("coordination lease should succeed")
            .expect("coordination lease should create");

        let output = run_automation_inspect_command(&[], &storage)
            .expect("automation inspect command should succeed");
        let snapshot: AutomationSnapshot =
            serde_json::from_str(&output).expect("automation inspect output should decode");

        assert_eq!(snapshot.repositories.len(), 1);
        let repository = &snapshot.repositories[0];
        assert_eq!(repository.repository_id, repository_id);
        assert_eq!(repository.repository_name, "runtime-bin-automation-inspect");
        assert!(repository.enabled);
        assert_eq!(repository.enabled_build_target_count, 2);
        assert_eq!(repository.pending_release_count, 1);
        assert_eq!(repository.queued_build_runs, 1);
        assert_eq!(repository.running_build_runs, 1);
        assert_eq!(repository.queued_publish_runs, 1);
        assert_eq!(repository.running_publish_runs, 0);
        assert_eq!(repository.release_queue.len(), 1);

        let release = &repository.release_queue[0];
        assert_eq!(release.release_run_id, release_run_id);
        assert_eq!(release.git_tag, "v15.0.0");
        assert_eq!(release.status, "queued");
        assert_eq!(release.unity_version.as_deref(), Some("2021.3.33f1"));
        assert!(release.planned);
        assert!(release.build_process_active);
        assert!(release.publish_process_active);
        assert_eq!(release.queued_build_runs, 1);
        assert_eq!(release.running_build_runs, 1);
        assert_eq!(release.total_build_runs, 2);
        assert_eq!(release.queued_publish_runs, 1);
        assert_eq!(release.running_publish_runs, 0);
        assert_eq!(release.total_publish_runs, 1);

        let build_queue = snapshot
            .queue_messages
            .iter()
            .find(|queue| queue.queue_name == "build-runs")
            .expect("build queue snapshot should exist");
        assert_eq!(build_queue.ready_count, 1);
        assert_eq!(build_queue.leased_count, 1);

        let publish_queue = snapshot
            .queue_messages
            .iter()
            .find(|queue| queue.queue_name == "publish-runs")
            .expect("publish queue snapshot should exist");
        assert_eq!(publish_queue.ready_count, 0);
        assert_eq!(publish_queue.leased_count, 1);

        let release_queue = snapshot
            .queue_messages
            .iter()
            .find(|queue| queue.queue_name == "release-runs")
            .expect("release queue snapshot should exist");
        assert_eq!(release_queue.ready_count, 0);
        assert_eq!(release_queue.leased_count, 0);

        assert_eq!(snapshot.coordination_leases.len(), 1);
        assert_eq!(snapshot.coordination_leases[0].name, lease.name);
        assert!(
            snapshot.coordination_leases[0].lease_expires_at_unix_millis
                >= lease.lease_expires_at_unix_millis
        );
        assert_eq!(claimed_build_message.queue_name, "build-runs");
        assert_eq!(claimed_publish_message.queue_name, "publish-runs");

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn manual_release_dispatch_command_outputs_queued_release_json() {
        let root = test_root("runtime-bin-release-dispatch");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        seed_repository(&connection, "runtime-bin-release-dispatch");
        drop(connection);

        let output = run_manual_release_dispatch_command(
            &[
                String::from("--repository-id"),
                String::from("1"),
                String::from("--git-tag"),
                String::from("v9.0.0"),
                String::from("--git-commit"),
                String::from("cafebabe"),
            ],
            &storage,
        )
        .expect("manual release dispatch command should succeed");
        let record: ReleaseRunRecord =
            serde_json::from_str(&output).expect("release dispatch output should decode");

        assert_eq!(record.repository_id, 1);
        assert_eq!(record.git_tag, "v9.0.0");
        assert_eq!(record.git_commit.as_deref(), Some("cafebabe"));
        assert_eq!(record.trigger_source, "manual");
        assert_eq!(record.status, "queued");

        let metadata: serde_json::Value = serde_json::from_str(&record.source_metadata_json)
            .expect("manual release metadata should decode");
        assert_eq!(metadata["requested_via"], "runtime-bin");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(queue_message_count(&connection, "release-runs"), 1);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn manual_release_dispatch_command_rebuild_reuses_release_and_clears_derived_state() {
        let root = test_root("runtime-bin-release-dispatch-rebuild");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(&connection, "runtime-bin-release-dispatch-rebuild");
        let build_target_id = seed_build_target(&connection, repository_id, "windows-player", "windows");
        let publish_target_id = seed_publish_target(
            &connection,
            repository_id,
            "filesystem-release",
            "filesystem",
        );
        seed_build_publish_binding(&connection, build_target_id, publish_target_id);
        let release_run_id = seed_manual_release_for_rebuild(
            &connection,
            repository_id,
            "v9.0.1",
            "2022.3.20f1",
        );
        let artifact_root_path = root.join("artifacts");
        fs::create_dir_all(&artifact_root_path).expect("artifact root should create");
        let build_run_id = seed_succeeded_build_run(
            &connection,
            release_run_id,
            build_target_id,
            &artifact_root_path,
        );
        let artifact_id = insert_artifact_record(
            &connection,
            build_run_id,
            "rebuilt.zip",
            "archive",
            "rebuilt.zip",
        );
        insert_publish_run_record(
            &connection,
            release_run_id,
            build_run_id,
            publish_target_id,
            artifact_id,
            "succeeded",
        );
        drop(connection);

        let output = run_manual_release_dispatch_command(
            &[
                String::from("--repository-id"),
                repository_id.to_string(),
                String::from("--git-tag"),
                String::from("v9.0.1"),
                String::from("--git-commit"),
                String::from("feedface"),
                String::from("--requested-via"),
                String::from("hub"),
                String::from("--rebuild"),
            ],
            &storage,
        )
        .expect("manual release rebuild command should succeed");
        let record: ReleaseRunRecord =
            serde_json::from_str(&output).expect("release rebuild output should decode");

        assert_eq!(record.id, release_run_id);
        assert_eq!(record.git_commit.as_deref(), Some("feedface"));
        assert_eq!(record.status, "queued");
        assert!(record.unity_version.is_none());

        let metadata: serde_json::Value = serde_json::from_str(&record.source_metadata_json)
            .expect("rebuild metadata should decode");
        assert_eq!(metadata["requested_via"], "hub");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let persisted_build_run_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM build_runs WHERE release_run_id = ?",
                [release_run_id],
                |row| row.get(0),
            )
            .expect("build run count should load");
        let persisted_publish_run_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM publish_runs WHERE release_run_id = ?",
                [release_run_id],
                |row| row.get(0),
            )
            .expect("publish run count should load");
        assert_eq!(persisted_build_run_count, 0);
        assert_eq!(persisted_publish_run_count, 0);
        assert_eq!(queue_message_count(&connection, "release-runs"), 1);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn release_plan_command_outputs_planned_build_runs_json() {
        let root = test_root("runtime-bin-release-plan");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(&connection, "runtime-bin-release-plan");
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        seed_build_target(&connection, repository_id, "linux-player", "linux");
        let release_run_id = seed_queued_release(&connection, repository_id, "v9.1.0", "2022.3.20f1");
        drop(connection);

        let output = run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");
        let runs: Vec<BuildRunRecord> =
            serde_json::from_str(&output).expect("release plan output should decode");

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].status, "queued");
        assert_eq!(runs[1].status, "queued");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(queue_message_count(&connection, "build-runs"), 2);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn release_plan_command_reports_missing_enabled_targets() {
        let root = test_root("runtime-bin-release-plan-no-enabled-targets");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(&connection, "runtime-bin-release-plan-no-enabled-targets");
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        connection
            .execute(
                "UPDATE build_targets SET enabled = 0 WHERE repository_id = ?",
                [repository_id],
            )
            .expect("build targets should disable");
        let release_run_id = seed_queued_release(&connection, repository_id, "v9.1.1", "2022.3.20f1");
        drop(connection);

        let error = run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect_err("release plan command should fail when no enabled targets exist");
        let error = error
            .downcast::<std::io::Error>()
            .expect("release plan error should be an io::Error");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error
            .to_string()
            .contains("has no enabled build targets"));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn release_plan_command_rejects_release_that_is_not_queued() {
        let root = test_root("runtime-bin-release-plan-not-queued");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(&connection, "runtime-bin-release-plan-not-queued");
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        let release_run_id = seed_manual_release_for_rebuild(
            &connection,
            repository_id,
            "v9.1.2",
            "2022.3.20f1",
        );
        drop(connection);

        let error = run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect_err("release plan command should reject releases outside the queued state");
        let error = error
            .downcast::<std::io::Error>()
            .expect("release plan error should be an io::Error");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error
            .to_string()
            .contains("must be queued before build planning"));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn release_plan_command_detects_unity_version_from_git_repository() {
        let root = test_root("runtime-bin-release-plan-git");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-release-plan-source"),
            "v10.0.0",
            "2021.3.33f1",
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-release-plan-git",
            &repository_url,
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        drop(connection);

        let dispatch_output = run_manual_release_dispatch_command(
            &[
                String::from("--repository-id"),
                repository_id.to_string(),
                String::from("--git-tag"),
                String::from("v10.0.0"),
            ],
            &storage,
        )
        .expect("manual release dispatch command should succeed");
        let release: ReleaseRunRecord = serde_json::from_str(&dispatch_output)
            .expect("release dispatch output should decode");

        let output = run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release.id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should detect unity version from git");
        let runs: Vec<BuildRunRecord> =
            serde_json::from_str(&output).expect("release plan output should decode");

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].unity_version.as_deref(), Some("2021.3.33f1"));
        assert_eq!(
            runs[0].image_ref.as_deref(),
            Some("host-native"),
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let persisted_unity_version: String = connection
            .query_row(
                "SELECT unity_version FROM release_runs WHERE id = ?",
                [release.id],
                |row| row.get(0),
            )
            .expect("release unity version should load");
        assert_eq!(persisted_unity_version, "2021.3.33f1");
        assert_eq!(queue_message_count(&connection, "release-runs"), 1);
        assert_eq!(queue_message_count(&connection, "build-runs"), 1);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn release_planner_cycle_processes_manual_release_queue_message() {
        let root = test_root("runtime-bin-release-planner-cycle");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-release-planner-cycle-source"),
            "v11.0.0",
            "2021.3.33f1",
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-release-planner-cycle",
            &repository_url,
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        drop(connection);

        let output = run_manual_release_dispatch_command(
            &[
                String::from("--repository-id"),
                repository_id.to_string(),
                String::from("--git-tag"),
                String::from("v11.0.0"),
            ],
            &storage,
        )
        .expect("manual release dispatch command should succeed");
        let release: ReleaseRunRecord =
            serde_json::from_str(&output).expect("release dispatch output should decode");

        assert!(run_release_planner_cycle(&storage)
            .expect("release planner cycle should process one queued release"));

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let persisted_unity_version: String = connection
            .query_row(
                "SELECT unity_version FROM release_runs WHERE id = ?",
                [release.id],
                |row| row.get(0),
            )
            .expect("release unity version should load");
        assert_eq!(persisted_unity_version, "2021.3.33f1");
        assert_eq!(queue_message_count(&connection, "release-runs"), 0);
        assert_eq!(queue_message_count(&connection, "build-runs"), 1);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn select_queued_repository_tags_falls_back_to_latest_when_baseline_is_missing() {
        let tags = vec![
            GitTag {
                name: String::from("v1.0.0"),
                commit: String::from("1111111"),
            },
            GitTag {
                name: String::from("v1.1.0"),
                commit: String::from("2222222"),
            },
            GitTag {
                name: String::from("v1.2.0"),
                commit: String::from("3333333"),
            },
        ];

        let (selected, status, ok) =
            select_queued_repository_tags(&tags, Some("v0.9.0"));

        assert!(ok);
        assert_eq!(status, "");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "v1.2.0");
    }

    #[test]
    fn automation_poll_once_command_queues_unseen_tags_and_updates_baseline() {
        let root = test_root("runtime-bin-automation-poll-once-queue");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_unity_repository_with_tags(
            &root.join("runtime-bin-automation-poll-once-queue-source"),
            "2021.3.33f1",
            &["v1.0.0", "v1.1.0", "v1.2.0"],
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-automation-poll-once-queue",
            &repository_url,
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        connection
            .execute(
                "UPDATE repositories SET last_seen_tag = ? WHERE id = ?",
                params!["v1.0.0", repository_id],
            )
            .expect("repository baseline should update");
        drop(connection);

        let output = run_automation_poll_once_command(&[], &storage)
            .expect("automation poll-once command should succeed");
        let report: AutomationPollReport =
            serde_json::from_str(&output).expect("poll output should decode");

        assert_eq!(report.repositories.len(), 1);
        let repository = &report.repositories[0];
        assert_eq!(repository.repository_id, repository_id);
        assert_eq!(repository.status, "queued");
        assert_eq!(repository.last_seen_tag_before.as_deref(), Some("v1.0.0"));
        assert_eq!(repository.last_seen_tag_after.as_deref(), Some("v1.2.0"));
        assert_eq!(repository.queued_release_ids.len(), 2);
        assert_eq!(repository.discovered_tags.len(), 2);
        assert_eq!(repository.discovered_tags[0].name, "v1.1.0");
        assert_eq!(repository.discovered_tags[1].name, "v1.2.0");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(load_repository_last_seen_tag(&connection, repository_id).as_deref(), Some("v1.2.0"));
        assert_eq!(release_tags_for_repository(&connection, repository_id), vec![
            String::from("v1.1.0"),
            String::from("v1.2.0"),
        ]);
        assert_eq!(queue_message_count(&connection, "release-runs"), 2);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn automation_poll_once_command_skips_repository_backlog_before_remote_poll() {
        let root = test_root("runtime-bin-automation-poll-once-backlog");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_unity_repository_with_tags(
            &root.join("runtime-bin-automation-poll-once-backlog-source"),
            "2021.3.33f1",
            &["v2.0.0", "v2.1.0"],
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-automation-poll-once-backlog",
            &repository_url,
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        seed_queued_release(&connection, repository_id, "v2.0.0", "2021.3.33f1");
        drop(connection);

        let output = run_automation_poll_once_command(&[], &storage)
            .expect("automation poll-once command should succeed");
        let report: AutomationPollReport =
            serde_json::from_str(&output).expect("poll output should decode");

        assert_eq!(report.repositories.len(), 1);
        let repository = &report.repositories[0];
        assert_eq!(repository.repository_id, repository_id);
        assert_eq!(repository.status, "skipped_active_release_backlog");
        assert!(repository.queued_release_ids.is_empty());
        assert!(repository.discovered_tags.is_empty());

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(release_tags_for_repository(&connection, repository_id), vec![String::from("v2.0.0")]);
        assert_eq!(load_repository_last_seen_tag(&connection, repository_id), None);
        assert_eq!(queue_message_count(&connection, "build-runs"), 1);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_stage_next_command_marks_build_running_and_prepares_workspace() {
        let root = test_root("runtime-bin-build-stage-next-success");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-stage-next-source"),
            "v12.0.0",
            "2021.3.33f1",
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let credentials_id = seed_credentials(
            &connection,
            "runtime-bin-build-stage-next-basic",
            "git-http-basic",
            r#"{"username":"worker","password":"solidarity"}"#,
        );
        let repository_id = seed_repository_with_url_and_credentials(
            &connection,
            "runtime-bin-build-stage-next-success",
            &repository_url,
            Some(credentials_id),
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v12.0.0",
            "2021.3.33f1",
        );
        drop(connection);

        let planner_output = run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");
        let planned_runs: Vec<BuildRunRecord> =
            serde_json::from_str(&planner_output).expect("planned runs should decode");

        let output = run_build_stage_next_command(&[], &config, &storage)
            .expect("build stage-next command should succeed");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build stage output should decode");

        assert_eq!(record.id, planned_runs[0].id);
        assert_eq!(record.status, "running");
        assert!(record.started_at.is_some());
        assert_eq!(queue_message_count(&Connection::open(&storage.database_path).expect("connection should open"), "build-runs"), 0);

        let workspace_path = PathBuf::from(record.workspace_path.clone().expect("workspace path should persist"));
        assert!(workspace_path.is_dir());
        assert!(workspace_path.join("source").join("ProjectSettings").join("ProjectVersion.txt").is_file());

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_stage_next_command_fails_build_when_workspace_materialization_breaks() {
        let root = test_root("runtime-bin-build-stage-next-fail");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(
            &connection,
            "runtime-bin-build-stage-next-fail",
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v99.0.0",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let output = run_build_stage_next_command(&[], &config, &storage)
            .expect("build stage-next command should persist a failed run");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build stage output should decode");
        let error_message = record.error_message.as_deref().unwrap_or_default();

        assert_eq!(record.status, "failed");
        assert!(error_message.contains("fetch repository tag")
            || error_message.contains("clone repository into workspace"));
        assert!(error_message.contains("exit code"));
        assert!(error_message.contains("stderr:"));

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(queue_message_count(&connection, "build-runs"), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_run_next_command_completes_host_native_build() {
        let root = test_root("runtime-bin-build-run-next-success");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-run-next-source"),
            "v13.0.0",
            "2021.3.33f1",
        );
        let script_path = create_fake_unity_script(&root, "run-next-success", ScriptKind::Success);

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-build-run-next-success",
            &repository_url,
        );
        let build_target_id = seed_host_native_build_target(
            &connection,
            repository_id,
            "webgl-player",
            "webgl",
            "Builder.PerformWebGL",
            &script_path,
        );
        let publish_target_id =
            seed_publish_target(&connection, repository_id, "filesystem-release", "filesystem");
        seed_build_publish_binding(&connection, build_target_id, publish_target_id);
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v13.0.0",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let output = run_build_run_next_command(&[], &config, &storage)
            .expect("build run-next command should succeed");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build run-next output should decode");

        assert_eq!(record.status, "succeeded");
        assert!(record.started_at.is_some());
        assert!(record.finished_at.is_some());
        assert!(record.error_message.is_none());

        let workspace_path = PathBuf::from(
            record
                .workspace_path
                .clone()
                .expect("workspace path should persist"),
        );
        let log_path = PathBuf::from(record.log_path.clone().expect("log path should persist"));
        let workspace_name = workspace_path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("workspace directory name should exist")
            .to_owned();
        let archived_logs_path = build_execution_logs_archive_path(&workspace_path);
        let report = load_build_execution_report(&workspace_path);
        assert_eq!(log_path, workspace_path.join("logs").join("03-unity-build.log"));
        assert!(!log_path.exists());
        assert!(archived_logs_path.is_file());
        let log_contents = read_archive_entry(
            &archived_logs_path,
            &format!("{workspace_name}/logs/03-unity-build.log"),
        );
        assert!(log_contents.contains("build_method: Builder.PerformWebGL"));
        assert!(log_contents.contains("build_target: WebGL"));
        assert!(!workspace_path.join("source").exists());
        assert!(!workspace_path.join("logs").exists());
        assert_eq!(report.cleanup.status, "completed");
        assert_eq!(report.attempts.len(), 1);
        assert_eq!(report.retained_files.len(), 1);
        assert_eq!(report.publish_runs.len(), 1);
        assert_eq!(
            archive_entry_names(&archived_logs_path),
            vec![
                format!("{workspace_name}/logs/01-validate-build-context.log"),
                format!("{workspace_name}/logs/02-checkout-repository.log"),
                format!("{workspace_name}/logs/03-unity-build.log"),
                format!("{workspace_name}/logs/04-package-artifact.log"),
                format!("{workspace_name}/logs/05-register-artifacts.log"),
            ]
        );

        let artifact_path = config
            .directories
            .artifacts_dir
            .join("runtime-bin-build-run-next-success.v13.0.0")
            .join("runtime-bin-build-run-next-success.v13.0.0.webgl-player.zip");
        assert!(artifact_path.is_file());

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(artifact_count_for_build_run(&connection, record.id), 1);
        assert_eq!(publish_run_count_for_build_run(&connection, record.id), 1);
        assert_eq!(queue_message_count(&connection, "build-runs"), 0);
        assert_eq!(queue_message_count(&connection, "publish-runs"), 1);
        drop(connection);

        let stages = runtime_store::LocalCoordinator::new(&storage)
            .list_build_run_stages(record.id)
            .expect("build stages should load");
        assert_eq!(stages.len(), 5);
        assert_eq!(
            stages
                .iter()
                .map(|stage| stage.step_key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "validate-build-context",
                "checkout-repository",
                "unity-build",
                "package-artifact",
                "register-artifacts",
            ]
        );
        assert!(stages.iter().all(|stage| stage.status == "succeeded"));
        assert_eq!(stages[2].log_path, log_path.display().to_string());

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_run_next_command_uses_repository_workspace_and_artifact_overrides() {
        let root = test_root("runtime-bin-build-run-next-overrides");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-run-next-overrides-source"),
            "v13.1.0",
            "2021.3.33f1",
        );
        let script_path = create_fake_unity_script(&root, "run-next-overrides", ScriptKind::Success);
        let workspace_root_override = root.join("managed-workspace");
        let build_output_override = root.join("build-output");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-build-run-next-overrides",
            &repository_url,
        );
        connection
            .execute(
                "
                UPDATE repositories
                SET workspace_root_override = ?,
                    artifacts_root_override = ?
                WHERE id = ?
                ",
                params![
                    workspace_root_override.display().to_string(),
                    build_output_override.display().to_string(),
                    repository_id,
                ],
            )
            .expect("repository overrides should persist");
        seed_host_native_build_target(
            &connection,
            repository_id,
            "webgl-player",
            "webgl",
            "Builder.PerformWebGL",
            &script_path,
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v13.1.0",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let output = run_build_run_next_command(&[], &config, &storage)
            .expect("build run-next command should succeed with overrides");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build run-next output should decode");

        let expected_workspace_root = PathBuf::from(
            record
                .workspace_path
                .clone()
                .expect("workspace path should persist"),
        );
        let expected_log_path = PathBuf::from(
            record.log_path.clone().expect("log path should persist"),
        );
        let expected_artifact_root = build_output_override
            .join("runtime-bin-build-run-next-overrides.v13.1.0");
        let expected_artifact_path = expected_artifact_root
            .join("runtime-bin-build-run-next-overrides.v13.1.0.webgl-player.zip");
        let expected_workspace_path = expected_workspace_root.display().to_string();
        let expected_log_path_string = expected_log_path.display().to_string();
        let expected_artifact_root_string = expected_artifact_root.display().to_string();

        assert_eq!(record.status, "succeeded");
        assert!(expected_workspace_root.starts_with(workspace_root_override.join("runs")));
        assert_eq!(expected_log_path, expected_workspace_root.join("logs").join("03-unity-build.log"));
        assert!(expected_workspace_root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .starts_with(&format!("build-run-{}-attempt-", record.id)));
        assert_eq!(record.workspace_path.as_deref(), Some(expected_workspace_path.as_str()));
        assert_eq!(record.log_path.as_deref(), Some(expected_log_path_string.as_str()));
        assert_eq!(
            record.artifact_root_path.as_deref(),
            Some(expected_artifact_root_string.as_str())
        );
        assert!(!expected_workspace_root.join("source").exists());
        assert!(!expected_workspace_root.join("logs").exists());
        assert!(!expected_log_path.exists());
        assert!(build_execution_logs_archive_path(&expected_workspace_root).is_file());
        assert!(build_execution_report_path(&expected_workspace_root).is_file());
        assert!(expected_artifact_path.is_file());
        assert!(!config
            .directories
            .artifacts_dir
            .join("runtime-bin-build-run-next-overrides.v13.1.0")
            .exists());

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(artifact_count_for_build_run(&connection, record.id), 1);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_run_next_command_numbers_logs_by_execution_order_without_packaging() {
        let root = test_root("runtime-bin-build-run-next-directory-output");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-run-next-directory-output-source"),
            "v13.1.1",
            "2021.3.33f1",
        );
        let script_path =
            create_fake_unity_script(&root, "run-next-directory-output", ScriptKind::Success);

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-build-run-next-directory-output",
            &repository_url,
        );
        seed_host_native_build_target_with_output_kind(
            &connection,
            repository_id,
            "windows-player",
            "windows",
            "Builder.PerformWindows",
            &script_path,
            "directory",
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v13.1.1",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let output = run_build_run_next_command(&[], &config, &storage)
            .expect("build run-next should succeed for non-archive output");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build run-next output should decode");

        assert_eq!(record.status, "succeeded");

        let workspace_path = PathBuf::from(
            record
                .workspace_path
                .clone()
                .expect("workspace path should persist"),
        );
        let log_path = PathBuf::from(record.log_path.clone().expect("log path should persist"));
        let validate_log_path = workspace_path
            .join("logs")
            .join("01-validate-build-context.log");
        let checkout_log_path = workspace_path
            .join("logs")
            .join("02-checkout-repository.log");
        let unity_log_path = workspace_path.join("logs").join("03-unity-build.log");
        let register_log_path = workspace_path
            .join("logs")
            .join("04-register-artifacts.log");

        assert_eq!(log_path, unity_log_path);
        assert!(!validate_log_path.exists());
        assert!(!checkout_log_path.exists());
        assert!(!unity_log_path.exists());
        assert!(!register_log_path.exists());
        assert!(!workspace_path.join("logs").exists());
        assert!(build_execution_logs_archive_path(&workspace_path).is_file());
        assert!(build_execution_report_path(&workspace_path).is_file());

        let stages = runtime_store::LocalCoordinator::new(&storage)
            .list_build_run_stages(record.id)
            .expect("build stages should load");
        assert_eq!(stages.len(), 4);
        assert_eq!(
            stages
                .iter()
                .map(|stage| (stage.position, stage.step_key.clone(), stage.log_path.clone()))
                .collect::<Vec<_>>(),
            vec![
                (
                    1,
                    String::from("validate-build-context"),
                    validate_log_path.display().to_string(),
                ),
                (
                    2,
                    String::from("checkout-repository"),
                    checkout_log_path.display().to_string(),
                ),
                (
                    3,
                    String::from("unity-build"),
                    unity_log_path.display().to_string(),
                ),
                (
                    4,
                    String::from("register-artifacts"),
                    register_log_path.display().to_string(),
                ),
            ]
        );

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn resolve_runtime_build_execution_plan_with_profile_injects_discovered_editor() {
        let root = test_root("runtime-bin-resolve-build-plan");
        fs::create_dir_all(&root).expect("test root should create");
        let script_path = create_fake_unity_script(&root, "resolved-runner", ScriptKind::Success);
        let platform = HostPlatform::current();
        let plan = BuildExecutionPlan {
            build_run_id: 1,
            release_run_id: 2,
            repository_id: 3,
            repository_name: String::from("revolutions"),
            repository_credentials_id: None,
            workspace_root_override: None,
            artifacts_root_override: None,
            build_target_id: 4,
            repository_url: String::from("https://example.com/revolutions.git"),
            git_tag: String::from("v1.0.0"),
            git_commit: Some(String::from("deadbeef")),
            target_name: String::from("windows-player"),
            platform: String::from("windows"),
            runner_type: String::from("host-native"),
            build_method: Some(String::from("Builder.PerformWindows")),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Players")),
            config_json: String::from("{}"),
            unity_version: String::from("2021.3.33f1"),
            image_ref: String::new(),
            timeout_seconds: 900,
            status: String::from("queued"),
        };
        let capability_profile = test_host_capability_profile(
            platform,
            vec![DiscoveredUnityEditor {
                version: String::from("2021.3.33f1"),
                source: String::from("unity-hub"),
                install_root_path: root.display().to_string(),
                executable_path: script_path.display().to_string(),
                executable_exists: true,
                executable_is_file: true,
                supported_build_targets: vec![String::from("windows")],
                status: String::from("ready"),
                message: String::from("ready"),
            }],
        );

        let resolved = resolve_runtime_build_execution_plan_with_profile(
            &plan,
            &capability_profile,
        )
        .expect("runtime build plan should resolve with discovered editor");

        assert_eq!(
            resolved.runner_type,
            String::from(selected_host_runner_family_label(platform))
        );
        let resolved_config: serde_json::Value = serde_json::from_str(&resolved.config_json)
            .expect("resolved config should decode");
        assert_eq!(
            resolved_config
                .get("unity_executable_path")
                .and_then(serde_json::Value::as_str),
            Some(script_path.display().to_string().as_str())
        );

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn build_run_next_command_fails_when_no_artifacts_are_produced() {
        let root = test_root("runtime-bin-build-run-next-no-artifacts");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-run-next-no-artifacts-source"),
            "v13.0.1",
            "2021.3.33f1",
        );
        let script_path = create_fake_unity_script(&root, "run-next-no-artifacts", ScriptKind::NoArtifact);

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-build-run-next-no-artifacts",
            &repository_url,
        );
        seed_host_native_build_target(
            &connection,
            repository_id,
            "windows-player",
            "windows",
            "Builder.PerformWindows",
            &script_path,
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v13.0.1",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let output = run_build_run_next_command(&[], &config, &storage)
            .expect("build run-next should persist a failed run when no artifacts exist");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build run-next output should decode");

        assert_eq!(record.status, "failed");
        assert!(record
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("expected Unity archive source directory"));

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(artifact_count_for_build_run(&connection, record.id), 0);
        assert_eq!(publish_run_count_for_build_run(&connection, record.id), 0);
        assert_eq!(queue_message_count(&connection, "build-runs"), 0);
        assert_eq!(queue_message_count(&connection, "publish-runs"), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_run_next_command_cancels_timed_out_host_native_build() {
        let root = test_root("runtime-bin-build-run-next-timeout");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-run-next-timeout-source"),
            "v13.0.2",
            "2021.3.33f1",
        );
        let script_path = create_fake_unity_script(&root, "run-next-timeout", ScriptKind::Slow);

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-build-run-next-timeout",
            &repository_url,
        );
        seed_host_native_build_target_with_timeout(
            &connection,
            repository_id,
            "linux-player",
            "linux",
            "Builder.PerformLinux",
            &script_path,
            1,
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v13.0.2",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let output = run_build_run_next_command(&[], &config, &storage)
            .expect("build run-next should persist a canceled run on timeout");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build run-next output should decode");

        assert_eq!(record.status, "canceled");
        assert!(record
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("timeout: host-native unity runner exceeded 1s timeout"));

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(artifact_count_for_build_run(&connection, record.id), 0);
        assert_eq!(publish_run_count_for_build_run(&connection, record.id), 0);
        assert_eq!(queue_message_count(&connection, "build-runs"), 0);
        assert_eq!(queue_message_count(&connection, "publish-runs"), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_run_next_command_persists_failed_host_native_build() {
        let root = test_root("runtime-bin-build-run-next-fail");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-run-next-fail-source"),
            "v13.1.0",
            "2021.3.33f1",
        );
        let script_path = create_fake_unity_script(&root, "run-next-fail", ScriptKind::Failure);

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-build-run-next-fail",
            &repository_url,
        );
        seed_host_native_build_target(
            &connection,
            repository_id,
            "windows-player",
            "windows",
            "Builder.PerformWindows",
            &script_path,
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v13.1.0",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let output = run_build_run_next_command(&[], &config, &storage)
            .expect("build run-next command should persist a failed run");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build run-next output should decode");

        assert_eq!(record.status, "failed");
        assert!(record
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("No valid Unity Editor license found. Please activate your license."));

        let log_path = PathBuf::from(record.log_path.clone().expect("log path should persist"));
        let workspace_path = PathBuf::from(
            record
                .workspace_path
                .clone()
                .expect("workspace path should persist"),
        );
        let workspace_name = workspace_path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("workspace directory name should exist");
        let archive_path = build_execution_logs_archive_path(&workspace_path);
        let log_contents = read_archive_entry(
            &archive_path,
            &format!("{workspace_name}/logs/03-unity-build.log"),
        );
        assert!(!log_path.exists());
        assert!(log_contents.contains("No valid Unity Editor license found. Please activate your license."));
        assert_eq!(load_build_execution_report(&workspace_path).cleanup.status, "completed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(queue_message_count(&connection, "build-runs"), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_run_next_command_retries_package_cache_rename_failure_in_fresh_workspace() {
        let root = test_root("runtime-bin-build-run-next-retry-package-cache");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-run-next-retry-package-cache-source"),
            "v13.2.0",
            "2021.3.33f1",
        );
        let script_path = create_fake_unity_script(
            &root,
            "run-next-retry-package-cache",
            ScriptKind::PackageCacheRetrySuccess,
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-build-run-next-retry-package-cache",
            &repository_url,
        );
        seed_host_native_build_target(
            &connection,
            repository_id,
            "windows-player",
            "windows",
            "Builder.PerformWindows",
            &script_path,
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v13.2.0",
            "2021.3.33f1",
        );
        drop(connection);

        run_release_plan_command(
            &[
                String::from("--release-run-id"),
                release_run_id.to_string(),
            ],
            &storage,
        )
        .expect("release plan command should succeed");

        let output = run_build_run_next_command(&[], &config, &storage)
            .expect("build run-next should retry package cache failures once");
        let record: BuildRunRecord =
            serde_json::from_str(&output).expect("build run-next output should decode");

        assert_eq!(record.status, "succeeded");
        let workspace_path = PathBuf::from(
            record
                .workspace_path
                .clone()
                .expect("workspace path should persist"),
        );
        let log_path = PathBuf::from(record.log_path.clone().expect("log path should persist"));
        assert!(workspace_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .starts_with(&format!("build-run-{}-attempt-", record.id)));
        assert_eq!(log_path, workspace_path.join("logs").join("03-unity-build.log"));
        assert!(!log_path.exists());
        assert!(!workspace_path.join("logs").exists());
        let report = load_build_execution_report(&workspace_path);
        assert_eq!(report.attempts.len(), 2);
        assert!(report.attempts.iter().any(|attempt| attempt.removed_after_cleanup));
        assert!(build_execution_logs_archive_path(&workspace_path).is_file());

        let state_path = root.join("run-next-retry-package-cache.state");
        let attempts = fs::read_to_string(&state_path).expect("retry state file should exist");
        assert_eq!(attempts.trim(), "2");

        let artifact_path = config
            .directories
            .artifacts_dir
            .join("runtime-bin-build-run-next-retry-package-cache.v13.2.0")
            .join("runtime-bin-build-run-next-retry-package-cache.v13.2.0.windows-player.zip");
        assert!(artifact_path.is_file());

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn queue_lease_renewer_keeps_claimed_message_leased_until_acknowledged() {
        let root = test_root("runtime-bin-queue-lease-renewer");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let coordinator = LocalCoordinator::new(&storage);
        coordinator
            .enqueue("build-runs", br#"{"build_run_id":1}"#)
            .expect("queue message should enqueue");
        let lease_ttl = Duration::from_millis(60);
        let message = coordinator
            .claim_next(
                "build-runs",
                "queue-lease-renewer-test",
                Duration::ZERO,
                lease_ttl,
            )
            .expect("queue claim should succeed")
            .expect("queue claim should return one message");
        let lease_renewer = QueueLeaseRenewer::spawn(
            coordinator.clone(),
            message.id,
            message.lease_token.clone(),
            lease_ttl,
            "test queue message",
        );

        std::thread::sleep(Duration::from_millis(180));

        assert!(coordinator
            .claim_next(
                "build-runs",
                "queue-lease-renewer-test-observer",
                Duration::ZERO,
                lease_ttl,
            )
            .expect("observer claim should succeed")
            .is_none());

        lease_renewer.stop();
        assert!(coordinator
            .acknowledge_message(message.id, &message.lease_token)
            .expect("acknowledge should succeed"));
        lease_renewer
            .finish()
            .expect("queue lease renewer should stop cleanly");

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn recover_interrupted_build_attempts_cleans_workspace_and_persists_requested_trace() {
        let root = test_root("runtime-bin-interrupted-build-cleanup");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-interrupted-build-cleanup-source"),
            "v16.0.0",
            "2021.3.33f1",
        );
        let script_path =
            create_fake_unity_script(&root, "interrupted-build-cleanup", ScriptKind::Success);
        let runs_root = root.join("managed-workspace").join("runs");
        let interrupted_workspace = runs_root.join("build-run-1-attempt-111-1");
        let prior_attempt_workspace = runs_root.join("build-run-1-attempt-110-1");
        let interrupted_logs = interrupted_workspace.join("logs");
        let prior_logs = prior_attempt_workspace.join("logs");
        fs::create_dir_all(interrupted_workspace.join("source"))
            .expect("interrupted source directory should create");
        fs::create_dir_all(&interrupted_logs)
            .expect("interrupted logs directory should create");
        fs::create_dir_all(prior_attempt_workspace.join("source"))
            .expect("prior source directory should create");
        fs::create_dir_all(&prior_logs)
            .expect("prior logs directory should create");
        fs::write(
            interrupted_logs.join("02-checkout-repository.log"),
            "checking out repository\n",
        )
        .expect("interrupted checkout log should write");
        fs::write(
            prior_logs.join("01-validate-build-context.log"),
            "validated build context\n",
        )
        .expect("prior validation log should write");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-interrupted-build-cleanup",
            &repository_url,
        );
        let build_target_id = seed_host_native_build_target(
            &connection,
            repository_id,
            "windows-player",
            "windows",
            "Builder.PerformWindows",
            &script_path,
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v16.0.0",
            "2021.3.33f1",
        );
        let build_run_id = seed_requeued_build_run(
            &connection,
            release_run_id,
            build_target_id,
            "2021.3.33f1",
            "host-native",
            "checkout-repository",
            "Checkout Repository",
            "failed",
            "build attempt interrupted after a requested runtime shutdown",
        );
        connection
            .execute(
                "
                INSERT INTO build_run_steps (
                    build_run_id,
                    position,
                    step_key,
                    step_label,
                    status,
                    log_path,
                    last_message,
                    heartbeat_at,
                    started_at,
                    finished_at,
                    error_message,
                    updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, ?, CURRENT_TIMESTAMP)
                ",
                params![
                    build_run_id,
                    2,
                    "checkout-repository",
                    "Checkout Repository",
                    "failed",
                    interrupted_logs
                        .join("02-checkout-repository.log")
                        .display()
                        .to_string(),
                    "build attempt interrupted after a requested runtime shutdown",
                    "build attempt interrupted after a requested runtime shutdown",
                ],
            )
            .expect("interrupted stage record should insert");
        drop(connection);

        let coordinator = LocalCoordinator::new(&storage);
        recover_interrupted_build_attempts(
            &coordinator,
            &RuntimeRecoveryReport {
                interrupted_builds: vec![InterruptedBuildRecoveryRecord {
                    build_run_id,
                    workspace_path: interrupted_workspace.display().to_string(),
                    log_path: Some(
                        interrupted_logs
                            .join("02-checkout-repository.log")
                            .display()
                            .to_string(),
                    ),
                    interruption_kind: String::from(RECOVERY_INTERRUPTION_KIND_REQUESTED),
                    interruption_message: String::from(
                        "build attempt interrupted after a requested runtime shutdown",
                    ),
                }],
                ..RuntimeRecoveryReport::default()
            },
        );

        let report = load_build_execution_report(&interrupted_workspace);
        assert_eq!(report.cleanup.status, "completed");
        assert_eq!(report.cleanup.trigger, "requested_interruption");
        assert_eq!(
            report
                .interruption
                .as_ref()
                .map(|interruption| interruption.kind.as_str()),
            Some("requested_shutdown")
        );
        assert_eq!(
            report
                .interruption
                .as_ref()
                .map(|interruption| interruption.message.as_str()),
            Some("build attempt interrupted after a requested runtime shutdown")
        );
        assert!(build_execution_logs_archive_path(&interrupted_workspace).is_file());
        assert!(!interrupted_workspace.join("source").exists());
        assert!(!interrupted_workspace.join("logs").exists());
        assert!(!prior_attempt_workspace.exists());
        assert!(report.attempts.iter().any(|attempt| {
            attempt.workspace_path == interrupted_workspace.display().to_string()
                && attempt.is_final_workspace
        }));
        assert!(report.attempts.iter().any(|attempt| {
            attempt.workspace_path == prior_attempt_workspace.display().to_string()
                && attempt.removed_after_cleanup
        }));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn publish_run_next_command_completes_filesystem_publish() {
        let root = test_root("runtime-bin-publish-run-next-success");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let artifact_root = root.join("publish-artifacts");
        let publish_root = root.join("published-artifacts");
        let workspace_path = config.directories.runs_dir.join("publish-run-report-success");
        fs::create_dir_all(artifact_root.join("nested"))
            .expect("artifact directory should create");
        let source_path = artifact_root.join("nested").join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(&connection, "runtime-bin-publish-run-next-success");
        let build_target_id = seed_build_target(&connection, repository_id, "windows-player", "windows");
        let release_run_id = seed_queued_release(&connection, repository_id, "v14.0.0", "2021.3.33f1");
        let build_run_id = seed_succeeded_build_run_with_workspace(
            &connection,
            release_run_id,
            build_target_id,
            &artifact_root,
            &workspace_path,
            "2021.3.33f1",
            "host-native",
        );
        let artifact_id = insert_artifact_record(
            &connection,
            build_run_id,
            "nested/game.zip",
            "archive",
            "nested/game.zip",
        );
        let publish_target_id = seed_publish_target_with_config(
            &connection,
            repository_id,
            "filesystem-release",
            "filesystem",
            &json!({"root_path": publish_root.display().to_string()}).to_string(),
        );
        let publish_run_id = insert_publish_run_record(
            &connection,
            release_run_id,
            build_run_id,
            publish_target_id,
            artifact_id,
            "queued",
        );
        drop(connection);

        runtime_store::LocalCoordinator::new(&storage)
            .dispatch_publish_run(publish_run_id)
            .expect("publish run should dispatch");

        let output = run_publish_run_next_command(&[], &config, &storage)
            .expect("publish run-next command should succeed");
        let record: PublishRunRecord =
            serde_json::from_str(&output).expect("publish run-next output should decode");

        assert_eq!(record.status, "succeeded");
        let destination_path = publish_root
            .join("runtime-bin-publish-run-next-success")
            .join("v14.0.0")
            .join("nested")
            .join("game.zip");
        let destination_ref = destination_path.display().to_string();
        assert_eq!(
            record.destination_ref.as_deref(),
            Some(destination_ref.as_str())
        );
        assert_eq!(
            fs::read_to_string(destination_path).expect("published artifact should exist"),
            "artifact"
        );
        let report = load_build_execution_report(&workspace_path);
        assert_eq!(report.publish_runs.len(), 1);
        assert_eq!(report.publish_runs[0].record.status, "succeeded");
        assert_eq!(
            report.publish_runs[0].record.destination_ref.as_deref(),
            Some(destination_ref.as_str())
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(queue_message_count(&connection, "publish-runs"), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn publish_inspect_command_reports_persisted_destination_status() {
        let root = test_root("runtime-bin-publish-inspect");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let artifact_root = root.join("publish-inspect-artifacts");
        let publish_root = root.join("publish-inspect-output");
        fs::create_dir_all(artifact_root.join("nested"))
            .expect("artifact directory should create");
        let source_path = artifact_root.join("nested").join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(&connection, "runtime-bin-publish-inspect");
        let build_target_id = seed_build_target(&connection, repository_id, "windows-player", "windows");
        let release_run_id = seed_queued_release(&connection, repository_id, "v15.0.0", "2021.3.33f1");
        let build_run_id = seed_succeeded_build_run(&connection, release_run_id, build_target_id, &artifact_root);
        let artifact_id = insert_artifact_record(
            &connection,
            build_run_id,
            "nested/game.zip",
            "archive",
            "nested/game.zip",
        );
        let publish_target_id = seed_publish_target_with_config(
            &connection,
            repository_id,
            "filesystem-release",
            "filesystem",
            &json!({"root_path": publish_root.display().to_string()}).to_string(),
        );
        let publish_run_id = insert_publish_run_record(
            &connection,
            release_run_id,
            build_run_id,
            publish_target_id,
            artifact_id,
            "queued",
        );
        drop(connection);

        runtime_store::LocalCoordinator::new(&storage)
            .dispatch_publish_run(publish_run_id)
            .expect("publish run should dispatch");

        let publish_output = run_publish_run_next_command(&[], &config, &storage)
            .expect("publish run-next command should succeed");
        let record: PublishRunRecord =
            serde_json::from_str(&publish_output).expect("publish run-next output should decode");
        let destination_ref = record
            .destination_ref
            .clone()
            .expect("destination ref should persist");
        let destination_path = PathBuf::from(&destination_ref);

        let inspect_output = run_publish_inspect_command(
            &[
                String::from("--publish-run-id"),
                publish_run_id.to_string(),
            ],
            &storage,
        )
        .expect("publish inspect command should succeed for one publish run");
        let inspect_report: PublishedOutputInspectionReport = serde_json::from_str(&inspect_output)
            .expect("publish inspect output should decode");

        assert_eq!(inspect_report.requested_publish_run_id, Some(publish_run_id));
        assert_eq!(inspect_report.requested_build_run_id, None);
        assert_eq!(inspect_report.publish_runs.len(), 1);
        let diagnostic = &inspect_report.publish_runs[0];
        assert_eq!(diagnostic.publish_run_id, publish_run_id);
        assert_eq!(diagnostic.build_run_id, build_run_id);
        assert!(diagnostic.destination_exists);
        assert!(diagnostic.destination_is_file);
        assert_eq!(diagnostic.destination_size_bytes, Some(8));
        assert_eq!(diagnostic.destination_ref.as_deref(), Some(destination_ref.as_str()));
        assert_eq!(
            diagnostic.expected_destination_ref.as_deref(),
            Some(destination_ref.as_str())
        );
        assert_eq!(
            diagnostic.publish_target_name.as_deref(),
            Some("filesystem-release")
        );
        assert_eq!(
            diagnostic.artifact_path.as_deref(),
            Some("nested/game.zip")
        );
        assert!(diagnostic.destination_error.is_none());
        assert!(diagnostic.expected_destination_error.is_none());
        assert!(diagnostic.plan_error.is_none());

        fs::remove_file(&destination_path).expect("published artifact should be removable");

        let build_inspect_output = run_publish_inspect_command(
            &[
                String::from("--build-run-id"),
                build_run_id.to_string(),
            ],
            &storage,
        )
        .expect("publish inspect command should succeed for one build run");
        let build_inspect_report: PublishedOutputInspectionReport =
            serde_json::from_str(&build_inspect_output)
                .expect("build publish inspect output should decode");

        assert_eq!(build_inspect_report.requested_build_run_id, Some(build_run_id));
        assert_eq!(build_inspect_report.requested_publish_run_id, None);
        assert_eq!(build_inspect_report.publish_runs.len(), 1);
        let diagnostic = &build_inspect_report.publish_runs[0];
        assert!(!diagnostic.destination_exists);
        assert!(!diagnostic.destination_is_file);
        assert_eq!(diagnostic.destination_size_bytes, None);
        assert!(diagnostic
            .destination_error
            .as_deref()
            .unwrap_or_default()
            .contains("was not found"));
        assert_eq!(
            diagnostic.expected_destination_ref.as_deref(),
            Some(destination_ref.as_str())
        );

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn publish_run_next_command_persists_failed_publish() {
        let root = test_root("runtime-bin-publish-run-next-fail");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let artifact_root = root.join("publish-artifacts-fail");
        let workspace_path = config.directories.runs_dir.join("publish-run-report-fail");
        fs::create_dir_all(&artifact_root).expect("artifact directory should create");
        let source_path = artifact_root.join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository(&connection, "runtime-bin-publish-run-next-fail");
        let build_target_id = seed_build_target(&connection, repository_id, "linux-player", "linux");
        let release_run_id = seed_queued_release(&connection, repository_id, "v14.1.0", "2021.3.33f1");
        let build_run_id = seed_succeeded_build_run_with_workspace(
            &connection,
            release_run_id,
            build_target_id,
            &artifact_root,
            &workspace_path,
            "2021.3.33f1",
            "host-native",
        );
        let artifact_id = insert_artifact_record(
            &connection,
            build_run_id,
            "game.zip",
            "archive",
            "game.zip",
        );
        let publish_target_id = seed_publish_target_with_config(
            &connection,
            repository_id,
            "filesystem-release",
            "filesystem",
            r#"{"root_path":"relative-output"}"#,
        );
        let publish_run_id = insert_publish_run_record(
            &connection,
            release_run_id,
            build_run_id,
            publish_target_id,
            artifact_id,
            "queued",
        );
        drop(connection);

        runtime_store::LocalCoordinator::new(&storage)
            .dispatch_publish_run(publish_run_id)
            .expect("publish run should dispatch");

        let output = run_publish_run_next_command(&[], &config, &storage)
            .expect("publish run-next command should persist a failed run");
        let record: PublishRunRecord =
            serde_json::from_str(&output).expect("publish run-next output should decode");

        assert_eq!(record.status, "failed");
        assert!(record
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("root_path must be absolute"));
        let report = load_build_execution_report(&workspace_path);
        assert_eq!(report.publish_runs.len(), 1);
        assert_eq!(report.publish_runs[0].record.status, "failed");
        assert!(report.publish_runs[0]
            .record
            .error_message
            .as_deref()
            .unwrap_or_default()
            .contains("root_path must be absolute"));

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(queue_message_count(&connection, "publish-runs"), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    fn seed_repository(connection: &Connection, name: &str) -> i64 {
        seed_repository_with_url(
            connection,
            name,
            &format!("https://example.com/{name}.git"),
        )
    }

    fn seed_repository_with_url(
        connection: &Connection,
        name: &str,
        repository_url: &str,
    ) -> i64 {
        seed_repository_with_url_and_credentials(connection, name, repository_url, None)
    }

    fn seed_repository_with_url_and_credentials(
        connection: &Connection,
        name: &str,
        repository_url: &str,
        credentials_id: Option<i64>,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, credentials_id) VALUES (?, ?, ?)",
                params![name, repository_url, credentials_id],
            )
            .expect("repository should insert");

        connection.last_insert_rowid()
    }

    fn seed_credentials(
        connection: &Connection,
        name: &str,
        kind: &str,
        config_json: &str,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![name, kind, config_json],
            )
            .expect("credentials should insert");

        connection.last_insert_rowid()
    }

    fn seed_build_target(
        connection: &Connection,
        repository_id: i64,
        name: &str,
        platform: &str,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO build_targets (repository_id, name, platform) VALUES (?, ?, ?)",
                params![repository_id, name, platform],
            )
            .expect("build target should insert");

        connection.last_insert_rowid()
    }

    fn seed_host_native_build_target(
        connection: &Connection,
        repository_id: i64,
        name: &str,
        platform: &str,
        build_method: &str,
        script_path: &Path,
    ) -> i64 {
        seed_host_native_build_target_with_timeout(
            connection,
            repository_id,
            name,
            platform,
            build_method,
            script_path,
            900,
        )
    }

    fn seed_host_native_build_target_with_output_kind(
        connection: &Connection,
        repository_id: i64,
        name: &str,
        platform: &str,
        build_method: &str,
        script_path: &Path,
        output_kind: &str,
    ) -> i64 {
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    output_kind,
                    output_path_template,
                    timeout_seconds,
                    config_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    name,
                    platform,
                    "host-native",
                    build_method,
                    output_kind,
                    "Builds/Players",
                    900,
                    json!({
                        "unity_executable_path": script_path.display().to_string()
                    })
                    .to_string(),
                ],
            )
            .expect("host-native build target should insert");

        connection.last_insert_rowid()
    }

    fn seed_host_native_build_target_with_timeout(
        connection: &Connection,
        repository_id: i64,
        name: &str,
        platform: &str,
        build_method: &str,
        script_path: &Path,
        timeout_seconds: i64,
    ) -> i64 {
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    platform,
                    runner_type,
                    build_method,
                    output_kind,
                    output_path_template,
                    timeout_seconds,
                    config_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    name,
                    platform,
                    "host-native",
                    build_method,
                    "archive",
                    "Builds/Players",
                    timeout_seconds,
                    json!({
                        "unity_executable_path": script_path.display().to_string()
                    })
                    .to_string(),
                ],
            )
            .expect("host-native build target should insert");

        connection.last_insert_rowid()
    }

    fn seed_publish_target(
        connection: &Connection,
        repository_id: i64,
        name: &str,
        kind: &str,
    ) -> i64 {
        seed_publish_target_with_config(connection, repository_id, name, kind, "{}")
    }

    fn seed_publish_target_with_config(
        connection: &Connection,
        repository_id: i64,
        name: &str,
        kind: &str,
        config_json: &str,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind, config_json) VALUES (?, ?, ?, ?)",
                params![repository_id, name, kind, config_json],
            )
            .expect("publish target should insert");

        connection.last_insert_rowid()
    }

    fn seed_succeeded_build_run(
        connection: &Connection,
        release_run_id: i64,
        build_target_id: i64,
        artifact_root_path: &Path,
    ) -> i64 {
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    status,
                    artifact_root_path,
                    finished_at
                ) VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "succeeded",
                    artifact_root_path.display().to_string(),
                ],
            )
            .expect("succeeded build run should insert");

        connection.last_insert_rowid()
    }

    fn seed_succeeded_build_run_with_workspace(
        connection: &Connection,
        release_run_id: i64,
        build_target_id: i64,
        artifact_root_path: &Path,
        workspace_path: &Path,
        unity_version: &str,
        image_ref: &str,
    ) -> i64 {
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
                    artifact_root_path,
                    finished_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    unity_version,
                    image_ref,
                    "succeeded",
                    workspace_path.display().to_string(),
                    artifact_root_path.display().to_string(),
                ],
            )
            .expect("succeeded build run with workspace should insert");

        connection.last_insert_rowid()
    }

    fn seed_requeued_build_run(
        connection: &Connection,
        release_run_id: i64,
        build_target_id: i64,
        unity_version: &str,
        image_ref: &str,
        current_stage_key: &str,
        current_stage_label: &str,
        current_stage_status: &str,
        last_progress_message: &str,
    ) -> i64 {
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    unity_version,
                    image_ref,
                    status,
                    current_stage_key,
                    current_stage_label,
                    current_stage_status,
                    last_progress_message
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    unity_version,
                    image_ref,
                    "queued",
                    current_stage_key,
                    current_stage_label,
                    current_stage_status,
                    last_progress_message,
                ],
            )
            .expect("requeued build run should insert");

        connection.last_insert_rowid()
    }

    fn insert_artifact_record(
        connection: &Connection,
        build_run_id: i64,
        name: &str,
        kind: &str,
        path: &str,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO artifacts (build_run_id, name, kind, path) VALUES (?, ?, ?, ?)",
                params![build_run_id, name, kind, path],
            )
            .expect("artifact record should insert");

        connection.last_insert_rowid()
    }

    fn insert_publish_run_record(
        connection: &Connection,
        release_run_id: i64,
        build_run_id: i64,
        publish_target_id: i64,
        artifact_id: i64,
        status: &str,
    ) -> i64 {
        connection
            .execute(
                "
                INSERT INTO publish_runs (
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    status
                ) VALUES (?, ?, ?, ?, ?)
                ",
                params![release_run_id, build_run_id, publish_target_id, artifact_id, status],
            )
            .expect("publish run should insert");

        connection.last_insert_rowid()
    }

    fn seed_build_publish_binding(
        connection: &Connection,
        build_target_id: i64,
        publish_target_id: i64,
    ) -> i64 {
        connection
            .execute(
                "
                INSERT INTO build_publish_bindings (
                    build_target_id,
                    publish_target_id,
                    enabled,
                    options_json
                ) VALUES (?, ?, ?, ?)
                ",
                params![build_target_id, publish_target_id, 1, "{}"],
            )
            .expect("build publish binding should insert");

        connection.last_insert_rowid()
    }

    fn seed_queued_release(
        connection: &Connection,
        repository_id: i64,
        git_tag: &str,
        unity_version: &str,
    ) -> i64 {
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    trigger_source,
                    source_metadata_json,
                    unity_version,
                    status
                ) VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    git_tag,
                    "manual",
                    "{}",
                    unity_version,
                    "queued",
                ],
            )
            .expect("queued release should insert");

        connection.last_insert_rowid()
    }

    fn seed_manual_release_for_rebuild(
        connection: &Connection,
        repository_id: i64,
        git_tag: &str,
        unity_version: &str,
    ) -> i64 {
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    trigger_source,
                    source_metadata_json,
                    unity_version,
                    status,
                    started_at,
                    finished_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                ",
                params![
                    repository_id,
                    git_tag,
                    "cafebabe",
                    "manual",
                    r#"{"requested_via":"runtime-bin"}"#,
                    unity_version,
                    "succeeded",
                ],
            )
            .expect("manual release rebuild fixture should insert");

        connection.last_insert_rowid()
    }

    fn queue_message_count(connection: &Connection, queue_name: &str) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(1) FROM worker_queue_messages WHERE queue_name = ?",
                [queue_name],
                |row| row.get(0),
            )
            .expect("queue message count should load")
    }

    fn artifact_count_for_build_run(connection: &Connection, build_run_id: i64) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(1) FROM artifacts WHERE build_run_id = ?",
                [build_run_id],
                |row| row.get(0),
            )
            .expect("artifact count should load")
    }

    fn publish_run_count_for_build_run(connection: &Connection, build_run_id: i64) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(1) FROM publish_runs WHERE build_run_id = ?",
                [build_run_id],
                |row| row.get(0),
            )
            .expect("publish run count should load")
    }

    fn load_repository_last_seen_tag(connection: &Connection, repository_id: i64) -> Option<String> {
        connection
            .query_row(
                "SELECT last_seen_tag FROM repositories WHERE id = ?",
                [repository_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .expect("repository last seen tag should load")
    }

    fn release_tags_for_repository(connection: &Connection, repository_id: i64) -> Vec<String> {
        connection
            .prepare(
                "
                SELECT git_tag
                FROM release_runs
                WHERE repository_id = ?
                ORDER BY id ASC
                ",
            )
            .expect("release tag query should prepare")
            .query_map([repository_id], |row| row.get::<_, String>(0))
            .expect("release tag query should execute")
            .collect::<Result<Vec<_>, _>>()
            .expect("release tags should collect")
    }

    fn create_unity_repository_with_tags(
        repository_path: &Path,
        unity_version: &str,
        git_tags: &[&str],
    ) -> String {
        if repository_path.exists() {
            std::fs::remove_dir_all(repository_path)
                .expect("existing repository fixture should be removable");
        }
        std::fs::create_dir_all(repository_path.join("ProjectSettings"))
            .expect("project settings directory should create");
        std::fs::write(
            repository_path.join("ProjectSettings/ProjectVersion.txt"),
            format!("m_EditorVersion: {unity_version}\n"),
        )
        .expect("project version file should write");

        run_git_test_command(repository_path, &["init"]);
        run_git_test_command(
            repository_path,
            &["config", "user.name", "runtime-bin-tests"],
        );
        run_git_test_command(
            repository_path,
            &["config", "user.email", "runtime-bin-tests@example.com"],
        );
        run_git_test_command(repository_path, &["add", "."]);
        run_git_test_command(repository_path, &["commit", "-m", "seed unity version"]);
        for git_tag in git_tags {
            run_git_test_command(repository_path, &["tag", git_tag]);
        }

        repository_path.display().to_string()
    }

    fn create_tagged_unity_repository(
        repository_path: &Path,
        git_tag: &str,
        unity_version: &str,
    ) -> String {
        create_unity_repository_with_tags(repository_path, unity_version, &[git_tag])
    }

    fn current_git_branch_name(repository_path: &Path) -> String {
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(repository_path)
            .output()
            .expect("git branch --show-current should spawn");
        if !output.status.success() {
            panic!(
                "git branch --show-current failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }

        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn current_git_head_commit(repository_path: &Path) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repository_path)
            .output()
            .expect("git rev-parse HEAD should spawn");
        if !output.status.success() {
            panic!(
                "git rev-parse HEAD failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }

        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn run_git_test_command(working_dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(working_dir)
            .output()
            .expect("git test command should spawn");
        if output.status.success() {
            return;
        }

        panic!(
            "git {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn create_fake_unity_script(root: &Path, name: &str, kind: ScriptKind) -> PathBuf {
        let script_path = if cfg!(windows) {
            root.join(format!("{name}.cmd"))
        } else {
            root.join(format!("{name}.sh"))
        };
        let state_path = root.join(format!("{name}.state"));
        let contents = match kind {
            ScriptKind::Success if cfg!(windows) => String::from(
                "@echo off\r\nset \"HGB_OUTPUT_IS_FILE=0\"\r\nfor %%I in (\"%HGB_OUTPUT_PATH%\") do (set \"HGB_OUTPUT_DIR=%%~dpI\" & set \"HGB_OUTPUT_EXT=%%~xI\")\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".zip\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".exe\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".x86_64\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".app\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".apk\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".aab\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif not exist \"%HGB_OUTPUT_DIR%\" mkdir \"%HGB_OUTPUT_DIR%\"\r\necho args:%*\r\nif \"%HGB_OUTPUT_IS_FILE%\"==\"1\" (\r\n  > \"%HGB_OUTPUT_PATH%\" echo artifact\r\n) else (\r\n  if not exist \"%HGB_OUTPUT_PATH%\" mkdir \"%HGB_OUTPUT_PATH%\"\r\n  > \"%HGB_OUTPUT_PATH%\\artifact.txt\" echo artifact\r\n)\r\nexit /B 0\r\n",
            ),
            ScriptKind::NoArtifact if cfg!(windows) => String::from(
                "@echo off\r\nfor %%I in (\"%HGB_OUTPUT_PATH%\") do set \"HGB_OUTPUT_DIR=%%~dpI\"\r\nif not exist \"%HGB_OUTPUT_DIR%\" mkdir \"%HGB_OUTPUT_DIR%\"\r\necho args:%*\r\nexit /B 0\r\n",
            ),
            ScriptKind::Failure if cfg!(windows) => String::from(
                "@echo off\r\n> \"%HGB_LOG_PATH%\" echo No valid Unity Editor license found. Please activate your license.\r\nexit /B 9\r\n",
            ),
            ScriptKind::Slow if cfg!(windows) => String::from(
                "@echo off\r\necho args:%*\r\npowershell -NoProfile -Command \"Start-Sleep -Seconds 3\"\r\nexit /B 0\r\n",
            ),
            ScriptKind::PackageCacheRetrySuccess if cfg!(windows) => format!(
                "@echo off\r\nset \"STATE_FILE={}\"\r\nif exist \"%STATE_FILE%\" (set /p COUNT=<\"%STATE_FILE%\") else (set COUNT=0)\r\nset /a COUNT=%COUNT%+1\r\n> \"%STATE_FILE%\" echo %COUNT%\r\nif %COUNT%==1 (\r\n  > \"%HGB_LOG_PATH%\" echo An error occurred while resolving packages:\r\n  >> \"%HGB_LOG_PATH%\" echo   One or more packages could not be added to the local file system:\r\n  >> \"%HGB_LOG_PATH%\" echo     com.unity.burst: EPERM: operation not permitted, rename 'C:\\tmp\\PackageCache\\.tmp-1\\package' -^> 'C:\\tmp\\PackageCache\\com.unity.burst@6bb9aca3ef38'\r\n  exit /B 1\r\n)\r\nset \"HGB_OUTPUT_IS_FILE=0\"\r\nfor %%I in (\"%HGB_OUTPUT_PATH%\") do (set \"HGB_OUTPUT_DIR=%%~dpI\" & set \"HGB_OUTPUT_EXT=%%~xI\")\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".zip\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".exe\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".x86_64\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".app\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".apk\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".aab\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif not exist \"%HGB_OUTPUT_DIR%\" mkdir \"%HGB_OUTPUT_DIR%\"\r\n> \"%HGB_LOG_PATH%\" echo args:%*\r\n>> \"%HGB_LOG_PATH%\" echo output:%HGB_OUTPUT_PATH%\r\nif \"%HGB_OUTPUT_IS_FILE%\"==\"1\" (\r\n  > \"%HGB_OUTPUT_PATH%\" echo artifact\r\n) else (\r\n  if not exist \"%HGB_OUTPUT_PATH%\" mkdir \"%HGB_OUTPUT_PATH%\"\r\n  > \"%HGB_OUTPUT_PATH%\\artifact.txt\" echo artifact\r\n)\r\nexit /B 0\r\n",
                state_path.display()
            ),
            ScriptKind::Success => String::from(
                "#!/bin/sh\nset -eu\nmkdir -p \"$(dirname \"$HGB_OUTPUT_PATH\")\"\necho \"args:$*\"\ncase \"$HGB_OUTPUT_PATH\" in\n  *.zip|*.exe|*.x86_64|*.app|*.apk|*.aab)\n    printf 'artifact\\n' > \"$HGB_OUTPUT_PATH\"\n    ;;\n  *)\n    mkdir -p \"$HGB_OUTPUT_PATH\"\n    printf 'artifact\\n' > \"$HGB_OUTPUT_PATH/artifact.txt\"\n    ;;\nesac\nexit 0\n",
            ),
            ScriptKind::NoArtifact => String::from(
                "#!/bin/sh\nset -eu\nmkdir -p \"$(dirname \"$HGB_OUTPUT_PATH\")\"\necho \"args:$*\"\nexit 0\n",
            ),
            ScriptKind::Failure => String::from(
                "#!/bin/sh\nset -eu\nprintf 'No valid Unity Editor license found. Please activate your license.\\n' > \"$HGB_LOG_PATH\"\nexit 9\n",
            ),
            ScriptKind::Slow => String::from(
                "#!/bin/sh\nset -eu\necho 'args:$*'\nsleep 3\nexit 0\n",
            ),
            ScriptKind::PackageCacheRetrySuccess => format!(
                "#!/bin/sh\nset -eu\nstate_file=\"{}\"\ncount=0\nif [ -f \"$state_file\" ]; then\n  count=$(cat \"$state_file\")\nfi\ncount=$((count + 1))\nprintf '%s\\n' \"$count\" > \"$state_file\"\nif [ \"$count\" -eq 1 ]; then\n  printf 'An error occurred while resolving packages:\\n' > \"$HGB_LOG_PATH\"\n  printf '  One or more packages could not be added to the local file system:\\n' >> \"$HGB_LOG_PATH\"\n  printf '    com.unity.burst: EPERM: operation not permitted, rename /tmp/PackageCache/.tmp-1/package -> /tmp/PackageCache/com.unity.burst@6bb9aca3ef38\\n' >> \"$HGB_LOG_PATH\"\n  exit 1\nfi\nmkdir -p \"$(dirname \"$HGB_OUTPUT_PATH\")\"\nprintf 'args:%s\\n' \"$*\" > \"$HGB_LOG_PATH\"\nprintf 'output:%s\\n' \"$HGB_OUTPUT_PATH\" >> \"$HGB_LOG_PATH\"\ncase \"$HGB_OUTPUT_PATH\" in\n  *.zip|*.exe|*.x86_64|*.app|*.apk|*.aab)\n    printf 'artifact\\n' > \"$HGB_OUTPUT_PATH\"\n    ;;\n  *)\n    mkdir -p \"$HGB_OUTPUT_PATH\"\n    printf 'artifact\\n' > \"$HGB_OUTPUT_PATH/artifact.txt\"\n    ;;\nesac\nexit 0\n",
                state_path.display()
            ),
        };
        fs::write(&script_path, contents).expect("fake unity script should write");

        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&script_path)
                .expect("fake unity script metadata should load")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script_path, permissions)
                .expect("fake unity script permissions should set");
        }

        script_path
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ScriptKind {
        Success,
        NoArtifact,
        Failure,
        Slow,
        PackageCacheRetrySuccess,
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "handy-unity-builder-runtime-bin-{label}-{}",
            std::process::id()
        ))
    }

    fn test_host_capability_profile(
        platform: HostPlatform,
        discovered_editors: Vec<DiscoveredUnityEditor>,
    ) -> HostCapabilityProfile {
        HostCapabilityProfile {
            platform: String::from(platform.as_str()),
            architecture: String::from("x86_64"),
            packaging_mode: String::from("development"),
            inside_wsl: false,
            git_tool: HostToolCapability {
                name: String::from("Git"),
                available: true,
                path: Some(String::from("git")),
                version: Some(String::from("2.49.0")),
                status: String::from("ready"),
                message: String::from("ready"),
            },
            unity_license: UnityLicenseDiagnostics {
                searched_paths: vec![String::from("C:/ProgramData/Unity/Unity_lic.ulf")],
                resolved_path: Some(String::from("C:/ProgramData/Unity/Unity_lic.ulf")),
                exists: true,
                status: String::from("ready"),
                message: String::from("ready"),
            },
            platform_prerequisites: Vec::new(),
            discovered_editors,
            runner_selection: RunnerSelectionDiagnostics {
                selected_runner_family: Some(String::from(
                    selected_host_runner_family_label(platform),
                )),
                status: String::from("ready"),
                message: String::from("ready"),
            },
        }
    }

    fn selected_host_runner_family_label(platform: HostPlatform) -> &'static str {
        match platform {
            HostPlatform::Windows => "host-windows-unity",
            HostPlatform::MacOS => "host-macos-unity",
            HostPlatform::Linux => "host-linux-unity",
        }
    }
}