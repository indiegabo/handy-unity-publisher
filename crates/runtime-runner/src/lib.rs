//! Prepares host-native Unity workspaces, resolves execution plans, inspects
//! host capabilities, and captures artifacts produced by build runs.

#![forbid(unsafe_code)]

pub mod engine;
pub(crate) mod host;
pub mod unity;

pub use engine::{BuildExecutionAdapter, EngineAdapterRegistry};

use runtime_config::RuntimeDirectories;
use runtime_git::{
    GitAuthOptions, GitProgressReporter, GitWorkspaceSyncRefRequest, GitWorkspaceSyncer,
};
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

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

/// Describes the filesystem layout prepared for one build run before execution starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedWorkspace {
    pub root_path: PathBuf,
    pub build_root_path: PathBuf,
    pub source_path: PathBuf,
    pub source_is_local_workspace: bool,
    pub host_root_path: PathBuf,
    pub host_build_root_path: PathBuf,
    pub host_source_path: PathBuf,
    pub log_path: PathBuf,
    pub artifact_root_path: PathBuf,
    pub host_artifact_root_path: PathBuf,
}

/// Defines the source that must back one prepared process workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspacePreparationSource {
    GitRef {
        repository_url: String,
        git_auth: GitAuthOptions,
        git_ref: String,
    },
    LocalWorkspace {
        local_path: PathBuf,
    },
}

/// Defines the repository snapshot that must be materialized for one build run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePreparationInput {
    pub release_run_id: i64,
    pub build_run_id: i64,
    pub attempt_token: String,
    pub repository_name: String,
    pub source: WorkspacePreparationSource,
    pub workspace_root_override: Option<String>,
    pub artifacts_root_override: Option<String>,
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

type ExecutionPlan = self::unity::UnityBuildExecutionPlan;

/// Reports one coarse-grained execution heartbeat while a long-running runner task is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProgress {
    pub message: String,
}

/// Receives coarse execution progress updates from workspace preparation and host runners.
pub trait ExecutionProgressReporter {
    fn heartbeat(&mut self, progress: ExecutionProgress);

    fn check_cancellation(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(crate) struct NoopExecutionProgressReporter;

impl ExecutionProgressReporter for NoopExecutionProgressReporter {
    fn heartbeat(&mut self, _progress: ExecutionProgress) {}
}

struct WorkspacePreparationProgressReporter<'a> {
    reporter: &'a mut dyn ExecutionProgressReporter,
}

impl GitProgressReporter for WorkspacePreparationProgressReporter<'_> {
    fn report(&mut self, message: &str) {
        self.reporter.heartbeat(ExecutionProgress {
            message: message.to_owned(),
        });
    }

    fn check_cancellation(&mut self) -> io::Result<()> {
        self.reporter.check_cancellation()
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

fn reset_prepared_workspace_root(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn process_checkout_marker_path(root_path: &Path) -> PathBuf {
    root_path.join(".process-checkout-ready")
}

fn process_checkout_git_dir_path(source_path: &Path) -> PathBuf {
    source_path.join(".git")
}

fn git_process_checkout_has_materialized_worktree(source_path: &Path) -> bool {
    if !process_checkout_git_dir_path(source_path).is_dir() {
        return false;
    }

    fs::read_dir(source_path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name() != std::ffi::OsStr::new(".git"))
}

fn process_checkout_is_ready(planned: &PreparedWorkspace) -> bool {
    if planned.source_is_local_workspace {
        return planned.source_path.is_dir();
    }

    process_checkout_marker_path(&planned.root_path).is_file()
        && git_process_checkout_has_materialized_worktree(&planned.source_path)
}

fn write_process_checkout_marker(planned: &PreparedWorkspace) -> io::Result<()> {
    fs::write(process_checkout_marker_path(&planned.root_path), b"ready\n")
}

/// Allocates deterministic per-run directories and checks out one repository ref into source.
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

    /// Reports whether the release-scoped checkout already exists for this process workspace.
    pub fn is_process_prepared(&self, input: &WorkspacePreparationInput) -> io::Result<bool> {
        Ok(process_checkout_is_ready(&self.plan(input)?))
    }

    /// Resolves the deterministic filesystem layout for one build run without touching Git.
    pub fn plan(&self, input: &WorkspacePreparationInput) -> io::Result<PreparedWorkspace> {
        if input.build_run_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "build run id must be greater than zero",
            ));
        }

        if input.release_run_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "release run id must be greater than zero",
            ));
        }
        let process_name = format!("release-run-{}", input.release_run_id);
        let build_workspace_name = build_workspace_name(input.build_run_id, &input.attempt_token);
        let runs_root_path = self.resolve_runs_root(input)?;

        let root_path = runs_root_path.join(&process_name);
        let build_root_path = root_path.join("builds").join(&build_workspace_name);
        let host_root_path = root_path.clone();
        let host_build_root_path = host_root_path.join("builds").join(&build_workspace_name);
        let (source_path, host_source_path, source_is_local_workspace) = match &input.source {
            WorkspacePreparationSource::GitRef {
                repository_url,
                git_ref,
                ..
            } => {
                require_non_empty(repository_url, "repository url")?;
                require_non_empty(git_ref, "git ref")?;
                let source_path = root_path.join("source");
                let host_source_path = host_root_path.join("source");
                (source_path, host_source_path, false)
            }
            WorkspacePreparationSource::LocalWorkspace { local_path } => {
                if local_path.as_os_str().is_empty() || !local_path.is_absolute() {
                    return Err(io::Error::new(
                        ErrorKind::InvalidInput,
                        "local workspace path must be absolute",
                    ));
                }
                (local_path.clone(), local_path.clone(), true)
            }
        };
        let log_path = build_root_path.join("logs").join("unity-build.log");
        let artifact_root_path = build_root_path.join("outputs");
        let host_artifact_root_path = artifact_root_path.clone();

        Ok(PreparedWorkspace {
            root_path,
            build_root_path,
            source_path,
            source_is_local_workspace,
            host_root_path,
            host_build_root_path,
            host_source_path,
            log_path,
            artifact_root_path,
            host_artifact_root_path,
        })
    }

    /// Creates isolated directories and checks out the requested repository tag.
    pub fn prepare(&self, input: &WorkspacePreparationInput) -> io::Result<PreparedWorkspace> {
        let mut reporter = NoopExecutionProgressReporter;
        self.prepare_with_reporter(input, &mut reporter)
    }

    /// Creates isolated directories and checks out the requested repository ref.
    pub fn prepare_process(
        &self,
        input: &WorkspacePreparationInput,
    ) -> io::Result<PreparedWorkspace> {
        let mut reporter = NoopExecutionProgressReporter;
        self.prepare_process_with_reporter(input, &mut reporter)
    }

    /// Creates or reuses the release-scoped process checkout without touching per-build roots.
    pub fn prepare_process_with_reporter(
        &self,
        input: &WorkspacePreparationInput,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> io::Result<PreparedWorkspace> {
        let planned = self.plan(input)?;
        self.directories.ensure_exists()?;

        reporter.heartbeat(ExecutionProgress {
            message: format!(
                "Creating process workspace directories under '{}'.",
                planned.root_path.display(),
            ),
        });

        let logs_root_path = planned.root_path.join("logs");
        let mut directories = vec![
            planned.root_path.clone(),
            planned.artifact_root_path.clone(),
            logs_root_path,
        ];
        if !planned.source_is_local_workspace {
            directories.push(
                planned
                    .source_path
                    .parent()
                    .unwrap_or(planned.root_path.as_path())
                    .to_path_buf(),
            );
        }

        for directory in directories {
            fs::create_dir_all(&directory)?;
        }

        let checkout_marker_path = process_checkout_marker_path(&planned.root_path);
        if checkout_marker_path.is_file() {
            let source_ready = if planned.source_is_local_workspace {
                planned.source_path.is_dir()
            } else {
                git_process_checkout_has_materialized_worktree(&planned.source_path)
            };
            if !source_ready {
                if planned.source_is_local_workspace {
                    return Err(io::Error::new(
                        ErrorKind::NotFound,
                        format!(
                            "release process workspace '{}' is marked as checked out but source is missing at '{}'",
                            planned.root_path.display(),
                            planned.source_path.display(),
                        ),
                    ));
                }

                reporter.heartbeat(ExecutionProgress {
                    message: format!(
                        "Incomplete process checkout detected at '{}'; clearing the stale checkout marker and re-syncing the requested repository ref.",
                        planned.source_path.display(),
                    ),
                });
                fs::remove_file(&checkout_marker_path)?;
            } else {
                reporter.heartbeat(ExecutionProgress {
                    message: format!(
                        "Release process checkout already exists at '{}'; skipping Git sync.",
                        planned.source_path.display(),
                    ),
                });
                return Ok(planned);
            }
        }

        if planned.source_is_local_workspace {
            if !planned.source_path.is_dir() {
                return Err(io::Error::new(
                    ErrorKind::NotFound,
                    format!(
                        "local workspace source '{}' does not exist",
                        planned.source_path.display(),
                    ),
                ));
            }

            reporter.heartbeat(ExecutionProgress {
                message: format!(
                    "Using local workspace source at '{}'.",
                    planned.source_path.display(),
                ),
            });
            write_process_checkout_marker(&planned)?;
            return Ok(planned);
        }

        if process_checkout_git_dir_path(&planned.source_path).is_dir()
            && !git_process_checkout_has_materialized_worktree(&planned.source_path)
        {
            reporter.heartbeat(ExecutionProgress {
                message: format!(
                    "Incomplete process checkout detected at '{}'; existing Git metadata will be re-synced before the build continues.",
                    planned.source_path.display(),
                ),
            });
        }

        let WorkspacePreparationSource::GitRef {
            repository_url,
            git_auth,
            git_ref,
        } = &input.source
        else {
            return Err(io::Error::other(
                "git-backed workspace preparation expected a Git source",
            ));
        };
        let mut sync_reporter = WorkspacePreparationProgressReporter { reporter };
        self.syncer.sync_ref_with_progress(
            &GitWorkspaceSyncRefRequest {
                repository_url: repository_url.clone(),
                workspace_path: planned.source_path.clone(),
                git_ref: git_ref.clone(),
                auth: git_auth.clone(),
            },
            &mut sync_reporter,
        )?;
        write_process_checkout_marker(&planned)?;

        Ok(planned)
    }

    /// Creates the per-build workspace only after the release process checkout exists.
    pub fn prepare_build(
        &self,
        input: &WorkspacePreparationInput,
    ) -> io::Result<PreparedWorkspace> {
        let mut reporter = NoopExecutionProgressReporter;
        self.prepare_build_with_reporter(input, &mut reporter)
    }

    /// Creates the per-build workspace only after the release process checkout exists.
    pub fn prepare_build_with_reporter(
        &self,
        input: &WorkspacePreparationInput,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> io::Result<PreparedWorkspace> {
        let planned = self.plan(input)?;
        self.directories.ensure_exists()?;

        if !process_checkout_is_ready(&planned) {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                format!(
                    "release process checkout is not ready at '{}'; prepare the process checkout before building",
                    planned.root_path.display(),
                ),
            ));
        }

        reporter.heartbeat(ExecutionProgress {
            message: format!(
                "Resetting build workspace root at '{}'.",
                planned.build_root_path.display(),
            ),
        });
        reset_prepared_workspace_root(&planned.build_root_path)?;

        for directory in [
            planned.root_path.as_path(),
            planned.build_root_path.as_path(),
            planned
                .log_path
                .parent()
                .expect("workspace log path should have a parent"),
            planned.artifact_root_path.as_path(),
        ] {
            fs::create_dir_all(directory)?;
        }

        Ok(planned)
    }

    /// Creates isolated directories and checks out the requested repository tag.
    pub fn prepare_with_reporter(
        &self,
        input: &WorkspacePreparationInput,
        reporter: &mut dyn ExecutionProgressReporter,
    ) -> io::Result<PreparedWorkspace> {
        self.prepare_process_with_reporter(input, reporter)?;
        self.prepare_build_with_reporter(input, reporter)
    }

    fn resolve_runs_root(&self, input: &WorkspacePreparationInput) -> io::Result<PathBuf> {
        Ok(
            match normalize_override_path(
                input.workspace_root_override.as_deref(),
                "workspace root override",
            )? {
                Some(workspace_root) => workspace_root.join("runs"),
                None => self.directories.runs_dir.clone(),
            },
        )
    }
}

fn build_workspace_name(build_run_id: i64, attempt_token: &str) -> String {
    let normalized_attempt_token = attempt_token
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();

    if normalized_attempt_token.is_empty() {
        format!("build-run-{build_run_id}")
    } else {
        format!("build-run-{build_run_id}-{normalized_attempt_token}")
    }
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
        .and_then(|output_path_template| {
            Path::new(&output_path_template)
                .extension()
                .map(|value| value.to_string_lossy().to_string())
        })
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
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" => String::from("archive"),
        "apk" | "aab" | "ipa" | "exe" | "appimage" | "pkg" | "dmg" => String::from("binary"),
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

    let lowered = trimmed.to_ascii_lowercase();
    let mut normalized = String::new();
    let mut previous_separator = false;
    for character in lowered.chars() {
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

    let Some(relative_path) = output_path_template
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
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
            .build_root_path
            .join("outputs")
            .join(artifact_output_base_name(plan)));
    }

    resolve_final_artifact_output_path(plan, &workspace.artifact_root_path)
}

use self::unity::resolve_final_unity_artifact_output_path as resolve_final_artifact_output_path;

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

fn normalized_optional_string(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::unity::{
        classify_execution_error, diagnose_host_native_runner_config,
        inspect_host_capability_profile_with_input, resolve_host_native_unity_execution_plan,
        selected_host_runner_family, CapabilityInspectionInput, DiscoveredUnityEditor,
        HostCapabilityProfile, HostNativeUnityExecutor, HostToolCapability,
        RunnerSelectionDiagnostics, UnityBuildExecutionProcessor, UnityLicenseDiagnostics,
    };
    use super::{
        artifact_output_relative_path, discover_artifacts, process_checkout_git_dir_path,
        process_checkout_marker_path, ExecutionPlan, ExecutionProgress,
        ExecutionProgressReporter, WorkspacePreparationInput, WorkspacePreparationSource,
        WorkspacePreparer,
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

    #[derive(Debug, Default)]
    struct RecordingExecutionProgressReporter {
        messages: Vec<String>,
    }

    impl ExecutionProgressReporter for RecordingExecutionProgressReporter {
        fn heartbeat(&mut self, progress: ExecutionProgress) {
            self.messages.push(progress.message);
        }
    }

    fn git_source(repository_path: &Path, git_ref: &str) -> WorkspacePreparationSource {
        WorkspacePreparationSource::GitRef {
            repository_url: repository_path.display().to_string(),
            git_auth: GitAuthOptions::default(),
            git_ref: String::from(git_ref),
        }
    }

    fn git_source_from_plan(plan: &ExecutionPlan) -> WorkspacePreparationSource {
        WorkspacePreparationSource::GitRef {
            repository_url: plan.repository_url.clone(),
            git_auth: GitAuthOptions::default(),
            git_ref: plan.git_tag.clone(),
        }
    }

    #[test]
    fn workspace_preparer_creates_isolated_run_directories() {
        let root = test_root("prepare-workspace");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("workspace-source-repo"),
            "2022.3.14f1",
            "v5.0.0",
        );
        let preparer = WorkspacePreparer::new(&directories);

        let prepared = preparer
            .prepare(&WorkspacePreparationInput {
                release_run_id: 24,
                build_run_id: 42,
                attempt_token: String::from("attempt-42"),
                repository_name: String::from("revolutions"),
                source: git_source(&repository_path, "v5.0.0"),
                workspace_root_override: None,
                artifacts_root_override: None,
            })
            .expect("workspace preparation should succeed");

        let expected_root = directories.runs_dir.join("release-run-24");
        assert_eq!(prepared.root_path, expected_root);
        assert_eq!(prepared.host_root_path, prepared.root_path);
        assert_eq!(
            prepared.build_root_path,
            expected_root.join("builds").join("build-run-42-attempt-42")
        );
        assert_eq!(prepared.host_build_root_path, prepared.build_root_path);

        let contents = fs::read_to_string(prepared.source_path.join(PROJECT_VERSION_FILE_PATH))
            .expect("project version file should exist in prepared workspace");
        assert!(contents.contains("m_EditorVersion: 2022.3.14f1"));

        for path in [
            prepared.root_path.as_path(),
            prepared.build_root_path.as_path(),
            prepared.source_path.as_path(),
            prepared.artifact_root_path.as_path(),
        ] {
            assert!(path.is_dir(), "expected {:?} to be a directory", path);
        }

        assert_eq!(
            prepared.log_path,
            prepared
                .build_root_path
                .join("logs")
                .join("unity-build.log")
        );
        assert_eq!(
            prepared.artifact_root_path,
            prepared.build_root_path.join("outputs")
        );
        assert_eq!(
            prepared.host_artifact_root_path,
            prepared.artifact_root_path
        );

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn workspace_preparer_plan_matches_prepared_layout() {
        let root = test_root("plan-workspace");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("workspace-plan-source-repo"),
            "2022.3.14f1",
            "v5.1.0",
        );
        let preparer = WorkspacePreparer::new(&directories);
        let input = WorkspacePreparationInput {
            release_run_id: 25,
            build_run_id: 52,
            attempt_token: String::from("attempt-52"),
            repository_name: String::from("revolutions"),
            source: git_source(&repository_path, "v5.1.0"),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let planned = preparer
            .plan(&input)
            .expect("workspace plan should resolve");
        let prepared = preparer
            .prepare(&input)
            .expect("workspace preparation should succeed");

        assert_eq!(planned, prepared);

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn workspace_preparer_syncs_process_checkout_only_once() {
        let root = test_root("prepare-process-once");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("process-once-source-repo"),
            "2022.3.14f1",
            "v5.1.1",
        );
        let preparer = WorkspacePreparer::new(&directories);
        let input = WorkspacePreparationInput {
            release_run_id: 25,
            build_run_id: 43,
            attempt_token: String::from("attempt-43"),
            repository_name: String::from("revolutions"),
            source: git_source(&repository_path, "v5.1.1"),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let mut first_reporter = RecordingExecutionProgressReporter::default();
        let first = preparer
            .prepare_process_with_reporter(&input, &mut first_reporter)
            .expect("first process checkout should succeed");
        let marker_path = process_checkout_marker_path(&first.root_path);
        assert!(marker_path.is_file());
        assert!(first.source_path.join(PROJECT_VERSION_FILE_PATH).is_file());
        assert!(first_reporter
            .messages
            .iter()
            .any(|message| message.contains("Fetching ref 'v5.1.1'")));

        let mut second_reporter = RecordingExecutionProgressReporter::default();
        let second = preparer
            .prepare_process_with_reporter(&input, &mut second_reporter)
            .expect("second process checkout should reuse existing source");

        assert_eq!(first.root_path, second.root_path);
        assert!(second_reporter
            .messages
            .iter()
            .any(|message| message.contains("skipping Git sync")));
        assert!(second_reporter
            .messages
            .iter()
            .all(|message| !message.contains("Fetching ref")));

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn workspace_preparer_resyncs_partial_git_checkout_without_marker() {
        let root = test_root("prepare-process-partial-git-checkout");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("partial-process-source-repo"),
            "2022.3.14f1",
            "v5.1.3",
        );
        let preparer = WorkspacePreparer::new(&directories);
        let input = WorkspacePreparationInput {
            release_run_id: 26,
            build_run_id: 44,
            attempt_token: String::from("attempt-44"),
            repository_name: String::from("revolutions"),
            source: git_source(&repository_path, "v5.1.3"),
            workspace_root_override: None,
            artifacts_root_override: None,
        };
        let planned = preparer.plan(&input).expect("workspace plan should resolve");

        fs::create_dir_all(planned.source_path.parent().expect("source parent should exist"))
            .expect("source parent should create");
        run_git_test_command(
            planned
                .source_path
                .parent()
                .expect("source parent should exist"),
            &[
                "clone",
                "--no-checkout",
                repository_path.to_string_lossy().as_ref(),
                planned.source_path.to_string_lossy().as_ref(),
            ],
        );

        assert!(process_checkout_git_dir_path(&planned.source_path).is_dir());
        assert!(!process_checkout_marker_path(&planned.root_path).is_file());
        assert!(!preparer
            .is_process_prepared(&input)
            .expect("partial checkout should be inspectable"));

        let mut reporter = RecordingExecutionProgressReporter::default();
        let prepared = preparer
            .prepare_process_with_reporter(&input, &mut reporter)
            .expect("partial checkout should resync successfully");

        assert!(process_checkout_marker_path(&prepared.root_path).is_file());
        assert!(prepared
            .source_path
            .join(PROJECT_VERSION_FILE_PATH)
            .is_file());
        assert!(reporter
            .messages
            .iter()
            .any(|message| message.contains("Fetching ref 'v5.1.3'")));

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn workspace_preparer_resyncs_stale_marker_git_checkout() {
        let root = test_root("prepare-process-stale-marker-checkout");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("stale-marker-source-repo"),
            "2022.3.14f1",
            "v5.1.4",
        );
        let preparer = WorkspacePreparer::new(&directories);
        let input = WorkspacePreparationInput {
            release_run_id: 28,
            build_run_id: 45,
            attempt_token: String::from("attempt-45"),
            repository_name: String::from("revolutions"),
            source: git_source(&repository_path, "v5.1.4"),
            workspace_root_override: None,
            artifacts_root_override: None,
        };
        let planned = preparer.plan(&input).expect("workspace plan should resolve");

        fs::create_dir_all(planned.source_path.parent().expect("source parent should exist"))
            .expect("source parent should create");
        run_git_test_command(
            planned
                .source_path
                .parent()
                .expect("source parent should exist"),
            &[
                "clone",
                "--no-checkout",
                repository_path.to_string_lossy().as_ref(),
                planned.source_path.to_string_lossy().as_ref(),
            ],
        );
        fs::write(process_checkout_marker_path(&planned.root_path), b"ready\n")
            .expect("stale checkout marker should write");

        assert!(!preparer
            .is_process_prepared(&input)
            .expect("stale marker checkout should be inspectable"));

        let mut reporter = RecordingExecutionProgressReporter::default();
        let prepared = preparer
            .prepare_process_with_reporter(&input, &mut reporter)
            .expect("stale marker checkout should resync successfully");

        assert!(process_checkout_marker_path(&prepared.root_path).is_file());
        assert!(prepared
            .source_path
            .join(PROJECT_VERSION_FILE_PATH)
            .is_file());
        assert!(reporter.messages.iter().any(|message| {
            message.contains("Incomplete process checkout detected")
                && message.contains("clearing the stale checkout marker")
        }));

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn workspace_preparer_prepare_build_requires_existing_process_checkout() {
        let root = test_root("prepare-build-requires-process-checkout");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("prepare-build-requires-process-checkout-source-repo"),
            "2022.3.14f1",
            "v5.1.2",
        );
        let preparer = WorkspacePreparer::new(&directories);

        let error = preparer
            .prepare_build(&WorkspacePreparationInput {
                release_run_id: 26,
                build_run_id: 44,
                attempt_token: String::from("attempt-44"),
                repository_name: String::from("revolutions"),
                source: git_source(&repository_path, "v5.1.2"),
                workspace_root_override: None,
                artifacts_root_override: None,
            })
            .expect_err("build preparation should fail without a process checkout");

        assert!(error
            .to_string()
            .contains("prepare the process checkout before building"));

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn workspace_preparer_uses_workspace_root_override_and_keeps_outputs_in_process() {
        let root = test_root("prepare-workspace-overrides");
        let directories = RuntimeDirectories::from_root(&root.join("runtime-root"));
        directories
            .ensure_exists()
            .expect("runtime directories should create");
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
                release_run_id: 26,
                build_run_id: 53,
                attempt_token: String::from("attempt-53"),
                repository_name: String::from("revolutions"),
                source: git_source(&repository_path, "v5.2.0"),
                workspace_root_override: Some(workspace_root_override.display().to_string()),
                artifacts_root_override: Some(build_output_override.display().to_string()),
            })
            .expect("workspace preparation with overrides should succeed");

        assert_eq!(
            prepared.root_path,
            workspace_root_override.join("runs").join("release-run-26")
        );
        assert_eq!(
            prepared.log_path,
            workspace_root_override
                .join("runs")
                .join("release-run-26")
                .join("builds")
                .join("build-run-53-attempt-53")
                .join("logs")
                .join("unity-build.log")
        );
        assert_eq!(
            prepared.artifact_root_path,
            prepared.build_root_path.join("outputs")
        );
        assert_eq!(build_output_override, root.join("build-output"));
        assert!(prepared
            .source_path
            .join(PROJECT_VERSION_FILE_PATH)
            .is_file());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn execution_processor_prepare_workspace_reports_checkout_progress() {
        let root = test_root("prepare-workspace-progress");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("workspace-progress-source-repo"),
            "2022.3.14f1",
            "v5.3.0",
        );
        let processor =
            UnityBuildExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let preparation = WorkspacePreparationInput {
            release_run_id: 27,
            build_run_id: 54,
            attempt_token: String::from("attempt-54"),
            repository_name: String::from("revolutions"),
            source: git_source(&repository_path, "v5.3.0"),
            workspace_root_override: None,
            artifacts_root_override: None,
        };
        let mut reporter = RecordingExecutionProgressReporter::default();

        let prepared = processor
            .prepare_workspace_with_reporter(&preparation, &mut reporter)
            .expect("workspace preparation should emit progress");

        assert!(prepared
            .source_path
            .join(PROJECT_VERSION_FILE_PATH)
            .is_file());
        assert!(reporter
            .messages
            .iter()
            .any(|message| { message.contains("Resetting build workspace root") }));
        assert!(reporter
            .messages
            .iter()
            .any(|message| { message.contains("Creating process workspace directories") }));
        assert!(reporter
            .messages
            .iter()
            .any(|message| { message.contains("Cloning repository metadata") }));
        assert!(reporter
            .messages
            .iter()
            .any(|message| { message.contains("Fetching ref 'v5.3.0'") }));
        assert!(reporter
            .messages
            .iter()
            .any(|message| { message.contains("Checking out fetched ref 'v5.3.0'") }));
        assert!(reporter
            .messages
            .iter()
            .any(|message| { message.contains("Repository ref 'v5.3.0' is ready") }));

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
        assert_eq!(diagnostics.process_priority, "low");
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
        assert_eq!(diagnostics.process_priority, "low");
        assert!(!diagnostics.unity_executable_exists);
        assert!(!diagnostics.unity_executable_is_file);
    }

    #[test]
    fn diagnose_host_native_runner_config_reports_invalid_config() {
        let diagnostics = diagnose_host_native_runner_config("{}");

        assert_eq!(diagnostics.status, "invalid_config");
        assert_eq!(diagnostics.unity_executable_path, None);
        assert_eq!(diagnostics.process_priority, "low");
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
            unity_target_platform: String::from("windows"),
            runner_type: String::from("host-native"),
            unity_build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Players")),
            engine_version: String::from("2022.3.14f1"),
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

        let resolved = resolve_host_native_unity_execution_plan(&plan, &profile)
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
            unity_target_platform: String::from("windows"),
            runner_type: String::from("host-native"),
            unity_build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Players")),
            engine_version: String::from("2022.3.30f1"),
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

        let error = resolve_host_native_unity_execution_plan(&plan, &profile)
            .expect_err("plan should fail when the requested version was not discovered");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("2022.3.30f1"));
    }

    #[test]
    fn inspect_host_capability_profile_discovers_editors_and_selects_runner() {
        let root = test_root("host-capability-profile-ready");
        fs::create_dir_all(&root).expect("test root should create");

        let platform = HostPlatform::current();
        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).expect("bin directory should create");
        for command_name in required_test_commands(platform) {
            fs::write(bin_dir.join(command_name), b"tool").expect("fake tool should write");
        }

        let discovery_root =
            create_discovered_unity_install(&root.join("unity-root"), platform, "2022.3.14f1");
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
            fs::write(bin_dir.join(command_name), b"tool").expect("fake tool should write");
        }

        let discovery_root =
            create_discovered_unity_install(&root.join("unity-root"), platform, "2022.3.15f1");

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
        assert_eq!(
            profile.runner_selection.status,
            "warning_license_unconfirmed"
        );

        fs::remove_dir_all(root).expect("test root should be removable");
    }

    #[test]
    fn workspace_preparer_groups_artifacts_by_repository_and_tag() {
        let root = test_root("prepare-workspace-grouping");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("workspace-grouping-source-repo"),
            "2021.3.18f1",
            "v6.0.0",
        );
        let preparer = WorkspacePreparer::new(&directories);

        let first = preparer
            .prepare(&WorkspacePreparationInput {
                release_run_id: 31,
                build_run_id: 7,
                attempt_token: String::from("attempt-a"),
                repository_name: String::from("revolutions"),
                source: git_source(&repository_path, "v6.0.0"),
                workspace_root_override: None,
                artifacts_root_override: None,
            })
            .expect("first workspace should prepare");
        let stale_path = first.build_root_path.join("stale-checkout.txt");
        fs::write(&stale_path, "stale")
            .expect("stale marker should write into the first workspace");
        let second = preparer
            .prepare(&WorkspacePreparationInput {
                release_run_id: 31,
                build_run_id: 7,
                attempt_token: String::from("attempt-b"),
                repository_name: String::from("revolutions"),
                source: git_source(&repository_path, "v6.0.0"),
                workspace_root_override: None,
                artifacts_root_override: None,
            })
            .expect("second workspace should prepare");

        assert_eq!(first.root_path, second.root_path);
        assert_ne!(first.artifact_root_path, second.artifact_root_path);
        assert_ne!(first.build_root_path, second.build_root_path);
        assert_ne!(first.log_path, second.log_path);
        assert!(stale_path.exists());
        assert!(second.source_path.join(PROJECT_VERSION_FILE_PATH).is_file());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn discover_artifacts_lists_regular_files_in_sorted_order() {
        let root = test_root("discover-artifacts");
        let artifact_root = root.join("artifacts");
        fs::create_dir_all(artifact_root.join("nested")).expect("artifact directory should create");
        fs::write(artifact_root.join("game.zip"), "artifact").expect("root artifact should write");
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
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("host-native-success-source-repo"),
            "2022.3.14f1",
            "v7.0.0",
        );
        let script_path = create_fake_unity_script(&root, "success", ScriptKind::Success);
        let processor =
            UnityBuildExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let plan = ExecutionPlan {
            build_run_id: 61,
            release_run_id: 71,
            build_target_id: 81,
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_tag: String::from("v7.0.0"),
            target_name: String::from("webgl"),
            unity_target_platform: String::from("webgl"),
            runner_type: String::from("host-native"),
            unity_build_method: String::from("Builder.PerformWebGL"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/WebGL")),
            engine_version: String::from("2022.3.14f1"),
            config_json: json!({
                "unity_executable_path": script_path.display().to_string(),
                "additional_arguments": ["--custom-flag"],
                "environment": {"CUSTOM_FLAG": "workers"}
            })
            .to_string(),
            timeout_seconds: 5,
        };
        let preparation = WorkspacePreparationInput {
            release_run_id: plan.release_run_id,
            build_run_id: plan.build_run_id,
            attempt_token: String::from("attempt-61"),
            repository_name: plan.repository_name.clone(),
            source: git_source_from_plan(&plan),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let outcome = processor
            .process(&plan, &preparation)
            .expect("host-native execution should process");

        assert!(outcome.error.is_none());
        let contents =
            fs::read_to_string(&outcome.result.log_path).expect("execution log should exist");
        assert!(contents.contains("-batchmode"));
        assert!(contents.contains("-buildTarget webgl"));
        assert!(contents.contains("-executeMethod Builder.PerformWebGL"));
        assert!(contents.contains("--custom-flag"));
        assert!(contents.contains("custom:workers"));

        let expected_output = outcome
            .result
            .build_root_path
            .join("outputs")
            .join("revolutions.v7.0.0.webgl");
        assert_eq!(outcome.result.output_path, expected_output);
        assert!(expected_output.is_dir());
        assert!(expected_output.join("artifact.txt").is_file());
        assert!(!outcome
            .result
            .artifact_root_path
            .join("revolutions.v7.0.0.webgl.zip")
            .exists());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn execution_processor_persists_preamble_when_host_native_command_writes_no_log() {
        let root = test_root("host-native-missing-log");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("host-native-missing-log-source-repo"),
            "2022.3.14f1",
            "v7.0.1",
        );
        let script_path = create_fake_unity_script(&root, "missing-log", ScriptKind::SilentSuccess);
        let processor =
            UnityBuildExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let plan = ExecutionPlan {
            build_run_id: 64,
            release_run_id: 74,
            build_target_id: 84,
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_tag: String::from("v7.0.1"),
            target_name: String::from("windows"),
            unity_target_platform: String::from("windows"),
            runner_type: String::from("host-native"),
            unity_build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Windows")),
            engine_version: String::from("2022.3.14f1"),
            config_json: json!({
                "unity_executable_path": script_path.display().to_string()
            })
            .to_string(),
            timeout_seconds: 5,
        };
        let preparation = WorkspacePreparationInput {
            release_run_id: plan.release_run_id,
            build_run_id: plan.build_run_id,
            attempt_token: String::from("attempt-64"),
            repository_name: plan.repository_name.clone(),
            source: git_source_from_plan(&plan),
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
            .build_root_path
            .join("outputs")
            .join("revolutions.v7.0.1.windows");
        assert_eq!(outcome.result.output_path, expected_output);
        assert!(expected_output.is_dir());
        assert!(expected_output.join("artifact.txt").is_file());
        assert!(!outcome
            .result
            .artifact_root_path
            .join("revolutions.v7.0.1.windows.zip")
            .exists());
        assert!(!outcome
            .result
            .artifact_root_path
            .join("revolutions.v7.0.1.windows.zip")
            .exists());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn execution_processor_enriches_failure_and_preserves_paths() {
        let root = test_root("host-native-failure");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("host-native-failure-source-repo"),
            "2022.3.14f1",
            "v7.1.0",
        );
        let script_path = create_fake_unity_script(&root, "failure", ScriptKind::Failure);
        let processor =
            UnityBuildExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let plan = ExecutionPlan {
            build_run_id: 62,
            release_run_id: 72,
            build_target_id: 82,
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_tag: String::from("v7.1.0"),
            target_name: String::from("windows"),
            unity_target_platform: String::from("windows"),
            runner_type: String::from("host-native"),
            unity_build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Windows")),
            engine_version: String::from("2022.3.14f1"),
            config_json: json!({
                "unity_executable_path": script_path.display().to_string()
            })
            .to_string(),
            timeout_seconds: 5,
        };
        let preparation = WorkspacePreparationInput {
            release_run_id: plan.release_run_id,
            build_run_id: plan.build_run_id,
            attempt_token: String::from("attempt-62"),
            repository_name: plan.repository_name.clone(),
            source: git_source_from_plan(&plan),
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
        assert!(
            contents.contains("No valid Unity Editor license found. Please activate your license.")
        );
        assert!(outcome.result.workspace_path.is_dir());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn execution_processor_times_out_host_native_command_and_reports_cancellation() {
        let root = test_root("host-native-timeout");
        let directories = RuntimeDirectories::from_root(&root);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository_path = create_tagged_unity_repository(
            &root.join("host-native-timeout-source-repo"),
            "2022.3.14f1",
            "v7.2.0",
        );
        let script_path = create_fake_unity_script(&root, "timeout", ScriptKind::Slow);
        let processor =
            UnityBuildExecutionProcessor::new(&directories, HostNativeUnityExecutor::new());
        let plan = ExecutionPlan {
            build_run_id: 63,
            release_run_id: 73,
            build_target_id: 83,
            repository_name: String::from("revolutions"),
            repository_url: repository_path.display().to_string(),
            git_tag: String::from("v7.2.0"),
            target_name: String::from("linux"),
            unity_target_platform: String::from("linux"),
            runner_type: String::from("host-native"),
            unity_build_method: String::from("Builder.PerformLinux"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Linux")),
            engine_version: String::from("2022.3.14f1"),
            config_json: json!({
                "unity_executable_path": script_path.display().to_string()
            })
            .to_string(),
            timeout_seconds: 1,
        };
        let preparation = WorkspacePreparationInput {
            release_run_id: plan.release_run_id,
            build_run_id: plan.build_run_id,
            attempt_token: String::from("attempt-63"),
            repository_name: plan.repository_name.clone(),
            source: git_source_from_plan(&plan),
            workspace_root_override: None,
            artifacts_root_override: None,
        };

        let outcome = processor
            .process(&plan, &preparation)
            .expect("timed out execution should still return paths");

        let error = outcome.error.expect("timed out execution should fail");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error
            .to_string()
            .contains("timeout: host-native unity runner exceeded 1s timeout"));
        assert!(!outcome
            .result
            .artifact_root_path
            .join("revolutions.v7.2.0.linux.zip")
            .exists());

        fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn artifact_output_relative_path_normalizes_all_parts_to_lowercase() {
        let plan = ExecutionPlan {
            build_run_id: 63,
            release_run_id: 73,
            build_target_id: 83,
            repository_name: String::from("Revolutions"),
            repository_url: String::from("https://example.com/Revolutions.git"),
            git_tag: String::from("V7.3.0"),
            target_name: String::from("Windows"),
            unity_target_platform: String::from("windows"),
            runner_type: String::from("host-native"),
            unity_build_method: String::from("Builder.PerformWindows"),
            output_kind: Some(String::from("archive")),
            output_path_template: Some(String::from("Builds/Windows")),
            engine_version: String::from("2022.3.14f1"),
            config_json: String::from("{}"),
            timeout_seconds: 1,
        };

        assert_eq!(
            artifact_output_relative_path(&plan),
            String::from("revolutions.v7.3.0.windows.zip"),
        );
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
            "handy-games-publisher-runtime-runner-{label}-{}",
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
