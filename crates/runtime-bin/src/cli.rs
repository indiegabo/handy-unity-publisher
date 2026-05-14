//! Defines the runtime CLI surface, argument parsing, usage text, and
//! operator-facing inspection payloads returned by command handlers.

use super::*;

pub(crate) enum RuntimeCommand {
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
    pub(crate) fn from_args(arguments: &[String]) -> Self {
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
pub(crate) struct ManualReleaseDispatchCommand {
    pub(crate) repository_id: i64,
    pub(crate) git_tag: String,
    pub(crate) git_commit: String,
    pub(crate) requested_via: String,
    pub(crate) rebuild: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestSyncCommand {
    pub(crate) manifest_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeedRevolutionsRegistrationCommand {
    pub(crate) project_pat_env: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistrationCheckoutCommand {
    pub(crate) repository_id: i64,
    pub(crate) git_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistrationImportRuntimeDbCommand {
    pub(crate) source_db_path: PathBuf,
    pub(crate) repository_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublishInspectScope {
    BuildRun(i64),
    PublishRun(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PublishInspectCommand {
    pub(crate) scope: PublishInspectScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReleasePlanCommand {
    pub(crate) release_run_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublishedOutputInspectionReport {
    pub(crate) requested_build_run_id: Option<i64>,
    pub(crate) requested_publish_run_id: Option<i64>,
    pub(crate) publish_runs: Vec<PublishedOutputDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PublishedOutputDiagnostic {
    pub(crate) publish_run_id: i64,
    pub(crate) build_run_id: i64,
    pub(crate) release_run_id: i64,
    pub(crate) publish_target_id: i64,
    pub(crate) artifact_id: Option<i64>,
    pub(crate) status: String,
    pub(crate) destination_ref: Option<String>,
    pub(crate) expected_destination_ref: Option<String>,
    pub(crate) publish_target_name: Option<String>,
    pub(crate) publish_target_kind: Option<String>,
    pub(crate) artifact_name: Option<String>,
    pub(crate) artifact_path: Option<String>,
    pub(crate) source_path: Option<String>,
    pub(crate) destination_exists: bool,
    pub(crate) destination_is_file: bool,
    pub(crate) destination_size_bytes: Option<u64>,
    pub(crate) destination_error: Option<String>,
    pub(crate) expected_destination_error: Option<String>,
    pub(crate) plan_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct PublishedDestinationStatus {
    pub(crate) exists: bool,
    pub(crate) is_file: bool,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildExecutionReport {
    pub(crate) schema_version: u32,
    pub(crate) cleanup_policy: String,
    pub(crate) build_plan: StoredBuildExecutionPlan,
    pub(crate) build_run: BuildRunRecord,
    pub(crate) stages: Vec<BuildRunStageRecord>,
    pub(crate) artifacts: Vec<ArtifactRecord>,
    pub(crate) publish_runs: Vec<BuildExecutionPublishSnapshot>,
    pub(crate) attempts: Vec<BuildExecutionAttemptSnapshot>,
    pub(crate) cleanup: BuildExecutionCleanupSnapshot,
    pub(crate) interruption: Option<BuildExecutionInterruptionSnapshot>,
    pub(crate) retained_files: Vec<BuildExecutionRetainedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildExecutionPublishSnapshot {
    pub(crate) record: PublishRunRecord,
    pub(crate) execution_plan: Option<StoredPublishExecutionPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildExecutionAttemptSnapshot {
    pub(crate) workspace_path: String,
    pub(crate) is_final_workspace: bool,
    pub(crate) removed_after_cleanup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildExecutionCleanupSnapshot {
    pub(crate) status: String,
    pub(crate) trigger: String,
    pub(crate) workspace_path: String,
    pub(crate) workspace_bytes_before: u64,
    pub(crate) workspace_bytes_after: u64,
    pub(crate) removed_attempt_count: usize,
    pub(crate) error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildExecutionInterruptionSnapshot {
    pub(crate) kind: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildExecutionRetainedFile {
    pub(crate) role: String,
    pub(crate) path: String,
    pub(crate) source_path: Option<String>,
    pub(crate) content_type: String,
    pub(crate) content_encoding: Option<String>,
    pub(crate) size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AutomationPollReport {
    pub(crate) repositories: Vec<RepositoryPollResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RepositoryPollResult {
    pub(crate) repository_id: i64,
    pub(crate) repository_name: String,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) last_seen_tag_before: Option<String>,
    pub(crate) last_seen_tag_after: Option<String>,
    pub(crate) discovered_tags: Vec<GitTag>,
    pub(crate) queued_release_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RegistrationSeedReport {
    pub(crate) registration_name: String,
    pub(crate) repository_id: i64,
    pub(crate) build_target_count: i64,
    pub(crate) workspace_root_override: Option<String>,
    pub(crate) artifacts_root_override: Option<String>,
    pub(crate) project_pat_env: String,
    pub(crate) seed_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RegistrationCheckoutReport {
    pub(crate) repository_id: i64,
    pub(crate) repository_name: String,
    pub(crate) source_mode: String,
    pub(crate) workspace_strategy: String,
    pub(crate) git_ref: String,
    pub(crate) git_ref_source: String,
    pub(crate) workspace_root_path: String,
    pub(crate) checkout_path: String,
    pub(crate) head_commit: String,
}

pub(crate) struct ResolvedBuildContext {
    pub(crate) plan: StoredBuildExecutionPlan,
    pub(crate) preparation: WorkspacePreparationInput,
}

pub(crate) struct ResolvedPublishContext {
    pub(crate) plan: StoredPublishExecutionPlan,
}

pub(crate) fn print_status(config: &RuntimeConfig, storage: &StorageLayout) {
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
        "worker loop interval: {} ms",
        config.runtime_loop.worker_loop_interval_millis
    );
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

pub(crate) fn print_help() {
    println!("HUP runtime scaffold");
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

pub(crate) fn parse_manual_release_dispatch_command(
    arguments: &[String],
) -> io::Result<ManualReleaseDispatchCommand> {
    let mut repository_id = None;
    let mut git_tag = None;
    let mut git_commit = String::new();
    let mut requested_via = String::from("hup-runtime");
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

pub(crate) fn parse_release_plan_command(arguments: &[String]) -> io::Result<ReleasePlanCommand> {
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

pub(crate) fn parse_manifest_sync_command(arguments: &[String]) -> io::Result<ManifestSyncCommand> {
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

pub(crate) fn parse_seed_revolutions_registration_command(
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

pub(crate) fn parse_registration_checkout_command(
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

pub(crate) fn parse_registration_import_runtime_db_command(
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

pub(crate) fn parse_publish_inspect_command(arguments: &[String]) -> io::Result<PublishInspectCommand> {
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

pub(crate) fn default_manifest_directory() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("pipelines")
}

pub(crate) fn revolutions_managed_repository_seed_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("revolutions-managed-repository.sql")
}

pub(crate) fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

pub(crate) fn read_flag_value(arguments: &[String], index: usize, flag: &str) -> io::Result<String> {
    arguments
        .get(index + 1)
        .cloned()
        .ok_or_else(|| cli_usage_error(format!("missing value for {flag}")))
}

pub(crate) fn parse_positive_i64_flag(value: &str, label: &str) -> io::Result<i64> {
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

pub(crate) fn require_cli_value(value: &str, label: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(cli_usage_error(format!("{label} must not be empty")));
    }

    Ok(trimmed.to_owned())
}

pub(crate) fn is_help_request(arguments: &[String]) -> bool {
    matches!(arguments.first().map(String::as_str), Some("help") | Some("--help") | Some("-h"))
}

pub(crate) fn cli_usage_error(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message.into())
}

pub(crate) fn releases_usage() -> &'static str {
    "HUP runtime releases commands\n\nUsage:\n  releases dispatch manual --repository-id <id> --git-tag <tag> [--git-commit <sha>] [--requested-via <source>] [--rebuild]\n  releases plan --release-run-id <id>\n"
}

pub(crate) fn automation_usage() -> &'static str {
    "HUP runtime automation commands\n\nUsage:\n  automation inspect\n  automation poll-once\n"
}

pub(crate) fn registrations_usage() -> &'static str {
    "HUP runtime registrations commands\n\nUsage:\n  registrations checkout --repository-id <id> [--ref <git-ref>]\n  registrations import-runtime-db --source-db <path> --repository-name <name>\n  registrations seed-revolutions [--project-pat-env <env>]\n"
}

pub(crate) fn registrations_checkout_usage() -> &'static str {
    "HUP runtime registrations checkout\n\nUsage:\n  registrations checkout --repository-id <id> [--ref <git-ref>]\n\nDefaults:\n  --ref defaults to the repository default_branch stored in SQLite\n"
}

pub(crate) fn registrations_import_runtime_db_usage() -> &'static str {
    "HUP runtime registrations import-runtime-db\n\nUsage:\n  registrations import-runtime-db --source-db <path> --repository-name <name>\n\nBehavior:\n  imports repository configuration from another runtime.db into the current app database without copying release, build, or publish runs\n"
}

pub(crate) fn manifests_usage() -> &'static str {
    "HUP runtime manifests commands\n\nUsage:\n  manifests sync [--dir <path>]\n"
}

pub(crate) fn builds_usage() -> &'static str {
    "HUP runtime builds commands\n\nUsage:\n  builds stage-next\n  builds run-next\n"
}

pub(crate) fn publishes_usage() -> &'static str {
    "HUP runtime publishes commands\n\nUsage:\n  publishes run-next\n  publishes inspect (--build-run-id <id> | --publish-run-id <id>)\n"
}

pub(crate) fn release_dispatch_usage() -> &'static str {
    "HUP runtime release dispatch commands\n\nUsage:\n  releases dispatch manual --repository-id <id> --git-tag <tag> [--git-commit <sha>] [--requested-via <source>] [--rebuild]\n"
}

pub(crate) fn manual_release_dispatch_usage() -> &'static str {
    "HUP runtime releases dispatch manual\n\nUsage:\n  releases dispatch manual --repository-id <id> --git-tag <tag> [--git-commit <sha>] [--requested-via <source>] [--rebuild]\n"
}

pub(crate) fn release_plan_usage() -> &'static str {
    "HUP runtime releases plan\n\nUsage:\n  releases plan --release-run-id <id>\n"
}

pub(crate) fn automation_inspect_usage() -> &'static str {
    "HUP runtime automation inspect\n\nUsage:\n  automation inspect\n"
}

pub(crate) fn automation_poll_once_usage() -> &'static str {
    "HUP runtime automation poll-once\n\nUsage:\n  automation poll-once\n"
}

pub(crate) fn registrations_seed_revolutions_usage() -> &'static str {
    "HUP runtime registrations seed-revolutions\n\nUsage:\n  registrations seed-revolutions [--project-pat-env <env>]\n\nDefaults:\n  --project-pat-env defaults to REVOLUTIONS_PROJECT_PAT\n"
}

pub(crate) fn manifest_sync_usage() -> &'static str {
    "HUP runtime manifests sync\n\nUsage:\n  manifests sync [--dir <path>]\n\nDefaults:\n  --dir defaults to ./pipelines relative to the current working directory\n"
}

pub(crate) fn build_stage_next_usage() -> &'static str {
    "HUP runtime builds stage-next\n\nUsage:\n  builds stage-next\n"
}

pub(crate) fn build_run_next_usage() -> &'static str {
    "HUP runtime builds run-next\n\nUsage:\n  builds run-next\n"
}

pub(crate) fn publish_run_next_usage() -> &'static str {
    "HUP runtime publishes run-next\n\nUsage:\n  publishes run-next\n"
}

pub(crate) fn publish_inspect_usage() -> &'static str {
    "HUP runtime publishes inspect\n\nUsage:\n  publishes inspect --build-run-id <id>\n  publishes inspect --publish-run-id <id>\n"
}
