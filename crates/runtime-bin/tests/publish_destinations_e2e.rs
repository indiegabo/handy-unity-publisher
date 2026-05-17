//! End-to-end smoke coverage for publish destination execution through the runtime binary.

use runtime_config::{RUNTIME_ROOT_ENV, RuntimeDirectories};
use runtime_store::{
    CompleteBuildRunInput, CreateArtifactRecordInput,
    CreateRepositoryProjectBuildTargetInput,
    CreateRepositoryProjectInput,
    CreateRepositoryProjectPublishBindingInput,
    CreateRepositoryProjectPublishTargetInput,
    KIND_ITCH_API_KEY, LocalCoordinator, StartBuildRunInput, StorageLayout,
    UpsertCredentialRecordInput, open_connection,
};
use rusqlite::params;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const DEFAULT_HOST_NATIVE_RUNNER_TYPE: &str = "host-native";

#[test]
fn smoke_keeps_unbound_artifacts_in_runtime_output_when_project_has_no_destinations() {
    let root = test_root("publish-no-destinations");
    bootstrap_runtime_root(&root);

    let layout = storage_layout(&root);
    let coordinator = LocalCoordinator::new(&layout);
    let created = create_repository_project(
        &coordinator,
        "Zero Destinations",
        vec![build_target_input("Windows")],
        Vec::new(),
    );

    let build_run_id = seed_release_build_runs(
        &layout,
        created.repository_id,
        &["Windows"],
        &created.build_target_ids,
    )
        .remove("Windows")
        .expect("windows build run should exist");
    let artifact_root = root.join("artifacts").join("zero-destinations");
    let source_path = complete_build_with_artifact(
        &coordinator,
        build_run_id,
        &artifact_root,
        "windows/game.zip",
        b"zero-destination-artifact",
    );

    let runs = coordinator
        .plan_build_publish_runs(build_run_id)
        .expect("planning without destinations should succeed");
    assert!(runs.is_empty());

    let next_publish = run_runtime_command(
        &root,
        &[String::from("publishes"), String::from("run-next")],
        &[],
    );
    assert_command_success("publishes run-next without destinations", &next_publish);
    assert_eq!(String::from_utf8_lossy(&next_publish.stdout).trim(), "null");

    let inspect = run_runtime_json_command(
        &root,
        &[
            String::from("publishes"),
            String::from("inspect"),
            String::from("--build-run-id"),
            build_run_id.to_string(),
        ],
        &[],
    );
    assert_eq!(inspect["publish_runs"].as_array().map(Vec::len), Some(0));
    assert!(source_path.is_file());

    fs::remove_dir_all(root).expect("temporary smoke root should be removable");
}

#[test]
fn smoke_moves_artifacts_into_one_filesystem_destination() {
    let root = test_root("publish-filesystem-destination");
    bootstrap_runtime_root(&root);

    let destination_directory = root.join("published").join("windows-release");
    let layout = storage_layout(&root);
    let coordinator = LocalCoordinator::new(&layout);
    let created = create_repository_project(
        &coordinator,
        "Filesystem Destination",
        vec![build_target_input("Windows")],
        vec![CreateRepositoryProjectPublishTargetInput {
            name: String::from("Windows Move"),
            kind: String::from("filesystem"),
            enabled: true,
                config_json: String::from("{}"),
            credentials_id: None,
            bindings: vec![CreateRepositoryProjectPublishBindingInput {
                build_target_name: String::from("Windows"),
                enabled: true,
                options_json: serde_json::json!({
                    "operation": "move",
                    "directory_path": destination_directory.display().to_string(),
                })
                .to_string(),
            }],
        }],
    );

    let build_run_id = seed_release_build_runs(
        &layout,
        created.repository_id,
        &["Windows"],
        &created.build_target_ids,
    )
        .remove("Windows")
        .expect("windows build run should exist");
    let artifact_root = root.join("artifacts").join("filesystem-destination");
    let source_path = complete_build_with_artifact(
        &coordinator,
        build_run_id,
        &artifact_root,
        "windows/game.zip",
        b"filesystem-destination-artifact",
    );

    let publish_runs = coordinator
        .plan_build_publish_runs(build_run_id)
        .expect("filesystem publish planning should succeed");
    assert_eq!(publish_runs.len(), 1);
    coordinator
        .dispatch_publish_run(publish_runs[0].id)
        .expect("filesystem publish run should dispatch");

    let executed = run_runtime_json_command(
        &root,
        &[String::from("publishes"), String::from("run-next")],
        &[],
    );
    let destination_path = destination_directory.join("game.zip");
    assert_eq!(executed["status"], "succeeded");
    assert_eq!(
        executed["destination_ref"].as_str(),
        Some(destination_path.display().to_string().as_str())
    );

    let inspect = run_runtime_json_command(
        &root,
        &[
            String::from("publishes"),
            String::from("inspect"),
            String::from("--build-run-id"),
            build_run_id.to_string(),
        ],
        &[],
    );
    let diagnostics = inspect["publish_runs"]
        .as_array()
        .expect("publish diagnostics should be an array");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["publish_target_kind"], "filesystem");
    assert_eq!(diagnostics[0]["destination_exists"], true);
    assert_eq!(diagnostics[0]["destination_is_file"], true);

    let artifacts = coordinator
        .list_artifacts_by_build_run(build_run_id)
        .expect("artifacts should reload after filesystem publish");
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].active_location_kind, "filesystem_absolute");
    assert_eq!(
        artifacts[0].active_location_ref,
        destination_path.display().to_string()
    );
    assert!(!source_path.exists());
    assert!(destination_path.is_file());

    fs::remove_dir_all(root).expect("temporary smoke root should be removable");
}

#[test]
fn smoke_runs_mixed_itch_and_filesystem_destinations_only_for_bound_targets() {
    let root = test_root("publish-mixed-destinations");
    bootstrap_runtime_root(&root);

    let (fake_butler_path, fake_butler_log_path) = write_fake_butler(&root);
    let layout = storage_layout(&root);
    let coordinator = LocalCoordinator::new(&layout);
    let itch_credentials = coordinator
        .upsert_credential_record(UpsertCredentialRecordInput {
            credential_id: None,
            name: String::from("Itch Smoke"),
            kind: String::from(KIND_ITCH_API_KEY),
            config_json: serde_json::json!({
                "api_key": "itch-secret"
            })
            .to_string(),
        })
        .expect("itch smoke credential should persist");

    let windows_move_directory = root.join("published").join("windows");
    let created = create_repository_project(
        &coordinator,
        "Mixed Destinations",
        vec![build_target_input("Windows"), build_target_input("Linux")],
        vec![
            CreateRepositoryProjectPublishTargetInput {
                name: String::from("Itch Windows"),
                kind: String::from("itch"),
                enabled: true,
                config_json: serde_json::json!({
                    "account_name": "indiegabo",
                    "game_slug": "red-horizon",
                    "butler_path": fake_butler_path.display().to_string(),
                })
                .to_string(),
                credentials_id: Some(itch_credentials.id),
                bindings: vec![CreateRepositoryProjectPublishBindingInput {
                    build_target_name: String::from("Windows"),
                    enabled: true,
                    options_json: serde_json::json!({
                        "channel": "windows",
                        "userversion_template": "build-{{git_tag}}",
                    })
                    .to_string(),
                }],
            },
            CreateRepositoryProjectPublishTargetInput {
                name: String::from("Windows Move"),
                kind: String::from("filesystem"),
                enabled: true,
                    config_json: String::from("{}"),
                credentials_id: None,
                bindings: vec![CreateRepositoryProjectPublishBindingInput {
                    build_target_name: String::from("Windows"),
                    enabled: true,
                    options_json: serde_json::json!({
                        "operation": "move",
                        "directory_path": windows_move_directory.display().to_string(),
                    })
                    .to_string(),
                }],
            },
        ],
    );

    let mut build_runs = seed_release_build_runs(
        &layout,
        created.repository_id,
        &["Windows", "Linux"],
        &created.build_target_ids,
    );
    let windows_build_run_id = build_runs
        .remove("Windows")
        .expect("windows build run should exist");
    let linux_build_run_id = build_runs
        .remove("Linux")
        .expect("linux build run should exist");

    let windows_artifact_root = root.join("artifacts").join("mixed-windows");
    let linux_artifact_root = root.join("artifacts").join("mixed-linux");
    let windows_source_path = complete_build_with_artifact(
        &coordinator,
        windows_build_run_id,
        &windows_artifact_root,
        "windows/game.zip",
        b"mixed-windows-artifact",
    );
    let linux_source_path = complete_build_with_artifact(
        &coordinator,
        linux_build_run_id,
        &linux_artifact_root,
        "linux/game.tar.gz",
        b"mixed-linux-artifact",
    );

    let linux_publish_runs = coordinator
        .plan_build_publish_runs(linux_build_run_id)
        .expect("unbound linux build planning should succeed");
    assert!(linux_publish_runs.is_empty());

    let windows_publish_runs = coordinator
        .plan_build_publish_runs(windows_build_run_id)
        .expect("mixed publish planning should succeed");
    assert_eq!(windows_publish_runs.len(), 2);
    for run in &windows_publish_runs {
        coordinator
            .dispatch_publish_run(run.id)
            .expect("mixed publish run should dispatch");
    }

    let first_publish = run_runtime_json_command(
        &root,
        &[String::from("publishes"), String::from("run-next")],
        &[],
    );
    let second_publish = run_runtime_json_command(
        &root,
        &[String::from("publishes"), String::from("run-next")],
        &[],
    );
    assert_eq!(first_publish["status"], "succeeded");
    assert_eq!(second_publish["status"], "succeeded");

    let linux_inspect = run_runtime_json_command(
        &root,
        &[
            String::from("publishes"),
            String::from("inspect"),
            String::from("--build-run-id"),
            linux_build_run_id.to_string(),
        ],
        &[],
    );
    assert_eq!(linux_inspect["publish_runs"].as_array().map(Vec::len), Some(0));

    let windows_inspect = run_runtime_json_command(
        &root,
        &[
            String::from("publishes"),
            String::from("inspect"),
            String::from("--build-run-id"),
            windows_build_run_id.to_string(),
        ],
        &[],
    );
    let windows_diagnostics = windows_inspect["publish_runs"]
        .as_array()
        .expect("windows publish diagnostics should be an array");
    assert_eq!(windows_diagnostics.len(), 2);

    let itch_diagnostic = windows_diagnostics
        .iter()
        .find(|entry| entry["publish_target_kind"] == "itch")
        .expect("mixed publish diagnostics should include the itch upload");
    assert_eq!(itch_diagnostic["status"], "succeeded");
    assert_eq!(
        itch_diagnostic["destination_ref"].as_str(),
        Some("itch://indiegabo/red-horizon:windows@build-v3.0.0")
    );

    let filesystem_diagnostic = windows_diagnostics
        .iter()
        .find(|entry| entry["publish_target_kind"] == "filesystem")
        .expect("mixed publish diagnostics should include the filesystem move");
    let moved_path = windows_move_directory.join("game.zip");
    assert_eq!(filesystem_diagnostic["status"], "succeeded");
    assert_eq!(filesystem_diagnostic["destination_exists"], true);
    assert_eq!(
        filesystem_diagnostic["destination_ref"].as_str(),
        Some(moved_path.display().to_string().as_str())
    );

    let fake_butler_log = fs::read_to_string(&fake_butler_log_path)
        .expect("fake butler should record one invocation");
    assert!(fake_butler_log.contains("indiegabo/red-horizon:windows"));
    assert!(fake_butler_log.contains("--userversion build-v3.0.0"));

    let windows_artifacts = coordinator
        .list_artifacts_by_build_run(windows_build_run_id)
        .expect("windows artifacts should reload after mixed publish");
    assert_eq!(windows_artifacts.len(), 1);
    assert_eq!(windows_artifacts[0].active_location_kind, "filesystem_absolute");
    assert_eq!(
        windows_artifacts[0].active_location_ref,
        moved_path.display().to_string()
    );

    let linux_artifacts = coordinator
        .list_artifacts_by_build_run(linux_build_run_id)
        .expect("linux artifacts should remain unchanged");
    assert_eq!(linux_artifacts.len(), 1);
    assert_eq!(linux_artifacts[0].active_location_kind, "runtime_artifact");
    assert_eq!(linux_artifacts[0].active_location_ref, "linux/game.tar.gz");
    assert!(!windows_source_path.exists());
    assert!(moved_path.is_file());
    assert!(linux_source_path.is_file());

    fs::remove_dir_all(root).expect("temporary smoke root should be removable");
}

fn create_repository_project(
    coordinator: &LocalCoordinator,
    name: &str,
    build_targets: Vec<CreateRepositoryProjectBuildTargetInput>,
    publish_targets: Vec<CreateRepositoryProjectPublishTargetInput>,
) -> runtime_store::CreatedRepositoryProjectRecord {
    coordinator
        .create_repository_project(CreateRepositoryProjectInput {
            name: String::from(name),
            engine_kind: String::from("unity"),
            repo_url: format!("https://example.com/{}.git", name.replace(' ', "-").to_ascii_lowercase()),
            credentials: None,
            default_branch: Some(String::from("main")),
            artifacts_root_override: None,
            workspace_root_override: None,
            polling_interval_seconds: 300,
            enabled: true,
            build_targets,
            publish_targets,
        })
        .expect("repository project should persist")
}

fn build_target_input(name: &str) -> CreateRepositoryProjectBuildTargetInput {
    CreateRepositoryProjectBuildTargetInput {
        name: String::from(name),
        build_kind: String::from("player"),
        runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
        output_kind: Some(String::from("archive")),
        output_path_template: None,
        timeout_seconds: 3600,
        enabled: true,
        contract_json: serde_json::json!({
            "unity": {
                "targetPlatform": target_platform_for(name),
                "buildMethod": format!("Builder.Perform{}", name.replace(' ', "")),
            }
        })
        .to_string(),
        runner_config_json: String::from(
            r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
        ),
    }
}

fn target_platform_for(name: &str) -> &'static str {
    match name {
        "Linux" => "StandaloneLinux64",
        _ => "StandaloneWindows64",
    }
}

fn seed_release_build_runs(
    layout: &StorageLayout,
    repository_id: i64,
    build_target_names: &[&str],
    build_target_ids: &[i64],
) -> HashMap<String, i64> {
    let build_target_name_by_id = build_target_ids
        .iter()
        .copied()
        .zip(build_target_names.iter().map(|name| String::from(*name)))
        .collect::<HashMap<_, _>>();
    let mut connection = open_connection(&layout.database_path)
        .expect("database connection should open for smoke release seeding");
    let transaction = connection
        .transaction()
        .expect("smoke release transaction should open");

    transaction
        .execute(
            "
            INSERT INTO release_runs (
                repository_id,
                git_tag,
                git_commit,
                trigger_source,
                source_metadata_json,
                status
            ) VALUES (?, ?, ?, ?, ?, ?)
            ",
            params![
                repository_id,
                "v3.0.0",
                "deadbeefcafebabe",
                "manual",
                "{}",
                "queued",
            ],
        )
        .expect("smoke release row should insert");
    let release_run_id = transaction.last_insert_rowid();

    let mut build_runs = HashMap::new();
    for build_target_id in build_target_ids {
        transaction
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    engine_version,
                    image_ref,
                    status
                ) VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "2022.3.20f1",
                    DEFAULT_HOST_NATIVE_RUNNER_TYPE,
                    "queued",
                ],
            )
            .expect("smoke build run should insert");
        let build_run_id = transaction.last_insert_rowid();
        let target_name = build_target_name_by_id
            .get(build_target_id)
            .cloned()
            .expect("smoke build target should map back to its seeded name");
        build_runs.insert(target_name, build_run_id);
    }

    transaction
        .commit()
        .expect("smoke release transaction should commit");

    build_runs
}

fn complete_build_with_artifact(
    coordinator: &LocalCoordinator,
    build_run_id: i64,
    artifact_root: &Path,
    artifact_relative_path: &str,
    contents: &[u8],
) -> PathBuf {
    let workspace_path = artifact_root
        .parent()
        .unwrap_or(artifact_root)
        .join("workspace");
    let log_path = artifact_root.join("build.log");
    let source_path = artifact_root.join(artifact_relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));

    if let Some(parent) = source_path.parent() {
        fs::create_dir_all(parent).expect("artifact parent directory should create");
    }
    if let Some(parent) = workspace_path.parent() {
        fs::create_dir_all(parent).expect("workspace parent directory should create");
    }
    fs::write(&source_path, contents).expect("artifact source file should persist");

    coordinator
        .start_build_run(
            build_run_id,
            StartBuildRunInput {
                workspace_path: workspace_path.display().to_string(),
                log_path: log_path.display().to_string(),
                artifact_root_path: artifact_root.display().to_string(),
            },
        )
        .expect("build run should start for smoke setup");
    coordinator
        .replace_build_artifacts(
            build_run_id,
            vec![CreateArtifactRecordInput {
                name: Path::new(artifact_relative_path)
                    .file_name()
                    .expect("artifact relative path should include a file name")
                    .to_string_lossy()
                    .into_owned(),
                kind: String::from("archive"),
                path: String::from(artifact_relative_path),
                size_bytes: Some(contents.len() as i64),
                checksum_sha256: None,
            }],
        )
        .expect("artifact metadata should persist");
    coordinator
        .complete_build_run(
            build_run_id,
            CompleteBuildRunInput {
                workspace_path: workspace_path.display().to_string(),
                log_path: log_path.display().to_string(),
                artifact_root_path: artifact_root.display().to_string(),
            },
        )
        .expect("build run should complete for smoke setup");

    source_path
}

fn storage_layout(root: &Path) -> StorageLayout {
    let directories = RuntimeDirectories::from_root(root);
    StorageLayout::from_directories(&directories)
}

fn bootstrap_runtime_root(root: &Path) {
    let output = run_runtime_command(root, &[String::from("bootstrap")], &[]);
    assert_command_success("runtime bootstrap", &output);
}

fn run_runtime_json_command(
    root: &Path,
    arguments: &[String],
    extra_env: &[(&str, &str)],
) -> Value {
    let output = run_runtime_command(root, arguments, extra_env);
    assert_command_success("runtime json command", &output);
    serde_json::from_slice(&output.stdout).expect("runtime command should emit json")
}

fn run_runtime_command(
    root: &Path,
    arguments: &[String],
    extra_env: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(runtime_bin_path());
    command.args(arguments.iter().map(String::as_str));
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

fn assert_command_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn write_fake_butler(root: &Path) -> (PathBuf, PathBuf) {
    let log_path = root.join("fake-butler.log");

    #[cfg(windows)]
    {
        let script_path = root.join("fake-butler.cmd");
        fs::write(
            &script_path,
            format!(
                "@echo off\r\nsetlocal\r\necho %* > \"{}\"\r\nif /I not \"%BUTLER_API_KEY%\"==\"itch-secret\" exit /b 87\r\nexit /b 0\r\n",
                log_path.display()
            ),
        )
        .expect("fake butler script should persist");

        (script_path, log_path)
    }

    #[cfg(not(windows))]
    {
        let script_path = root.join("fake-butler.sh");
        fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"{}\"\n[ \"$BUTLER_API_KEY\" = \"itch-secret\" ]\n",
                log_path.display()
            ),
        )
        .expect("fake butler script should persist");
        let mut permissions = fs::metadata(&script_path)
            .expect("fake butler metadata should load")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions)
            .expect("fake butler permissions should update");

        (script_path, log_path)
    }
}

fn test_root(name: &str) -> PathBuf {
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "handy-games-publisher-publish-smoke-{name}-{}-{unique_suffix}",
        process::id()
    ));
    if root.exists() {
        fs::remove_dir_all(&root).expect("stale temp directory should be removable");
    }
    root
}