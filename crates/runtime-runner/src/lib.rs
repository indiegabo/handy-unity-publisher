#![forbid(unsafe_code)]

use std::env;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};
use runtime_config::{HostPlatform, RuntimeDirectories};
use runtime_git::{GitAuthOptions, GitWorkspaceSyncRequest, GitWorkspaceSyncer};
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
const EXECUTION_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// Declares the runner family selected by the local runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerFamily {
    HostNative,
}

impl RunnerFamily {
    /// Returns the stable label for the bundled runner family.
    pub const fn label(self) -> &'static str {
        match self {
            Self::HostNative => "host-native",
        }
    }
}

/// Summarizes whether one host-native runner config can execute on the current host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostNativeRunnerDiagnostics {
    pub runner_family: String,
    pub unity_executable_path: Option<String>,
    pub unity_executable_exists: bool,
    pub unity_executable_is_file: bool,
    pub additional_argument_count: usize,
    pub environment_variable_count: usize,
    pub status: String,
    pub message: String,
}

/// Summarizes the current host state that affects Unity runner selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostCapabilityProfile {
    pub platform: String,
    pub architecture: String,
    pub packaging_mode: String,
    pub inside_wsl: bool,
    pub git_tool: HostToolCapability,
    pub unity_license: UnityLicenseDiagnostics,
    pub platform_prerequisites: Vec<HostToolCapability>,
    pub discovered_editors: Vec<DiscoveredUnityEditor>,
    pub runner_selection: RunnerSelectionDiagnostics,
}

/// Describes one host tool required by repository sync or runner launch flows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostToolCapability {
    pub name: String,
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub status: String,
    pub message: String,
}

/// Reports the local Unity licensing hints visible from the current execution context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnityLicenseDiagnostics {
    pub searched_paths: Vec<String>,
    pub resolved_path: Option<String>,
    pub exists: bool,
    pub status: String,
    pub message: String,
}

/// Describes one Unity editor discovered under the common host installation roots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredUnityEditor {
    pub version: String,
    pub source: String,
    pub install_root_path: String,
    pub executable_path: String,
    pub executable_exists: bool,
    pub executable_is_file: bool,
    pub supported_build_targets: Vec<String>,
    pub status: String,
    pub message: String,
}

/// Captures the runner family currently selected for the inspected host profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerSelectionDiagnostics {
    pub selected_runner_family: Option<String>,
    pub status: String,
    pub message: String,
}

/// Describes the filesystem layout prepared for one build run before execution starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedWorkspace {
    pub root_path: PathBuf,
    pub source_path: PathBuf,
    pub host_root_path: PathBuf,
    pub host_source_path: PathBuf,
    pub log_path: PathBuf,
    pub artifact_root_path: PathBuf,
    pub host_artifact_root_path: PathBuf,
}

/// Defines the repository snapshot that must be materialized for one build run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePreparationInput {
    pub build_run_id: i64,
    pub attempt_token: String,
    pub repository_name: String,
    pub repository_url: String,
    pub git_auth: GitAuthOptions,
    pub git_tag: String,
    pub workspace_root_override: Option<String>,
    pub artifacts_root_override: Option<String>,
}

/// Describes the joined metadata required to execute one host-native build run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub build_run_id: i64,
    pub release_run_id: i64,
    pub build_target_id: i64,
    pub repository_name: String,
    pub repository_url: String,
    pub git_tag: String,
    pub target_name: String,
    pub platform: String,
    pub runner_type: String,
    pub build_method: String,
    pub output_kind: Option<String>,
    pub output_path_template: Option<String>,
    pub unity_version: String,
    pub config_json: String,
    pub timeout_seconds: i64,
}

/// Captures the persisted filesystem paths produced by one attempted build execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub workspace_path: PathBuf,
    pub log_path: PathBuf,
    pub artifact_root_path: PathBuf,
    pub output_path: PathBuf,
}

/// Reports one in-flight execution heartbeat emitted while a host-native command is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProgress {
    pub message: String,
}

/// Describes one regular file discovered under a completed build artifact root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredArtifact {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub size_bytes: Option<i64>,
    pub checksum_sha256: Option<String>,
}

/// Bundles one prepared workspace, its execution plan, and the resolved output path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteRequest {
    pub plan: ExecutionPlan,
    pub workspace: PreparedWorkspace,
    pub output_path: PathBuf,
}

/// Captures one executor invocation including combined output and an optional terminal error.
#[derive(Debug)]
pub struct ExecutionOutcome {
    pub output: Vec<u8>,
    pub error: Option<io::Error>,
}

/// Captures one full build processing attempt after workspace preparation succeeds.
#[derive(Debug)]
pub struct ExecutionProcessOutcome {
    pub result: ExecutionResult,
    pub error: Option<io::Error>,
}

/// Receives coarse execution progress updates from long-running host-native commands.
pub trait ExecutionProgressReporter {
    fn heartbeat(&mut self, progress: ExecutionProgress);
}

#[derive(Debug, Default)]
struct NoopExecutionProgressReporter;

impl ExecutionProgressReporter for NoopExecutionProgressReporter {
    fn heartbeat(&mut self, _progress: ExecutionProgress) {}
}

/// Executes one prepared build request and returns combined stdout/stderr output.
pub trait Executor {
    fn execute(
        &self,
        request: &ExecuteRequest,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> ExecutionOutcome;
}

/// Prepares one build workspace, computes the canonical output path, and delegates execution.
#[derive(Debug)]
pub struct ExecutionProcessor<E> {
    preparer: WorkspacePreparer,
    executor: E,
}

impl<E> ExecutionProcessor<E>
where
    E: Executor,
{
    /// Creates one processor over the runtime filesystem layout and an injected executor.
    pub fn new(directories: &RuntimeDirectories, executor: E) -> Self {
        Self {
            preparer: WorkspacePreparer::new(directories),
            executor,
        }
    }

    /// Processes one build execution from workspace preparation through log capture.
    pub fn process(
        &self,
        plan: &ExecutionPlan,
        preparation: &WorkspacePreparationInput,
    ) -> io::Result<ExecutionProcessOutcome> {
        let workspace = self.prepare_workspace(preparation)?;
        let mut reporter = NoopExecutionProgressReporter;
        self.execute_prepared(plan, workspace, &mut reporter)
    }

    /// Prepares the workspace only, allowing callers to track checkout separately.
    pub fn prepare_workspace(
        &self,
        preparation: &WorkspacePreparationInput,
    ) -> io::Result<PreparedWorkspace> {
        self.preparer.prepare(preparation)
    }

    /// Executes one already-prepared workspace and reports heartbeats while Unity is running.
    pub fn execute_prepared(
        &self,
        plan: &ExecutionPlan,
        workspace: PreparedWorkspace,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> io::Result<ExecutionProcessOutcome> {
        let mut canonical_plan = plan.clone();
        canonical_plan.output_path_template = Some(artifact_output_relative_path(&canonical_plan));

        let output_path = resolve_runtime_output_path(&workspace, &canonical_plan)?;
        cleanup_previous_artifact_output(&output_path)?;

        let result = ExecutionResult {
            workspace_path: workspace.root_path.clone(),
            log_path: workspace.log_path.clone(),
            artifact_root_path: workspace.artifact_root_path.clone(),
            output_path: output_path.clone(),
        };

        let outcome = self.executor.execute(
            &ExecuteRequest {
                plan: canonical_plan,
                workspace,
                output_path,
            },
            reporter,
        );
        fs::write(&result.log_path, &outcome.output)?;

        Ok(ExecutionProcessOutcome {
            result,
            error: outcome
                .error
                .map(|error| enrich_execution_error(error, &outcome.output)),
        })
    }
}

/// Walks one artifact root and returns every regular file that must be recorded durably.
pub fn discover_artifacts(root_path: &Path) -> io::Result<Vec<DiscoveredArtifact>> {
    if root_path.as_os_str().is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "artifact root path must not be empty",
        ));
    }

    let metadata = fs::metadata(root_path).map_err(|error| {
        io::Error::new(
            ErrorKind::NotFound,
            format!("stat artifact root {:?}: {error}", root_path.display()),
        )
    })?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("artifact root {:?} is not a directory", root_path.display()),
        ));
    }

    let mut artifacts = Vec::new();
    collect_discovered_artifacts(root_path, root_path, &mut artifacts)?;
    if artifacts.is_empty() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("no files found under {:?}", root_path.display()),
        ));
    }

    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(artifacts)
}

/// Executes one host-native Unity command using the configured local executable path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HostNativeUnityExecutor;

impl HostNativeUnityExecutor {
    /// Creates the default host-native Unity executor.
    pub const fn new() -> Self {
        Self
    }
}

impl Executor for HostNativeUnityExecutor {
    fn execute(
        &self,
        request: &ExecuteRequest,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> ExecutionOutcome {
        match execute_host_native_unity(request, reporter) {
            Ok(output) => ExecutionOutcome {
                output,
                error: None,
            },
            Err((output, error)) => ExecutionOutcome {
                output,
                error: Some(error),
            },
        }
    }
}

/// Resolves one stored host-native execution plan into the concrete local runner invocation.
pub fn resolve_host_native_execution_plan(
    plan: &ExecutionPlan,
    profile: &HostCapabilityProfile,
) -> io::Result<ExecutionPlan> {
    if !supports_host_native_runner_type(&plan.runner_type) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "runner type {:?} is not supported by the host-native executor",
                plan.runner_type
            ),
        ));
    }

    let mut config_json = parse_host_native_runner_config_json(&plan.config_json)?;
    let unity_executable_path = configured_unity_executable_path(&config_json)
        .map(str::to_owned)
        .map(Ok)
        .unwrap_or_else(|| resolve_discovered_unity_executable(plan, profile))?;
    config_json.insert(
        String::from("unity_executable_path"),
        JsonValue::String(unity_executable_path),
    );

    let mut resolved = plan.clone();
    resolved.runner_type = profile
        .runner_selection
        .selected_runner_family
        .clone()
        .filter(|runner_type| supports_host_native_runner_type(runner_type))
        .unwrap_or_else(|| String::from(RunnerFamily::HostNative.label()));
    resolved.config_json = serde_json::to_string(&JsonValue::Object(config_json))
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    Ok(resolved)
}

/// Allocates deterministic per-run directories and checks out one repository tag into source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePreparer {
    directories: RuntimeDirectories,
    syncer: GitWorkspaceSyncer,
}

impl WorkspacePreparer {
    /// Creates the default filesystem-backed workspace preparer for the runtime directories.
    pub fn new(directories: &RuntimeDirectories) -> Self {
        Self {
            directories: directories.clone(),
            syncer: GitWorkspaceSyncer::new(),
        }
    }

    /// Resolves the deterministic filesystem layout for one build run without touching Git.
    pub fn plan(&self, input: &WorkspacePreparationInput) -> io::Result<PreparedWorkspace> {
        if input.build_run_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "build run id must be greater than zero",
            ));
        }

        let attempt_token = require_non_empty(&input.attempt_token, "attempt token")?;
        let repository_url = require_non_empty(&input.repository_url, "repository url")?;
        let git_tag = require_non_empty(&input.git_tag, "git tag")?;
        let workspace_name = format!("build-run-{}-{attempt_token}", input.build_run_id);
        let runs_root_path = self.resolve_runs_root(input)?;
        let artifacts_root_path = self.resolve_artifacts_root(input)?;

        let root_path = runs_root_path.join(&workspace_name);
        let host_root_path = root_path.clone();
        let source_path = root_path.join("source");
        let host_source_path = host_root_path.join("source");
        let log_path = root_path.join("logs").join("unity-build.log");
        let artifact_root_path = artifacts_root_path.join(artifact_release_dir_name(
            &input.repository_name,
            &repository_url,
            &git_tag,
        ));
        let host_artifact_root_path = artifact_root_path.clone();

        Ok(PreparedWorkspace {
            root_path,
            source_path,
            host_root_path,
            host_source_path,
            log_path,
            artifact_root_path,
            host_artifact_root_path,
        })
    }

    /// Creates isolated directories and checks out the requested repository tag.
    pub fn prepare(&self, input: &WorkspacePreparationInput) -> io::Result<PreparedWorkspace> {
        let repository_url = require_non_empty(&input.repository_url, "repository url")?;
        let git_tag = require_non_empty(&input.git_tag, "git tag")?;
        let planned = self.plan(input)?;
        self.directories.ensure_exists()?;

        for directory in [
            planned.root_path.as_path(),
            planned
                .log_path
                .parent()
                .expect("workspace log path should have a parent"),
            planned.artifact_root_path.as_path(),
        ] {
            fs::create_dir_all(directory)?;
        }

        self.syncer.sync_tag(&GitWorkspaceSyncRequest {
            repository_url,
            workspace_path: planned.source_path.clone(),
            git_tag,
            auth: input.git_auth.clone(),
        })?;

        Ok(planned)
    }

    fn resolve_runs_root(&self, input: &WorkspacePreparationInput) -> io::Result<PathBuf> {
        Ok(match normalize_override_path(
            input.workspace_root_override.as_deref(),
            "workspace root override",
        )? {
            Some(workspace_root) => workspace_root.join("runs"),
            None => self.directories.runs_dir.clone(),
        })
    }

    fn resolve_artifacts_root(&self, input: &WorkspacePreparationInput) -> io::Result<PathBuf> {
        Ok(match normalize_override_path(
            input.artifacts_root_override.as_deref(),
            "artifacts root override",
        )? {
            Some(artifacts_root) => artifacts_root,
            None => self.directories.artifacts_dir.clone(),
        })
    }
}

/// Generates one filesystem-safe token for a single build execution attempt.
pub fn next_workspace_attempt_token() -> io::Result<String> {
    let issued_at_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();

    Ok(format!("attempt-{}-{issued_at_nanos}", std::process::id()))
}

fn require_non_empty(value: &str, label: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{label} must not be empty"),
        ));
    }

    Ok(trimmed.to_owned())
}

fn normalize_override_path(value: Option<&str>, label: &str) -> io::Result<Option<PathBuf>> {
    let Some(value) = value else {
        return Ok(None);
    };

    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{label} must be an absolute path"),
        ));
    }

    Ok(Some(path))
}

fn execute_host_native_unity(
    request: &ExecuteRequest,
    reporter: &mut dyn ExecutionProgressReporter,
) -> Result<Vec<u8>, (Vec<u8>, io::Error)> {
    if !supports_host_native_runner_type(&request.plan.runner_type) {
        return Err((
            Vec::new(),
            io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "runner type {:?} is not supported by the host-native executor",
                    request.plan.runner_type
                ),
            ),
        ));
    }

    let build_method = match require_non_empty(&request.plan.build_method, "build method") {
        Ok(build_method) => build_method,
        Err(error) => return Err((Vec::new(), error)),
    };
    let output_path = request.output_path.display().to_string();
    let build_target = match platform_to_unity_build_target(&request.plan.platform) {
        Ok(build_target) => build_target,
        Err(error) => return Err((Vec::new(), error)),
    };
    let config = match parse_host_native_runner_config(&request.plan.config_json) {
        Ok(config) => config,
        Err(error) => return Err((Vec::new(), error)),
    };
    let timeout = match resolve_execution_timeout(request.plan.timeout_seconds) {
        Ok(timeout) => timeout,
        Err(error) => return Err((Vec::new(), error)),
    };
    let editor_log_path = default_unity_editor_log_path();

    let mut command = Command::new(&config.unity_executable_path);
    command.current_dir(&request.workspace.source_path);
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.args(&config.additional_arguments);
    let log_path = request.workspace.log_path.display().to_string();
    command.args([
        "-batchmode",
        "-quit",
        "-nographics",
        "-logFile",
        log_path.as_str(),
        "-projectPath",
        &request.workspace.source_path.display().to_string(),
        "-buildTarget",
        build_target.as_str(),
        "-executeMethod",
        build_method.as_str(),
        "-hgbOutputPath",
        output_path.as_str(),
    ]);
    command.env("HGB_OUTPUT_PATH", &output_path);
    if let Some(output_kind) = normalized_optional_string(&request.plan.output_kind)
        .filter(|output_kind| !output_kind.eq_ignore_ascii_case("archive"))
    {
        command.env("HGB_OUTPUT_KIND", output_kind);
    }
    command.env("HGB_BUILD_RUN_ID", request.plan.build_run_id.to_string());
    command.env("HGB_RELEASE_RUN_ID", request.plan.release_run_id.to_string());
    command.env("HGB_BUILD_TARGET_ID", request.plan.build_target_id.to_string());
    command.env("HGB_LOG_PATH", &log_path);
    command.env("HGB_TARGET_PLATFORM", request.plan.platform.trim());
    command.env("HGB_UNITY_VERSION", request.plan.unity_version.trim());

    for (key, value) in config.environment {
        if key.trim().is_empty() {
            continue;
        }

        command.env(key, value);
    }

    let log_preamble = execution_log_preamble(
        request,
        &config.unity_executable_path,
        build_target.as_str(),
        build_method.as_str(),
        output_path.as_str(),
        log_path.as_str(),
        config.additional_arguments.len(),
        editor_log_path.as_deref(),
    );

    let output = match execute_command_with_timeout(
        &mut command,
        timeout,
        &request.workspace.log_path,
        &log_preamble,
        editor_log_path.as_deref(),
        reporter,
    ) {
        Ok(output) => output,
        Err(error) => {
            let classified =
                classify_execution_error(error.error, &error.output, error.exit_status);
            return Err((
                error.output,
                classified,
            ))
        }
    };

    if output.status.success() {
        return Ok(output.output);
    }

    Err((
        output.output.clone(),
        classify_execution_error(
            io::Error::other(format!(
                "host-native unity runner exited with code {:?}",
                output.status.code()
            )),
            &output.output,
            Some(output.status),
        ),
    ))
}

fn supports_host_native_runner_type(runner_type: &str) -> bool {
    let normalized = runner_type.trim();
    normalized == RunnerFamily::HostNative.label()
        || normalized == selected_host_runner_family(HostPlatform::Windows)
        || normalized == selected_host_runner_family(HostPlatform::MacOS)
        || normalized == selected_host_runner_family(HostPlatform::Linux)
}

fn parse_host_native_runner_config_json(
    config_json: &str,
) -> io::Result<JsonMap<String, JsonValue>> {
    let trimmed = config_json.trim();
    if trimmed.is_empty() {
        return Ok(JsonMap::new());
    }

    match serde_json::from_str::<JsonValue>(trimmed)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?
    {
        JsonValue::Object(map) => Ok(map),
        _ => Err(io::Error::new(
            ErrorKind::InvalidData,
            "host-native runner config_json must be a JSON object",
        )),
    }
}

fn configured_unity_executable_path(
    config_json: &JsonMap<String, JsonValue>,
) -> Option<&str> {
    config_json
        .get("unity_executable_path")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn resolve_discovered_unity_executable(
    plan: &ExecutionPlan,
    profile: &HostCapabilityProfile,
) -> io::Result<String> {
    let requested_version = plan.unity_version.trim();
    let discovered = profile
        .discovered_editors
        .iter()
        .filter(|editor| editor.executable_exists && editor.executable_is_file)
        .collect::<Vec<_>>();

    if requested_version.is_empty() {
        return match discovered.as_slice() {
            [editor] => Ok(editor.executable_path.clone()),
            [] => Err(io::Error::new(
                ErrorKind::NotFound,
                "host capability discovery did not find any runnable Unity editor and the build plan does not declare a unity_version",
            )),
            _ => Err(io::Error::new(
                ErrorKind::InvalidInput,
                "host capability discovery found multiple Unity editors, but the build plan does not declare a unity_version to disambiguate them",
            )),
        };
    }

    if let Some(editor) = discovered
        .iter()
        .find(|editor| editor.version.eq_ignore_ascii_case(requested_version))
    {
        return Ok(editor.executable_path.clone());
    }

    let available_versions = discovered
        .iter()
        .map(|editor| editor.version.as_str())
        .collect::<Vec<_>>();
    let detail = if available_versions.is_empty() {
        String::from("none discovered")
    } else {
        available_versions.join(", ")
    };
    Err(io::Error::new(
        ErrorKind::NotFound,
        format!(
            "no discovered Unity editor matched requested version {:?}; available versions: {}",
            requested_version, detail
        ),
    ))
}

fn resolve_execution_timeout(timeout_seconds: i64) -> io::Result<Duration> {
    let timeout_seconds = u64::try_from(timeout_seconds).map_err(|_| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "execution timeout seconds must be greater than zero",
        )
    })?;
    if timeout_seconds == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "execution timeout seconds must be greater than zero",
        ));
    }

    Ok(Duration::from_secs(timeout_seconds))
}

struct CommandExecutionOutput {
    output: Vec<u8>,
    status: ExitStatus,
}

struct CommandExecutionError {
    output: Vec<u8>,
    error: io::Error,
    exit_status: Option<ExitStatus>,
}

fn execution_log_preamble(
    request: &ExecuteRequest,
    unity_executable_path: &str,
    build_target: &str,
    build_method: &str,
    output_path: &str,
    log_path: &str,
    additional_argument_count: usize,
    editor_log_path: Option<&Path>,
) -> Vec<u8> {
    let mut preamble = String::new();
    preamble.push_str("handy-unity-builder host-native execution\n");
    preamble.push_str(&format!(
        "unity_executable_path: {}\n",
        unity_executable_path.trim()
    ));
    preamble.push_str(&format!(
        "workspace_source_path: {}\n",
        request.workspace.source_path.display()
    ));
    preamble.push_str(&format!("requested_log_path: {log_path}\n"));
    preamble.push_str(&format!("artifact_output_path: {output_path}\n"));
    preamble.push_str(&format!("build_target: {}\n", build_target.trim()));
    preamble.push_str(&format!("build_method: {}\n", build_method.trim()));
    preamble.push_str(&format!(
        "additional_argument_count: {additional_argument_count}\n"
    ));
    if let Some(editor_log_path) = editor_log_path {
        preamble.push_str(&format!(
            "editor_fallback_log_path: {}\n",
            editor_log_path.display()
        ));
    }
    preamble.push_str("\n");

    preamble.into_bytes()
}

fn execute_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
    log_path: &Path,
    log_preamble: &[u8],
    editor_log_path: Option<&Path>,
    reporter: &mut dyn ExecutionProgressReporter,
) -> Result<CommandExecutionOutput, CommandExecutionError> {
    let mut child = command.spawn().map_err(|error| CommandExecutionError {
        output: Vec::new(),
        error: io::Error::other(format!("spawn host-native unity runner: {error}")),
        exit_status: None,
    })?;

    let (status, timed_out) = match wait_for_child(&mut child, timeout, log_path, reporter) {
        Ok(result) => result,
        Err(error) => {
            return Err(CommandExecutionError {
                output: read_command_log(log_path, log_preamble, editor_log_path),
                error,
                exit_status: None,
            })
        }
    };
    let output = read_command_log(log_path, log_preamble, editor_log_path);
    if timed_out {
        return Err(CommandExecutionError {
            output,
            error: io::Error::new(
                ErrorKind::TimedOut,
                format!(
                    "host-native unity runner exceeded {}s timeout",
                    timeout.as_secs()
                ),
            ),
            exit_status: Some(status),
        });
    }

    Ok(CommandExecutionOutput { output, status })
}

fn read_command_log(
    log_path: &Path,
    log_preamble: &[u8],
    editor_log_path: Option<&Path>,
) -> Vec<u8> {
    let mut output = Vec::new();
    output.extend_from_slice(log_preamble);

    match fs::read(log_path) {
        Ok(contents) if contains_visible_text(&contents) => {
            output.extend_from_slice(&contents);
            return output;
        }
        Ok(_) | Err(_) => {
            output.extend_from_slice(
                format!(
                    "execution log file {} was not written by the host-native unity runner\n",
                    log_path.display()
                )
                .as_bytes(),
            );
        }
    }

    if let Some(editor_log_path) = editor_log_path {
        output.extend_from_slice(
            format!(
                "attempting fallback Unity Editor log at {}\n",
                editor_log_path.display()
            )
            .as_bytes(),
        );
        if let Some(tail) = read_log_tail(editor_log_path, 32 * 1024) {
            if contains_visible_text(&tail) {
                output.extend_from_slice(b"\n--- Unity Editor.log tail ---\n");
                output.extend_from_slice(&tail);
                return output;
            }
        }
    }

    output
}

fn contains_visible_text(contents: &[u8]) -> bool {
    contents.iter().any(|byte| !byte.is_ascii_whitespace())
}

fn read_log_tail(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    let contents = fs::read(path).ok()?;
    if contents.len() <= max_bytes {
        return Some(contents);
    }

    Some(contents[contents.len() - max_bytes..].to_vec())
}

fn last_meaningful_log_line(path: &Path, max_bytes: usize) -> Option<String> {
    let contents = read_log_tail(path, max_bytes)?;
    String::from_utf8_lossy(&contents)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(normalize_failure_summary_line)
}

fn default_unity_editor_log_path() -> Option<PathBuf> {
    match HostPlatform::current() {
        HostPlatform::Windows => env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("APPDATA"))
            .map(PathBuf::from)
            .map(|root| root.join("Unity").join("Editor").join("Editor.log")),
        HostPlatform::MacOS => env::var_os("HOME")
            .map(PathBuf::from)
            .map(|root| root.join("Library").join("Logs").join("Unity").join("Editor.log")),
        HostPlatform::Linux => env::var_os("HOME")
            .map(PathBuf::from)
            .map(|root| root.join(".config").join("unity3d").join("Editor.log")),
    }
}

fn wait_for_child(
    child: &mut Child,
    timeout: Duration,
    log_path: &Path,
    reporter: &mut dyn ExecutionProgressReporter,
) -> io::Result<(ExitStatus, bool)> {
    let started_at = Instant::now();
    let mut last_heartbeat_at = Instant::now() - EXECUTION_HEARTBEAT_INTERVAL;
    let mut last_message = String::new();
    let mut last_log_size = None;

    loop {
        if let Some(status) = child.try_wait()? {
            return Ok((status, false));
        }

        if last_heartbeat_at.elapsed() >= EXECUTION_HEARTBEAT_INTERVAL {
            let (message, log_size) = match fs::metadata(log_path) {
                Ok(metadata) if metadata.is_file() && metadata.len() > 0 => {
                    let log_size = metadata.len();
                    let message = last_meaningful_log_line(log_path, 4 * 1024)
                        .filter(|line| !line.is_empty())
                        .map(|line| format!("unity log: {line}"))
                        .unwrap_or_else(|| {
                            format!("unity process running; log size {log_size} bytes")
                        });
                    (message, Some(log_size))
                }
                Ok(_) | Err(_) => (
                    format!(
                        "unity process running for {}s; waiting for log output",
                        started_at.elapsed().as_secs()
                    ),
                    None,
                ),
            };

            if message != last_message || log_size != last_log_size {
                reporter.heartbeat(ExecutionProgress { message: message.clone() });
                last_message = message;
                last_log_size = log_size;
            }

            last_heartbeat_at = Instant::now();
        }

        if started_at.elapsed() >= timeout {
            terminate_child_process(child)?;

            return child.wait().map(|status| (status, true));
        }

        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn terminate_child_process(child: &mut Child) -> io::Result<()> {
    #[cfg(windows)]
    {
        match Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) if status.success() => return Ok(()),
            Ok(_) | Err(_) => {}
        }
    }

    match child.kill() {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::InvalidInput => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "terminate timed out host-native unity runner: {error}"
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionFailureClass {
    Timeout,
    Licensing,
    Permission,
    Compile,
    Package,
    Runtime,
    Unknown,
}

impl ExecutionFailureClass {
    fn label(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Licensing => "licensing",
            Self::Permission => "permission",
            Self::Compile => "compile",
            Self::Package => "package",
            Self::Runtime => "runtime",
            Self::Unknown => "unknown",
        }
    }

    fn error_kind(self, fallback: ErrorKind) -> ErrorKind {
        match self {
            Self::Timeout => ErrorKind::TimedOut,
            Self::Permission => ErrorKind::PermissionDenied,
            Self::Compile | Self::Package => ErrorKind::InvalidData,
            _ => fallback,
        }
    }
}

fn classify_execution_error(
    error: io::Error,
    output: &[u8],
    exit_status: Option<ExitStatus>,
) -> io::Error {
    let summary = summarize_execution_failure(output);
    let classification = classify_execution_failure_class(&error, &summary);
    let base_message = if error.kind() == ErrorKind::TimedOut
        || summary.is_empty()
        || error
            .to_string()
            .to_ascii_lowercase()
            .contains(&summary.to_ascii_lowercase())
    {
        error.to_string()
    } else if let Some(exit_status) = exit_status {
        format!(
            "host-native unity runner exited with code {:?}: {}",
            exit_status.code(),
            summary
        )
    } else {
        format!("{}: {}", error, summary)
    };

    io::Error::new(
        classification.error_kind(error.kind()),
        format!("{}: {}", classification.label(), base_message),
    )
}

fn classify_execution_failure_class(
    error: &io::Error,
    summary: &str,
) -> ExecutionFailureClass {
    if error.kind() == ErrorKind::TimedOut {
        return ExecutionFailureClass::Timeout;
    }
    if error.kind() == ErrorKind::PermissionDenied {
        return ExecutionFailureClass::Permission;
    }

    let lowered = format!("{} {}", error, summary).to_ascii_lowercase();
    if lowered.contains("no valid unity editor license found")
        || lowered.contains("please activate your license")
        || lowered.contains("licensing initialization failed")
    {
        return ExecutionFailureClass::Licensing;
    }
    if lowered.contains("permission denied")
        || lowered.contains("access to the path")
        || lowered.contains("access is denied")
        || lowered.contains("sharing violation")
        || lowered.contains("unauthorizedaccessexception")
    {
        return ExecutionFailureClass::Permission;
    }
    if lowered.contains("error cs")
        || lowered.contains("compiler error")
        || lowered.contains("compilation failed")
        || lowered.contains("scripts have compiler errors")
        || lowered.contains("all compiler errors have to be fixed")
    {
        return ExecutionFailureClass::Compile;
    }
    if lowered.contains("failed to resolve packages")
        || lowered.contains("package manager")
        || lowered.contains("manifest parse error")
        || lowered.contains("package resolution")
        || lowered.contains("unable to add package")
    {
        return ExecutionFailureClass::Package;
    }
    if lowered.contains("exception") || lowered.contains("failed") {
        return ExecutionFailureClass::Runtime;
    }

    ExecutionFailureClass::Unknown
}

fn parse_host_native_runner_config(config_json: &str) -> io::Result<HostNativeRunnerConfig> {
    let trimmed = config_json.trim();
    let config: HostNativeRunnerConfig = serde_json::from_str(if trimmed.is_empty() {
        "{}"
    } else {
        trimmed
    })
    .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;

    let unity_executable_path = require_non_empty(
        &config.unity_executable_path,
        "host-native unity executable path",
    )?;
    Ok(HostNativeRunnerConfig {
        unity_executable_path,
        additional_arguments: config.additional_arguments,
        environment: config.environment,
    })
}

/// Inspects one host-native runner config without exposing environment values.
pub fn diagnose_host_native_runner_config(config_json: &str) -> HostNativeRunnerDiagnostics {
    match parse_host_native_runner_config(config_json) {
        Ok(config) => build_host_native_runner_diagnostics(&config),
        Err(error) => HostNativeRunnerDiagnostics {
            runner_family: String::from(RunnerFamily::HostNative.label()),
            unity_executable_path: None,
            unity_executable_exists: false,
            unity_executable_is_file: false,
            additional_argument_count: 0,
            environment_variable_count: 0,
            status: String::from("invalid_config"),
            message: error.to_string(),
        },
    }
}

fn build_host_native_runner_diagnostics(
    config: &HostNativeRunnerConfig,
) -> HostNativeRunnerDiagnostics {
    let unity_executable_path = PathBuf::from(&config.unity_executable_path);
    let mut diagnostics = HostNativeRunnerDiagnostics {
        runner_family: String::from(RunnerFamily::HostNative.label()),
        unity_executable_path: Some(config.unity_executable_path.clone()),
        unity_executable_exists: false,
        unity_executable_is_file: false,
        additional_argument_count: config.additional_arguments.len(),
        environment_variable_count: config.environment.len(),
        status: String::new(),
        message: String::new(),
    };

    match fs::metadata(&unity_executable_path) {
        Ok(metadata) if metadata.is_file() => {
            diagnostics.unity_executable_exists = true;
            diagnostics.unity_executable_is_file = true;
            diagnostics.status = String::from("ready");
            diagnostics.message = String::from(
                "host-native unity executable path resolves to a regular file",
            );
        }
        Ok(_) => {
            diagnostics.unity_executable_exists = true;
            diagnostics.status = String::from("invalid_path");
            diagnostics.message = String::from(
                "host-native unity executable path resolves but is not a regular file",
            );
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            diagnostics.status = String::from("missing_executable");
            diagnostics.message = String::from(
                "host-native unity executable path does not exist on the current host",
            );
        }
        Err(error) => {
            diagnostics.status = String::from("inaccessible_path");
            diagnostics.message = error.to_string();
        }
    }

    diagnostics
}

/// Returns the common Unity installation roots for one host platform.
pub fn default_unity_discovery_root_paths(platform: HostPlatform) -> Vec<PathBuf> {
    match platform {
        HostPlatform::Windows => vec![
            PathBuf::from("C:/Program Files/Unity/Hub/Editor"),
            PathBuf::from("C:/Program Files/Unity/Editor"),
        ],
        HostPlatform::MacOS => vec![
            PathBuf::from("/Applications/Unity/Hub/Editor"),
            PathBuf::from("/Applications/Unity"),
        ],
        HostPlatform::Linux => vec![
            PathBuf::from("/opt/Unity/Hub/Editor"),
            PathBuf::from("/opt/Unity/Editor"),
        ],
    }
}

/// Inspects the local host and summarizes the capability profile used by Unity runner selection.
pub fn inspect_host_capability_profile(platform: HostPlatform) -> HostCapabilityProfile {
    inspect_host_capability_profile_with_input(
        platform,
        CapabilityInspectionInput::current(platform),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapabilityInspectionInput {
    architecture: String,
    packaging_mode: String,
    inside_wsl: bool,
    path_entries: Vec<PathBuf>,
    discovery_roots: Vec<PathBuf>,
    unity_license_paths: Vec<PathBuf>,
}

impl CapabilityInspectionInput {
    fn current(platform: HostPlatform) -> Self {
        let path_entries = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect())
            .unwrap_or_default();

        Self {
            architecture: String::from(std::env::consts::ARCH),
            packaging_mode: if cfg!(debug_assertions) {
                String::from("development")
            } else {
                String::from("packaged")
            },
            inside_wsl: detect_wsl_from_environment(),
            path_entries,
            discovery_roots: default_unity_discovery_root_paths(platform),
            unity_license_paths: default_unity_license_paths(platform),
        }
    }
}

fn inspect_host_capability_profile_with_input(
    platform: HostPlatform,
    input: CapabilityInspectionInput,
) -> HostCapabilityProfile {
    let git_tool = detect_git_tool(&input.path_entries);
    let platform_prerequisites =
        detect_platform_prerequisites(platform, &input.path_entries);
    let unity_license = detect_unity_license(&input.unity_license_paths);
    let discovered_editors = discover_unity_editors(platform, &input.discovery_roots);
    let runner_selection = select_runner_family(
        platform,
        input.inside_wsl,
        &git_tool,
        &platform_prerequisites,
        &unity_license,
        &discovered_editors,
    );

    HostCapabilityProfile {
        platform: String::from(platform.as_str()),
        architecture: input.architecture,
        packaging_mode: input.packaging_mode,
        inside_wsl: input.inside_wsl,
        git_tool,
        unity_license,
        platform_prerequisites,
        discovered_editors,
        runner_selection,
    }
}

fn detect_wsl_from_environment() -> bool {
    if std::env::var_os("WSL_INTEROP").is_some()
        || std::env::var_os("WSL_DISTRO_NAME").is_some()
    {
        return true;
    }

    ["/proc/sys/kernel/osrelease", "/proc/version"]
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .any(|content| content.to_ascii_lowercase().contains("microsoft"))
}

fn default_unity_license_paths(platform: HostPlatform) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    match platform {
        HostPlatform::Windows => {
            push_env_path(
                &mut candidates,
                "ProgramData",
                ["Unity", "Unity_lic.ulf"],
            );
            push_env_path(
                &mut candidates,
                "LOCALAPPDATA",
                ["Unity", "licenses", "Unity_lic.ulf"],
            );
            push_env_path(
                &mut candidates,
                "APPDATA",
                ["Unity", "licenses", "Unity_lic.ulf"],
            );
            candidates.push(
                PathBuf::from("C:/ProgramData")
                    .join("Unity")
                    .join("Unity_lic.ulf"),
            );
        }
        HostPlatform::MacOS => {
            candidates.push(
                PathBuf::from("/Library/Application Support/Unity")
                    .join("Unity_lic.ulf"),
            );
            push_env_path(
                &mut candidates,
                "HOME",
                ["Library", "Application Support", "Unity", "Unity_lic.ulf"],
            );
        }
        HostPlatform::Linux => {
            push_env_path(
                &mut candidates,
                "HOME",
                [".local", "share", "unity3d", "Unity", "Unity_lic.ulf"],
            );
            push_env_path(
                &mut candidates,
                "HOME",
                [".config", "unity3d", "Unity", "Unity_lic.ulf"],
            );
        }
    }

    deduplicate_paths(candidates)
}

fn push_env_path<const N: usize>(
    values: &mut Vec<PathBuf>,
    env_key: &str,
    segments: [&str; N],
) {
    let Some(base) = std::env::var_os(env_key).map(PathBuf::from) else {
        return;
    };

    let mut candidate = base;
    for segment in segments {
        candidate.push(segment);
    }
    values.push(candidate);
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduplicated = Vec::new();
    let mut seen = BTreeSet::new();

    for path in paths {
        let key = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        if seen.insert(key) {
            deduplicated.push(path);
        }
    }

    deduplicated
}

fn detect_git_tool(path_entries: &[PathBuf]) -> HostToolCapability {
    let Some(path) = resolve_command_path("git", path_entries) else {
        return HostToolCapability {
            name: String::from("Git"),
            available: false,
            path: None,
            version: None,
            status: String::from("error_missing"),
            message: String::from(
                "Git was not found on PATH. Repository sync and release dispatch cannot run on this host.",
            ),
        };
    };

    let version = probe_command_version(&path, &["--version"]);
    let path_string = path.display().to_string();
    let message = match version.as_deref() {
        Some(version) => {
            format!("Git {version} detected at {path_string}.")
        }
        None => {
            format!(
                "Git was detected at {path_string}, but the version probe did not return a readable result."
            )
        }
    };

    HostToolCapability {
        name: String::from("Git"),
        available: true,
        path: Some(path_string),
        version,
        status: String::from("ready"),
        message,
    }
}

fn detect_platform_prerequisites(
    platform: HostPlatform,
    path_entries: &[PathBuf],
) -> Vec<HostToolCapability> {
    match platform {
        HostPlatform::Windows => vec![
            detect_host_tool(
                "PowerShell",
                "powershell",
                path_entries,
                "PowerShell is available for Windows-local orchestration helpers.",
                "PowerShell was not found on PATH. Some Windows-local helper flows may fail.",
            ),
            detect_host_tool(
                "Command Prompt",
                "cmd",
                path_entries,
                "cmd.exe is available for Windows shell fallback commands.",
                "cmd.exe was not found on PATH. Windows shell fallbacks may fail.",
            ),
        ],
        HostPlatform::MacOS | HostPlatform::Linux => vec![detect_host_tool(
            "POSIX shell",
            "sh",
            path_entries,
            "A POSIX shell is available for local helper flows.",
            "No POSIX shell was found on PATH.",
        )],
    }
}

fn detect_host_tool(
    name: &str,
    command_name: &str,
    path_entries: &[PathBuf],
    ready_message: &str,
    missing_message: &str,
) -> HostToolCapability {
    let Some(path) = resolve_command_path(command_name, path_entries) else {
        return HostToolCapability {
            name: String::from(name),
            available: false,
            path: None,
            version: None,
            status: String::from("error_missing"),
            message: String::from(missing_message),
        };
    };

    HostToolCapability {
        name: String::from(name),
        available: true,
        path: Some(path.display().to_string()),
        version: None,
        status: String::from("ready"),
        message: String::from(ready_message),
    }
}

fn resolve_command_path(command_name: &str, path_entries: &[PathBuf]) -> Option<PathBuf> {
    let explicit_path = PathBuf::from(command_name);
    if explicit_path.components().count() > 1 || explicit_path.is_absolute() {
        return resolve_explicit_command_path(&explicit_path);
    }

    let candidates = command_candidate_names(command_name);
    for directory in path_entries {
        for candidate in &candidates {
            let path = directory.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    None
}

fn resolve_explicit_command_path(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }

    if cfg!(windows) && path.extension().is_none() {
        for extension in ["exe", "cmd", "bat", "com"] {
            let candidate = path.with_extension(extension);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn command_candidate_names(command_name: &str) -> Vec<String> {
    let mut candidates = vec![String::from(command_name)];

    if cfg!(windows) && Path::new(command_name).extension().is_none() {
        for extension in ["exe", "cmd", "bat", "com"] {
            candidates.push(format!("{command_name}.{extension}"));
        }
    }

    candidates
}

fn probe_command_version(path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let version = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_start_matches("git version")
        .trim()
        .to_owned();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

fn detect_unity_license(paths: &[PathBuf]) -> UnityLicenseDiagnostics {
    let resolved_path = paths.iter().find(|path| path.is_file()).cloned();
    let searched_paths = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();

    match resolved_path {
        Some(path) => UnityLicenseDiagnostics {
            searched_paths,
            resolved_path: Some(path.display().to_string()),
            exists: true,
            status: String::from("ready"),
            message: String::from(
                "A common local Unity license file was detected for this execution context.",
            ),
        },
        None => UnityLicenseDiagnostics {
            searched_paths,
            resolved_path: None,
            exists: false,
            status: String::from("warning_not_detected"),
            message: String::from(
                "No common local Unity license file was detected. Batchmode builds may still work if this host is already activated through Unity Hub or another local entitlement flow.",
            ),
        },
    }
}

fn discover_unity_editors(
    platform: HostPlatform,
    discovery_roots: &[PathBuf],
) -> Vec<DiscoveredUnityEditor> {
    let mut editors = BTreeMap::new();

    for root in discovery_roots {
        if !root.exists() {
            continue;
        }

        if is_unity_hub_root(root) {
            if let Ok(entries) = fs::read_dir(root) {
                for entry in entries.flatten() {
                    let install_root = entry.path();
                    if !install_root.is_dir() {
                        continue;
                    }

                    let version = entry.file_name().to_string_lossy().trim().to_owned();
                    if version.is_empty() {
                        continue;
                    }

                    let editor = build_discovered_unity_editor(
                        platform,
                        version,
                        String::from("unity-hub"),
                        install_root.clone(),
                        hub_unity_executable_path(platform, &install_root),
                    );
                    editors.entry(editor_key(&editor)).or_insert(editor);
                }
            }
            continue;
        }

        let editor = build_discovered_unity_editor(
            platform,
            String::from("standalone-install"),
            String::from("standalone-install"),
            root.clone(),
            standalone_unity_executable_path(platform, root),
        );
        editors.entry(editor_key(&editor)).or_insert(editor);
    }

    let mut discovered = editors.into_values().collect::<Vec<_>>();
    discovered.sort_by(|left, right| {
        right
            .version
            .cmp(&left.version)
            .then(left.executable_path.cmp(&right.executable_path))
    });
    discovered
}

fn editor_key(editor: &DiscoveredUnityEditor) -> String {
    editor.executable_path.replace('\\', "/").to_ascii_lowercase()
}

fn is_unity_hub_root(root: &Path) -> bool {
    let Some(name) = root.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(parent_name) = root
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
    else {
        return false;
    };

    name.eq_ignore_ascii_case("editor") && parent_name.eq_ignore_ascii_case("hub")
}

fn hub_unity_executable_path(platform: HostPlatform, install_root: &Path) -> PathBuf {
    match platform {
        HostPlatform::Windows => install_root.join("Editor").join("Unity.exe"),
        HostPlatform::MacOS => install_root
            .join("Unity.app")
            .join("Contents")
            .join("MacOS")
            .join("Unity"),
        HostPlatform::Linux => install_root.join("Editor").join("Unity"),
    }
}

fn standalone_unity_executable_path(platform: HostPlatform, install_root: &Path) -> PathBuf {
    match platform {
        HostPlatform::Windows => install_root.join("Unity.exe"),
        HostPlatform::MacOS => install_root
            .join("Unity.app")
            .join("Contents")
            .join("MacOS")
            .join("Unity"),
        HostPlatform::Linux => install_root.join("Unity"),
    }
}

fn build_discovered_unity_editor(
    platform: HostPlatform,
    version: String,
    source: String,
    install_root: PathBuf,
    executable_path: PathBuf,
) -> DiscoveredUnityEditor {
    let executable_exists = executable_path.exists();
    let executable_is_file = executable_path.is_file();
    let supported_build_targets = if executable_exists && executable_is_file {
        discover_supported_build_targets(platform, &install_root)
    } else {
        Vec::new()
    };
    let (status, message) = if executable_exists && executable_is_file {
        (
            String::from("ready"),
            format!(
                "Discovered Unity editor {version} via {source} at {}.",
                executable_path.display()
            ),
        )
    } else {
        (
            String::from("error_missing_executable"),
            format!(
                "Unity editor {version} was found under {} but the expected executable path {} is not a regular file.",
                install_root.display(),
                executable_path.display()
            ),
        )
    };

    DiscoveredUnityEditor {
        version,
        source,
        install_root_path: install_root.display().to_string(),
        executable_path: executable_path.display().to_string(),
        executable_exists,
        executable_is_file,
        supported_build_targets,
        status,
        message,
    }
}

fn discover_supported_build_targets(
    platform: HostPlatform,
    install_root: &Path,
) -> Vec<String> {
    let mut targets = BTreeSet::new();
    targets.insert(String::from(platform.as_str()));

    let playback_engines_root = match platform {
        HostPlatform::Windows | HostPlatform::Linux => [
            install_root.join("Data").join("PlaybackEngines"),
            install_root.join("Editor").join("Data").join("PlaybackEngines"),
        ],
        HostPlatform::MacOS => [
            install_root.join("Contents").join("PlaybackEngines"),
            install_root
                .join("Unity.app")
                .join("Contents")
                .join("PlaybackEngines"),
        ],
    }
    .into_iter()
    .find(|path| path.is_dir());

    if let Some(playback_engines_root) = playback_engines_root {
        if let Ok(entries) = fs::read_dir(playback_engines_root) {
            for entry in entries.flatten() {
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if let Some(target) = map_playback_engine_target(&name) {
                    targets.insert(String::from(target));
                }
            }
        }
    }

    targets.into_iter().collect()
}

fn map_playback_engine_target(name: &str) -> Option<&'static str> {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("windows") {
        Some("windows")
    } else if normalized.contains("linux") {
        Some("linux")
    } else if normalized.contains("mac") || normalized.contains("osx") {
        Some("macos")
    } else if normalized.contains("webgl") {
        Some("webgl")
    } else if normalized.contains("android") {
        Some("android")
    } else if normalized.contains("ios") {
        Some("ios")
    } else {
        None
    }
}

fn select_runner_family(
    platform: HostPlatform,
    inside_wsl: bool,
    git_tool: &HostToolCapability,
    platform_prerequisites: &[HostToolCapability],
    unity_license: &UnityLicenseDiagnostics,
    discovered_editors: &[DiscoveredUnityEditor],
) -> RunnerSelectionDiagnostics {
    if inside_wsl {
        return RunnerSelectionDiagnostics {
            selected_runner_family: None,
            status: String::from("error_unsupported_wsl"),
            message: String::from(
                "WSL was detected. Host-native Unity execution is currently evaluated against full host platforms, not WSL guest environments.",
            ),
        };
    }

    if !git_tool.available {
        return RunnerSelectionDiagnostics {
            selected_runner_family: None,
            status: String::from("error_missing_git"),
            message: String::from(
                "Git is not available on PATH, so repository sync cannot start on this host.",
            ),
        };
    }

    let missing_prerequisites = platform_prerequisites
        .iter()
        .filter(|tool| !tool.available)
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>();
    if !missing_prerequisites.is_empty() {
        return RunnerSelectionDiagnostics {
            selected_runner_family: None,
            status: String::from("error_missing_prerequisites"),
            message: format!(
                "Missing host prerequisites: {}.",
                missing_prerequisites.join(", ")
            ),
        };
    }

    let selected_runner_family = String::from(selected_host_runner_family(platform));
    if discovered_editors.is_empty() {
        return RunnerSelectionDiagnostics {
            selected_runner_family: Some(selected_runner_family),
            status: String::from("warning_no_discovered_editors"),
            message: String::from(
                "No Unity editors were discovered under the common installation roots. Explicit build-target executable paths may still work on this host.",
            ),
        };
    }

    if !unity_license.exists {
        return RunnerSelectionDiagnostics {
            selected_runner_family: Some(selected_runner_family),
            status: String::from("warning_license_unconfirmed"),
            message: String::from(
                "Unity editors were discovered, but no common local license file was detected. Batchmode builds may still work if this host is already activated through Unity Hub or another local entitlement flow.",
            ),
        };
    }

    RunnerSelectionDiagnostics {
        selected_runner_family: Some(selected_runner_family),
        status: String::from("ready"),
        message: String::from(
            "The host satisfies the current Windows-first Unity runner selection checks.",
        ),
    }
}

fn selected_host_runner_family(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Windows => "host-windows-unity",
        HostPlatform::MacOS => "host-macos-unity",
        HostPlatform::Linux => "host-linux-unity",
    }
}

fn collect_discovered_artifacts(
    root_path: &Path,
    current_path: &Path,
    artifacts: &mut Vec<DiscoveredArtifact>,
) -> io::Result<()> {
    for entry in fs::read_dir(current_path)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_discovered_artifacts(root_path, &path, artifacts)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }

        let relative_path = path
            .strip_prefix(root_path)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .replace('\\', "/");
        artifacts.push(DiscoveredArtifact {
            name: relative_path.clone(),
            kind: detect_artifact_kind(&relative_path),
            path: relative_path,
            size_bytes: Some(metadata.len() as i64),
            checksum_sha256: None,
        });
    }

    Ok(())
}

fn platform_to_unity_build_target(platform: &str) -> io::Result<String> {
    match platform.trim().to_ascii_lowercase().as_str() {
        "linux" => Ok(String::from("StandaloneLinux64")),
        "windows" => Ok(String::from("StandaloneWindows64")),
        "macos" => Ok(String::from("StandaloneOSX")),
        "webgl" => Ok(String::from("WebGL")),
        "android" => Ok(String::from("Android")),
        other => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsupported Unity platform {other:?}"),
        )),
    }
}

fn artifact_release_dir_name(repository_name: &str, repository_url: &str, git_tag: &str) -> String {
    join_artifact_name_parts([
        artifact_repository_name(repository_name, repository_url),
        git_tag.trim().to_owned(),
    ])
}

fn artifact_output_relative_path(plan: &ExecutionPlan) -> String {
    let base_name = join_artifact_name_parts([
        artifact_repository_name(&plan.repository_name, &plan.repository_url),
        plan.git_tag.trim().to_owned(),
        plan.target_name.trim().to_owned(),
    ]);
    let extension = artifact_output_extension(plan);
    if extension.is_empty() {
        return base_name;
    }

    format!("{base_name}{extension}")
}

fn artifact_output_base_name(plan: &ExecutionPlan) -> String {
    join_artifact_name_parts([
        artifact_repository_name(&plan.repository_name, &plan.repository_url),
        plan.git_tag.trim().to_owned(),
        plan.target_name.trim().to_owned(),
    ])
}

fn artifact_output_extension(plan: &ExecutionPlan) -> String {
    if normalized_optional_string(&plan.output_kind)
        .is_some_and(|output_kind| output_kind.eq_ignore_ascii_case("archive"))
    {
        return String::from(".zip");
    }

    normalized_optional_string(&plan.output_path_template)
        .and_then(|output_path_template| Path::new(&output_path_template).extension().map(|value| value.to_string_lossy().to_string()))
        .map(|extension| format!(".{}", extension.to_ascii_lowercase()))
        .unwrap_or_default()
}

fn detect_artifact_kind(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" => {
            String::from("archive")
        }
        "apk" | "aab" | "ipa" | "exe" | "appimage" | "pkg" | "dmg" => {
            String::from("binary")
        }
        _ => String::from("file"),
    }
}

fn artifact_repository_name(repository_name: &str, repository_url: &str) -> String {
    let trimmed_name = repository_name.trim();
    if !trimmed_name.is_empty() {
        return normalize_repository_artifact_name(trimmed_name);
    }

    let fallback = repository_url
        .trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("repository")
        .trim_end_matches(".git");
    normalize_repository_artifact_name(fallback)
}

fn normalize_repository_artifact_name(input: &str) -> String {
    let lowered = input.trim().to_lowercase();
    if lowered.is_empty() {
        return String::from("repository");
    }

    let mut normalized = String::new();
    let mut previous_separator = false;
    for character in lowered.chars() {
        if character.is_alphanumeric() {
            normalized.push(character);
            previous_separator = false;
            continue;
        }

        if matches!(character, ' ' | '-' | '_' | '.') || !previous_separator {
            normalized.push('-');
            previous_separator = true;
        }
    }

    let trimmed = normalized.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        String::from("repository")
    } else {
        trimmed
    }
}

fn join_artifact_name_parts<I>(parts: I) -> String
where
    I: IntoIterator<Item = String>,
{
    let cleaned = parts
        .into_iter()
        .map(|part| normalize_artifact_name_part(&part))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if cleaned.is_empty() {
        return String::from("artifact");
    }

    cleaned.join(".")
}

fn normalize_artifact_name_part(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut normalized = String::new();
    let mut previous_separator = false;
    for character in trimmed.chars() {
        if character.is_alphanumeric() {
            normalized.push(character);
            previous_separator = false;
            continue;
        }

        if matches!(character, '.' | '-' | '_') {
            normalized.push(character);
            previous_separator = false;
            continue;
        }

        if previous_separator {
            continue;
        }

        normalized.push('-');
        previous_separator = true;
    }

    let trimmed = normalized.trim_matches(|character| matches!(character, '.' | '-' | '_'));
    if trimmed.is_empty() {
        String::from("artifact")
    } else {
        trimmed.to_owned()
    }
}

fn resolve_artifact_output_path(
    artifact_root_path: &Path,
    output_path_template: Option<&str>,
) -> io::Result<PathBuf> {
    if artifact_root_path.as_os_str().is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "artifact root path must not be empty",
        ));
    }

    let Some(relative_path) = output_path_template.map(str::trim).filter(|path| !path.is_empty()) else {
        return Ok(artifact_root_path.to_path_buf());
    };

    let mut resolved = artifact_root_path.to_path_buf();
    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(segment) => resolved.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "artifact output path must stay within the artifact root",
                ))
            }
        }
    }

    Ok(resolved)
}

fn resolve_runtime_output_path(
    workspace: &PreparedWorkspace,
    plan: &ExecutionPlan,
) -> io::Result<PathBuf> {
    if normalized_optional_string(&plan.output_kind)
        .is_some_and(|output_kind| output_kind.eq_ignore_ascii_case("archive"))
    {
        return Ok(workspace
            .root_path
            .join("outputs")
            .join(artifact_output_base_name(plan)));
    }

    resolve_final_artifact_output_path(plan, &workspace.artifact_root_path)
}

/// Resolves the final artifact path that should exist under the artifact root after packaging.
pub fn resolve_final_artifact_output_path(
    plan: &ExecutionPlan,
    artifact_root_path: &Path,
) -> io::Result<PathBuf> {
    resolve_artifact_output_path(
        artifact_root_path,
        Some(artifact_output_relative_path(plan).as_str()),
    )
}

fn cleanup_previous_artifact_output(output_path: &Path) -> io::Result<()> {
    if !output_path.exists() {
        return Ok(());
    }

    let metadata = fs::metadata(output_path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(output_path)
    } else {
        fs::remove_file(output_path)
    }
}

fn enrich_execution_error(error: io::Error, output: &[u8]) -> io::Error {
    classify_execution_error(error, output, None)
}

fn summarize_execution_failure(output: &[u8]) -> String {
    let mut best_line = String::new();
    let mut best_score = 0;

    for raw_line in String::from_utf8_lossy(output).lines() {
        let line = normalize_failure_summary_line(raw_line);
        if line.is_empty() {
            continue;
        }

        let score = failure_summary_score(&line);
        if score >= best_score {
            best_line = line;
            best_score = score;
        }
    }

    best_line
}

fn normalize_failure_summary_line(raw_line: &str) -> String {
    let mut line = raw_line.trim().to_owned();
    if line.is_empty() {
        return String::new();
    }

    let parts = line.splitn(2, '|').collect::<Vec<_>>();
    if parts.len() == 2 {
        let left = parts[0].trim();
        let right = parts[1].trim();
        if !right.is_empty() && left.chars().any(|character| ":-/.T".contains(character)) {
            line = right.to_owned();
        }
    }

    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn failure_summary_score(line: &str) -> i32 {
    let lowered = line.trim().to_ascii_lowercase();

    if lowered.contains("no valid unity editor license found") {
        100
    } else if lowered.contains("please activate your license") {
        95
    } else if lowered.contains("error cs") {
        90
    } else if lowered.contains("unauthorizedaccessexception") {
        85
    } else if lowered.contains("directorynotfoundexception")
        || lowered.contains("filenotfoundexception")
        || lowered.contains("buildfailedexception")
        || lowered.contains("nullreferenceexception")
        || lowered.contains("invalidoperationexception")
        || lowered.contains("exception:")
    {
        80
    } else if lowered.contains("threw exception") {
        75
    } else if lowered.contains("access to the path") {
        70
    } else if lowered.contains("permission denied") {
        65
    } else if lowered.contains("licensing initialization failed") {
        60
    } else if lowered.contains("error:") {
        55
    } else if lowered.contains("exception") {
        50
    } else if lowered.contains("failed") {
        45
    } else {
        0
    }
}

fn normalized_optional_string(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[derive(Debug, Clone, Default, Deserialize)]
struct HostNativeRunnerConfig {
    #[serde(default)]
    unity_executable_path: String,
    #[serde(default)]
    additional_arguments: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::{
        classify_execution_error, diagnose_host_native_runner_config,
        discover_artifacts, inspect_host_capability_profile_with_input,
        resolve_host_native_execution_plan, selected_host_runner_family,
        CapabilityInspectionInput, DiscoveredUnityEditor, ExecutionPlan,
        ExecutionProcessor, HostCapabilityProfile, HostNativeUnityExecutor,
        HostToolCapability, RunnerSelectionDiagnostics, UnityLicenseDiagnostics,
        WorkspacePreparationInput, WorkspacePreparer,
    };
    use runtime_config::{HostPlatform, RuntimeDirectories};
    use runtime_git::GitAuthOptions;
    use serde_json::json;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    const PROJECT_VERSION_FILE_PATH: &str = "ProjectSettings/ProjectVersion.txt";

    #[test]
    fn workspace_preparer_creates_isolated_run_directories() {
        let root = test_root("prepare-workspace");
        let directories = RuntimeDirectories::from_root(&root);
        directories.ensure_exists().expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("workspace-source-repo"),
            "2022.3.14f1",
            "v5.0.0",
        );
        let preparer = WorkspacePreparer::new(&directories);

        let prepared = preparer
            .prepare(&WorkspacePreparationInput {
                build_run_id: 42,
                attempt_token: String::from("attempt-42"),
                repository_name: String::from("revolutions"),
                repository_url: repository_path.display().to_string(),
                git_auth: GitAuthOptions::default(),
                git_tag: String::from("v5.0.0"),
                workspace_root_override: None,
                artifacts_root_override: None,
            })
            .expect("workspace preparation should succeed");

        let expected_root = directories
            .runs_dir
            .join("build-run-42-attempt-42");
        assert_eq!(prepared.root_path, expected_root);
        assert_eq!(prepared.host_root_path, prepared.root_path);

        let contents = fs::read_to_string(prepared.source_path.join(PROJECT_VERSION_FILE_PATH))
            .expect("project version file should exist in prepared workspace");
        assert!(contents.contains("m_EditorVersion: 2022.3.14f1"));

        for path in [
            prepared.root_path.as_path(),
            prepared.source_path.as_path(),
            prepared.artifact_root_path.as_path(),
        ] {
            assert!(path.is_dir(), "expected {:?} to be a directory", path);
        }

        assert_eq!(
            prepared.log_path,
            prepared.root_path.join("logs").join("unity-build.log")
        );
        assert_eq!(
            prepared.artifact_root_path,
            directories.artifacts_dir.join("revolutions.v5.0.0")
        );
        assert_eq!(prepared.host_artifact_root_path, prepared.artifact_root_path);

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn workspace_preparer_plan_matches_prepared_layout() {
        let root = test_root("plan-workspace");
        let directories = RuntimeDirectories::from_root(&root);
        directories.ensure_exists().expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("workspace-plan-source-repo"),
            "2022.3.14f1",
            "v5.1.0",
        );
        let preparer = WorkspacePreparer::new(&directories);
        let input = WorkspacePreparationInput {
            build_run_id: 52,
            attempt_token: String::from("attempt-52"),
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_auth: GitAuthOptions::default(),
            git_tag: String::from("v5.1.0"),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let planned = preparer.plan(&input).expect("workspace plan should resolve");
        let prepared = preparer.prepare(&input).expect("workspace preparation should succeed");

        assert_eq!(planned, prepared);

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn workspace_preparer_uses_workspace_root_and_artifact_overrides() {
        let root = test_root("prepare-workspace-overrides");
        let directories = RuntimeDirectories::from_root(&root.join("runtime-root"));
        directories.ensure_exists().expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("workspace-override-source-repo"),
            "2022.3.14f1",
            "v5.2.0",
        );
        let workspace_root_override = root.join("managed-workspace");
        let build_output_override = root.join("build-output");
        let preparer = WorkspacePreparer::new(&directories);

        let prepared = preparer
            .prepare(&WorkspacePreparationInput {
                build_run_id: 53,
                attempt_token: String::from("attempt-53"),
                repository_name: String::from("revolutions"),
                repository_url: repository_path.display().to_string(),
                git_auth: GitAuthOptions::default(),
                git_tag: String::from("v5.2.0"),
                workspace_root_override: Some(
                    workspace_root_override.display().to_string(),
                ),
                artifacts_root_override: Some(
                    build_output_override.display().to_string(),
                ),
            })
            .expect("workspace preparation with overrides should succeed");

        assert_eq!(
            prepared.root_path,
            workspace_root_override
                .join("runs")
                .join("build-run-53-attempt-53")
        );
        assert_eq!(
            prepared.log_path,
            workspace_root_override
                .join("runs")
                .join("build-run-53-attempt-53")
                .join("logs")
                .join("unity-build.log")
        );
        assert_eq!(
            prepared.artifact_root_path,
            build_output_override.join("revolutions.v5.2.0")
        );
        assert!(prepared.source_path.join(PROJECT_VERSION_FILE_PATH).is_file());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn diagnose_host_native_runner_config_reports_ready_executable_path() {
        let root = test_root("runner-diagnostics-ready");
        fs::create_dir_all(&root).expect("test root should create");
        let executable_path = root.join(format!("unity{}", std::env::consts::EXE_SUFFIX));
        fs::write(&executable_path, b"unity").expect("fake unity executable should write");

        let diagnostics = diagnose_host_native_runner_config(
            &json!({
                "unity_executable_path": executable_path.display().to_string(),
                "additional_arguments": ["-silent-crashes", "-accept-apiupdate"],
                "environment": {
                    "UNITY_LICENSE": "redacted"
                }
            })
            .to_string(),
        );

        assert_eq!(diagnostics.status, "ready");
        assert_eq!(
            diagnostics.unity_executable_path,
            Some(executable_path.display().to_string())
        );
        assert!(diagnostics.unity_executable_exists);
        assert!(diagnostics.unity_executable_is_file);
        assert_eq!(diagnostics.additional_argument_count, 2);
        assert_eq!(diagnostics.environment_variable_count, 1);

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn diagnose_host_native_runner_config_reports_missing_executable_path() {
        let root = test_root("runner-diagnostics-missing");
        let executable_path = root.join(format!("missing{}", std::env::consts::EXE_SUFFIX));

        let diagnostics = diagnose_host_native_runner_config(
            &json!({
                "unity_executable_path": executable_path.display().to_string()
            })
            .to_string(),
        );

        assert_eq!(diagnostics.status, "missing_executable");
        assert_eq!(
            diagnostics.unity_executable_path,
            Some(executable_path.display().to_string())
        );
        assert!(!diagnostics.unity_executable_exists);
        assert!(!diagnostics.unity_executable_is_file);
    }

    #[test]
    fn diagnose_host_native_runner_config_reports_invalid_config() {
        let diagnostics = diagnose_host_native_runner_config("{}");

        assert_eq!(diagnostics.status, "invalid_config");
        assert_eq!(diagnostics.unity_executable_path, None);
        assert!(!diagnostics.unity_executable_exists);
        assert!(!diagnostics.unity_executable_is_file);
        assert!(diagnostics
            .message
            .contains("host-native unity executable path must not be empty"));
    }

    #[test]
    fn resolve_host_native_execution_plan_uses_discovered_editor_when_path_is_missing() {
        let root = test_root("resolve-plan-discovered-editor");
        fs::create_dir_all(&root).expect("test root should create");
        let executable_path = root.join(format!("unity{}", std::env::consts::EXE_SUFFIX));
        fs::write(&executable_path, b"unity").expect("fake unity executable should write");

        let platform = HostPlatform::current();
        let plan = ExecutionPlan {
            build_run_id: 1,
            release_run_id: 2,
            build_target_id: 3,
            repository_name: String::from("revolutions"),
            repository_url: String::from("https://example.com/revolutions.git"),
            git_tag: String::from("v1.0.0"),
            target_name: String::from("windows-player"),
            platform: String::from("windows"),
            runner_type: String::from("host-native"),
            build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Players")),
            unity_version: String::from("2022.3.14f1"),
            config_json: String::from("{}"),
            timeout_seconds: 900,
        };
        let profile = test_host_capability_profile(
            platform,
            vec![DiscoveredUnityEditor {
                version: String::from("2022.3.14f1"),
                source: String::from("unity-hub"),
                install_root_path: root.display().to_string(),
                executable_path: executable_path.display().to_string(),
                executable_exists: true,
                executable_is_file: true,
                supported_build_targets: vec![String::from("windows")],
                status: String::from("ready"),
                message: String::from("ready"),
            }],
            Some(String::from(selected_host_runner_family(platform))),
        );

        let resolved = resolve_host_native_execution_plan(&plan, &profile)
            .expect("plan should resolve with discovered editor");
        let diagnostics = diagnose_host_native_runner_config(&resolved.config_json);

        assert_eq!(
            resolved.runner_type,
            String::from(selected_host_runner_family(platform))
        );
        assert_eq!(
            diagnostics.unity_executable_path,
            Some(executable_path.display().to_string())
        );
        assert_eq!(diagnostics.status, "ready");

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn resolve_host_native_execution_plan_fails_when_requested_version_is_missing() {
        let platform = HostPlatform::current();
        let plan = ExecutionPlan {
            build_run_id: 1,
            release_run_id: 2,
            build_target_id: 3,
            repository_name: String::from("revolutions"),
            repository_url: String::from("https://example.com/revolutions.git"),
            git_tag: String::from("v1.0.0"),
            target_name: String::from("windows-player"),
            platform: String::from("windows"),
            runner_type: String::from("host-native"),
            build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Players")),
            unity_version: String::from("2022.3.30f1"),
            config_json: String::from("{}"),
            timeout_seconds: 900,
        };
        let profile = test_host_capability_profile(
            platform,
            vec![DiscoveredUnityEditor {
                version: String::from("2022.3.14f1"),
                source: String::from("unity-hub"),
                install_root_path: String::from("C:/Unity/Hub/Editor/2022.3.14f1"),
                executable_path: String::from("C:/Unity/Hub/Editor/2022.3.14f1/Editor/Unity.exe"),
                executable_exists: true,
                executable_is_file: true,
                supported_build_targets: vec![String::from("windows")],
                status: String::from("ready"),
                message: String::from("ready"),
            }],
            Some(String::from(selected_host_runner_family(platform))),
        );

        let error = resolve_host_native_execution_plan(&plan, &profile)
            .expect_err("plan should fail when the requested version was not discovered");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error
            .to_string()
            .contains("2022.3.30f1"));
    }

    #[test]
    fn inspect_host_capability_profile_discovers_editors_and_selects_runner() {
        let root = test_root("host-capability-profile-ready");
        fs::create_dir_all(&root).expect("test root should create");

        let platform = HostPlatform::current();
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).expect("bin directory should create");
        for command_name in required_test_commands(platform) {
            fs::write(bin_dir.join(command_name), b"tool")
                .expect("fake tool should write");
        }

        let discovery_root = create_discovered_unity_install(
            &root.join("unity-root"),
            platform,
            "2022.3.14f1",
        );
        let license_path = root.join("licenses").join("Unity_lic.ulf");
        fs::create_dir_all(
            license_path
                .parent()
                .expect("license path should have a parent"),
        )
        .expect("license directory should create");
        fs::write(&license_path, b"licensed").expect("license file should write");

        let profile = inspect_host_capability_profile_with_input(
            platform,
            CapabilityInspectionInput {
                architecture: String::from("x86_64"),
                packaging_mode: String::from("development"),
                inside_wsl: false,
                path_entries: vec![bin_dir],
                discovery_roots: vec![discovery_root],
                unity_license_paths: vec![license_path],
            },
        );

        assert_eq!(profile.platform, platform.as_str());
        assert_eq!(profile.architecture, "x86_64");
        assert!(!profile.inside_wsl);
        assert_eq!(profile.git_tool.status, "ready");
        assert!(profile.unity_license.exists);
        assert_eq!(profile.unity_license.status, "ready");
        assert_eq!(profile.discovered_editors.len(), 1);
        assert_eq!(profile.discovered_editors[0].version, "2022.3.14f1");
        assert_eq!(profile.discovered_editors[0].status, "ready");
        assert!(profile.discovered_editors[0]
            .supported_build_targets
            .contains(&String::from(platform.as_str())));
        assert!(profile.discovered_editors[0]
            .supported_build_targets
            .contains(&String::from("webgl")));
        assert_eq!(
            profile.runner_selection.selected_runner_family.as_deref(),
            Some(selected_host_runner_family(platform))
        );
        assert_eq!(profile.runner_selection.status, "ready");

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn inspect_host_capability_profile_warns_when_license_is_not_detected() {
        let root = test_root("host-capability-profile-license-warning");
        fs::create_dir_all(&root).expect("test root should create");

        let platform = HostPlatform::current();
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).expect("bin directory should create");
        for command_name in required_test_commands(platform) {
            fs::write(bin_dir.join(command_name), b"tool")
                .expect("fake tool should write");
        }

        let discovery_root = create_discovered_unity_install(
            &root.join("unity-root"),
            platform,
            "2022.3.15f1",
        );

        let profile = inspect_host_capability_profile_with_input(
            platform,
            CapabilityInspectionInput {
                architecture: String::from("x86_64"),
                packaging_mode: String::from("development"),
                inside_wsl: false,
                path_entries: vec![bin_dir],
                discovery_roots: vec![discovery_root],
                unity_license_paths: vec![root.join("missing-license.ulf")],
            },
        );

        assert!(!profile.unity_license.exists);
        assert_eq!(profile.unity_license.status, "warning_not_detected");
        assert_eq!(
            profile.runner_selection.selected_runner_family.as_deref(),
            Some(selected_host_runner_family(platform))
        );
        assert_eq!(profile.runner_selection.status, "warning_license_unconfirmed");

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn workspace_preparer_groups_artifacts_by_repository_and_tag() {
        let root = test_root("prepare-workspace-grouping");
        let directories = RuntimeDirectories::from_root(&root);
        directories.ensure_exists().expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("workspace-grouping-source-repo"),
            "2021.3.18f1",
            "v6.0.0",
        );
        let preparer = WorkspacePreparer::new(&directories);

        let first = preparer
            .prepare(&WorkspacePreparationInput {
                build_run_id: 7,
                attempt_token: String::from("attempt-a"),
                repository_name: String::from("revolutions"),
                repository_url: repository_path.display().to_string(),
                git_auth: GitAuthOptions::default(),
                git_tag: String::from("v6.0.0"),
                workspace_root_override: None,
                artifacts_root_override: None,
            })
            .expect("first workspace should prepare");
        let second = preparer
            .prepare(&WorkspacePreparationInput {
                build_run_id: 7,
                attempt_token: String::from("attempt-b"),
                repository_name: String::from("revolutions"),
                repository_url: repository_path.display().to_string(),
                git_auth: GitAuthOptions::default(),
                git_tag: String::from("v6.0.0"),
                workspace_root_override: None,
                artifacts_root_override: None,
            })
            .expect("second workspace should prepare");

        assert_ne!(first.root_path, second.root_path);
        assert_eq!(first.artifact_root_path, second.artifact_root_path);
        assert_ne!(first.log_path, second.log_path);

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn discover_artifacts_lists_regular_files_in_sorted_order() {
        let root = test_root("discover-artifacts");
        let artifact_root = root.join("artifacts");
        fs::create_dir_all(artifact_root.join("nested"))
            .expect("artifact directory should create");
        fs::write(artifact_root.join("game.zip"), "artifact")
            .expect("root artifact should write");
        fs::write(artifact_root.join("nested").join("notes.txt"), "notes")
            .expect("nested artifact should write");

        let artifacts = discover_artifacts(&artifact_root).expect("artifacts should discover");

        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].path, "game.zip");
        assert_eq!(artifacts[0].kind, "archive");
        assert_eq!(artifacts[1].path, "nested/notes.txt");
        assert_eq!(artifacts[1].kind, "file");

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn discover_artifacts_rejects_empty_roots() {
        let root = test_root("discover-artifacts-empty");
        let artifact_root = root.join("artifacts-empty");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");

        let error = discover_artifacts(&artifact_root)
            .expect_err("empty artifact roots should fail discovery");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("no files found under"));

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn execution_processor_runs_host_native_command_and_writes_log() {
        let root = test_root("host-native-success");
        let directories = RuntimeDirectories::from_root(&root);
        directories.ensure_exists().expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("host-native-success-source-repo"),
            "2022.3.14f1",
            "v7.0.0",
        );
        let script_path = create_fake_unity_script(&root, "success", ScriptKind::Success);
        let processor = ExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let plan = ExecutionPlan {
            build_run_id: 61,
            release_run_id: 71,
            build_target_id: 81,
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_tag: String::from("v7.0.0"),
            target_name: String::from("webgl"),
            platform: String::from("webgl"),
            runner_type: String::from("host-native"),
            build_method: String::from("Builder.PerformWebGL"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/WebGL")),
            unity_version: String::from("2022.3.14f1"),
            config_json: json!({
                "unity_executable_path": script_path.display().to_string(),
                "additional_arguments": ["--custom-flag"],
                "environment": {"CUSTOM_FLAG": "workers"}
            })
            .to_string(),
            timeout_seconds: 5,
        };
        let preparation = WorkspacePreparationInput {
            build_run_id: plan.build_run_id,
            attempt_token: String::from("attempt-61"),
            repository_name: plan.repository_name.clone(),
            repository_url: plan.repository_url.clone(),
            git_auth: GitAuthOptions::default(),
            git_tag: plan.git_tag.clone(),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let outcome = processor
            .process(&plan, &preparation)
            .expect("host-native execution should process");

        assert!(outcome.error.is_none());
        let contents = fs::read_to_string(&outcome.result.log_path)
            .expect("execution log should exist");
        assert!(contents.contains("-batchmode"));
        assert!(contents.contains("-buildTarget WebGL"));
        assert!(contents.contains("-executeMethod Builder.PerformWebGL"));
        assert!(contents.contains("--custom-flag"));
        assert!(contents.contains("custom:workers"));

        let expected_output = outcome
            .result
            .workspace_path
            .join("outputs")
            .join("revolutions.v7.0.0.webgl");
        assert_eq!(outcome.result.output_path, expected_output);
        assert!(expected_output.is_dir());
        assert!(expected_output.join("artifact.txt").is_file());
        assert!(!directories
            .artifacts_dir
            .join("revolutions.v7.0.0")
            .join("revolutions.v7.0.0.webgl.zip")
            .exists());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn execution_processor_persists_preamble_when_host_native_command_writes_no_log() {
        let root = test_root("host-native-missing-log");
        let directories = RuntimeDirectories::from_root(&root);
        directories.ensure_exists().expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("host-native-missing-log-source-repo"),
            "2022.3.14f1",
            "v7.0.1",
        );
        let script_path = create_fake_unity_script(&root, "missing-log", ScriptKind::SilentSuccess);
        let processor = ExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let plan = ExecutionPlan {
            build_run_id: 64,
            release_run_id: 74,
            build_target_id: 84,
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_tag: String::from("v7.0.1"),
            target_name: String::from("windows"),
            platform: String::from("windows"),
            runner_type: String::from("host-native"),
            build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Windows")),
            unity_version: String::from("2022.3.14f1"),
            config_json: json!({
                "unity_executable_path": script_path.display().to_string()
            })
            .to_string(),
            timeout_seconds: 5,
        };
        let preparation = WorkspacePreparationInput {
            build_run_id: plan.build_run_id,
            attempt_token: String::from("attempt-64"),
            repository_name: plan.repository_name.clone(),
            repository_url: plan.repository_url.clone(),
            git_auth: GitAuthOptions::default(),
            git_tag: plan.git_tag.clone(),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let outcome = processor
            .process(&plan, &preparation)
            .expect("execution without an explicit log file should still process");

        assert!(outcome.error.is_none());
        let contents = fs::read_to_string(&outcome.result.log_path)
            .expect("fallback execution log should exist");
        assert!(contents.contains("requested_log_path:"));
        assert!(contents.contains("workspace_source_path:"));
        assert!(contents.contains("build_method: Builder.PerformWindows"));
        assert!(contents.contains("execution log file"));

        let expected_output = outcome
            .result
            .workspace_path
            .join("outputs")
            .join("revolutions.v7.0.1.windows");
        assert_eq!(outcome.result.output_path, expected_output);
        assert!(expected_output.is_dir());
        assert!(expected_output.join("artifact.txt").is_file());
        assert!(!directories
            .artifacts_dir
            .join("revolutions.v7.0.1")
            .join("revolutions.v7.0.1.windows.zip")
            .exists());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn execution_processor_enriches_failure_and_preserves_paths() {
        let root = test_root("host-native-failure");
        let directories = RuntimeDirectories::from_root(&root);
        directories.ensure_exists().expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("host-native-failure-source-repo"),
            "2022.3.14f1",
            "v7.1.0",
        );
        let script_path = create_fake_unity_script(&root, "failure", ScriptKind::Failure);
        let processor = ExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let plan = ExecutionPlan {
            build_run_id: 62,
            release_run_id: 72,
            build_target_id: 82,
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_tag: String::from("v7.1.0"),
            target_name: String::from("windows"),
            platform: String::from("windows"),
            runner_type: String::from("host-native"),
            build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Windows")),
            unity_version: String::from("2022.3.14f1"),
            config_json: json!({
                "unity_executable_path": script_path.display().to_string()
            })
            .to_string(),
            timeout_seconds: 5,
        };
        let preparation = WorkspacePreparationInput {
            build_run_id: plan.build_run_id,
            attempt_token: String::from("attempt-62"),
            repository_name: plan.repository_name.clone(),
            repository_url: plan.repository_url.clone(),
            git_auth: GitAuthOptions::default(),
            git_tag: plan.git_tag.clone(),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let outcome = processor
            .process(&plan, &preparation)
            .expect("failed execution should still return paths");

        let error = outcome.error.expect("host-native execution should fail");
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error
            .to_string()
            .contains("licensing: host-native unity runner exited with code"));
        assert!(error
            .to_string()
            .contains("No valid Unity Editor license found. Please activate your license."));
        let contents = fs::read_to_string(&outcome.result.log_path)
            .expect("failed execution log should exist");
        assert!(contents.contains("No valid Unity Editor license found. Please activate your license."));
        assert!(outcome.result.workspace_path.is_dir());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn execution_processor_times_out_host_native_command_and_reports_cancellation() {
        let root = test_root("host-native-timeout");
        let directories = RuntimeDirectories::from_root(&root);
        directories.ensure_exists().expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("host-native-timeout-source-repo"),
            "2022.3.14f1",
            "v7.2.0",
        );
        let script_path = create_fake_unity_script(&root, "timeout", ScriptKind::Slow);
        let processor = ExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let plan = ExecutionPlan {
            build_run_id: 63,
            release_run_id: 73,
            build_target_id: 83,
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_tag: String::from("v7.2.0"),
            target_name: String::from("linux"),
            platform: String::from("linux"),
            runner_type: String::from("host-native"),
            build_method: String::from("Builder.PerformLinux"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Linux")),
            unity_version: String::from("2022.3.14f1"),
            config_json: json!({
                "unity_executable_path": script_path.display().to_string()
            })
            .to_string(),
            timeout_seconds: 1,
        };
        let preparation = WorkspacePreparationInput {
            build_run_id: plan.build_run_id,
            attempt_token: String::from("attempt-63"),
            repository_name: plan.repository_name.clone(),
            repository_url: plan.repository_url.clone(),
            git_auth: GitAuthOptions::default(),
            git_tag: plan.git_tag.clone(),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let outcome = processor
            .process(&plan, &preparation)
            .expect("timed out execution should still return paths");

        let error = outcome.error.expect("timed out execution should fail");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("timeout: host-native unity runner exceeded 1s timeout"));
        assert!(!directories
            .artifacts_dir
            .join("revolutions.v7.2.0")
            .join("revolutions.v7.2.0.linux.zip")
            .exists());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn classify_execution_error_marks_compile_failures_as_invalid_data() {
        let error = classify_execution_error(
            io::Error::other("host-native unity runner exited with code Some(1)"),
            b"Assets/Editor/Build.cs(10,5): error CS1002: ; expected\n",
            None,
        );

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("compile:"));
        assert!(error.to_string().contains("error CS1002"));
    }

    #[test]
    fn classify_execution_error_prefers_terminal_exception_over_earlier_error_noise() {
        let error = classify_execution_error(
            io::Error::other("host-native unity runner exited with code Some(1)"),
            br#"
[Licensing::Module] Error: Access token is unavailable; failed to update
Builder:PerformWindows () (at Assets/_Scripts/Editor/Builder.cs:22)
DirectoryNotFoundException: Could not find a part of the path "D:\temp\output\Managed\Unity.InternalAPIEngineBridge.RenderPipelines.Core.Runtime.Shared.dll"
executeMethod method Builder.PerformWindows threw exception.
"#,
            None,
        );

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("runtime:"));
        assert!(error
            .to_string()
            .contains("DirectoryNotFoundException: Could not find a part of the path"));
        assert!(!error
            .to_string()
            .contains("Access token is unavailable; failed to update"));
    }

    fn create_tagged_unity_repository(
        repository_path: &Path,
        unity_version: &str,
        git_tag: &str,
    ) -> PathBuf {
        if repository_path.exists() {
            fs::remove_dir_all(repository_path)
                .expect("existing repository fixture should be removable");
        }
        fs::create_dir_all(repository_path.join("ProjectSettings"))
            .expect("project settings directory should create");
        fs::write(
            repository_path.join(PROJECT_VERSION_FILE_PATH),
            format!("m_EditorVersion: {unity_version}\n"),
        )
        .expect("project version file should write");

        run_git_test_command(repository_path, &["init"]);
        run_git_test_command(
            repository_path,
            &["config", "user.name", "runtime-runner-tests"],
        );
        run_git_test_command(
            repository_path,
            &["config", "user.email", "runtime-runner-tests@example.com"],
        );
        run_git_test_command(repository_path, &["add", "."]);
        run_git_test_command(repository_path, &["commit", "-m", "seed workspace repo"]);
        run_git_test_command(repository_path, &["tag", git_tag]);

        repository_path.to_path_buf()
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
        let contents = match kind {
            ScriptKind::Success if cfg!(windows) => String::from(
                "@echo off\r\nset \"HGB_OUTPUT_IS_FILE=0\"\r\nfor %%I in (\"%HGB_OUTPUT_PATH%\") do (set \"HGB_OUTPUT_DIR=%%~dpI\" & set \"HGB_OUTPUT_EXT=%%~xI\")\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".zip\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".exe\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".x86_64\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".app\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".apk\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".aab\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif not exist \"%HGB_OUTPUT_DIR%\" mkdir \"%HGB_OUTPUT_DIR%\"\r\n> \"%HGB_LOG_PATH%\" echo args:%*\r\n>> \"%HGB_LOG_PATH%\" echo custom:%CUSTOM_FLAG%\r\n>> \"%HGB_LOG_PATH%\" echo output:%HGB_OUTPUT_PATH%\r\nif \"%HGB_OUTPUT_IS_FILE%\"==\"1\" (\r\n  > \"%HGB_OUTPUT_PATH%\" echo artifact\r\n) else (\r\n  if not exist \"%HGB_OUTPUT_PATH%\" mkdir \"%HGB_OUTPUT_PATH%\"\r\n  > \"%HGB_OUTPUT_PATH%\\artifact.txt\" echo artifact\r\n)\r\nexit /B 0\r\n",
            ),
            ScriptKind::Failure if cfg!(windows) => String::from(
                "@echo off\r\n> \"%HGB_LOG_PATH%\" echo No valid Unity Editor license found. Please activate your license.\r\nexit /B 9\r\n",
            ),
            ScriptKind::Slow if cfg!(windows) => String::from(
                "@echo off\r\n> \"%HGB_LOG_PATH%\" echo waiting\r\npowershell -NoProfile -Command \"Start-Sleep -Seconds 3\"\r\nexit /B 0\r\n",
            ),
            ScriptKind::SilentSuccess if cfg!(windows) => String::from(
                "@echo off\r\nset \"HGB_OUTPUT_IS_FILE=0\"\r\nfor %%I in (\"%HGB_OUTPUT_PATH%\") do (set \"HGB_OUTPUT_DIR=%%~dpI\" & set \"HGB_OUTPUT_EXT=%%~xI\")\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".zip\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".exe\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".x86_64\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".app\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".apk\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif /I \"%HGB_OUTPUT_EXT%\"==\".aab\" set \"HGB_OUTPUT_IS_FILE=1\"\r\nif not exist \"%HGB_OUTPUT_DIR%\" mkdir \"%HGB_OUTPUT_DIR%\"\r\nif \"%HGB_OUTPUT_IS_FILE%\"==\"1\" (\r\n  > \"%HGB_OUTPUT_PATH%\" echo artifact\r\n) else (\r\n  if not exist \"%HGB_OUTPUT_PATH%\" mkdir \"%HGB_OUTPUT_PATH%\"\r\n  > \"%HGB_OUTPUT_PATH%\\artifact.txt\" echo artifact\r\n)\r\nexit /B 0\r\n",
            ),
            ScriptKind::Success => String::from(
                "#!/bin/sh\nset -eu\nmkdir -p \"$(dirname \"$HGB_OUTPUT_PATH\")\"\nprintf 'args:%s\\n' \"$*\" > \"$HGB_LOG_PATH\"\nprintf 'custom:%s\\n' \"${CUSTOM_FLAG:-}\" >> \"$HGB_LOG_PATH\"\nprintf 'output:%s\\n' \"$HGB_OUTPUT_PATH\" >> \"$HGB_LOG_PATH\"\ncase \"$HGB_OUTPUT_PATH\" in\n  *.zip|*.exe|*.x86_64|*.app|*.apk|*.aab)\n    printf 'artifact\\n' > \"$HGB_OUTPUT_PATH\"\n    ;;\n  *)\n    mkdir -p \"$HGB_OUTPUT_PATH\"\n    printf 'artifact\\n' > \"$HGB_OUTPUT_PATH/artifact.txt\"\n    ;;\nesac\nexit 0\n",
            ),
            ScriptKind::Failure => String::from(
                "#!/bin/sh\nset -eu\nprintf 'No valid Unity Editor license found. Please activate your license.\\n' > \"$HGB_LOG_PATH\"\nexit 9\n",
            ),
            ScriptKind::Slow => String::from(
                "#!/bin/sh\nset -eu\nprintf 'waiting\\n' > \"$HGB_LOG_PATH\"\nsleep 3\nexit 0\n",
            ),
            ScriptKind::SilentSuccess => String::from(
                "#!/bin/sh\nset -eu\nmkdir -p \"$(dirname \"$HGB_OUTPUT_PATH\")\"\ncase \"$HGB_OUTPUT_PATH\" in\n  *.zip|*.exe|*.x86_64|*.app|*.apk|*.aab)\n    printf 'artifact\\n' > \"$HGB_OUTPUT_PATH\"\n    ;;\n  *)\n    mkdir -p \"$HGB_OUTPUT_PATH\"\n    printf 'artifact\\n' > \"$HGB_OUTPUT_PATH/artifact.txt\"\n    ;;\nesac\nexit 0\n",
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
        Failure,
        Slow,
        SilentSuccess,
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "handy-unity-builder-runtime-runner-{label}-{}",
            std::process::id()
        ))
    }

    fn test_host_capability_profile(
        platform: HostPlatform,
        discovered_editors: Vec<DiscoveredUnityEditor>,
        selected_runner_family: Option<String>,
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
                selected_runner_family,
                status: String::from("ready"),
                message: String::from("ready"),
            },
        }
    }

    fn required_test_commands(platform: HostPlatform) -> Vec<String> {
        match platform {
            HostPlatform::Windows => vec![
                fake_command_name("git"),
                fake_command_name("powershell"),
                fake_command_name("cmd"),
            ],
            HostPlatform::MacOS | HostPlatform::Linux => {
                vec![fake_command_name("git"), String::from("sh")]
            }
        }
    }

    fn fake_command_name(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            String::from(name)
        }
    }

    fn create_discovered_unity_install(
        root: &Path,
        platform: HostPlatform,
        version: &str,
    ) -> PathBuf {
        let discovery_root = match platform {
            HostPlatform::Windows => root.join("Unity").join("Hub").join("Editor"),
            HostPlatform::MacOS => root
                .join("Applications")
                .join("Unity")
                .join("Hub")
                .join("Editor"),
            HostPlatform::Linux => root.join("opt").join("Unity").join("Hub").join("Editor"),
        };
        let install_root = discovery_root.join(version);

        let executable_path = match platform {
            HostPlatform::Windows => install_root.join("Editor").join("Unity.exe"),
            HostPlatform::MacOS => install_root
                .join("Unity.app")
                .join("Contents")
                .join("MacOS")
                .join("Unity"),
            HostPlatform::Linux => install_root.join("Editor").join("Unity"),
        };
        fs::create_dir_all(
            executable_path
                .parent()
                .expect("executable path should have a parent"),
        )
        .expect("unity executable directory should create");
        fs::write(&executable_path, b"unity").expect("unity executable should write");

        let playback_engines_root = match platform {
            HostPlatform::Windows | HostPlatform::Linux => install_root
                .join("Editor")
                .join("Data")
                .join("PlaybackEngines"),
            HostPlatform::MacOS => install_root
                .join("Unity.app")
                .join("Contents")
                .join("PlaybackEngines"),
        };
        fs::create_dir_all(playback_engines_root.join("WebGLSupport"))
            .expect("webgl playback engine should create");

        discovery_root
    }
}