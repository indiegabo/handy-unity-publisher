use super::*;
use runtime_contracts::ProcessPriority;

/// Describes the joined metadata required to execute one host-native Unity build run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnityBuildExecutionPlan {
    pub build_run_id: i64,
    pub release_run_id: i64,
    pub build_target_id: i64,
    pub repository_name: String,
    pub repository_url: String,
    pub git_tag: String,
    pub target_name: String,
    pub unity_target_platform: String,
    pub runner_type: String,
    pub unity_build_method: String,
    pub output_kind: Option<String>,
    pub output_path_template: Option<String>,
    pub engine_version: String,
    pub process_priority: ProcessPriority,
    pub config_json: String,
    pub timeout_seconds: i64,
}

/// Captures the persisted filesystem paths produced by one attempted Unity build execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnityBuildExecutionResult {
    pub workspace_path: PathBuf,
    pub build_root_path: PathBuf,
    pub log_path: PathBuf,
    pub artifact_root_path: PathBuf,
    pub output_path: PathBuf,
}

/// Bundles one prepared workspace, its Unity execution plan, and the resolved output path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnityBuildExecutionRequest {
    pub plan: UnityBuildExecutionPlan,
    pub workspace: PreparedWorkspace,
    pub output_path: PathBuf,
}

/// Captures one Unity executor invocation including combined output and an optional terminal error.
#[derive(Debug)]
pub struct UnityBuildExecutionOutcome {
    pub output: Vec<u8>,
    pub error: Option<io::Error>,
}

/// Captures one full Unity build processing attempt after workspace preparation succeeds.
#[derive(Debug)]
pub struct UnityBuildExecutionProcessOutcome {
    pub result: UnityBuildExecutionResult,
    pub error: Option<io::Error>,
}

/// Executes one prepared Unity build request and returns combined stdout/stderr output.
pub trait UnityBuildExecutor {
    fn execute(
        &self,
        request: &UnityBuildExecutionRequest,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> UnityBuildExecutionOutcome;
}

/// Prepares one Unity build workspace, computes the canonical output path, and delegates execution.
#[derive(Debug)]
pub struct UnityBuildExecutionProcessor<E> {
    preparer: WorkspacePreparer,
    executor: E,
}

impl<E> UnityBuildExecutionProcessor<E>
where
    E: UnityBuildExecutor,
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
        plan: &UnityBuildExecutionPlan,
        preparation: &WorkspacePreparationInput,
    ) -> io::Result<UnityBuildExecutionProcessOutcome> {
        let workspace = self.prepare_workspace(preparation)?;
        let mut reporter = NoopExecutionProgressReporter;
        self.execute_prepared(plan, workspace, &mut reporter)
    }

    /// Prepares the workspace only, allowing callers to track checkout separately.
    pub fn prepare_workspace(
        &self,
        preparation: &WorkspacePreparationInput,
    ) -> io::Result<PreparedWorkspace> {
        let mut reporter = NoopExecutionProgressReporter;
        self.prepare_workspace_with_reporter(preparation, &mut reporter)
    }

    /// Prepares the workspace only, allowing callers to track checkout separately.
    pub fn prepare_workspace_with_reporter(
        &self,
        preparation: &WorkspacePreparationInput,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> io::Result<PreparedWorkspace> {
        self.preparer.prepare_with_reporter(preparation, reporter)
    }

    /// Prepares only the per-build workspace after the process checkout already exists.
    pub fn prepare_build_workspace(
        &self,
        preparation: &WorkspacePreparationInput,
    ) -> io::Result<PreparedWorkspace> {
        let mut reporter = NoopExecutionProgressReporter;
        self.prepare_build_workspace_with_reporter(preparation, &mut reporter)
    }

    /// Prepares only the per-build workspace after the process checkout already exists.
    pub fn prepare_build_workspace_with_reporter(
        &self,
        preparation: &WorkspacePreparationInput,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> io::Result<PreparedWorkspace> {
        self.preparer
            .prepare_build_with_reporter(preparation, reporter)
    }

    /// Executes one already-prepared workspace and reports heartbeats while Unity is running.
    pub fn execute_prepared(
        &self,
        plan: &UnityBuildExecutionPlan,
        workspace: PreparedWorkspace,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> io::Result<UnityBuildExecutionProcessOutcome> {
        let canonical_plan = plan.clone();
        let output_path = resolve_runtime_output_path(&workspace, &canonical_plan)?;
        cleanup_previous_artifact_output(&output_path)?;

        let result = UnityBuildExecutionResult {
            workspace_path: workspace.root_path.clone(),
            build_root_path: workspace.build_root_path.clone(),
            log_path: workspace.log_path.clone(),
            artifact_root_path: workspace.artifact_root_path.clone(),
            output_path: output_path.clone(),
        };

        let outcome = self.executor.execute(
            &UnityBuildExecutionRequest {
                plan: canonical_plan,
                workspace,
                output_path,
            },
            reporter,
        );
        fs::write(&result.log_path, &outcome.output)?;

        Ok(UnityBuildExecutionProcessOutcome {
            result,
            error: outcome
                .error
                .map(|error| super::classify_execution_error(error, &outcome.output, None)),
        })
    }
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

impl UnityBuildExecutor for HostNativeUnityExecutor {
    fn execute(
        &self,
        request: &UnityBuildExecutionRequest,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> UnityBuildExecutionOutcome {
        match super::execute_host_native_unity(request, reporter) {
            Ok(output) => UnityBuildExecutionOutcome {
                output,
                error: None,
            },
            Err((output, error)) => UnityBuildExecutionOutcome {
                output,
                error: Some(error),
            },
        }
    }
}

/// Resolves one stored host-native Unity execution plan into the concrete local runner invocation.
pub fn resolve_host_native_unity_execution_plan(
    plan: &UnityBuildExecutionPlan,
    profile: &HostCapabilityProfile,
) -> io::Result<UnityBuildExecutionPlan> {
    if !super::supports_host_native_runner_type(&plan.runner_type) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "runner type {:?} is not supported by the host-native executor",
                plan.runner_type
            ),
        ));
    }

    let mut config_json = super::parse_host_native_runner_config_json(&plan.config_json)?;
    let unity_executable_path = super::configured_unity_executable_path(&config_json)
        .map(str::to_owned)
        .filter(|path| !path.trim().is_empty())
        .map(Ok)
        .unwrap_or_else(|| super::resolve_discovered_unity_executable(plan, profile))?;
    config_json.insert(
        String::from("unity_executable_path"),
        JsonValue::String(unity_executable_path),
    );

    let mut resolved = plan.clone();
    resolved.runner_type = profile
        .runner_selection
        .selected_runner_family
        .clone()
        .filter(|runner_type| super::supports_host_native_runner_type(runner_type))
        .unwrap_or_else(|| String::from(RunnerFamily::HostNative.label()));
    resolved.config_json = serde_json::to_string(&JsonValue::Object(config_json))
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    Ok(resolved)
}
