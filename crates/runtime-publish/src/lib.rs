//! Resolves publish destinations and executes filesystem-backed artifact
//! publication for completed build outputs.

#![forbid(unsafe_code)]

use serde::Deserialize;
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Lists the publish backends enabled by the local runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishBackend {
    Filesystem,
}

impl PublishBackend {
    /// Returns the operator-facing label for the selected publish backend.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
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
    pub build_run_id: i64,
    pub publish_target_id: i64,
    pub publish_target_name: String,
    pub publish_target_kind: String,
    pub publish_target_config_json: String,
    pub artifact_id: i64,
    pub artifact_name: String,
    pub artifact_kind: String,
    pub artifact_path: String,
    pub artifact_root_path: String,
    pub source_path: String,
    pub status: String,
}

/// Captures the persisted destination returned by a successful publish execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub destination_ref: String,
}

/// Executes one publish plan against the configured backend.
pub trait Processor {
    fn process(&self, plan: &ExecutionPlan) -> io::Result<ExecutionResult>;
}

/// Resolves the final destination path that one publish backend will write for the given plan.
pub fn resolve_destination_path(plan: &ExecutionPlan) -> io::Result<PathBuf> {
    match parse_publish_backend(&plan.publish_target_kind)? {
        PublishBackend::Filesystem => resolve_filesystem_destination_path(plan),
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
        match parse_publish_backend(&plan.publish_target_kind)? {
            PublishBackend::Filesystem => publish_to_filesystem(plan),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct FilesystemTargetConfig {
    root_path: String,
}

fn parse_publish_backend(kind: &str) -> io::Result<PublishBackend> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "filesystem" => Ok(PublishBackend::Filesystem),
        other => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsupported publish target kind {other:?}"),
        )),
    }
}

fn publish_to_filesystem(plan: &ExecutionPlan) -> io::Result<ExecutionResult> {
    let source_path = require_regular_source_path(&plan.source_path)?;
    let destination_path = resolve_destination_path(plan)?;
    copy_regular_file(&source_path, &destination_path)?;

    Ok(ExecutionResult {
        destination_ref: destination_path.display().to_string(),
    })
}

fn resolve_filesystem_destination_path(plan: &ExecutionPlan) -> io::Result<PathBuf> {
    let config = parse_filesystem_target_config(&plan.publish_target_config_json)?;
    let repository_segment = sanitize_path_segment(&plan.repository_name, "repository_name")?;
    let git_tag_segment = sanitize_path_segment(&plan.git_tag, "git_tag")?;
    let artifact_path = normalize_relative_artifact_path(&plan.artifact_path)?;

    Ok(Path::new(&config.root_path)
        .join(repository_segment)
        .join(git_tag_segment)
        .join(PathBuf::from(artifact_path.replace('/', std::path::MAIN_SEPARATOR_STR))))
}

fn parse_filesystem_target_config(raw: &str) -> io::Result<FilesystemTargetConfig> {
    let trimmed = raw.trim();
    let config: FilesystemTargetConfig = serde_json::from_str(if trimmed.is_empty() {
        "{}"
    } else {
        trimmed
    })
    .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;

    let root_path = PathBuf::from(config.root_path.trim());
    if config.root_path.trim().is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "filesystem target root_path must not be empty",
        ));
    }
    if !root_path.is_absolute() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "filesystem target root_path must be absolute",
        ));
    }

    Ok(FilesystemTargetConfig {
        root_path: root_path.display().to_string(),
    })
}

fn sanitize_path_segment(value: &str, field_name: &str) -> io::Result<String> {
    let normalized = value
        .trim()
        .replace(['/', '\\'], "-")
        .trim()
        .to_owned();
    if normalized.is_empty() || normalized == "." || normalized == ".." {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{field_name} must not be empty"),
        ));
    }

    Ok(normalized)
}

fn normalize_relative_artifact_path(path: &str) -> io::Result<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "artifact path must not be empty",
        ));
    }

    let normalized = PathBuf::from(trimmed.replace('/', &std::path::MAIN_SEPARATOR.to_string()))
        .components()
        .collect::<PathBuf>();
    if normalized.as_os_str().is_empty() || normalized.is_absolute() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "artifact path must be relative",
        ));
    }
    if normalized == Path::new("..")
        || normalized
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "artifact path must not escape the artifact root",
        ));
    }

    Ok(normalized.to_string_lossy().replace('\\', "/"))
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

fn copy_regular_file(source_path: &Path, destination_path: &Path) -> io::Result<()> {
    let source_metadata = fs::metadata(source_path)?;
    if !source_metadata.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("publish source {:?} is not a regular file", source_path.display()),
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

#[cfg(test)]
mod tests {
    use super::{resolve_destination_path, ExecutionPlan, ExecutionProcessor, Processor};
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn execution_processor_copies_filesystem_artifact_into_release_path() {
        let root = test_root("filesystem-success");
        let artifact_root = root.join("artifacts");
        let publish_root = root.join("published");
        fs::create_dir_all(artifact_root.join("nested"))
            .expect("artifact root should create");
        let source_path = artifact_root.join("nested").join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");

        let result = ExecutionProcessor::new()
            .process(&ExecutionPlan {
                publish_run_id: 1,
                release_run_id: 2,
                repository_id: 3,
                repository_name: String::from("revolutions"),
                git_tag: String::from("v1.2.3"),
                build_run_id: 4,
                publish_target_id: 5,
                publish_target_name: String::from("filesystem"),
                publish_target_kind: String::from("filesystem"),
                publish_target_config_json: json!({
                    "root_path": publish_root.display().to_string()
                })
                .to_string(),
                artifact_id: 6,
                artifact_name: String::from("nested/game.zip"),
                artifact_kind: String::from("archive"),
                artifact_path: String::from("nested/game.zip"),
                artifact_root_path: artifact_root.display().to_string(),
                source_path: source_path.display().to_string(),
                status: String::from("running"),
            })
            .expect("filesystem publish should succeed");

        let destination_path = publish_root
            .join("revolutions")
            .join("v1.2.3")
            .join("nested")
            .join("game.zip");
        assert_eq!(PathBuf::from(&result.destination_ref), destination_path);
        assert_eq!(
            fs::read_to_string(destination_path).expect("published artifact should exist"),
            "artifact"
        );

        fs::remove_dir_all(root).expect("temporary publish root should be removable");
    }

    #[test]
    fn resolve_destination_path_matches_filesystem_publish_layout() {
        let root = test_root("filesystem-destination");
        let publish_root = root.join("published");
        fs::create_dir_all(&root).expect("temporary publish root should create");

        let destination_path = resolve_destination_path(&ExecutionPlan {
            publish_run_id: 1,
            release_run_id: 2,
            repository_id: 3,
            repository_name: String::from("revolutions"),
            git_tag: String::from("v1.2.3"),
            build_run_id: 4,
            publish_target_id: 5,
            publish_target_name: String::from("filesystem"),
            publish_target_kind: String::from("filesystem"),
            publish_target_config_json: json!({
                "root_path": publish_root.display().to_string()
            })
            .to_string(),
            artifact_id: 6,
            artifact_name: String::from("nested/game.zip"),
            artifact_kind: String::from("archive"),
            artifact_path: String::from("nested/game.zip"),
            artifact_root_path: String::from("unused"),
            source_path: String::from("unused"),
            status: String::from("queued"),
        })
        .expect("destination path should resolve");

        let expected_path = publish_root
            .join("revolutions")
            .join("v1.2.3")
            .join("nested")
            .join("game.zip");
        assert_eq!(destination_path, expected_path);

        fs::remove_dir_all(root).expect("temporary publish root should be removable");
    }

    #[test]
    fn execution_processor_rejects_invalid_filesystem_config() {
        let root = test_root("filesystem-invalid-config");
        let artifact_root = root.join("artifacts");
        fs::create_dir_all(&artifact_root).expect("artifact root should create");
        let source_path = artifact_root.join("game.zip");
        fs::write(&source_path, "artifact").expect("artifact source should write");

        let error = ExecutionProcessor::new()
            .process(&ExecutionPlan {
                publish_run_id: 1,
                release_run_id: 2,
                repository_id: 3,
                repository_name: String::from("revolutions"),
                git_tag: String::from("v1.2.3"),
                build_run_id: 4,
                publish_target_id: 5,
                publish_target_name: String::from("filesystem"),
                publish_target_kind: String::from("filesystem"),
                publish_target_config_json: String::from(r#"{"root_path":"relative/output"}"#),
                artifact_id: 6,
                artifact_name: String::from("game.zip"),
                artifact_kind: String::from("archive"),
                artifact_path: String::from("game.zip"),
                artifact_root_path: artifact_root.display().to_string(),
                source_path: source_path.display().to_string(),
                status: String::from("running"),
            })
            .expect_err("relative filesystem roots should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("root_path must be absolute"));

        fs::remove_dir_all(root).expect("temporary publish root should be removable");
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "handy-games-publisher-runtime-publish-{label}-{}",
            std::process::id()
        ))
    }
}