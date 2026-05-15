//! Runs the local runtime binary, including supervision, polling, build
//! execution, publish execution, and recovery-oriented cleanup flows.

mod builds;
mod cli;
mod workers;

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
    update_runtime_health, write_supervisor_snapshot, emit_runtime_event,
    RuntimeEventInput, RuntimeRestartPolicy,
    RuntimeSupervisionContract, RuntimeSupervisorSnapshot, RuntimeSupervisorStatus,
    RuntimeStatus, RUNTIME_HEARTBEAT_EVENT,
};
use runtime_runner::{
    discover_artifacts, resolve_final_artifact_output_path, ExecutionPlan,
    ExecutionProcessOutcome, ExecutionProcessor, ExecutionProgress,
    ExecutionProgressReporter, ExecutionResult,
    inspect_host_capability_profile, resolve_host_native_execution_plan,
    HostCapabilityProfile, HostNativeUnityExecutor, PreparedWorkspace, RunnerFamily,
    WorkspacePreparationInput, WorkspacePreparer,
};
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
    RepositoryPollDispatchInput, StartBuildRunInput,
    StartBuildRunStageInput, StartPublishRunInput, StorageLayout, CompletePublishRunInput,
    FailPublishRunInput,
    RuntimeControlRequest, take_runtime_control_requests,
    RuntimeRecoveryReport,
    RECOVERY_INTERRUPTION_KIND_REQUESTED, RECOVERY_INTERRUPTION_KIND_SYSTEM,
    open_connection,
};
use builds::*;
use serde::{Deserialize, Serialize};
use cli::*;
use workers::*;
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
const POLL_OBSERVED_VIA: &str = "hup-runtime";
const DEFAULT_REVOLUTIONS_PROJECT_PAT_ENV: &str = "REVOLUTIONS_PROJECT_PAT";
const EVENT_TOPIC_RELEASE_QUEUED: &str = "automation.release_queued";
const EVENT_TOPIC_POLL_AUTH_FAILED: &str = "automation.poll_auth_failed";
const EVENT_TOPIC_BUILD_RUN_STARTED: &str = "build.run_started";
const EVENT_TOPIC_BUILD_RUN_FINISHED: &str = "build.run_finished";
const EVENT_TOPIC_BUILD_RUN_STAGE_UPDATED: &str = "build.stage_updated";
const EVENT_TOPIC_PUBLISH_RUN_STARTED: &str = "publish.run_started";
const EVENT_TOPIC_PUBLISH_RUN_FINISHED: &str = "publish.run_finished";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseEventContext {
    release_run_id: i64,
    repository_id: i64,
    repository_name: String,
    git_tag: String,
    git_commit: Option<String>,
    user_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildRunEventContext {
    release_run_id: i64,
    build_run_id: i64,
    repository_id: i64,
    repository_name: String,
    git_tag: String,
    target_name: String,
    platform: String,
    user_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishRunEventContext {
    release_run_id: i64,
    build_run_id: i64,
    publish_run_id: i64,
    repository_id: i64,
    repository_name: String,
    git_tag: String,
    publish_target_id: i64,
    publish_target_name: String,
    artifact_name: String,
    user_requested: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("runtime command failed: {error}");
        process::exit(1);
    }
}

fn user_requested_from_trigger_source(trigger_source: &str) -> bool {
    trigger_source.eq_ignore_ascii_case("manual")
}

fn build_run_event_context(
    coordinator: &LocalCoordinator,
    plan: &StoredBuildExecutionPlan,
) -> BuildRunEventContext {
    let user_requested = coordinator
        .get_release_run_record(plan.release_run_id)
        .map(|release| user_requested_from_trigger_source(&release.trigger_source))
        .unwrap_or(false);

    BuildRunEventContext {
        release_run_id: plan.release_run_id,
        build_run_id: plan.build_run_id,
        repository_id: plan.repository_id,
        repository_name: plan.repository_name.clone(),
        git_tag: plan.git_tag.clone(),
        target_name: plan.target_name.clone(),
        platform: plan.platform.clone(),
        user_requested,
    }
}

fn publish_run_event_context(
    coordinator: &LocalCoordinator,
    plan: &PublishExecutionPlan,
) -> PublishRunEventContext {
    let user_requested = coordinator
        .get_release_run_record(plan.release_run_id)
        .map(|release| user_requested_from_trigger_source(&release.trigger_source))
        .unwrap_or(false);

    PublishRunEventContext {
        release_run_id: plan.release_run_id,
        build_run_id: plan.build_run_id,
        publish_run_id: plan.publish_run_id,
        repository_id: plan.repository_id,
        repository_name: plan.repository_name.clone(),
        git_tag: plan.git_tag.clone(),
        publish_target_id: plan.publish_target_id,
        publish_target_name: plan.publish_target_name.clone(),
        artifact_name: plan.artifact_name.clone(),
        user_requested,
    }
}

fn log_runtime_event_failure(topic: &str, error: &io::Error) {
    eprintln!("failed to emit runtime event {topic}: {error}");
}

fn terminal_event_severity(status: &str) -> &'static str {
    match status {
        "failed" => "error",
        "canceled" | "cancelled" => "warn",
        _ => "info",
    }
}

fn emit_release_queued_event(
    storage: &StorageLayout,
    context: &ReleaseEventContext,
) -> io::Result<()> {
    let mode = if context.user_requested {
        "Manual"
    } else {
        "Automatic"
    };
    emit_runtime_event(
        storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_RELEASE_QUEUED),
            severity: String::from("info"),
            origin: String::from("runtime-bin"),
            user_requested: context.user_requested,
            repository_id: Some(context.repository_id),
            release_run_id: Some(context.release_run_id),
            build_run_id: None,
            publish_run_id: None,
            summary: format!(
                "{mode} release queued for {} {}",
                context.repository_name, context.git_tag
            ),
            payload: serde_json::json!({
                "repository_name": &context.repository_name,
                "git_tag": &context.git_tag,
                "git_commit": &context.git_commit,
                "status": "queued",
            }),
        },
    )?;
    Ok(())
}

fn emit_build_run_started_event(
    storage: &StorageLayout,
    context: &BuildRunEventContext,
) -> io::Result<()> {
    let mode = if context.user_requested {
        "Manual"
    } else {
        "Automatic"
    };
    emit_runtime_event(
        storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_BUILD_RUN_STARTED),
            severity: String::from("info"),
            origin: String::from("runtime-bin"),
            user_requested: context.user_requested,
            repository_id: Some(context.repository_id),
            release_run_id: Some(context.release_run_id),
            build_run_id: Some(context.build_run_id),
            publish_run_id: None,
            summary: format!(
                "{mode} build started for {} {} ({})",
                context.repository_name, context.git_tag, context.target_name
            ),
            payload: serde_json::json!({
                "repository_name": &context.repository_name,
                "git_tag": &context.git_tag,
                "target_name": &context.target_name,
                "platform": &context.platform,
                "status": "running",
            }),
        },
    )?;
    Ok(())
}

fn emit_build_run_finished_event(
    storage: &StorageLayout,
    context: &BuildRunEventContext,
    record: &BuildRunRecord,
) -> io::Result<()> {
    let mode = if context.user_requested {
        "Manual"
    } else {
        "Automatic"
    };
    emit_runtime_event(
        storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_BUILD_RUN_FINISHED),
            severity: String::from(terminal_event_severity(&record.status)),
            origin: String::from("runtime-bin"),
            user_requested: context.user_requested,
            repository_id: Some(context.repository_id),
            release_run_id: Some(context.release_run_id),
            build_run_id: Some(context.build_run_id),
            publish_run_id: None,
            summary: format!(
                "{mode} build {} for {} {} ({})",
                record.status, context.repository_name, context.git_tag, context.target_name
            ),
            payload: serde_json::json!({
                "repository_name": &context.repository_name,
                "git_tag": &context.git_tag,
                "target_name": &context.target_name,
                "platform": &context.platform,
                "status": &record.status,
                "error_message": &record.error_message,
            }),
        },
    )?;
    Ok(())
}

fn emit_build_run_stage_updated_event(
    storage: &StorageLayout,
    context: &BuildRunEventContext,
    stage_key: &str,
    stage_label: &str,
    message: &str,
) -> io::Result<()> {
    let mode = if context.user_requested {
        "Manual"
    } else {
        "Automatic"
    };
    emit_runtime_event(
        storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_BUILD_RUN_STAGE_UPDATED),
            severity: String::from("info"),
            origin: String::from("runtime-bin"),
            user_requested: context.user_requested,
            repository_id: Some(context.repository_id),
            release_run_id: Some(context.release_run_id),
            build_run_id: Some(context.build_run_id),
            publish_run_id: None,
            summary: format!(
                "{mode} build stage updated for {} {} ({}): {}",
                context.repository_name, context.git_tag, context.target_name, stage_label
            ),
            payload: serde_json::json!({
                "repository_name": &context.repository_name,
                "git_tag": &context.git_tag,
                "target_name": &context.target_name,
                "platform": &context.platform,
                "stage_key": stage_key,
                "stage_label": stage_label,
                "status": "running",
                "message": message,
            }),
        },
    )?;
    Ok(())
}

fn emit_publish_run_started_event(
    storage: &StorageLayout,
    context: &PublishRunEventContext,
) -> io::Result<()> {
    let mode = if context.user_requested {
        "Manual"
    } else {
        "Automatic"
    };
    emit_runtime_event(
        storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_PUBLISH_RUN_STARTED),
            severity: String::from("info"),
            origin: String::from("runtime-bin"),
            user_requested: context.user_requested,
            repository_id: Some(context.repository_id),
            release_run_id: Some(context.release_run_id),
            build_run_id: Some(context.build_run_id),
            publish_run_id: Some(context.publish_run_id),
            summary: format!(
                "{mode} publish started for {} {} ({})",
                context.repository_name, context.git_tag, context.publish_target_name
            ),
            payload: serde_json::json!({
                "repository_name": &context.repository_name,
                "git_tag": &context.git_tag,
                "publish_target_id": context.publish_target_id,
                "publish_target_name": &context.publish_target_name,
                "artifact_name": &context.artifact_name,
                "status": "running",
            }),
        },
    )?;
    Ok(())
}

fn emit_publish_run_finished_event(
    storage: &StorageLayout,
    context: &PublishRunEventContext,
    record: &PublishRunRecord,
) -> io::Result<()> {
    let mode = if context.user_requested {
        "Manual"
    } else {
        "Automatic"
    };
    emit_runtime_event(
        storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_PUBLISH_RUN_FINISHED),
            severity: String::from(terminal_event_severity(&record.status)),
            origin: String::from("runtime-bin"),
            user_requested: context.user_requested,
            repository_id: Some(context.repository_id),
            release_run_id: Some(context.release_run_id),
            build_run_id: Some(context.build_run_id),
            publish_run_id: Some(context.publish_run_id),
            summary: format!(
                "{mode} publish {} for {} {} ({})",
                record.status,
                context.repository_name,
                context.git_tag,
                context.publish_target_name
            ),
            payload: serde_json::json!({
                "repository_name": &context.repository_name,
                "git_tag": &context.git_tag,
                "publish_target_id": context.publish_target_id,
                "publish_target_name": &context.publish_target_name,
                "artifact_name": &context.artifact_name,
                "status": &record.status,
                "destination_ref": &record.destination_ref,
                "error_message": &record.error_message,
            }),
        },
    )?;
    Ok(())
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
    let report = run_repository_poll_cycle(&coordinator, storage, None)?;

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
    let repository = coordinator.get_repository_checkout_record(command.repository_id)?;
    let context = ReleaseEventContext {
        release_run_id: record.id,
        repository_id: record.repository_id,
        repository_name: repository.name,
        git_tag: record.git_tag.clone(),
        git_commit: record.git_commit.clone(),
        user_requested: user_requested_from_trigger_source(&record.trigger_source),
    };
    if let Err(error) = emit_release_queued_event(storage, &context) {
        log_runtime_event_failure(EVENT_TOPIC_RELEASE_QUEUED, &error);
    }

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
    let git_auth = resolve_repository_git_auth(
        &coordinator,
        &repository_url,
        repository.credentials_id,
    )?;

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
    let mut publish_event_context = None;
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
        let event_context = publish_run_event_context(&coordinator, &publish_plan);
        publish_event_context = Some(event_context.clone());

        coordinator.start_publish_run(
            resolved.plan.publish_run_id,
            StartPublishRunInput::default(),
        )?;
        if let Err(error) = emit_publish_run_started_event(storage, &event_context) {
            log_runtime_event_failure(EVENT_TOPIC_PUBLISH_RUN_STARTED, &error);
        }

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

    if let Some(context) = publish_event_context.as_ref() {
        if let Err(error) = emit_publish_run_finished_event(storage, context, &record) {
            log_runtime_event_failure(EVENT_TOPIC_PUBLISH_RUN_FINISHED, &error);
        }
    }

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

fn resolve_claimed_publish_context(
    coordinator: &LocalCoordinator,
    payload: &[u8],
) -> io::Result<ResolvedPublishContext> {
    let job: PublishDispatchJob = serde_json::from_slice(payload)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    let plan = coordinator.get_publish_execution_plan(job.publish_run_id)?;

    Ok(ResolvedPublishContext { plan })
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

#[cfg(test)]
mod tests {
    use super::{
        package_build_output,
        build_execution_logs_archive_path, build_execution_report_path,
        recover_interrupted_build_attempts,
        run_automation_inspect_command, run_automation_poll_once_command,
        run_repository_poll_cycle,
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
        run_runtime_worker_iteration,
        failed_poll_attempt_log_path, normalize_repository_git_auth_config,
        record_failed_poll_attempt, runtime_stop_requested,
        select_queued_repository_tags,
        AutomationPollReport, BuildExecutionReport, BuildRunRecord,
        EVENT_TOPIC_POLL_AUTH_FAILED,
        RegistrationCheckoutReport, RuntimeLoopCadence,
        RepositoryPollSchedule,
        RegistrationSeedReport,
        PublishedOutputInspectionReport,
    };
    use rusqlite::{params, Connection};
    use runtime_core::{read_runtime_event_batch, shutdown_runtime};
    use runtime_config::{HostPlatform, RuntimeConfig, RuntimeDirectories};
    use runtime_git::GitTag;
    use runtime_manifests::ApplyReport as ManifestApplyReport;
    use runtime_store::{
        enqueue_runtime_control_request,
        ImportedRepositoryRegistrationReport, InterruptedBuildRecoveryRecord,
        LocalCoordinator, RuntimeControlRequest, RuntimeRecoveryReport,
        RECOVERY_INTERRUPTION_KIND_REQUESTED,
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
        PollingRepositoryRecord, PublishRunRecord, ReleaseRunRecord, StorageLayout,
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
            build_root_path: root.join("workspace").join("builds").join("build-run-1-attempt-1"),
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
                "apiVersion: handy.unity.publisher/v1alpha1\n",
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
        assert_eq!(repository.pending_release_count, 0);
        assert_eq!(repository.queued_build_runs, 0);
        assert_eq!(repository.running_build_runs, 0);
        assert_eq!(repository.queued_publish_runs, 0);
        assert_eq!(repository.running_publish_runs, 0);
        assert_eq!(repository.release_queue.len(), 0);

        let build_queue = snapshot
            .queue_messages
            .iter()
            .find(|queue| queue.queue_name == "build-runs")
            .expect("build queue snapshot should exist");
        assert_eq!(build_queue.ready_count, 0);
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
        assert_eq!(metadata["requested_via"], "hup-runtime");

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
        let workspace_path = root.join("runs").join("build-run-88");
        fs::create_dir_all(&artifact_root_path).expect("artifact root should create");
        fs::create_dir_all(workspace_path.join("source"))
            .expect("workspace root should create");
        fs::write(workspace_path.join("source").join("build.txt"), "workspace")
            .expect("workspace marker should write");
        fs::write(artifact_root_path.join("rebuilt.zip"), "artifact")
            .expect("artifact marker should write");
        let build_run_id = seed_succeeded_build_run_with_workspace(
            &connection,
            release_run_id,
            build_target_id,
            &artifact_root_path,
            &workspace_path,
            "2022.3.20f1",
            "host-native",
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

        assert!(!workspace_path.exists());
        assert!(!artifact_root_path.exists());

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
        assert_eq!(queue_message_count(&connection, "build-runs"), 1);
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
    fn runtime_worker_iteration_does_not_starve_builds_when_release_retry_later_exists() {
        let root = test_root("runtime-bin-worker-iteration-no-starvation");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_unity_repository_with_tags(
            &root.join("runtime-bin-worker-iteration-source"),
            "2021.3.33f1",
            &["v13.0.5", "v13.0.6"],
        );
        let script_path =
            create_fake_unity_script(&root, "worker-iteration-no-starvation", ScriptKind::Success);

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-worker-iteration-no-starvation",
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
        let publish_target_id =
            seed_publish_target(&connection, repository_id, "filesystem-release", "filesystem");
        seed_build_publish_binding(&connection, build_target_id, publish_target_id);
        drop(connection);

        let first_release: ReleaseRunRecord = serde_json::from_str(
            &run_manual_release_dispatch_command(
                &[
                    String::from("--repository-id"),
                    repository_id.to_string(),
                    String::from("--git-tag"),
                    String::from("v13.0.5"),
                ],
                &storage,
            )
            .expect("first manual release dispatch should succeed"),
        )
        .expect("first release dispatch output should decode");

        let _: ReleaseRunRecord = serde_json::from_str(
            &run_manual_release_dispatch_command(
                &[
                    String::from("--repository-id"),
                    repository_id.to_string(),
                    String::from("--git-tag"),
                    String::from("v13.0.6"),
                ],
                &storage,
            )
            .expect("second manual release dispatch should succeed"),
        )
        .expect("second release dispatch output should decode");

        assert!(run_release_planner_cycle(&storage)
            .expect("release planner should queue the first release build"));

        let mut poll_schedule = RepositoryPollSchedule::default();
        run_runtime_worker_iteration(&config, &storage, &mut poll_schedule)
            .expect("worker iteration should continue past retry-later release planning");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let completed_builds: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM build_runs WHERE release_run_id = ? AND status = 'succeeded'",
                [first_release.id],
                |row| row.get(0),
            )
            .expect("first release build status should load");
        assert_eq!(completed_builds, 1);
        assert_eq!(queue_message_count(&connection, "release-runs"), 1);
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

        let (selected, status, ok) = select_queued_repository_tags(&tags, Some("v0.9.0"));

        assert!(ok);
        assert_eq!(status, "");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "v1.2.0");
    }

    #[test]
    fn select_queued_repository_tags_returns_all_tags_when_history_exists_without_baseline() {
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

        let (selected, status, ok) = select_queued_repository_tags(&tags, None);

        assert!(ok);
        assert_eq!(status, "");
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0].name, "v1.0.0");
        assert_eq!(selected[1].name, "v1.1.0");
        assert_eq!(selected[2].name, "v1.2.0");
    }

    #[test]
    fn runtime_loop_cadence_keeps_worker_ticks_fast_while_heartbeat_waits_five_seconds() {
        let config = RuntimeConfig::from_root(test_root("runtime-bin-loop-cadence"));
        let cadence = RuntimeLoopCadence::from_config(&config);

        assert_eq!(cadence.worker_loop_interval(), Duration::from_secs(1));
        assert!(!cadence.should_emit_heartbeat(Duration::from_secs(1)));
        assert!(!cadence.should_emit_heartbeat(Duration::from_secs(4)));
        assert!(cadence.should_emit_heartbeat(Duration::from_secs(5)));

        if config.directories.data_dir.exists() {
            std::fs::remove_dir_all(&config.directories.data_dir)
                .expect("temporary runtime root should be removable");
        }
    }

    #[test]
    fn runtime_poll_schedule_checks_repository_immediately_on_first_tick() {
        let root = test_root("runtime-bin-poll-immediate-first-tick");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_unity_repository_with_tags(
            &root.join("runtime-bin-poll-immediate-first-tick-source"),
            "2021.3.33f1",
            &["v1.0.0"],
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-poll-immediate-first-tick",
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

        let coordinator = LocalCoordinator::new(&storage);
        let mut poll_schedule = RepositoryPollSchedule::default();

        let first_report = run_repository_poll_cycle(
            &coordinator,
            &storage,
            Some(&mut poll_schedule),
        )
        .expect("first polling cycle should run immediately");

        assert_eq!(first_report.repositories.len(), 1);
        let repository = &first_report.repositories[0];
        assert_eq!(repository.repository_id, repository_id);
        assert_eq!(repository.status, "unchanged");
        assert_eq!(repository.last_seen_tag_before.as_deref(), Some("v1.0.0"));
        assert_eq!(repository.last_seen_tag_after.as_deref(), Some("v1.0.0"));
        assert!(repository.queued_release_ids.is_empty());
        assert!(repository.discovered_tags.is_empty());

        let second_report = run_repository_poll_cycle(
            &coordinator,
            &storage,
            Some(&mut poll_schedule),
        )
        .expect("second polling cycle should respect the scheduled wait");

        assert!(second_report.repositories.is_empty());

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(
            load_repository_last_seen_tag(&connection, repository_id).as_deref(),
            Some("v1.0.0")
        );
        assert_eq!(
            release_tags_for_repository(&connection, repository_id),
            Vec::<String>::new()
        );
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn runtime_worker_iteration_honors_forced_repository_poll_requests() {
        let root = test_root("runtime-bin-force-poll-request");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_path = root.join("runtime-bin-force-poll-request-source");
        let repository_url = create_unity_repository_with_tags(
            &repository_path,
            "2021.3.33f1",
            &["v1.0.0"],
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-force-poll-request",
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

        let coordinator = LocalCoordinator::new(&storage);
        let mut poll_schedule = RepositoryPollSchedule::default();

        let first_report = run_repository_poll_cycle(
            &coordinator,
            &storage,
            Some(&mut poll_schedule),
        )
        .expect("first polling cycle should run immediately");
        assert_eq!(first_report.repositories.len(), 1);
        assert_eq!(first_report.repositories[0].status, "unchanged");

        let second_report = run_repository_poll_cycle(
            &coordinator,
            &storage,
            Some(&mut poll_schedule),
        )
        .expect("second polling cycle should respect the scheduled wait");
        assert!(second_report.repositories.is_empty());

        std::fs::write(repository_path.join("README.md"), "v1.1.0")
            .expect("repository file should write");
        run_git_test_command(&repository_path, &["add", "."]);
        run_git_test_command(
            &repository_path,
            &["commit", "-m", "queue instant-check release"],
        );
        run_git_test_command(&repository_path, &["tag", "v1.1.0"]);

        enqueue_runtime_control_request(
            &storage,
            &RuntimeControlRequest::ForceRepositoryPoll { repository_id },
        )
        .expect("forced repository poll should queue");

        let config = RuntimeConfig::from_root(&root);
        run_runtime_worker_iteration(&config, &storage, &mut poll_schedule)
            .expect("runtime worker iteration should honor the forced poll request");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(
            load_repository_last_seen_tag(&connection, repository_id).as_deref(),
            Some("v1.1.0")
        );
        assert_eq!(
            release_tags_for_repository(&connection, repository_id),
            vec![String::from("v1.1.0")]
        );
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn automation_poll_once_command_registers_latest_baseline_for_repository_without_process_history() {
        let root = test_root("runtime-bin-automation-poll-once-initial-latest-only");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_unity_repository_with_tags(
            &root.join("runtime-bin-automation-poll-once-initial-latest-only-source"),
            "2021.3.33f1",
            &["v1.0.0", "v1.1.0", "v1.2.0"],
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-automation-poll-once-initial-latest-only",
            &repository_url,
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        drop(connection);

        let output = run_automation_poll_once_command(&[], &storage)
            .expect("automation poll-once command should succeed");
        let report: AutomationPollReport =
            serde_json::from_str(&output).expect("poll output should decode");

        assert_eq!(report.repositories.len(), 1);
        let repository = &report.repositories[0];
        assert_eq!(repository.repository_id, repository_id);
        assert_eq!(repository.status, "already_seen");
        assert_eq!(repository.last_seen_tag_before, None);
        assert_eq!(repository.last_seen_tag_after.as_deref(), Some("v1.2.0"));
        assert_eq!(repository.queued_release_ids.len(), 0);
        assert_eq!(repository.discovered_tags.len(), 1);
        assert_eq!(repository.discovered_tags[0].name, "v1.2.0");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(
            load_repository_last_seen_tag(&connection, repository_id).as_deref(),
            Some("v1.2.0")
        );
        assert_eq!(
            release_tags_for_repository(&connection, repository_id),
            Vec::<String>::new()
        );
        assert_eq!(queue_message_count(&connection, "release-runs"), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn automation_poll_once_command_preserves_latest_baseline_without_process_history() {
        let root = test_root("runtime-bin-automation-poll-once-reset-baseline");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_unity_repository_with_tags(
            &root.join("runtime-bin-automation-poll-once-reset-baseline-source"),
            "2021.3.33f1",
            &["v1.0.0", "v1.1.0", "v1.2.0"],
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-automation-poll-once-reset-baseline",
            &repository_url,
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        connection
            .execute(
                "UPDATE repositories SET last_seen_tag = ? WHERE id = ?",
                params!["v1.2.0", repository_id],
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
        assert_eq!(repository.status, "unchanged");
        assert_eq!(repository.last_seen_tag_before.as_deref(), Some("v1.2.0"));
        assert_eq!(repository.last_seen_tag_after.as_deref(), Some("v1.2.0"));
        assert_eq!(repository.queued_release_ids.len(), 0);
        assert!(repository.discovered_tags.is_empty());

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        assert_eq!(
            load_repository_last_seen_tag(&connection, repository_id).as_deref(),
            Some("v1.2.0")
        );
        assert_eq!(
            release_tags_for_repository(&connection, repository_id),
            Vec::<String>::new()
        );
        assert_eq!(queue_message_count(&connection, "release-runs"), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn normalize_repository_git_auth_config_rewrites_legacy_github_placeholder_username() {
        let normalized = normalize_repository_git_auth_config(
            "https://github.com/indiegabo/revolutions.git",
            "git-http-basic",
            r#"{"username":"git","password":"solidarity"}"#,
        )
        .expect("github auth config should normalize");
        let parsed: serde_json::Value =
            serde_json::from_str(&normalized).expect("normalized config should decode");

        assert_eq!(parsed["username"], "indiegabo");
        assert_eq!(parsed["password"], "solidarity");
    }

    #[test]
    fn record_failed_poll_attempt_writes_repository_scoped_jsonl_log() {
        let root = test_root("runtime-bin-failed-poll-attempt-log");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        directories
            .ensure_exists()
            .expect("runtime directories should create");
        let repository = PollingRepositoryRecord {
            id: 41,
            name: String::from("Revolutions Main"),
            repo_url: String::from("https://github.com/indiegabo/revolutions.git"),
            credentials_id: Some(7),
            enabled: true,
            polling_interval_seconds: 300,
            last_seen_tag: Some(String::from("v1.0.0")),
            enabled_build_target_count: 2,
            has_release_history: true,
        };
        let error = std::io::Error::other("Authentication failed for GitHub");

        let written_path = record_failed_poll_attempt(
            &storage,
            &repository,
            "poll_remote",
            &error,
        )
        .expect("failed poll attempt log should persist");

        assert_eq!(written_path, failed_poll_attempt_log_path(&storage, &repository));
        let contents = std::fs::read_to_string(&written_path)
            .expect("failed poll attempt log should read");
        let line = contents
            .lines()
            .last()
            .expect("failed poll attempt log should contain one line");
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("failed poll attempt line should decode");

        assert_eq!(parsed["repository_id"], 41);
        assert_eq!(parsed["repository_name"], "Revolutions Main");
        assert_eq!(parsed["stage"], "poll_remote");
        assert_eq!(parsed["last_seen_tag"], "v1.0.0");
        assert!(parsed["error"]
            .as_str()
            .unwrap_or_default()
            .contains("Authentication failed"));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn run_repository_poll_cycle_stops_on_authentication_failure_and_emits_runtime_event() {
        let root = test_root("runtime-bin-poll-auth-failure");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let credentials_id = seed_credentials(
            &connection,
            "Revolutions/origin",
            "git-http-basic",
            &json!({
                "username": "indiegabo",
                "password": "keyring://github/revolutions-origin"
            })
            .to_string(),
        );
        let repository_id = seed_repository_with_url_and_credentials(
            &connection,
            "Revolutions",
            "https://github.com/indiegabo/revolutions.git",
            Some(credentials_id),
        );
        seed_build_target(&connection, repository_id, "windows-player", "windows");
        drop(connection);

        let coordinator = LocalCoordinator::new(&storage);
        let error = run_repository_poll_cycle(&coordinator, &storage, None)
            .expect_err("authentication failures should stop the poll cycle");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(error
            .to_string()
            .contains("fatal repository poll authentication failure"));
        assert!(error
            .to_string()
            .contains("host keyring error"));

        let repository = coordinator
            .list_polling_repositories()
            .expect("repository listing should load")
            .into_iter()
            .next()
            .expect("polling repository should exist");
        let failed_attempt_path = failed_poll_attempt_log_path(&storage, &repository);
        let failed_attempt_contents = fs::read_to_string(&failed_attempt_path)
            .expect("failed poll attempt log should exist");
        let failed_attempt: serde_json::Value = serde_json::from_str(
            failed_attempt_contents
                .lines()
                .last()
                .expect("failed poll attempt log should contain one record"),
        )
        .expect("failed poll attempt log should decode");
        assert_eq!(failed_attempt["stage"], "poll_remote");
        assert!(failed_attempt["error"]
            .as_str()
            .unwrap_or_default()
            .contains("host keyring error"));

        let runtime_events = read_runtime_event_batch(&storage.runtime_events_path, 0)
            .expect("runtime event stream should read");
        assert_eq!(runtime_events.events.len(), 1);
        let event = &runtime_events.events[0];
        assert_eq!(event.topic, EVENT_TOPIC_POLL_AUTH_FAILED);
        assert_eq!(event.repository_id, Some(repository_id));
        assert!(event.summary.contains("Revolutions"));
        assert_eq!(event.payload["stage"], "poll_remote");
        assert_eq!(event.payload["worker_action"], "stop");
        assert!(event.payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("host keyring error"));

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn automation_poll_once_command_queues_newer_tags_after_stale_baseline_without_process_history() {
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
        assert_eq!(
            release_tags_for_repository(&connection, repository_id),
            vec![String::from("v1.1.0"), String::from("v1.2.0")]
        );
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
        let archived_logs_path = build_execution_logs_archive_path(&workspace_path);
        let workspace_output_path = workspace_path
            .join("builds")
            .join(format!("build-run-{}", record.id))
            .join("outputs")
            .join("runtime-bin-build-run-next-success.v13.0.0.webgl-player");
        let report = load_build_execution_report(&workspace_path);
        assert_eq!(log_path, workspace_path.join("logs").join("03-unity-build-webgl.log"));
        assert!(log_path.is_file());
        assert!(!archived_logs_path.exists());
        let log_contents = fs::read_to_string(&log_path).expect("unity log should exist");
        assert!(log_contents.contains("build_method: Builder.PerformWebGL"));
        assert!(log_contents.contains("build_target: WebGL"));
        assert!(workspace_path.join("source").exists());
        assert!(workspace_path.join("logs").exists());
        assert!(workspace_output_path.is_dir());
        assert!(workspace_output_path.join("artifact.txt").is_file());
        assert_eq!(report.cleanup.status, "pending");
        assert_eq!(report.attempts.len(), 0);
        assert_eq!(report.retained_files.len(), 0);
        assert_eq!(report.publish_runs.len(), 1);

        let artifact_path = workspace_path
            .join("outputs")
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
        assert_eq!(stages.len(), 4);
        assert_eq!(
            stages
                .iter()
                .map(|stage| stage.step_key.as_str())
                .collect::<Vec<_>>(),
            vec![
                "validate-build-context",
                "unity-build",
                "package-artifact",
                "register-artifacts",
            ]
        );
        assert!(stages.iter().all(|stage| stage.status == "succeeded"));
        assert_eq!(stages[1].log_path, log_path.display().to_string());

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
        let expected_build_root = expected_workspace_root
            .join("builds")
            .join(format!("build-run-{}", record.id));
        let expected_artifact_root = expected_workspace_root.join("outputs");
        let expected_artifact_path = expected_artifact_root
            .join("runtime-bin-build-run-next-overrides.v13.1.0.webgl-player.zip");
        let expected_workspace_output_path = expected_build_root
            .join("outputs")
            .join("runtime-bin-build-run-next-overrides.v13.1.0.webgl-player");
        let expected_workspace_path = expected_workspace_root.display().to_string();
        let expected_workspace_dir_name = format!("release-run-{}", record.release_run_id);
        let expected_log_path_string = expected_log_path.display().to_string();
        let expected_artifact_root_string = expected_artifact_root.display().to_string();

        assert_eq!(record.status, "succeeded");
        assert!(expected_workspace_root.starts_with(workspace_root_override.join("runs")));
        assert_eq!(
            expected_log_path,
            expected_workspace_root.join("logs").join("03-unity-build-webgl.log")
        );
        assert_eq!(
            expected_workspace_root
                .file_name()
                .and_then(|value| value.to_str()),
            Some(expected_workspace_dir_name.as_str())
        );
        assert_eq!(record.workspace_path.as_deref(), Some(expected_workspace_path.as_str()));
        assert_eq!(record.log_path.as_deref(), Some(expected_log_path_string.as_str()));
        assert_eq!(
            record.artifact_root_path.as_deref(),
            Some(expected_artifact_root_string.as_str())
        );
        assert!(!expected_workspace_root.join("source").exists());
        assert!(!expected_workspace_root.join("logs").exists());
        assert!(!expected_workspace_output_path.exists());
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
            .join("02-validate-build-context.log");
        let checkout_log_path = workspace_path
            .join("logs")
            .join("01-checkout-repository.log");
        let unity_log_path = workspace_path.join("logs").join("03-unity-build-windows.log");
        let register_log_path = workspace_path
            .join("builds")
            .join(format!("build-run-{}", record.id))
            .join("logs")
            .join("register-artifacts.log");

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
        assert_eq!(stages.len(), 3);
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
                    String::from("unity-build"),
                    unity_log_path.display().to_string(),
                ),
                (
                    3,
                    String::from("register-artifacts"),
                    register_log_path.display().to_string(),
                ),
            ]
        );

        std::fs::remove_dir_all(root).expect("temporary runtime root should be removable");
    }

    #[test]
    fn build_run_next_command_numbers_platform_logs_across_sequential_builds() {
        let root = test_root("runtime-bin-build-run-next-sequential-platform-logs");
        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("runtime-bin-build-run-next-sequential-platform-logs-source"),
            "v13.1.2",
            "2021.3.33f1",
        );
        let script_path = create_fake_unity_script(
            &root,
            "run-next-sequential-platform-logs",
            ScriptKind::Success,
        );

        let connection = Connection::open(&storage.database_path).expect("connection should open");
        let repository_id = seed_repository_with_url(
            &connection,
            "runtime-bin-build-run-next-sequential-platform-logs",
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
        seed_host_native_build_target(
            &connection,
            repository_id,
            "linux-player",
            "linux",
            "Builder.PerformLinux",
            &script_path,
        );
        let release_run_id = seed_queued_release(
            &connection,
            repository_id,
            "v13.1.2",
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

        let first_output = run_build_run_next_command(&[], &config, &storage)
            .expect("first build run-next should succeed");
        let first_record: BuildRunRecord =
            serde_json::from_str(&first_output).expect("first build output should decode");

        let second_output = run_build_run_next_command(&[], &config, &storage)
            .expect("second build run-next should succeed");
        let second_record: BuildRunRecord =
            serde_json::from_str(&second_output).expect("second build output should decode");

        let workspace_path = PathBuf::from(
            first_record
                .workspace_path
                .clone()
                .expect("workspace path should persist"),
        );
        let first_log_path = PathBuf::from(
            first_record
                .log_path
                .clone()
                .expect("first log path should persist"),
        );
        let second_log_path = PathBuf::from(
            second_record
                .log_path
                .clone()
                .expect("second log path should persist"),
        );

        assert_eq!(first_record.workspace_path, second_record.workspace_path);
        assert_eq!(
            first_log_path,
            workspace_path.join("logs").join("03-unity-build-windows.log")
        );
        assert_eq!(
            second_log_path,
            workspace_path.join("logs").join("04-unity-build-linux.log")
        );

        let expected_log_names = vec![
            String::from("01-checkout-repository.log"),
            String::from("02-validate-build-context.log"),
            String::from("03-unity-build-windows.log"),
            String::from("04-unity-build-linux.log"),
        ];
        if workspace_path.join("logs").is_dir() {
            let mut log_names = fs::read_dir(workspace_path.join("logs"))
                .expect("process logs directory should exist")
                .collect::<Result<Vec<_>, _>>()
                .expect("process logs should load")
                .into_iter()
                .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
                .collect::<Vec<_>>();
            log_names.sort();

            assert_eq!(log_names, expected_log_names);
        } else {
            let workspace_name = workspace_path
                .file_name()
                .and_then(|value| value.to_str())
                .expect("workspace directory name should exist");
            let archived_names = archive_entry_names(&build_execution_logs_archive_path(&workspace_path));

            for expected_name in expected_log_names {
                assert!(archived_names.contains(&format!("{workspace_name}/logs/{expected_name}")));
            }
        }

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
            &format!("{workspace_name}/logs/03-unity-build-windows.log"),
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
        assert_eq!(
            workspace_path
                .file_name()
                .and_then(|value| value.to_str()),
            Some(format!("release-run-{}", record.release_run_id).as_str())
        );
        assert_eq!(log_path, workspace_path.join("logs").join("03-unity-build-windows.log"));
        assert!(!log_path.exists());
        assert!(!workspace_path.join("logs").exists());
        let report = load_build_execution_report(&workspace_path);
        assert_eq!(report.attempts.len(), 2);
        assert!(report
            .attempts
            .iter()
            .any(|attempt| attempt.is_final_workspace && !attempt.removed_after_cleanup));
        assert!(report
            .attempts
            .iter()
            .any(|attempt| !attempt.is_final_workspace && attempt.removed_after_cleanup));
        assert!(build_execution_logs_archive_path(&workspace_path).is_file());

        let state_path = root.join("run-next-retry-package-cache.state");
        let attempts = fs::read_to_string(&state_path).expect("retry state file should exist");
        assert_eq!(attempts.trim(), "2");

        let artifact_path = workspace_path
            .join("outputs")
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
        let lease_ttl = Duration::from_millis(150);
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

        std::thread::sleep(Duration::from_millis(320));

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
                    r#"{"requested_via":"hup-runtime"}"#,
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
            "handy-unity-publisher-runtime-bin-{label}-{}",
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