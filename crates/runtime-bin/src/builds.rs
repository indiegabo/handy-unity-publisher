//! Encapsulates build dispatch, retained execution reporting, and archive
//! packaging for the current Unity adapter-backed build worker.

use super::*;
use runtime_contracts::BuildKind;
use runtime_runner::{
    BuildExecutionAdapter, DiscoveredArtifact, EngineAdapterRegistry,
    WorkspacePreparationInput, WorkspacePreparationSource,
    unity::{
        package_unity_build_output, resolve_final_unity_artifact_output_path,
        resolve_unity_build_stage_identity, UnityBuildStageIdentity,
    },
};
use runtime_store::{
    CreateArtifactRecordInput, LocalCoordinator, ReleaseSourceMetadata,
};
use runtime_store::lifecycle::ReleaseStatus;
use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(test)]
use runtime_config::RuntimeDirectories;
#[cfg(test)]
use runtime_store::{
    CreateRepositoryProjectBuildTargetInput, CreateRepositoryProjectInput,
    StorageLayout, initialize_database, open_connection,
};

const BUILD_EXECUTION_REPORT_SCHEMA_VERSION: u32 = 2;
const BUILD_EXECUTION_CLEANUP_POLICY: &str = "retain-zipped-logs-json-report";
const BUILD_EXECUTION_CLEANUP_PENDING: &str = "pending";
const BUILD_EXECUTION_CLEANUP_COMPLETED: &str = "completed";
const BUILD_EXECUTION_CLEANUP_FAILED: &str = "failed";
const BUILD_EXECUTION_CLEANUP_TRIGGER_TERMINAL_STATE: &str = "terminal_state";
const BUILD_EXECUTION_CLEANUP_TRIGGER_REQUESTED_INTERRUPTION: &str =
    "requested_interruption";
const BUILD_EXECUTION_CLEANUP_TRIGGER_SYSTEM_INTERRUPTION: &str =
    "system_interruption";
const BUILD_EXECUTION_RETAINED_DIR_NAME: &str = "retained";
const BUILD_EXECUTION_WORKSPACE_OUTPUTS_DIR_NAME: &str = "outputs";
const BUILD_EXECUTION_REPORT_FILE_NAME: &str = "execution-report.json";
const BUILD_EXECUTION_LOG_ARCHIVE_FILE_NAME: &str = "execution-logs.zip";
const PROCESS_CHECKOUT_LOG_FILE_NAME: &str = "01-checkout-repository.log";
const PROCESS_VALIDATION_LOG_FILE_NAME: &str = "02-validate-build-context.log";
const QUEUE_LEASE_RENEWER_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MIN_QUEUE_LEASE_RENEW_INTERVAL: Duration = Duration::from_millis(20);
const RELEASE_SOURCE_KIND_MANAGED_TAG: &str = "managed_tag";
const RELEASE_SOURCE_KIND_MANAGED_REF: &str = "managed_ref";
const RELEASE_SOURCE_KIND_LOCAL_WORKSPACE: &str = "local_workspace";
const REPOSITORY_SOURCE_MODE_LOCAL_WORKSPACE: &str = "local_workspace";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredBuildTargetContract {
    #[serde(default)]
    unity: Option<StoredUnityBuildTargetContract>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredUnityBuildTargetContract {
    #[serde(rename = "targetPlatform", default)]
    target_platform: String,
    #[serde(rename = "buildMethod", default)]
    build_method: String,
    #[serde(rename = "editorVersion", default)]
    editor_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedUnityBuildTargetContract {
    target_platform: String,
    build_method: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedBuildSourceMetadata {
    source_kind: String,
    source_ref: Option<String>,
    local_path: Option<String>,
    unity_executable_path_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BuildExecutionDispatchPlan {
    UnityHostNative(UnityBuildExecutionPlan),
}

impl BuildExecutionDispatchPlan {
    #[cfg(test)]
    pub(crate) fn into_unity_host_native(self) -> UnityBuildExecutionPlan {
        match self {
            Self::UnityHostNative(plan) => plan,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BuildProcessStage {
    ValidateContext,
    ExecuteBuild,
    PackageArtifact,
    RegisterArtifacts,
}

impl BuildProcessStage {
    const fn default_key(self) -> &'static str {
        match self {
            Self::ValidateContext => "validate-build-context",
            Self::ExecuteBuild => "execute-build",
            Self::PackageArtifact => "package-artifact",
            Self::RegisterArtifacts => "register-artifacts",
        }
    }

    const fn default_label(self) -> &'static str {
        match self {
            Self::ValidateContext => "Validate Build Context",
            Self::ExecuteBuild => "Execute Build",
            Self::PackageArtifact => "Package Artifact",
            Self::RegisterArtifacts => "Register Artifacts",
        }
    }

    const fn writes_runtime_log(self) -> bool {
        !matches!(self, Self::ExecuteBuild)
    }
}

fn generic_execute_build_stage_identity() -> UnityBuildStageIdentity {
    UnityBuildStageIdentity {
        step_key: String::from(BuildProcessStage::ExecuteBuild.default_key()),
        step_label: String::from(BuildProcessStage::ExecuteBuild.default_label()),
        log_stem: String::from("execute-build"),
    }
}

#[derive(Debug, Default)]
struct BuildRunStageSequence {
    ordered_stages: Vec<BuildProcessStage>,
    process_log_paths: HashMap<String, PathBuf>,
    next_process_log_index: Option<usize>,
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

    fn shared_process_log_path(&mut self, logs_dir: &Path, stem: &str) -> io::Result<PathBuf> {
        let cache_key = format!("shared:{stem}");
        if let Some(path) = self.process_log_paths.get(&cache_key) {
            return Ok(path.clone());
        }

        let path = match find_existing_process_log_path(logs_dir, stem)? {
            Some(path) => path,
            None => self.allocate_process_log_path(logs_dir, stem)?,
        };
        self.process_log_paths.insert(cache_key, path.clone());
        Ok(path)
    }

    fn unique_process_log_path(
        &mut self,
        logs_dir: &Path,
        cache_key: impl Into<String>,
        stem: &str,
    ) -> io::Result<PathBuf> {
        let cache_key = cache_key.into();
        if let Some(path) = self.process_log_paths.get(&cache_key) {
            return Ok(path.clone());
        }

        let path = self.allocate_process_log_path(logs_dir, stem)?;
        self.process_log_paths.insert(cache_key, path.clone());
        Ok(path)
    }

    fn allocate_process_log_path(&mut self, logs_dir: &Path, stem: &str) -> io::Result<PathBuf> {
        let next_index = match self.next_process_log_index {
            Some(index) => index + 1,
            None => max_process_log_index(logs_dir)? + 1,
        };
        self.next_process_log_index = Some(next_index);

        Ok(logs_dir.join(format!("{next_index:02}-{stem}.log")))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildRunStageExecution {
    position: i64,
    log_path: PathBuf,
}

fn append_timestamped_log_message(path: &Path, message: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
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

struct ProcessCheckoutLogReporter {
    log_path: PathBuf,
}

impl ProcessCheckoutLogReporter {
    fn new(log_path: PathBuf) -> io::Result<Self> {
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
        }

        Ok(Self { log_path })
    }
}

impl ExecutionProgressReporter for ProcessCheckoutLogReporter {
    fn heartbeat(&mut self, progress: ExecutionProgress) {
        if let Err(error) = append_timestamped_log_message(&self.log_path, &progress.message) {
            eprintln!(
                "runtime process checkout could not append '{}' to '{}': {}",
                progress.message,
                self.log_path.display(),
                error,
            );
        }
    }
}

fn process_checkout_log_path(workspace_path: &Path) -> PathBuf {
    workspace_path.join("logs").join(PROCESS_CHECKOUT_LOG_FILE_NAME)
}

fn process_validation_log_path(workspace_path: &Path) -> PathBuf {
    workspace_path.join("logs").join(PROCESS_VALIDATION_LOG_FILE_NAME)
}

fn ensure_release_process_checkout(
    directories: &runtime_config::RuntimeDirectories,
    preparation: &WorkspacePreparationInput,
) -> io::Result<PreparedWorkspace> {
    let preparer = WorkspacePreparer::new(directories);
    let planned = preparer.plan(preparation)?;
    if preparer.is_process_prepared(preparation)? {
        return Ok(planned);
    }

    let mut reporter = ProcessCheckoutLogReporter::new(process_checkout_log_path(&planned.root_path))?;
    preparer.prepare_process_with_reporter(preparation, &mut reporter)
}

struct BuildRunStageTracker<'a> {
    coordinator: &'a LocalCoordinator,
    build_run_id: i64,
    workspace_path: PathBuf,
    build_root_path: PathBuf,
    artifact_root_path: PathBuf,
    execute_build_stage: UnityBuildStageIdentity,
    stage_sequence: Rc<RefCell<BuildRunStageSequence>>,
}

impl<'a> BuildRunStageTracker<'a> {
    fn new(
        coordinator: &'a LocalCoordinator,
        build_run_id: i64,
        workspace_path: impl Into<PathBuf>,
        build_root_path: impl Into<PathBuf>,
        artifact_root_path: impl Into<PathBuf>,
        execute_build_stage: UnityBuildStageIdentity,
        stage_sequence: Rc<RefCell<BuildRunStageSequence>>,
    ) -> io::Result<Self> {
        let tracker = Self {
            coordinator,
            build_run_id,
            workspace_path: workspace_path.into(),
            build_root_path: build_root_path.into(),
            artifact_root_path: artifact_root_path.into(),
            execute_build_stage,
            stage_sequence,
        };
        fs::create_dir_all(tracker.process_logs_dir())?;
        fs::create_dir_all(tracker.build_logs_dir())?;
        Ok(tracker)
    }

    fn process_logs_dir(&self) -> PathBuf {
        self.workspace_path.join("logs")
    }

    fn build_logs_dir(&self) -> PathBuf {
        self.build_root_path.join("logs")
    }

    fn stage_log_path(&self, stage: BuildProcessStage) -> io::Result<PathBuf> {
        Ok(self.stage_execution(stage)?.log_path)
    }

    fn stage_execution(&self, stage: BuildProcessStage) -> io::Result<BuildRunStageExecution> {
        let execution_index = self.stage_sequence.borrow_mut().execution_index(stage);
        let log_path = self.resolve_stage_log_path(stage)?;

        Ok(BuildRunStageExecution {
            position: execution_index as i64,
            log_path,
        })
    }

    fn stage_key(&self, stage: BuildProcessStage) -> &str {
        match stage {
            BuildProcessStage::ExecuteBuild => self.execute_build_stage.step_key.as_str(),
            _ => stage.default_key(),
        }
    }

    fn stage_label(&self, stage: BuildProcessStage) -> &str {
        match stage {
            BuildProcessStage::ExecuteBuild => self.execute_build_stage.step_label.as_str(),
            _ => stage.default_label(),
        }
    }

    fn resolve_stage_log_path(&self, stage: BuildProcessStage) -> io::Result<PathBuf> {
        match stage {
            BuildProcessStage::ValidateContext => self
                .stage_sequence
                .borrow_mut()
                .shared_process_log_path(&self.process_logs_dir(), self.stage_key(stage)),
            BuildProcessStage::ExecuteBuild => {
                self.stage_sequence.borrow_mut().unique_process_log_path(
                    &self.process_logs_dir(),
                    format!("{}:{}", self.stage_key(stage), self.build_run_id),
                    self.execute_build_stage.log_stem.as_str(),
                )
            }
            BuildProcessStage::PackageArtifact => {
                Ok(self.build_logs_dir().join("package-artifact.log"))
            }
            BuildProcessStage::RegisterArtifacts => {
                Ok(self.build_logs_dir().join("register-artifacts.log"))
            }
        }
    }

    fn start_stage(&self, stage: BuildProcessStage, message: &str) -> io::Result<()> {
        let execution = self.stage_execution(stage)?;
        self.write_stage_message(stage, &execution.log_path, message)?;
        self.coordinator.start_build_run_stage(
            self.build_run_id,
            StartBuildRunStageInput {
                position: execution.position,
                step_key: String::from(self.stage_key(stage)),
                step_label: String::from(self.stage_label(stage)),
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
        let execution = self.stage_execution(stage)?;
        self.write_stage_message(stage, &execution.log_path, message)?;
        self.coordinator.heartbeat_build_run_stage(
            self.build_run_id,
            HeartbeatBuildRunStageInput {
                step_key: String::from(self.stage_key(stage)),
                step_label: String::from(self.stage_label(stage)),
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
        let execution = self.stage_execution(stage)?;
        self.write_stage_message(stage, &execution.log_path, message)?;
        self.coordinator.complete_build_run_stage(
            self.build_run_id,
            CompleteBuildRunStageInput {
                step_key: String::from(self.stage_key(stage)),
                step_label: String::from(self.stage_label(stage)),
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
        let execution = self.stage_execution(stage)?;
        self.write_stage_message(stage, &execution.log_path, error_message)?;
        self.coordinator.fail_build_run_stage(
            self.build_run_id,
            FailBuildRunStageInput {
                step_key: String::from(self.stage_key(stage)),
                step_label: String::from(self.stage_label(stage)),
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

        append_timestamped_log_message(path, message)
    }
}

fn max_process_log_index(logs_dir: &Path) -> io::Result<usize> {
    if !logs_dir.is_dir() {
        return Ok(0);
    }

    let mut max_index = 0_usize;
    for entry in fs::read_dir(logs_dir)? {
        let entry = entry?;
        if !entry.path().is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some((prefix, _)) = name.split_once('-') else {
            continue;
        };
        let Ok(index) = prefix.parse::<usize>() else {
            continue;
        };
        max_index = max_index.max(index);
    }

    Ok(max_index)
}

fn find_existing_process_log_path(logs_dir: &Path, stem: &str) -> io::Result<Option<PathBuf>> {
    if !logs_dir.is_dir() {
        return Ok(None);
    }

    let suffix = format!("-{stem}.log");
    let mut matches = fs::read_dir(logs_dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_str().map(str::to_owned)?;
            if path.is_file() && name.ends_with(&suffix) {
                Some(path)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    matches.sort();

    Ok(matches.into_iter().next())
}

struct BuildStageHeartbeatReporter<'a, 'b> {
    tracker: &'a BuildRunStageTracker<'b>,
    storage: &'a StorageLayout,
    context: &'a BuildRunEventContext,
    stage: BuildProcessStage,
    error: Option<io::Error>,
}

impl<'a, 'b> BuildStageHeartbeatReporter<'a, 'b> {
    fn new(
        tracker: &'a BuildRunStageTracker<'b>,
        storage: &'a StorageLayout,
        context: &'a BuildRunEventContext,
        stage: BuildProcessStage,
    ) -> Self {
        Self {
            tracker,
            storage,
            context,
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
            return;
        }

        if let Err(error) = emit_build_run_stage_updated_event(
            self.storage,
            self.context,
            self.tracker.stage_key(self.stage),
            self.tracker.stage_label(self.stage),
            &progress.message,
        ) {
            log_runtime_event_failure(EVENT_TOPIC_BUILD_RUN_STAGE_UPDATED, &error);
        }
    }
}

fn emit_build_stage_started_event(
    storage: &StorageLayout,
    context: &BuildRunEventContext,
    tracker: &BuildRunStageTracker<'_>,
    stage: BuildProcessStage,
    message: &str,
) {
    if let Err(error) = emit_build_run_stage_updated_event(
        storage,
        context,
        tracker.stage_key(stage),
        tracker.stage_label(stage),
        message,
    ) {
        log_runtime_event_failure(EVENT_TOPIC_BUILD_RUN_STAGE_UPDATED, &error);
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

pub(crate) struct QueueLeaseRenewer {
    stop_signal: Arc<AtomicBool>,
    error_message: Arc<Mutex<Option<String>>>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl QueueLeaseRenewer {
    pub(crate) fn spawn(
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
            let mut last_renewed_at = std::time::Instant::now()
                .checked_sub(renew_interval)
                .unwrap_or_else(std::time::Instant::now);

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
                            format!("renew {context} {message_id} lease: {error}"),
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

    pub(crate) fn stop(&self) {
        self.stop_signal.store(true, Ordering::Release);
    }

    pub(crate) fn finish(mut self) -> io::Result<()> {
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

pub(crate) fn run_build_stage_next_command(
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

    let staged_result =
        stage_claimed_build_job(&coordinator, config, storage, &message.payload).or_else(
            |error| {
                coordinator
                    .release_message(message.id, &message.lease_token)
                    .map_err(|release_error| {
                        Box::new(io::Error::other(format!(
                            "release claimed build message {} after error {error}: {release_error}",
                            message.id
                        ))) as Box<dyn Error>
                    })
                    .and_then(|_| Err(Box::new(error) as Box<dyn Error>))
            },
        );

    lease_renewer.stop();
    let staged = match staged_result {
        Ok(staged) => staged,
        Err(error) => {
            if let Err(lease_error) = lease_renewer.finish() {
                eprintln!(
                    "queue lease renewer stopped with error after build staging failure: {lease_error}"
                );
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

pub(crate) fn run_build_run_next_command(
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
    let mut build_event_context = None;
    let record_result = (|| -> Result<BuildRunRecord, Box<dyn Error>> {
        let plan = match load_claimed_build_plan(&coordinator, &message.payload) {
            Ok(plan) => plan,
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
        let event_context = build_run_event_context(&coordinator, &plan);
        build_event_context = Some(event_context.clone());

        let planned_preparation =
            match build_workspace_preparation(&plan, GitAuthOptions::default()) {
                Ok(preparation) => preparation,
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
        let planned = match WorkspacePreparer::new(&config.directories).plan(&planned_preparation)
        {
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

        let validation_log_path = process_validation_log_path(&planned.root_path);
        coordinator.start_build_run(
            plan.build_run_id,
            StartBuildRunInput {
                workspace_path: planned.root_path.display().to_string(),
                log_path: validation_log_path.display().to_string(),
                artifact_root_path: planned.artifact_root_path.display().to_string(),
            },
        )?;
        if let Err(error) = emit_build_run_started_event(storage, &event_context) {
            log_runtime_event_failure(EVENT_TOPIC_BUILD_RUN_STARTED, &error);
        }
        let preparation = match resolve_build_workspace_preparation(&coordinator, &plan) {
            Ok(preparation) => preparation,
            Err(error) => {
                if error_indicates_authentication_failure(&error) {
                    persist_repository_auth_runtime_failure(
                        &coordinator,
                        plan.repository_id,
                        &error,
                    );
                }
                let record = coordinator.fail_build_run(
                    plan.build_run_id,
                    FailBuildRunInput {
                        workspace_path: planned.root_path.display().to_string(),
                        log_path: validation_log_path.display().to_string(),
                        artifact_root_path: planned.artifact_root_path.display().to_string(),
                        error_message: error.to_string(),
                    },
                )?;
                return Ok(record);
            }
        };
        if let Err(error) = ensure_release_process_checkout(&config.directories, &preparation) {
            if error_indicates_authentication_failure(&error) {
                persist_repository_auth_runtime_failure(&coordinator, plan.repository_id, &error);
            }
            let record = coordinator.fail_build_run(
                plan.build_run_id,
                FailBuildRunInput {
                    workspace_path: planned.root_path.display().to_string(),
                    log_path: process_checkout_log_path(&planned.root_path).display().to_string(),
                    artifact_root_path: planned.artifact_root_path.display().to_string(),
                    error_message: error.to_string(),
                },
            )?;
            return Ok(record);
        }
        let stage_sequence = Rc::new(RefCell::new(BuildRunStageSequence::default()));
        let validation_tracker = BuildRunStageTracker::new(
            &coordinator,
            plan.build_run_id,
            planned.root_path.clone(),
            planned.build_root_path.clone(),
            planned.artifact_root_path.clone(),
            generic_execute_build_stage_identity(),
            stage_sequence.clone(),
        )?;
        let target_platform = if event_context.unity_target_platform.trim().is_empty() {
            "unknown"
        } else {
            event_context.unity_target_platform.as_str()
        };
        let repository_engine_kind = plan.engine_kind.as_str();
        let engine_version = if plan.engine_version.trim().is_empty() {
            "unknown"
        } else {
            plan.engine_version.as_str()
        };
        let validation_message = format!(
            "Validating build context for repository '{}' tag '{}' target '{}' ({}) using engine '{}' version '{}'.",
            plan.repository_name,
            plan.git_tag,
            plan.target_name,
            target_platform,
            repository_engine_kind,
            engine_version,
        );
        validation_tracker.start_stage(
            BuildProcessStage::ValidateContext,
            &validation_message,
        )?;
        emit_build_stage_started_event(
            storage,
            &event_context,
            &validation_tracker,
            BuildProcessStage::ValidateContext,
            &validation_message,
        );

        let dispatch_plan = match resolve_build_execution_dispatch_plan(config, &plan) {
            Ok(plan) => {
                validation_tracker.complete_stage(
                    BuildProcessStage::ValidateContext,
                    &build_dispatch_resolution_message(&plan),
                )?;
                plan
            }
            Err(error) => {
                validation_tracker.fail_stage(
                    BuildProcessStage::ValidateContext,
                    &error.to_string(),
                )?;
                let record = coordinator.fail_build_run(
                    plan.build_run_id,
                    FailBuildRunInput {
                        workspace_path: planned.root_path.display().to_string(),
                        log_path: validation_log_path.display().to_string(),
                        artifact_root_path: planned.artifact_root_path.display().to_string(),
                        error_message: error.to_string(),
                    },
                )?;
                return Ok(record);
            }
        };

        match dispatch_plan {
            BuildExecutionDispatchPlan::UnityHostNative(unity_plan) => {
                let processor = UnityBuildExecutionProcessor::new(
                    &config.directories,
                    HostNativeUnityExecutor::new(),
                );
                process_unity_build_run_with_retry(
                    &coordinator,
                    storage,
                    &config.directories,
                    &processor,
                    &unity_plan,
                    &preparation,
                    plan.build_run_id,
                    &event_context,
                    stage_sequence,
                    validation_log_path,
                )
                .map_err(|error| Box::new(error) as Box<dyn Error>)
            }
        }
    })();

    lease_renewer.stop();
    let record = match record_result {
        Ok(record) => record,
        Err(error) => {
            if let Err(lease_error) = lease_renewer.finish() {
                eprintln!(
                    "queue lease renewer stopped with error after build run failure: {lease_error}"
                );
            }
            return Err(error);
        }
    };

    synchronize_build_execution_report(&coordinator, record.id, None, None)?;

    maybe_run_release_cleanup(&coordinator, record.release_run_id, record.id);

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

    if let Some(context) = build_event_context.as_ref() {
        if let Err(error) = emit_build_run_finished_event(storage, context, &record) {
            log_runtime_event_failure(EVENT_TOPIC_BUILD_RUN_FINISHED, &error);
        }
    }

    serde_json::to_string_pretty(&record).map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn stage_claimed_build_job(
    coordinator: &LocalCoordinator,
    config: &RuntimeConfig,
    storage: &StorageLayout,
    payload: &[u8],
) -> io::Result<BuildRunRecord> {
    let plan = load_claimed_build_plan(coordinator, payload)?;
    let planned_preparation = build_workspace_preparation(&plan, GitAuthOptions::default())?;
    let planned = WorkspacePreparer::new(&config.directories)
        .plan(&planned_preparation)?;
    let event_context = build_run_event_context(coordinator, &plan);
    let started = coordinator.start_build_run(
        plan.build_run_id,
        StartBuildRunInput {
            workspace_path: planned.root_path.display().to_string(),
            log_path: process_validation_log_path(&planned.root_path).display().to_string(),
            artifact_root_path: planned.artifact_root_path.display().to_string(),
        },
    )?;
    if let Err(error) = emit_build_run_started_event(storage, &event_context) {
        log_runtime_event_failure(EVENT_TOPIC_BUILD_RUN_STARTED, &error);
    }

    let preparation = match resolve_build_workspace_preparation(coordinator, &plan) {
        Ok(preparation) => preparation,
        Err(error) => {
            if error_indicates_authentication_failure(&error) {
                persist_repository_auth_runtime_failure(coordinator, plan.repository_id, &error);
            }
            return coordinator.fail_build_run(
                plan.build_run_id,
                FailBuildRunInput {
                    workspace_path: planned.root_path.display().to_string(),
                    log_path: process_validation_log_path(&planned.root_path).display().to_string(),
                    artifact_root_path: planned.artifact_root_path.display().to_string(),
                    error_message: error.to_string(),
                },
            );
        }
    };

    match ensure_release_process_checkout(&config.directories, &preparation) {
        Ok(_) => Ok(started),
        Err(error) => {
            if error_indicates_authentication_failure(&error) {
                persist_repository_auth_runtime_failure(coordinator, plan.repository_id, &error);
            }
            coordinator.fail_build_run(
                plan.build_run_id,
                FailBuildRunInput {
                    workspace_path: planned.root_path.display().to_string(),
                    log_path: process_checkout_log_path(&planned.root_path).display().to_string(),
                    artifact_root_path: planned.artifact_root_path.display().to_string(),
                    error_message: error.to_string(),
                },
            )
        }
    }
}

fn complete_successful_build_run(
    coordinator: &LocalCoordinator,
    storage: &StorageLayout,
    event_context: &BuildRunEventContext,
    build_run_id: i64,
    runner_plan: &UnityBuildExecutionPlan,
    result: &UnityBuildExecutionResult,
    tracker: &BuildRunStageTracker<'_>,
) -> io::Result<BuildRunRecord> {
    if output_requires_runtime_archive(runner_plan) {
        let packaging_message =
            "Packaging Unity output into a runtime-owned zip archive.";
        tracker.start_stage(BuildProcessStage::PackageArtifact, packaging_message)?;
        emit_build_stage_started_event(
            storage,
            event_context,
            tracker,
            BuildProcessStage::PackageArtifact,
            packaging_message,
        );
        if let Err(error) = package_build_output(runner_plan, result) {
            tracker.fail_stage(BuildProcessStage::PackageArtifact, &error.to_string())?;
            return Err(error);
        }
        tracker.complete_stage(
            BuildProcessStage::PackageArtifact,
            "Runtime archive packaging completed.",
        )?;
    }

    let register_message =
        "Discovering artifacts and registering them for release-wide publish planning.";
    tracker.start_stage(BuildProcessStage::RegisterArtifacts, register_message)?;
    emit_build_stage_started_event(
        storage,
        event_context,
        tracker,
        BuildProcessStage::RegisterArtifacts,
        register_message,
    );
    register_build_artifacts(
        coordinator,
        build_run_id,
        runner_plan,
        &result.artifact_root_path,
    )
        .map_err(|error| {
            let _ = tracker.fail_stage(BuildProcessStage::RegisterArtifacts, &error.to_string());
            error
        })?;
    tracker.complete_stage(
        BuildProcessStage::RegisterArtifacts,
        "Artifacts registered for downstream release-wide publish planning.",
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

fn output_requires_runtime_archive(plan: &UnityBuildExecutionPlan) -> bool {
    plan.output_kind
        .as_deref()
        .is_some_and(|output_kind| output_kind.eq_ignore_ascii_case("archive"))
}

pub(crate) fn package_build_output(
    plan: &UnityBuildExecutionPlan,
    result: &UnityBuildExecutionResult,
) -> io::Result<()> {
    package_unity_build_output(plan, &result.output_path, &result.artifact_root_path)
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
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
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

pub(crate) fn build_execution_report_path(workspace_path: &Path) -> PathBuf {
    build_execution_retained_dir(workspace_path).join(BUILD_EXECUTION_REPORT_FILE_NAME)
}

pub(crate) fn build_execution_logs_archive_path(workspace_path: &Path) -> PathBuf {
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
                if entry.file_name() == BUILD_EXECUTION_RETAINED_DIR_NAME
                    || entry.file_name() == BUILD_EXECUTION_WORKSPACE_OUTPUTS_DIR_NAME
                {
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

fn default_build_execution_cleanup_snapshot(
    workspace_path: &Path,
) -> BuildExecutionCleanupSnapshot {
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

fn maybe_run_release_cleanup(
    coordinator: &LocalCoordinator,
    release_run_id: i64,
    build_run_id: i64,
) {
    let release_run = match coordinator.get_release_run_record(release_run_id) {
        Ok(release_run) => release_run,
        Err(error) => {
            eprintln!(
                "runtime cleanup could not reload release run {}: {}",
                release_run_id, error
            );
            return;
        }
    };
    if !release_status_is_terminal(&release_run.status) {
        return;
    }

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

    let all_attempt_roots = discover_release_attempt_roots(&final_workspace_path).unwrap_or_else(
        |error| {
            eprintln!(
                "runtime cleanup could not enumerate release workspace roots for {}: {}",
                release_run_id, error
            );
            vec![final_workspace_path.clone()]
        },
    );

    let workspace_bytes_before = match total_workspace_size_bytes(&all_attempt_roots) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!(
                "runtime cleanup could not size release run {} before pruning: {}",
                release_run_id, error
            );
            0
        }
    };

    let mut cleanup_error = None;
    if let Err(error) = archive_build_run_logs(&all_attempt_roots, &final_workspace_path) {
        append_cleanup_error(&mut cleanup_error, error.to_string());
    }
    let removed_attempt_count =
        match prune_build_run_workspaces(&all_attempt_roots, &final_workspace_path) {
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

fn release_status_is_terminal(status: &str) -> bool {
    status == ReleaseStatus::Succeeded.as_str()
        || status == ReleaseStatus::Failed.as_str()
    || status == ReleaseStatus::Canceled.as_str()
}

fn discover_release_attempt_roots(workspace_path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    let builds_root = workspace_path.join("builds");

    if builds_root.is_dir() {
        let mut entries = fs::read_dir(&builds_root)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                push_attempt_root(&mut roots, path);
            }
        }
    }

    push_attempt_root(&mut roots, workspace_path.to_path_buf());
    Ok(roots)
}

pub(crate) fn recover_interrupted_build_attempts(
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
    let removed_attempt_count = match prune_build_run_workspaces(&attempt_roots, &workspace_path) {
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

    let builds_root = workspace_path.join("builds");
    if builds_root.is_dir() {
        let mut entries = fs::read_dir(&builds_root)?.collect::<Result<Vec<_>, _>>()?;
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

    if roots.is_empty() {
        if let Some(parent) = workspace_path.parent() {
        if parent.is_dir() {
            let mut entries = fs::read_dir(parent)?.collect::<Result<Vec<_>, _>>()?;
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
    }

    push_attempt_root(&mut roots, workspace_path.to_path_buf());
    Ok(roots)
}

pub(crate) fn synchronize_build_execution_report_from_publish(
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

    maybe_run_release_cleanup(
        coordinator,
        publish_run.release_run_id,
        publish_run.build_run_id,
    );
}

fn process_unity_build_run_with_retry(
    coordinator: &LocalCoordinator,
    storage: &StorageLayout,
    directories: &runtime_config::RuntimeDirectories,
    processor: &UnityBuildExecutionProcessor<HostNativeUnityExecutor>,
    runner_plan: &UnityBuildExecutionPlan,
    preparation: &WorkspacePreparationInput,
    build_run_id: i64,
    event_context: &BuildRunEventContext,
    stage_sequence: Rc<RefCell<BuildRunStageSequence>>,
    validation_log_path: PathBuf,
) -> io::Result<BuildRunRecord> {
    let current_preparation = preparation.clone();
    let mut retry_available = true;

    loop {
        let planned = WorkspacePreparer::new(directories).plan(&current_preparation)?;
        let workspace = match processor.prepare_build_workspace(&current_preparation) {
            Ok(workspace) => workspace,
            Err(error) => {
                let record = coordinator.fail_build_run(
                    build_run_id,
                    FailBuildRunInput {
                        workspace_path: planned.root_path.display().to_string(),
                        log_path: validation_log_path.display().to_string(),
                        artifact_root_path: planned.artifact_root_path.display().to_string(),
                        error_message: error.to_string(),
                    },
                )?;
                return Ok(record);
            }
        };
        let tracker = BuildRunStageTracker::new(
            coordinator,
            build_run_id,
            planned.root_path.clone(),
            planned.build_root_path.clone(),
            planned.artifact_root_path.clone(),
            resolve_unity_build_stage_identity(runner_plan),
            stage_sequence.clone(),
        )?;

        let unity_log_path = tracker.stage_log_path(BuildProcessStage::ExecuteBuild)?;
        let mut workspace = workspace;
        workspace.log_path = unity_log_path.clone();

        let unity_build_message = format!(
            "Launching Unity build method '{}' for target '{}'.",
            runner_plan.unity_build_method,
            runner_plan.unity_target_platform,
        );
        tracker.start_stage(BuildProcessStage::ExecuteBuild, &unity_build_message)?;
        emit_build_stage_started_event(
            storage,
            event_context,
            &tracker,
            BuildProcessStage::ExecuteBuild,
            &unity_build_message,
        );

        let mut reporter = BuildStageHeartbeatReporter::new(
            &tracker,
            storage,
            event_context,
            BuildProcessStage::ExecuteBuild,
        );
        let execute_outcome = processor.execute_prepared(runner_plan, workspace, &mut reporter);
        if let Some(error) = reporter.take_error() {
            tracker.fail_stage(BuildProcessStage::ExecuteBuild, &error.to_string())?;
            let record = coordinator.fail_build_run(
                build_run_id,
                FailBuildRunInput {
                    workspace_path: planned.root_path.display().to_string(),
                    log_path: unity_log_path.display().to_string(),
                    artifact_root_path: planned.artifact_root_path.display().to_string(),
                    error_message: error.to_string(),
                },
            )?;
            return Ok(record);
        }

        match execute_outcome {
            Ok(UnityBuildExecutionProcessOutcome { result, error }) => match error {
                Some(error) if retry_available && should_retry_in_fresh_workspace(&result.log_path)? => {
                    tracker.fail_stage(
                        BuildProcessStage::ExecuteBuild,
                        &format!("{} Retrying once with a fresh workspace.", error),
                    )?;
                    retry_available = false;
                    continue;
                }
                Some(error) => {
                    tracker.fail_stage(BuildProcessStage::ExecuteBuild, &error.to_string())?;
                    let record = persist_host_native_failure(
                        coordinator,
                        build_run_id,
                        &result,
                        &error,
                    )?;
                    return Ok(record);
                }
                None => {
                    tracker.complete_stage(
                        BuildProcessStage::ExecuteBuild,
                        &format!(
                            "Unity build completed with raw output at '{}'.",
                            result.output_path.display(),
                        ),
                    )?;
                    let record = complete_successful_build_run(
                        coordinator,
                        storage,
                        event_context,
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
                    return Ok(record);
                }
            },
            Err(error) if retry_available && should_retry_in_fresh_workspace(&unity_log_path)? => {
                tracker.fail_stage(
                    BuildProcessStage::ExecuteBuild,
                    &format!("{} Retrying once with a fresh workspace.", error),
                )?;
                retry_available = false;
                continue;
            }
            Err(error) => {
                tracker.fail_stage(BuildProcessStage::ExecuteBuild, &error.to_string())?;
                let record = coordinator.fail_build_run(
                    build_run_id,
                    FailBuildRunInput {
                        workspace_path: planned.root_path.display().to_string(),
                        log_path: unity_log_path.display().to_string(),
                        artifact_root_path: planned.artifact_root_path.display().to_string(),
                        error_message: error.to_string(),
                    },
                )?;
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
        && (normalized.contains("eperm") || normalized.contains("operation not permitted")))
}

fn register_build_artifacts(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    plan: &UnityBuildExecutionPlan,
    artifact_root_path: &Path,
) -> io::Result<()> {
    let expected_output_path =
        resolve_final_unity_artifact_output_path(plan, artifact_root_path)?;
    let artifacts = select_target_aware_artifacts(
        artifact_root_path,
        &expected_output_path,
        discover_artifacts(artifact_root_path)?,
    )?;
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

    Ok(())
}

fn select_target_aware_artifacts(
    artifact_root_path: &Path,
    expected_output_path: &Path,
    artifacts: Vec<DiscoveredArtifact>,
) -> io::Result<Vec<DiscoveredArtifact>> {
    let expected_relative_path = normalize_relative_artifact_path(
        expected_output_path
            .strip_prefix(artifact_root_path)
            .map_err(io::Error::other)?,
    );

    let expected_is_directory = fs::metadata(expected_output_path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false);

    let selected = artifacts
        .into_iter()
        .filter(|artifact| {
            if expected_is_directory {
                artifact.path == expected_relative_path
                    || artifact
                        .path
                        .starts_with(&format!("{expected_relative_path}/"))
            } else {
                artifact.path == expected_relative_path
            }
        })
        .collect::<Vec<_>>();

    if selected.is_empty() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "no artifacts matched build target output {:?} under {:?}",
                expected_output_path.display(),
                artifact_root_path.display()
            ),
        ));
    }

    Ok(selected)
}

fn normalize_relative_artifact_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn load_claimed_build_plan(
    coordinator: &LocalCoordinator,
    payload: &[u8],
) -> io::Result<StoredBuildExecutionPlan> {
    let job: BuildDispatchJob = serde_json::from_slice(payload)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;

    coordinator.get_build_execution_plan(job.build_run_id)
}

fn resolve_build_workspace_preparation(
    coordinator: &LocalCoordinator,
    plan: &StoredBuildExecutionPlan,
) -> io::Result<WorkspacePreparationInput> {
    let git_auth = match plan.repository_credentials_id {
        Some(credentials_id) => {
            let credentials = coordinator.get_credential_record(credentials_id)?;
            let config_json = resolve_credential_secret_config_json(
                &credentials.kind,
                &credentials.config_json,
            )?;
            git_auth_options_from_credentials(&credentials.kind, &config_json)?
        }
        None => GitAuthOptions::default(),
    };

    build_workspace_preparation(plan, git_auth)
}

fn build_workspace_preparation(
    plan: &StoredBuildExecutionPlan,
    git_auth: GitAuthOptions,
) -> io::Result<WorkspacePreparationInput> {
    let source_metadata = resolve_build_source_metadata(plan)?;
    let source = match source_metadata.source_kind.as_str() {
        RELEASE_SOURCE_KIND_LOCAL_WORKSPACE => WorkspacePreparationSource::LocalWorkspace {
            local_path: PathBuf::from(
                source_metadata.local_path.ok_or_else(|| {
                    io::Error::new(
                        ErrorKind::InvalidInput,
                        format!(
                            "build run {} is missing a local workspace path",
                            plan.build_run_id
                        ),
                    )
                })?,
            ),
        },
        RELEASE_SOURCE_KIND_MANAGED_TAG | RELEASE_SOURCE_KIND_MANAGED_REF => {
            let git_ref = source_metadata.source_ref.ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "build run {} is missing a source ref",
                        plan.build_run_id
                    ),
                )
            })?;
            if plan.repository_url.trim().is_empty() {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "build run {} is missing a managed repository URL",
                        plan.build_run_id
                    ),
                ));
            }

            WorkspacePreparationSource::GitRef {
                repository_url: plan.repository_url.clone(),
                git_auth,
                git_ref,
            }
        }
        other => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "build run {} uses unsupported source_kind {:?}",
                    plan.build_run_id, other
                ),
            ));
        }
    };

    Ok(WorkspacePreparationInput {
        build_run_id: plan.build_run_id,
        release_run_id: plan.release_run_id,
        attempt_token: String::new(),
        repository_name: plan.repository_name.clone(),
        source,
        workspace_root_override: plan.workspace_root_override.clone(),
        artifacts_root_override: plan.artifacts_root_override.clone(),
    })
}

fn unity_runner_execution_plan(
    plan: &StoredBuildExecutionPlan,
) -> io::Result<UnityBuildExecutionPlan> {
    let contract = resolve_unity_build_target_contract(plan)?;
    if plan.timeout_seconds <= 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "build run {} has invalid timeout_seconds {}",
                plan.build_run_id, plan.timeout_seconds
            ),
        ));
    }

    Ok(UnityBuildExecutionPlan {
        build_run_id: plan.build_run_id,
        release_run_id: plan.release_run_id,
        build_target_id: plan.build_target_id,
        repository_name: plan.repository_name.clone(),
        repository_url: plan.repository_url.clone(),
        git_tag: plan.git_tag.clone(),
        target_name: plan.target_name.clone(),
        unity_target_platform: contract.target_platform,
        runner_type: plan.runner_type.clone(),
        unity_build_method: contract.build_method,
        output_kind: plan.output_kind.clone(),
        output_path_template: plan.output_path_template.clone(),
        engine_version: plan.engine_version.clone(),
        config_json: resolve_unity_runner_config_json(plan)?,
        timeout_seconds: plan.timeout_seconds,
    })
}

fn resolve_build_source_metadata(
    plan: &StoredBuildExecutionPlan,
) -> io::Result<ResolvedBuildSourceMetadata> {
    let metadata = if plan.source_metadata_json.trim().is_empty() {
        ReleaseSourceMetadata::default()
    } else {
        serde_json::from_str::<ReleaseSourceMetadata>(&plan.source_metadata_json).map_err(
            |error| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "decode build source metadata for build run {}: {error}",
                        plan.build_run_id
                    ),
                )
            },
        )?
    };
    let default_source_kind = if plan.repository_source_mode == REPOSITORY_SOURCE_MODE_LOCAL_WORKSPACE
    {
        RELEASE_SOURCE_KIND_LOCAL_WORKSPACE
    } else {
        RELEASE_SOURCE_KIND_MANAGED_TAG
    };
    let source_kind = metadata
        .source_kind
        .as_deref()
        .unwrap_or(default_source_kind)
        .trim()
        .to_owned();
    let source_ref = metadata
        .source_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            if source_kind == RELEASE_SOURCE_KIND_MANAGED_TAG {
                Some(plan.git_tag.clone())
            } else {
                None
            }
        });
    let local_path = metadata
        .local_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| plan.repository_local_path.clone());
    let unity_executable_path_override = metadata
        .unity_executable_path_override
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    Ok(ResolvedBuildSourceMetadata {
        source_kind,
        source_ref,
        local_path,
        unity_executable_path_override,
    })
}

fn resolve_unity_runner_config_json(
    plan: &StoredBuildExecutionPlan,
) -> io::Result<String> {
    let source_metadata = resolve_build_source_metadata(plan)?;
    let Some(unity_executable_path_override) = source_metadata.unity_executable_path_override else {
        return Ok(plan.config_json.clone());
    };

    let mut config = serde_json::from_str::<serde_json::Value>(&plan.config_json).map_err(
        |error| {
            io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "build run {} has invalid runner config_json: {error}",
                    plan.build_run_id
                ),
            )
        },
    )?;
    let object = config.as_object_mut().ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "build run {} runner config_json must be a JSON object",
                plan.build_run_id
            ),
        )
    })?;
    object.insert(
        String::from("unity_executable_path"),
        serde_json::Value::String(unity_executable_path_override),
    );

    serde_json::to_string(&config).map_err(io::Error::other)
}

fn resolve_unity_build_target_contract(
    plan: &StoredBuildExecutionPlan,
) -> io::Result<ResolvedUnityBuildTargetContract> {
    if plan.build_kind != BuildKind::Player {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "build run {} uses unsupported Unity build_kind {:?}",
                plan.build_run_id,
                plan.build_kind.as_str()
            ),
        ));
    }

    let contract_json = plan.contract_json.trim();
    if contract_json.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "build run {} is missing build target contract_json",
                plan.build_run_id
            ),
        ));
    }

    let contract = serde_json::from_str::<StoredBuildTargetContract>(contract_json)
        .map_err(|error| {
            io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "build run {} has invalid build target contract_json: {error}",
                    plan.build_run_id
                ),
            )
        })?;

    let Some(unity) = contract.unity else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "build run {} is missing a Unity build target contract payload",
                plan.build_run_id
            ),
        ));
    };

    Ok(ResolvedUnityBuildTargetContract {
        target_platform: require_cli_value(
            unity.target_platform.as_str(),
            "build target contract unity.targetPlatform",
        )?,
        build_method: require_cli_value(
            unity.build_method.as_str(),
            "build target contract unity.buildMethod",
        )?,
    })
}

fn resolve_build_execution_dispatch_plan(
    config: &RuntimeConfig,
    plan: &StoredBuildExecutionPlan,
) -> io::Result<BuildExecutionDispatchPlan> {
    let capability_profile = inspect_host_capability_profile(config.platform);
    resolve_build_execution_dispatch_plan_with_profile(plan, &capability_profile)
}

pub(crate) fn resolve_build_execution_dispatch_plan_with_profile(
    plan: &StoredBuildExecutionPlan,
    capability_profile: &HostCapabilityProfile,
) -> io::Result<BuildExecutionDispatchPlan> {
    match EngineAdapterRegistry::new().resolve_build_execution_adapter(plan.engine_kind)? {
        BuildExecutionAdapter::Unity => Ok(BuildExecutionDispatchPlan::UnityHostNative(
            resolve_unity_build_execution_plan_with_profile(plan, capability_profile)?,
        )),
    }
}

fn resolve_unity_build_execution_plan_with_profile(
    plan: &StoredBuildExecutionPlan,
    capability_profile: &HostCapabilityProfile,
) -> io::Result<UnityBuildExecutionPlan> {
    let runner_plan = unity_runner_execution_plan(plan)?;
    if runner_plan.runner_type.trim() != RunnerFamily::HostNative.label() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "build run {} uses unsupported runner_type {:?} for builds run-next",
                runner_plan.build_run_id, runner_plan.runner_type
            ),
        ));
    }

    resolve_host_native_unity_execution_plan(&runner_plan, capability_profile)
}

fn build_dispatch_resolution_message(plan: &BuildExecutionDispatchPlan) -> String {
    match plan {
        BuildExecutionDispatchPlan::UnityHostNative(plan) => format!(
            "Resolved build dispatch for engine 'unity' with runner '{}' and Unity method '{}'.",
            plan.runner_type, plan.unity_build_method
        ),
    }
}

fn persist_host_native_failure(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    result: &runtime_runner::unity::UnityBuildExecutionResult,
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

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_contracts::EngineKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> io::Result<Self> {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "hgp-runtime-bin-{label}-{}-{timestamp}",
                std::process::id()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn resolve_build_execution_dispatch_plan_with_profile_rejects_unsupported_engine_kind() {
        let error = resolve_build_execution_dispatch_plan_with_profile(
            &test_stored_build_execution_plan(EngineKind::Godot),
            &test_host_capability_profile(),
        )
        .expect_err("non-Unity engines should be rejected before runner resolution");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(
            error
                .to_string()
                .contains("unsupported repository engine_kind \"godot\"")
        );
    }

    #[test]
    fn resolve_build_execution_dispatch_plan_with_profile_keeps_unity_on_host_native_path() {
        let resolved = resolve_build_execution_dispatch_plan_with_profile(
            &test_stored_build_execution_plan(EngineKind::Unity),
            &test_host_capability_profile(),
        )
        .expect("Unity plans should keep using the host-native path");

        let BuildExecutionDispatchPlan::UnityHostNative(resolved) = resolved;

        assert_eq!(resolved.build_run_id, 41);
        assert_eq!(resolved.runner_type, RunnerFamily::HostNative.label());
        assert_eq!(resolved.unity_target_platform, "StandaloneWindows64");
        assert_eq!(resolved.unity_build_method, "Builder.PerformWindows");
        assert!(resolved.config_json.contains("unity_executable_path"));
    }

    #[test]
    fn resolve_build_execution_dispatch_plan_with_profile_rejects_missing_unity_contract() {
        let mut plan = test_stored_build_execution_plan(EngineKind::Unity);
        plan.contract_json = String::from("{}");

        let error = resolve_build_execution_dispatch_plan_with_profile(
            &plan,
            &test_host_capability_profile(),
        )
        .expect_err("Unity execution should reject build rows without a Unity contract payload");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(
            error
                .to_string()
                .contains("missing a Unity build target contract payload")
        );
    }

    #[test]
    fn prune_build_run_workspaces_preserves_outputs_in_final_workspace() {
        let root = TestDir::new("prune-build-workspaces")
            .expect("temporary test directory should be created");
        let previous_attempt_path = root.path().join("build-run-1-attempt-1");
        let final_workspace_path = root.path().join("build-run-1");
        let retained_path = final_workspace_path.join(BUILD_EXECUTION_RETAINED_DIR_NAME);
        let outputs_path = final_workspace_path
            .join(BUILD_EXECUTION_WORKSPACE_OUTPUTS_DIR_NAME)
            .join("artifact-output");

        fs::create_dir_all(previous_attempt_path.join("source"))
            .expect("previous attempt directory should be created");
        fs::write(previous_attempt_path.join("source").join("stale.txt"), b"stale")
            .expect("previous attempt file should be created");
        fs::create_dir_all(&retained_path)
            .expect("retained directory should be created");
        fs::create_dir_all(&outputs_path)
            .expect("outputs directory should be created");
        fs::create_dir_all(final_workspace_path.join("source"))
            .expect("final workspace source directory should be created");
        fs::create_dir_all(final_workspace_path.join("logs"))
            .expect("final workspace logs directory should be created");
        fs::write(
            retained_path.join(BUILD_EXECUTION_LOG_ARCHIVE_FILE_NAME),
            b"archive",
        )
        .expect("retained archive should be created");
        fs::write(outputs_path.join("artifact.txt"), b"artifact")
            .expect("output artifact should be created");
        fs::write(
            final_workspace_path.join(BUILD_EXECUTION_REPORT_FILE_NAME),
            b"{}",
        )
        .expect("execution report should be created");

        let removed_attempt_count = prune_build_run_workspaces(
            &[previous_attempt_path.clone(), final_workspace_path.clone()],
            &final_workspace_path,
        )
        .expect("workspace pruning should succeed");

        assert_eq!(removed_attempt_count, 1);
        assert!(!previous_attempt_path.exists());
        assert!(retained_path.join(BUILD_EXECUTION_LOG_ARCHIVE_FILE_NAME).is_file());
        assert!(outputs_path.join("artifact.txt").is_file());
        assert!(!final_workspace_path.join("source").exists());
        assert!(!final_workspace_path.join("logs").exists());
        assert!(
            !final_workspace_path
                .join(BUILD_EXECUTION_REPORT_FILE_NAME)
                .exists()
        );
    }

    #[test]
    fn register_build_artifacts_keeps_only_target_expected_outputs() {
        let root = TestDir::new("register-build-artifacts-target-aware")
            .expect("temporary test directory should be created");
        let directories = RuntimeDirectories::from_root(root.path());
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let coordinator = LocalCoordinator::new(&storage);
        let created = coordinator
            .create_repository_project(CreateRepositoryProjectInput {
                name: String::from("Revolutions"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repo_url: Some(String::from("https://example.com/revolutions.git")),
                local_path: None,
                credentials: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                enabled: true,
                build_targets: vec![
                    test_build_target_input("Windows", "StandaloneWindows64"),
                    test_build_target_input("Linux", "StandaloneLinux64"),
                ],
                publish_targets: Vec::new(),
            })
            .expect("repository project should persist");
        let connection = open_connection(&storage.database_path)
            .expect("database connection should open");
        connection
            .execute(
                "INSERT INTO release_runs (repository_id, git_tag, status) VALUES (?, ?, ?)",
                rusqlite::params![
                    created.repository_id,
                    "v1.1.12",
                    ReleaseStatus::Queued.as_str(),
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO build_runs (release_run_id, build_target_id, status) VALUES (?, ?, ?)",
                rusqlite::params![release_run_id, created.build_target_ids[1], "queued"],
            )
            .expect("linux build run should insert");
        let linux_build_run_id = connection.last_insert_rowid();
        drop(connection);

        let artifact_root = root
            .path()
            .join("release-run-1")
            .join("builds")
            .join("build-run-linux")
            .join("outputs");
        fs::create_dir_all(&artifact_root)
            .expect("artifact root should be created for test registration");
        fs::write(
            artifact_root.join("revolutions.v1.1.12.windows.zip"),
            b"stale-windows-artifact",
        )
        .expect("stale windows artifact should be created");
        fs::write(
            artifact_root.join("revolutions.v1.1.12.linux.zip"),
            b"linux-artifact",
        )
        .expect("linux artifact should be created");

        let linux_plan = UnityBuildExecutionPlan {
            build_run_id: linux_build_run_id,
            release_run_id,
            build_target_id: created.build_target_ids[1],
            repository_name: String::from("Revolutions"),
            repository_url: String::from("https://example.com/revolutions.git"),
            git_tag: String::from("v1.1.12"),
            target_name: String::from("Linux"),
            unity_target_platform: String::from("StandaloneLinux64"),
            runner_type: String::from(RunnerFamily::HostNative.label()),
            unity_build_method: String::from("Builder.PerformLinux"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Linux")),
            engine_version: String::from("2022.3.20f1"),
            config_json: String::from(
                r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
            ),
            timeout_seconds: 900,
        };

        register_build_artifacts(
            &coordinator,
            linux_build_run_id,
            &linux_plan,
            &artifact_root,
        )
        .expect("artifact registration should keep only linux output");

        let artifacts = coordinator
            .list_artifacts_by_build_run(linux_build_run_id)
            .expect("registered artifacts should load");

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].name, "revolutions.v1.1.12.linux.zip");
        assert_eq!(artifacts[0].path, "revolutions.v1.1.12.linux.zip");
    }

    fn test_stored_build_execution_plan(engine_kind: EngineKind) -> StoredBuildExecutionPlan {
        StoredBuildExecutionPlan {
            build_run_id: 41,
            release_run_id: 17,
            repository_id: 9,
            engine_kind,
            repository_name: String::from("Revolutions"),
            repository_credentials_id: None,
            workspace_root_override: None,
            artifacts_root_override: None,
            build_target_id: 23,
            repository_source_mode: String::from("managed_repository"),
            repository_url: String::from("https://example.com/revolutions.git"),
            repository_local_path: None,
            git_tag: String::from("v1.2.3"),
            git_commit: Some(String::from("deadbeef")),
            source_metadata_json: String::from("{}"),
            target_name: String::from("Windows"),
            build_kind: BuildKind::Player,
            contract_json: String::from(
                r#"{"unity":{"targetPlatform":"StandaloneWindows64","buildMethod":"Builder.PerformWindows","editorVersion":"2022.3.20f1"}}"#,
            ),
            runner_type: String::from(RunnerFamily::HostNative.label()),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("players/game.zip")),
            config_json: String::from(
                r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
            ),
            engine_version: String::from("2022.3.20f1"),
            image_ref: String::from("host-native"),
            timeout_seconds: 900,
            status: String::from("queued"),
        }
    }

    fn test_host_capability_profile() -> HostCapabilityProfile {
        HostCapabilityProfile {
            platform: String::from("windows"),
            architecture: String::from("x86_64"),
            packaging_mode: String::from("development"),
            inside_wsl: false,
            git_tool: runtime_runner::unity::HostToolCapability {
                name: String::from("Git"),
                available: true,
                path: Some(String::from("git")),
                version: Some(String::from("2.49.0")),
                status: String::from("ready"),
                message: String::from("ready"),
            },
            unity_license: runtime_runner::unity::UnityLicenseDiagnostics {
                searched_paths: vec![String::from("C:/ProgramData/Unity/Unity_lic.ulf")],
                resolved_path: Some(String::from("C:/ProgramData/Unity/Unity_lic.ulf")),
                exists: true,
                status: String::from("ready"),
                message: String::from("ready"),
            },
            platform_prerequisites: Vec::new(),
            discovered_editors: Vec::new(),
            runner_selection: runtime_runner::unity::RunnerSelectionDiagnostics {
                selected_runner_family: Some(String::from(RunnerFamily::HostNative.label())),
                status: String::from("ready"),
                message: String::from("ready"),
            },
        }
    }

    fn test_build_target_input(
        name: &str,
        unity_target_platform: &str,
    ) -> CreateRepositoryProjectBuildTargetInput {
        CreateRepositoryProjectBuildTargetInput {
            name: String::from(name),
            build_kind: String::from("player"),
            runner_type: String::from(RunnerFamily::HostNative.label()),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(format!("Builds/{name}")),
            timeout_seconds: 900,
            enabled: true,
            contract_json: serde_json::json!({
                "unity": {
                    "targetPlatform": unity_target_platform,
                    "buildMethod": format!("Builder.Perform{name}"),
                    "editorVersion": "2022.3.20f1"
                }
            })
            .to_string(),
            runner_config_json: String::from(
                r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
            ),
        }
    }
}