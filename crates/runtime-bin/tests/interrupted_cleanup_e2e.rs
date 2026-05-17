//! End-to-end coverage for interrupted build recovery through the runtime binary.

use rusqlite::{params, Connection};
use runtime_config::{
    MAX_HEARTBEATS_ENV, RUNTIME_ROOT_ENV, RuntimeDirectories,
    SUPERVISION_MAX_RESTARTS_ENV,
};
use runtime_store::{StorageLayout, open_connection};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::ZipArchive;

const BUILD_RUN_ID: i64 = 41;
const RELEASE_RUN_ID: i64 = 11;
const REPOSITORY_ID: i64 = 7;
const BUILD_TARGET_ID: i64 = 13;

#[test]
fn requested_shutdown_serve_restart_persists_interrupted_build_retention_report() {
    run_interrupted_recovery_case(
        "requested-shutdown-serve",
        true,
        RecoveryCommand::Serve,
        "requested_shutdown",
        "requested_interruption",
        "build attempt interrupted after a requested runtime shutdown",
    );
}

#[test]
fn unexpected_serve_restart_persists_system_interruption_retention_report() {
    run_interrupted_recovery_case(
        "system-interruption-serve",
        false,
        RecoveryCommand::Serve,
        "system_interruption",
        "system_interruption",
        "build attempt interrupted after an unexpected runtime interruption",
    );
}

#[test]
fn requested_shutdown_supervise_restart_persists_interrupted_build_retention_report() {
    run_interrupted_recovery_case(
        "requested-shutdown-supervise",
        true,
        RecoveryCommand::Supervise,
        "requested_shutdown",
        "requested_interruption",
        "build attempt interrupted after a requested runtime shutdown",
    );
}

#[test]
fn unexpected_supervise_restart_persists_system_interruption_retention_report() {
    run_interrupted_recovery_case(
        "system-interruption-supervise",
        false,
        RecoveryCommand::Supervise,
        "system_interruption",
        "system_interruption",
        "build attempt interrupted after an unexpected runtime interruption",
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryCommand {
    Serve,
    Supervise,
}

impl RecoveryCommand {
    fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::Serve => &["serve"],
            Self::Supervise => &["supervise"],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Serve => "runtime serve recovery",
            Self::Supervise => "runtime supervise recovery",
        }
    }

    fn persists_supervisor_snapshot(self) -> bool {
        matches!(self, Self::Supervise)
    }
}

fn run_interrupted_recovery_case(
    case_name: &str,
    requested_shutdown: bool,
    recovery_command: RecoveryCommand,
    expected_kind: &str,
    expected_trigger: &str,
    expected_message: &str,
) {
    let root = test_root(case_name);

    assert_command_success(
        "runtime bootstrap",
        run_runtime_command(&root, &["bootstrap"], &[]),
    );

    let fixture = seed_interrupted_build_fixture(&root, case_name);

    if requested_shutdown {
        assert_command_success(
            "runtime shutdown",
            run_runtime_command(&root, &["shutdown"], &[]),
        );
    }

    let mut runtime_env = vec![(MAX_HEARTBEATS_ENV, "0")];
    if recovery_command.persists_supervisor_snapshot() {
        runtime_env.push((SUPERVISION_MAX_RESTARTS_ENV, "0"));
    }

    assert_command_success(
        recovery_command.label(),
        run_runtime_command(&root, recovery_command.arguments(), &runtime_env),
    );

    if recovery_command.persists_supervisor_snapshot() {
        let supervisor_snapshot = load_json(&fixture.storage.supervisor_state_path);
        assert_eq!(
            supervisor_snapshot
                .pointer("/status")
                .and_then(Value::as_str),
            Some("completed")
        );
        assert_eq!(
            supervisor_snapshot
                .pointer("/attempt")
                .and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            supervisor_snapshot
                .pointer("/restart_count")
                .and_then(Value::as_u64),
            Some(0)
        );
        assert_eq!(
            supervisor_snapshot
                .pointer("/last_exit_code")
                .and_then(Value::as_i64),
            Some(0)
        );
        assert_eq!(
            supervisor_snapshot
                .pointer("/active_child_process_id")
                .and_then(Value::as_u64),
            None
        );
        assert!(
            supervisor_snapshot
                .pointer("/message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("completed cleanly"))
        );
    }

    let report = load_json(&fixture.report_path);
    assert_eq!(report.pointer("/schema_version").and_then(Value::as_u64), Some(2));
    assert_eq!(
        report
            .pointer("/build_run/status")
            .and_then(Value::as_str),
        Some("queued")
    );
    assert_eq!(
        report
            .pointer("/cleanup/status")
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        report
            .pointer("/cleanup/trigger")
            .and_then(Value::as_str),
        Some(expected_trigger)
    );
    assert_eq!(
        report
            .pointer("/cleanup/removed_attempt_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report
            .pointer("/cleanup/workspace_path")
            .and_then(Value::as_str),
        Some(fixture.workspace_path.display().to_string().as_str())
    );
    assert_eq!(
        report
            .pointer("/interruption/kind")
            .and_then(Value::as_str),
        Some(expected_kind)
    );
    assert_eq!(
        report
            .pointer("/interruption/message")
            .and_then(Value::as_str),
        Some(expected_message)
    );
    assert_eq!(
        report
            .pointer("/stages/0/status")
            .and_then(Value::as_str),
        Some("failed")
    );
    assert_eq!(
        report
            .pointer("/stages/0/error_message")
            .and_then(Value::as_str),
        Some(expected_message)
    );
    assert_attempt_record(&report, &fixture.workspace_path, true, false);
    assert_attempt_record(&report, &fixture.prior_attempt_path, false, true);

    assert!(fixture.archive_path.is_file());
    assert_archive_contains_entries(
        &fixture.archive_path,
        &[
            format!(
                "{}/logs/02-checkout-repository.log",
                fixture.workspace_path.file_name().unwrap().to_string_lossy()
            ),
            format!(
                "{}/logs/01-validate-build-context.log",
                fixture.prior_attempt_path.file_name().unwrap().to_string_lossy()
            ),
        ],
    );

    assert!(!fixture.prior_attempt_path.exists());
    assert!(!fixture.workspace_path.join("logs").exists());
    assert!(!fixture.workspace_path.join("source").exists());
    assert_eq!(list_directory_names(&fixture.workspace_path), vec![String::from("retained")]);

    let connection = open_connection(&fixture.storage.database_path)
        .expect("database should reopen after runtime recovery");
    let build_row: (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = connection
        .query_row(
            "
            SELECT status,
                   workspace_path,
                   log_path,
                   artifact_root_path,
                   current_stage_key,
                   current_stage_label,
                   current_stage_status,
                   last_progress_message
            FROM build_runs
            WHERE id = ?
            ",
            [BUILD_RUN_ID],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .expect("recovered build run should remain queryable");
    assert_eq!(build_row.0, "queued");
    assert!(build_row.1.is_none());
    assert!(build_row.2.is_none());
    assert!(build_row.3.is_none());
    assert!(build_row.4.is_none());
    assert!(build_row.5.is_none());
    assert!(build_row.6.is_none());
    assert!(build_row.7.is_none());

    let stage_row: (String, Option<String>, Option<String>) = connection
        .query_row(
            "
            SELECT status, last_message, error_message
            FROM build_run_steps
            WHERE build_run_id = ?
              AND step_key = ?
            ",
            params![BUILD_RUN_ID, "checkout-repository"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("recovered build stage should remain queryable");
    assert_eq!(stage_row.0, "failed");
    assert_eq!(stage_row.1.as_deref(), Some(expected_message));
    assert_eq!(stage_row.2.as_deref(), Some(expected_message));

    drop(connection);

    fs::remove_dir_all(root).expect("temporary runtime root should be removable");
}

struct InterruptedBuildFixture {
    storage: StorageLayout,
    workspace_path: PathBuf,
    prior_attempt_path: PathBuf,
    report_path: PathBuf,
    archive_path: PathBuf,
}

fn seed_interrupted_build_fixture(root: &Path, case_name: &str) -> InterruptedBuildFixture {
    let directories = RuntimeDirectories::from_root(root.to_path_buf());
    let storage = StorageLayout::from_directories(&directories);
    let workspace_path = directories
        .runs_dir
        .join(format!("build-run-{BUILD_RUN_ID}-attempt-101-1"));
    let prior_attempt_path = directories
        .runs_dir
        .join(format!("build-run-{BUILD_RUN_ID}-attempt-100-1"));
    let workspace_log_path = workspace_path.join("logs").join("02-checkout-repository.log");
    let prior_log_path = prior_attempt_path
        .join("logs")
        .join("01-validate-build-context.log");

    fs::create_dir_all(workspace_path.join("source"))
        .expect("interrupted workspace source directory should create");
    fs::create_dir_all(workspace_path.join("logs"))
        .expect("interrupted workspace logs directory should create");
    fs::create_dir_all(prior_attempt_path.join("source"))
        .expect("prior attempt source directory should create");
    fs::create_dir_all(prior_attempt_path.join("logs"))
        .expect("prior attempt logs directory should create");
    fs::write(
        &workspace_log_path,
        format!("checking out repository for {case_name}\n"),
    )
    .expect("interrupted workspace log should write");
    fs::write(
        &prior_log_path,
        format!("validated build context for {case_name}\n"),
    )
    .expect("prior attempt log should write");

    let connection = Connection::open(&storage.database_path)
        .expect("database should open for interrupted build fixture");
    connection
        .execute(
            "
            INSERT INTO repositories (
                id,
                name,
                source_mode,
                workspace_strategy,
                repo_url,
                polling_interval_seconds,
                enabled
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                REPOSITORY_ID,
                format!("runtime-e2e-{case_name}"),
                "managed_repository",
                "managed_checkout",
                format!("https://example.com/{case_name}.git"),
                300,
                1,
            ],
        )
        .expect("repository fixture should insert");
    connection
        .execute(
            "
            INSERT INTO build_targets (
                id,
                repository_id,
                name,
                build_kind,
                runner_type,
                contract_json,
                config_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                BUILD_TARGET_ID,
                REPOSITORY_ID,
                "windows-player",
                "player",
                "host-native",
                r#"{"unity":{"targetPlatform":"StandaloneWindows64","buildMethod":"Builder.PerformWindows"}}"#,
                "{}",
            ],
        )
        .expect("build target fixture should insert");
    connection
        .execute(
            "
            INSERT INTO release_runs (
                id,
                repository_id,
                git_tag,
                git_commit,
                trigger_source,
                source_metadata_json,
                engine_version,
                status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                RELEASE_RUN_ID,
                REPOSITORY_ID,
                "v16.0.0",
                "deadbeef",
                "manual",
                "{}",
                "2021.3.33f1",
                "running",
            ],
        )
        .expect("release run fixture should insert");
    connection
        .execute(
            "
            INSERT INTO build_runs (
                id,
                release_run_id,
                build_target_id,
                engine_version,
                image_ref,
                status,
                workspace_path,
                log_path,
                artifact_root_path,
                current_stage_key,
                current_stage_label,
                current_stage_status,
                heartbeat_at,
                last_progress_message,
                started_at,
                error_message
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?, CURRENT_TIMESTAMP, ?)
            ",
            params![
                BUILD_RUN_ID,
                RELEASE_RUN_ID,
                BUILD_TARGET_ID,
                "2021.3.33f1",
                "host-native",
                "running",
                workspace_path.display().to_string(),
                workspace_log_path.display().to_string(),
                workspace_path.join("artifacts").display().to_string(),
                "checkout-repository",
                "Checkout Repository",
                "running",
                "cloning repository",
                "interrupted mid-build",
            ],
        )
        .expect("running build run fixture should insert");
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
                updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ",
            params![
                BUILD_RUN_ID,
                2,
                "checkout-repository",
                "Checkout Repository",
                "running",
                workspace_log_path.display().to_string(),
                "cloning repository",
            ],
        )
        .expect("running build stage fixture should insert");
    drop(connection);

    InterruptedBuildFixture {
        report_path: workspace_path.join("retained").join("execution-report.json"),
        archive_path: workspace_path.join("retained").join("execution-logs.zip"),
        storage,
        workspace_path,
        prior_attempt_path,
    }
}

fn run_runtime_command(root: &Path, arguments: &[&str], extra_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(runtime_bin_path());
    command.args(arguments);
    command.env(RUNTIME_ROOT_ENV, root);
    for (key, value) in extra_env {
        command.env(key, value);
    }

    command
        .output()
        .expect("runtime command should execute for e2e validation")
}

fn runtime_bin_path() -> &'static str {
    env!("CARGO_BIN_EXE_hgp-runtime")
}

fn assert_command_success(label: &str, output: Output) {
    assert!(
        output.status.success(),
        "{label} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn load_json(path: &Path) -> Value {
    let contents = fs::read(path).expect("json artifact should be readable");
    serde_json::from_slice(&contents).expect("json artifact should decode")
}

fn assert_attempt_record(
    report: &Value,
    workspace_path: &Path,
    expected_final: bool,
    expected_removed_after_cleanup: bool,
) {
    let attempts = report["attempts"]
        .as_array()
        .expect("execution report should include attempt snapshots");
    let workspace_path = workspace_path.display().to_string();
    let attempt = attempts
        .iter()
        .find(|attempt| {
            attempt["workspace_path"].as_str() == Some(workspace_path.as_str())
        })
        .expect("expected attempt snapshot should exist");

    assert_eq!(attempt["is_final_workspace"].as_bool(), Some(expected_final));
    assert_eq!(
        attempt["removed_after_cleanup"].as_bool(),
        Some(expected_removed_after_cleanup)
    );
}

fn assert_archive_contains_entries(archive_path: &Path, expected_entries: &[String]) {
    let archive_file = fs::File::open(archive_path).expect("log archive should open");
    let mut archive = ZipArchive::new(archive_file).expect("log archive should decode");
    let names = (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .expect("zip entry should load")
                .name()
                .to_owned()
        })
        .collect::<Vec<_>>();

    for expected_entry in expected_entries {
        assert!(
            names.iter().any(|name| name == expected_entry),
            "zip archive did not contain expected entry {expected_entry:?}; found {:?}",
            names,
        );
    }
}

fn list_directory_names(path: &Path) -> Vec<String> {
    let mut names = fs::read_dir(path)
        .expect("directory should remain readable after cleanup")
        .map(|entry| {
            entry
                .expect("directory entry should load")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn test_root(name: &str) -> PathBuf {
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "handy-games-publisher-runtime-bin-e2e-{name}-{}-{unique_suffix}",
        process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).expect("stale temp directory should be removable");
    }
    root
}