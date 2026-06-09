//! Resolves publish destinations and executes filesystem and Itch-backed
//! publication for completed build outputs.

#![forbid(unsafe_code)]

use runtime_contracts::ProcessPriority;
use serde::Deserialize;
use std::fs;
use std::io;
use std::io::ErrorKind;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

const KIND_ITCH_API_KEY: &str = "itch-api-key";
const BUTLER_API_KEY_ENV: &str = "BUTLER_API_KEY";
const HGP_BUTLER_PATH_ENV: &str = "HGP_BUTLER_PATH";
const PUBLISH_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Lists the publish backends enabled by the local runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishBackend {
    Filesystem,
    Itch,
}

impl PublishBackend {
    /// Returns the operator-facing label for the selected publish backend.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::Itch => "itch",
        }
    }
}

/// Describes one claimed publish run with the resolved metadata required by a publisher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub publish_run_id: i64,
    pub release_run_id: i64,
    pub repository_id: i64,
    pub repository_name: String,
    pub git_tag: String,
    pub process_priority: ProcessPriority,
    pub build_run_id: i64,
    pub publish_target_id: i64,
    pub publish_target_name: String,
    pub publish_target_kind: String,
    pub publish_target_config_json: String,
    pub publish_target_credentials_kind: Option<String>,
    pub publish_target_credentials_config_json: Option<String>,
    pub execution_contract_json: String,
    pub artifact_id: i64,
    pub artifact_name: String,
    pub artifact_kind: String,
    pub artifact_path: String,
    pub artifact_active_location_kind: String,
    pub artifact_active_location_ref: String,
    pub artifact_root_path: String,
    pub source_path: String,
    pub status: String,
}

/// Captures the persisted destination returned by a successful publish execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub destination_ref: String,
    pub artifact_active_location_kind: Option<String>,
    pub artifact_active_location_ref: Option<String>,
}

/// Executes one publish plan against the configured backend.
pub trait Processor {
    fn process(&self, plan: &ExecutionPlan) -> io::Result<ExecutionResult>;

    fn process_with_controller(
        &self,
        plan: &ExecutionPlan,
        controller: &mut dyn ExecutionController,
    ) -> io::Result<ExecutionResult> {
        let _ = controller;
        self.process(plan)
    }
}

/// Allows the caller to interrupt long-running publish transports.
pub trait ExecutionController {
    fn check_cancellation(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopExecutionController;

impl ExecutionController for NoopExecutionController {}

/// Resolves the final destination path that one publish backend will write for the given plan.
pub fn resolve_destination_path(plan: &ExecutionPlan) -> io::Result<PathBuf> {
    match parse_publish_backend(&plan.publish_target_kind)? {
        PublishBackend::Filesystem => resolve_filesystem_destination_path(plan),
        PublishBackend::Itch => Err(io::Error::new(
            ErrorKind::InvalidInput,
            "itch destinations do not resolve to a local filesystem path",
        )),
    }
}

/// Selects the concrete publisher implementation for one publish run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecutionProcessor;

impl ExecutionProcessor {
    /// Creates the default publish execution processor.
    pub const fn new() -> Self {
        Self
    }
}

impl Processor for ExecutionProcessor {
    fn process(&self, plan: &ExecutionPlan) -> io::Result<ExecutionResult> {
        let mut controller = NoopExecutionController;
        self.process_with_controller(plan, &mut controller)
    }

    fn process_with_controller(
        &self,
        plan: &ExecutionPlan,
        controller: &mut dyn ExecutionController,
    ) -> io::Result<ExecutionResult> {
        match parse_publish_backend(&plan.publish_target_kind)? {
            PublishBackend::Filesystem => {
                controller.check_cancellation()?;
                publish_to_filesystem(plan)
            }
            PublishBackend::Itch => publish_to_itch(plan, controller),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
struct PublishExecutionContractSnapshot {
    #[serde(default)]
    binding_options_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
struct FilesystemBindingOptions {
    #[serde(default)]
    operation: String,
    #[serde(default)]
    directory_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
struct ItchTargetConfig {
    #[serde(default)]
    account_name: String,
    #[serde(default)]
    game_slug: String,
    #[serde(default)]
    butler_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
struct ItchBindingOptions {
    #[serde(default)]
    channel: String,
    #[serde(default)]
    userversion_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ItchCredentialConfig {
    api_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ItchPublishRequest {
    executable: String,
    channel_ref: String,
    userversion: String,
    api_key: String,
    destination_ref: String,
}

fn parse_publish_backend(kind: &str) -> io::Result<PublishBackend> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "filesystem" => Ok(PublishBackend::Filesystem),
        "itch" => Ok(PublishBackend::Itch),
        other => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsupported publish target kind {other:?}"),
        )),
    }
}

fn publish_to_filesystem(plan: &ExecutionPlan) -> io::Result<ExecutionResult> {
    let source_path = require_regular_source_path(&plan.source_path)?;
    let destination_path = resolve_destination_path(plan)?;
    move_regular_file(&source_path, &destination_path)?;

    Ok(ExecutionResult {
        destination_ref: destination_path.display().to_string(),
        artifact_active_location_kind: Some(String::from("filesystem_absolute")),
        artifact_active_location_ref: Some(destination_path.display().to_string()),
    })
}

fn publish_to_itch(
    plan: &ExecutionPlan,
    controller: &mut dyn ExecutionController,
) -> io::Result<ExecutionResult> {
    let request = build_itch_publish_request(plan)?;
    let source_path = require_existing_source_path(&plan.source_path)?;
    let output = run_itch_publish_command(
        &request,
        &source_path,
        plan.process_priority,
        controller,
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = sanitize_itch_command_output(
            if stderr.trim().is_empty() {
                stdout.as_ref()
            } else {
                stderr.as_ref()
            },
            &request.api_key,
        );

        return Err(io::Error::other(format!(
            "itch publish command failed for {}: {}",
            request.channel_ref,
            if message.is_empty() {
                format!("process exited with status {}", output.status)
            } else {
                message
            }
        )));
    }

    Ok(ExecutionResult {
        destination_ref: request.destination_ref,
        artifact_active_location_kind: None,
        artifact_active_location_ref: None,
    })
}

fn run_itch_publish_command(
    request: &ItchPublishRequest,
    source_path: &Path,
    process_priority: ProcessPriority,
    controller: &mut dyn ExecutionController,
) -> io::Result<std::process::Output> {
    controller.check_cancellation()?;

    let mut command = Command::new(&request.executable);
    command
        .arg("push")
        .arg(source_path)
        .arg(&request.channel_ref)
        .arg("--userversion")
        .arg(&request.userversion)
        .env(BUTLER_API_KEY_ENV, &request.api_key)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(process_priority.creation_flags());

    let child = command.spawn().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to launch Itch transport {:?}: {error}",
                request.executable
            ),
        )
    })?;

    wait_for_publish_process_output(child, controller, "itch publish transport")
}

fn wait_for_publish_process_output(
    mut child: Child,
    controller: &mut dyn ExecutionController,
    command_label: &str,
) -> io::Result<std::process::Output> {
    loop {
        if let Some(_status) = child.try_wait()? {
            return child.wait_with_output();
        }

        if let Err(error) = controller.check_cancellation() {
            terminate_child_process(&mut child, command_label)?;
            let _ = child.wait();
            return Err(error);
        }

        thread::sleep(PUBLISH_PROCESS_POLL_INTERVAL);
    }
}

fn terminate_child_process(child: &mut Child, command_label: &str) -> io::Result<()> {
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
            "terminate {command_label}: {error}"
        ))),
    }
}

fn resolve_filesystem_destination_path(plan: &ExecutionPlan) -> io::Result<PathBuf> {
    let binding_options = require_filesystem_move_binding_options(plan)?;
    resolve_filesystem_move_destination_path(plan, &binding_options)
}

fn resolve_filesystem_move_destination_path(
    plan: &ExecutionPlan,
    binding_options: &FilesystemBindingOptions,
) -> io::Result<PathBuf> {
    let directory_path = PathBuf::from(binding_options.directory_path.trim());
    if binding_options.directory_path.trim().is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "filesystem move directory_path must not be empty",
        ));
    }
    if !directory_path.is_absolute() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "filesystem move directory_path must be absolute",
        ));
    }

    let source_file_name = Path::new(plan.source_path.trim())
        .file_name()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::InvalidInput,
                "publish source path must resolve to a file name",
            )
        })?;

    Ok(directory_path.join(source_file_name))
}

fn build_itch_publish_request(plan: &ExecutionPlan) -> io::Result<ItchPublishRequest> {
    let target_config = parse_itch_target_config(&plan.publish_target_config_json)?;
    let binding_options = parse_itch_binding_options(plan)?;
    let credential_config = parse_itch_credential_config(
        plan.publish_target_credentials_kind.as_deref(),
        plan.publish_target_credentials_config_json.as_deref(),
    )?;
    let account_name =
        normalize_itch_identifier(target_config.account_name.as_str(), "itch account_name")?;
    let game_slug = normalize_itch_identifier(target_config.game_slug.as_str(), "itch game_slug")?;
    let channel = normalize_itch_channel(binding_options.channel.as_str())?;
    let userversion = resolve_itch_userversion(plan, &binding_options)?;
    let channel_ref = format!("{account_name}/{game_slug}:{channel}");

    Ok(ItchPublishRequest {
        executable: resolve_itch_butler_executable(target_config.butler_path.as_str()),
        channel_ref: channel_ref.clone(),
        userversion: userversion.clone(),
        api_key: credential_config.api_key,
        destination_ref: format!("itch://{channel_ref}@{userversion}"),
    })
}

fn parse_publish_execution_contract_snapshot(
    raw: &str,
) -> io::Result<PublishExecutionContractSnapshot> {
    let trimmed = raw.trim();
    serde_json::from_str(if trimmed.is_empty() { "{}" } else { trimmed })
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn parse_filesystem_binding_options(plan: &ExecutionPlan) -> io::Result<FilesystemBindingOptions> {
    let snapshot = parse_publish_execution_contract_snapshot(&plan.execution_contract_json)?;
    let trimmed = snapshot.binding_options_json.trim();
    serde_json::from_str(if trimmed.is_empty() { "{}" } else { trimmed })
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn parse_itch_binding_options(plan: &ExecutionPlan) -> io::Result<ItchBindingOptions> {
    let snapshot = parse_publish_execution_contract_snapshot(&plan.execution_contract_json)?;
    let trimmed = snapshot.binding_options_json.trim();
    serde_json::from_str(if trimmed.is_empty() { "{}" } else { trimmed })
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn require_filesystem_move_binding_options(
    plan: &ExecutionPlan,
) -> io::Result<FilesystemBindingOptions> {
    let binding_options = parse_filesystem_binding_options(plan)?;
    if !binding_options
        .operation
        .trim()
        .eq_ignore_ascii_case("move")
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "filesystem publish bindings must use the move operation",
        ));
    }

    Ok(binding_options)
}

fn parse_itch_target_config(raw: &str) -> io::Result<ItchTargetConfig> {
    let trimmed = raw.trim();
    serde_json::from_str(if trimmed.is_empty() { "{}" } else { trimmed })
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn parse_itch_credential_config(
    kind: Option<&str>,
    config_json: Option<&str>,
) -> io::Result<ItchCredentialConfig> {
    let kind = kind.map(str::trim).unwrap_or_default();
    if kind.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "itch publish targets require a bound credential",
        ));
    }
    if kind != KIND_ITCH_API_KEY {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("itch publish targets require credentials kind {KIND_ITCH_API_KEY:?}"),
        ));
    }

    let raw = config_json.map(str::trim).unwrap_or_default();
    if raw.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "itch publish credentials config_json must not be empty",
        ));
    }

    let config: ItchCredentialConfig =
        serde_json::from_str(raw).map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    if config.api_key.trim().is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "itch publish credentials api_key must not be empty",
        ));
    }

    Ok(ItchCredentialConfig {
        api_key: config.api_key.trim().to_owned(),
    })
}

fn resolve_itch_butler_executable(configured_path: &str) -> String {
    resolve_itch_butler_executable_with_sidecar(
        configured_path,
        std::env::var_os(HGP_BUTLER_PATH_ENV).as_deref(),
    )
}

fn resolve_itch_butler_executable_with_sidecar(
    configured_path: &str,
    shell_sidecar_path: Option<&std::ffi::OsStr>,
) -> String {
    let trimmed = configured_path.trim();
    if trimmed.is_empty() {
        if let Some(sidecar_path) = shell_sidecar_path.filter(|value| !value.is_empty()) {
            return PathBuf::from(sidecar_path).display().to_string();
        }

        return String::from("butler");
    }

    trimmed.to_owned()
}

fn resolve_itch_userversion(
    plan: &ExecutionPlan,
    binding_options: &ItchBindingOptions,
) -> io::Result<String> {
    let template = binding_options.userversion_template.trim();
    let resolved = if template.is_empty() {
        plan.git_tag.trim().to_owned()
    } else {
        template.replace("{{git_tag}}", plan.git_tag.trim())
    };

    let normalized = resolved.trim();
    if normalized.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "itch userversion must not resolve to an empty string",
        ));
    }

    Ok(normalized.to_owned())
}

fn normalize_itch_identifier(value: &str, field_name: &str) -> io::Result<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{field_name} must not be empty"),
        ));
    }
    if normalized.contains(['/', '\\']) || normalized.chars().any(char::is_whitespace) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{field_name} must not contain path separators or whitespace"),
        ));
    }

    Ok(normalized.to_owned())
}

fn normalize_itch_channel(value: &str) -> io::Result<String> {
    let normalized = normalize_itch_identifier(value, "itch channel")?;
    if normalized.contains(':') {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "itch channel must not contain ':'",
        ));
    }

    Ok(normalized)
}

fn sanitize_itch_command_output(output: &str, api_key: &str) -> String {
    output.trim().replace(api_key, "***")
}

fn require_regular_source_path(source_path: &str) -> io::Result<PathBuf> {
    let path = PathBuf::from(source_path.trim());
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "publish source path must not be empty",
        ));
    }

    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("publish source {:?} is not a regular file", path.display()),
        ));
    }

    Ok(path)
}

fn require_existing_source_path(source_path: &str) -> io::Result<PathBuf> {
    let path = PathBuf::from(source_path.trim());
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "publish source path must not be empty",
        ));
    }

    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "publish source {:?} is not a file or directory",
                path.display()
            ),
        ));
    }

    Ok(path)
}

fn copy_regular_file(source_path: &Path, destination_path: &Path) -> io::Result<()> {
    let source_metadata = fs::metadata(source_path)?;
    if !source_metadata.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "publish source {:?} is not a regular file",
                source_path.display()
            ),
        ));
    }

    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temporary_path = destination_path.with_file_name(format!(
        ".{}.tmp",
        destination_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("artifact")
    ));
    fs::copy(source_path, &temporary_path)?;
    fs::rename(&temporary_path, destination_path)?;

    Ok(())
}

fn move_regular_file(source_path: &Path, destination_path: &Path) -> io::Result<()> {
    copy_regular_file(source_path, destination_path)?;
    fs::remove_file(source_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_destination_path, resolve_itch_butler_executable_with_sidecar,
        ExecutionController, ExecutionPlan, ExecutionProcessor, Processor,
        HGP_BUTLER_PATH_ENV,
    };
    use runtime_contracts::ProcessPriority;
    use serde_json::json;
    use std::fs;
    use std::io::{self, ErrorKind};
    use std::path::{Path, PathBuf};

    #[test]
    fn execution_processor_moves_filesystem_artifact_into_binding_directory() {
        let root = test_root("filesystem-move-success");
        let artifact_root = root.join("artifacts");
        let publish_root = root.join("published");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let source_path = artifact_root.join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");

        let result = ExecutionProcessor::new()
            .process(&ExecutionPlan {
                source_path: source_path.display().to_string(),
                artifact_root_path: artifact_root.display().to_string(),
                execution_contract_json: json!({
                    "binding_options_json": json!({
                        "operation": "move",
                        "directory_path": publish_root.display().to_string()
                    })
                    .to_string()
                })
                .to_string(),
                ..base_execution_plan("filesystem", "v1.2.3", "nested/game.zip")
            })
            .expect("filesystem publish should succeed");

        let destination_path = publish_root.join("game.zip");
        assert_eq!(PathBuf::from(&result.destination_ref), destination_path);
        assert_eq!(
            result.artifact_active_location_kind.as_deref(),
            Some("filesystem_absolute")
        );
        assert_eq!(
            result.artifact_active_location_ref.as_deref(),
            Some(destination_path.display().to_string().as_str())
        );
        assert!(!source_path.exists());
        assert_eq!(
            fs::read_to_string(destination_path).expect("published artifact should exist"),
            "artifact"
        );

        fs::remove_dir_all(root).expect("temporary publish root should be removable");
    }

    #[test]
    fn resolve_destination_path_uses_binding_directory_for_filesystem_moves() {
        let root = test_root("filesystem-destination");
        let publish_root = root.join("published");
        fs::create_dir_all(&root).expect("temporary publish root should create");

        let destination_path = resolve_destination_path(&ExecutionPlan {
            execution_contract_json: json!({
                "binding_options_json": json!({
                    "operation": "move",
                    "directory_path": publish_root.display().to_string()
                })
                .to_string()
            })
            .to_string(),
            source_path: root.join("nested").join("game.zip").display().to_string(),
            ..base_execution_plan("filesystem", "v1.2.3", "nested/game.zip")
        })
        .expect("destination path should resolve");

        let expected_path = publish_root.join("game.zip");
        assert_eq!(destination_path, expected_path);

        fs::remove_dir_all(root).expect("temporary publish root should be removable");
    }

    #[test]
    fn execution_processor_rejects_filesystem_bindings_without_move_operation() {
        let root = test_root("filesystem-invalid-binding-operation");
        let artifact_root = root.join("artifacts");
        let publish_root = root.join("published");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let source_path = artifact_root.join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");

        let error = ExecutionProcessor::new()
            .process(&ExecutionPlan {
                source_path: source_path.display().to_string(),
                artifact_root_path: artifact_root.display().to_string(),
                execution_contract_json: json!({
                    "binding_options_json": json!({
                        "operation": "copy",
                        "directory_path": publish_root.display().to_string()
                    })
                    .to_string()
                })
                .to_string(),
                ..base_execution_plan("filesystem", "v1.2.3", "game.zip")
            })
            .expect_err("non-move filesystem bindings should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error
            .to_string()
            .contains("filesystem publish bindings must use the move operation"));

        fs::remove_dir_all(root).expect("temporary publish root should be removable");
    }

    #[test]
    fn execution_processor_runs_itch_publish_with_snapshotted_version() {
        let root = test_root("itch-publish-success");
        let artifact_root = root.join("artifacts");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let source_path = artifact_root.join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");
        let butler_path = write_fake_butler(&root);

        let result = ExecutionProcessor::new()
            .process(&ExecutionPlan {
                source_path: source_path.display().to_string(),
                artifact_root_path: artifact_root.display().to_string(),
                publish_target_config_json: json!({
                    "account_name": "indiegabo",
                    "game_slug": "revolutions",
                    "butler_path": butler_path.display().to_string()
                })
                .to_string(),
                publish_target_credentials_kind: Some(String::from("itch-api-key")),
                publish_target_credentials_config_json: Some(
                    json!({"api_key": "itch-secret"}).to_string(),
                ),
                execution_contract_json: json!({
                    "binding_options_json": json!({
                        "channel": "windows-stable",
                        "userversion_template": "release-{{git_tag}}"
                    })
                    .to_string()
                })
                .to_string(),
                ..base_execution_plan("itch", "v1.2.3", "game.zip")
            })
            .expect("itch publish should succeed");

        assert_eq!(
            result.destination_ref,
            "itch://indiegabo/revolutions:windows-stable@release-v1.2.3"
        );
        assert!(source_path.exists());

        let args = fs::read_to_string(root.join("butler-args.txt"))
            .expect("fake butler args should be captured");
        assert!(args.contains("push"));
        assert!(args.contains("windows-stable"));
        assert!(args.contains("--userversion"));
        assert!(args.contains("release-v1.2.3"));
        assert!(args.contains(source_path.display().to_string().as_str()));

        let api_key = fs::read_to_string(root.join("butler-api-key.txt"))
            .expect("fake butler api key should be captured");
        assert_eq!(api_key.trim(), "itch-secret");

        fs::remove_dir_all(root).expect("temporary publish root should be removable");
    }

    #[test]
    fn execution_processor_rejects_itch_without_bound_credentials() {
        let error = ExecutionProcessor::new()
            .process(&ExecutionPlan {
                publish_target_config_json: json!({
                    "account_name": "indiegabo",
                    "game_slug": "revolutions"
                })
                .to_string(),
                execution_contract_json: json!({
                    "binding_options_json": json!({"channel": "stable"}).to_string()
                })
                .to_string(),
                ..base_execution_plan("itch", "v1.2.3", "game.zip")
            })
            .expect_err("itch publishes should require a bound credential");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("require a bound credential"));
    }

    #[test]
    fn execution_processor_interrupts_itch_publish_when_cancellation_is_requested() {
        let root = test_root("itch-publish-cancel");
        let artifact_root = root.join("artifacts");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let source_path = artifact_root.join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");
        let butler_path = write_slow_fake_butler(&root);

        let error = ExecutionProcessor::new()
            .process_with_controller(
                &ExecutionPlan {
                    source_path: source_path.display().to_string(),
                    artifact_root_path: artifact_root.display().to_string(),
                    publish_target_config_json: json!({
                        "account_name": "indiegabo",
                        "game_slug": "revolutions",
                        "butler_path": butler_path.display().to_string()
                    })
                    .to_string(),
                    publish_target_credentials_kind: Some(String::from("itch-api-key")),
                    publish_target_credentials_config_json: Some(
                        json!({"api_key": "itch-secret"}).to_string(),
                    ),
                    execution_contract_json: json!({
                        "binding_options_json": json!({
                            "channel": "windows-stable",
                            "userversion_template": "release-{{git_tag}}"
                        })
                        .to_string()
                    })
                    .to_string(),
                    ..base_execution_plan("itch", "v1.2.3", "game.zip")
                },
                &mut CancelAfterSpawnController::default(),
            )
            .expect_err("itch publish should stop when cancellation is requested");

        assert_eq!(error.kind(), ErrorKind::Interrupted);

        fs::remove_dir_all(root).expect("temporary publish root should be removable");
    }

    #[test]
    fn resolve_itch_butler_executable_prefers_configured_override() {
        assert_eq!(
            resolve_itch_butler_executable_with_sidecar(
                "./custom-butler",
                Some(std::ffi::OsStr::new("ignored-sidecar")),
            ),
            String::from("./custom-butler")
        );
    }

    #[test]
    fn resolve_itch_butler_executable_uses_shell_sidecar_when_config_is_empty() {
        let butler_path = if cfg!(windows) {
            String::from("C:/repo/src-tauri/bin/hgp-butler-x86_64-pc-windows-msvc.exe")
        } else {
            String::from("/repo/src-tauri/bin/hgp-butler-x86_64-unknown-linux-gnu")
        };

        assert_eq!(
            resolve_itch_butler_executable_with_sidecar(
                "",
                Some(std::ffi::OsStr::new(butler_path.as_str())),
            ),
            butler_path,
        );
    }

    #[test]
    fn resolve_itch_butler_executable_falls_back_to_path_when_no_override_exists() {
        assert_eq!(
            resolve_itch_butler_executable_with_sidecar("", None),
            String::from("butler")
        );
    }

    #[test]
    fn resolve_itch_butler_executable_reads_shell_environment_variable() {
        let sidecar = if cfg!(windows) {
            std::ffi::OsString::from("C:/repo/src-tauri/bin/hgp-butler-x86_64-pc-windows-msvc.exe")
        } else {
            std::ffi::OsString::from("/repo/src-tauri/bin/hgp-butler-x86_64-unknown-linux-gnu")
        };

        assert_eq!(HGP_BUTLER_PATH_ENV, "HGP_BUTLER_PATH");
        assert_eq!(
            resolve_itch_butler_executable_with_sidecar("", Some(sidecar.as_os_str())),
            PathBuf::from(sidecar).display().to_string(),
        );
    }

    fn base_execution_plan(kind: &str, git_tag: &str, artifact_path: &str) -> ExecutionPlan {
        ExecutionPlan {
            publish_run_id: 1,
            release_run_id: 2,
            repository_id: 3,
            repository_name: String::from("revolutions"),
            git_tag: String::from(git_tag),
            process_priority: ProcessPriority::Low,
            build_run_id: 4,
            publish_target_id: 5,
            publish_target_name: String::from(kind),
            publish_target_kind: String::from(kind),
            publish_target_config_json: String::from("{}"),
            publish_target_credentials_kind: None,
            publish_target_credentials_config_json: None,
            execution_contract_json: String::from("{}"),
            artifact_id: 6,
            artifact_name: String::from(artifact_path),
            artifact_kind: String::from("archive"),
            artifact_path: String::from(artifact_path),
            artifact_active_location_kind: String::from("runtime_artifact"),
            artifact_active_location_ref: String::from(artifact_path),
            artifact_root_path: String::from("unused"),
            source_path: String::from("unused"),
            status: String::from("queued"),
        }
    }

    fn write_fake_butler(root: &Path) -> PathBuf {
        #[cfg(windows)]
        {
            let script_path = root.join("fake-butler.cmd");
            fs::write(
                &script_path,
                concat!(
                    "@echo off\r\n",
                    "set SCRIPT_DIR=%~dp0\r\n",
                    "> \"%SCRIPT_DIR%butler-args.txt\" echo %*\r\n",
                    "> \"%SCRIPT_DIR%butler-api-key.txt\" echo %BUTLER_API_KEY%\r\n",
                    "exit /b 0\r\n"
                ),
            )
            .expect("fake butler script should write");

            script_path
        }

        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;

            let script_path = root.join("fake-butler.sh");
            fs::write(
                &script_path,
                concat!(
                    "#!/bin/sh\n",
                    "SCRIPT_DIR=\"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\n",
                    "printf '%s' \"$*\" > \"$SCRIPT_DIR/butler-args.txt\"\n",
                    "printf '%s' \"$BUTLER_API_KEY\" > \"$SCRIPT_DIR/butler-api-key.txt\"\n"
                ),
            )
            .expect("fake butler script should write");
            let mut permissions = fs::metadata(&script_path)
                .expect("fake butler metadata should load")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script_path, permissions)
                .expect("fake butler permissions should update");

            script_path
        }
    }

    #[derive(Default)]
    struct CancelAfterSpawnController {
        check_count: usize,
    }

    impl ExecutionController for CancelAfterSpawnController {
        fn check_cancellation(&mut self) -> io::Result<()> {
            self.check_count += 1;
            if self.check_count >= 2 {
                return Err(io::Error::new(
                    ErrorKind::Interrupted,
                    "publish run canceled by operator",
                ));
            }

            Ok(())
        }
    }

    fn write_slow_fake_butler(root: &Path) -> PathBuf {
        #[cfg(windows)]
        {
            let script_path = root.join("fake-butler-slow.cmd");
            fs::write(
                &script_path,
                concat!(
                    "@echo off\r\n",
                    "powershell -NoProfile -Command \"Start-Sleep -Seconds 3\"\r\n",
                    "exit /b 0\r\n"
                ),
            )
            .expect("slow fake butler script should write");
            script_path
        }

        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;

            let script_path = root.join("fake-butler-slow.sh");
            fs::write(&script_path, "#!/bin/sh\nsleep 3\nexit 0\n")
                .expect("slow fake butler script should write");
            let mut permissions = fs::metadata(&script_path)
                .expect("slow fake butler metadata should load")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script_path, permissions)
                .expect("slow fake butler permissions should update");
            script_path
        }
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "handy-games-publisher-runtime-publish-{label}-{}",
            std::process::id()
        ))
    }
}
