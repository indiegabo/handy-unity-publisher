//! Hosts the Unity-specific runner surface so external consumers enter through
//! an explicit adapter module while the crate root keeps shared workspace and
//! artifact helpers.

mod capabilities;
mod execution;
mod output;

pub use self::capabilities::{
    default_unity_discovery_root_paths, diagnose_host_native_runner_config,
    inspect_host_capability_profile, DiscoveredUnityEditor,
    HostCapabilityProfile, HostNativeRunnerDiagnostics, HostToolCapability,
    RunnerSelectionDiagnostics, UnityLicenseDiagnostics,
};
#[cfg(test)]
pub(crate) use self::capabilities::{
    CapabilityInspectionInput, inspect_host_capability_profile_with_input,
};
pub use self::execution::{
    HostNativeUnityExecutor, UnityBuildExecutionOutcome,
    UnityBuildExecutionPlan, UnityBuildExecutionProcessOutcome,
    UnityBuildExecutionProcessor, UnityBuildExecutionRequest,
    UnityBuildExecutionResult, UnityBuildExecutor,
    resolve_host_native_unity_execution_plan,
};
pub use self::output::{
    package_unity_build_output, resolve_final_unity_artifact_output_path,
    resolve_unity_build_stage_identity, UnityBuildStageIdentity,
};

use runtime_config::{HostPlatform, RuntimeDirectories};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const UNITY_NON_SHIPPABLE_ARCHIVE_PATH_SUFFIXES: &[&str] = &[
    "_DoNotShip",
    "_BackUpThisFolder_ButDontShipItWithYourGame",
];
const UNITY_MACOS_OPTIONAL_ARCHIVE_PATH_SUFFIXES: &[&str] = &[".dSYM"];
const UNITY_WINDOWS_OPTIONAL_ARCHIVE_FILE_SUFFIXES: &[&str] = &[".pdb"];
const UNITY_WEBGL_OPTIONAL_ARCHIVE_FILE_SUFFIXES: &[&str] = &[".symbols.json"];

use crate::{
    artifact_output_relative_path,
    cleanup_previous_artifact_output,
    host::execute_command_with_timeout,
    normalized_optional_string,
    require_non_empty,
    resolve_artifact_output_path,
    resolve_runtime_output_path,
    ExecutionProgressReporter,
    NoopExecutionProgressReporter,
    PreparedWorkspace,
    RunnerFamily,
    WorkspacePreparationInput,
    WorkspacePreparer,
};

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

pub(crate) fn selected_host_runner_family(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Windows => "host-windows-unity",
        HostPlatform::MacOS => "host-macos-unity",
        HostPlatform::Linux => "host-linux-unity",
    }
}

fn add_unity_build_output_directory_to_zip<W>(
    zip: &mut ZipWriter<W>,
    root: &Path,
    current: &Path,
    options: SimpleFileOptions,
    plan: &UnityBuildExecutionPlan,
) -> io::Result<()>
where
    W: io::Write + io::Seek,
{
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let relative_path = path.strip_prefix(root).map_err(io::Error::other)?;
        if should_exclude_unity_build_output_archive_path(plan, relative_path) {
            continue;
        }

        let archive_relative = relative_path.to_string_lossy().replace('\\', "/");
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if !archive_relative.is_empty() {
                zip.add_directory(format!("{archive_relative}/"), options)
                    .map_err(io::Error::other)?;
            }
            add_unity_build_output_directory_to_zip(zip, root, &path, options, plan)?;
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

fn should_exclude_unity_build_output_archive_path(
    plan: &UnityBuildExecutionPlan,
    relative_path: &Path,
) -> bool {
    if unity_archive_path_has_non_shippable_segment(relative_path) {
        return true;
    }

    unity_archive_path_is_optional_debug_symbol(plan, relative_path)
}

fn unity_archive_path_has_non_shippable_segment(relative_path: &Path) -> bool {
    relative_path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|segment| {
            has_any_suffix_case_insensitive(segment, UNITY_NON_SHIPPABLE_ARCHIVE_PATH_SUFFIXES)
        })
}

fn unity_archive_path_is_optional_debug_symbol(
    plan: &UnityBuildExecutionPlan,
    relative_path: &Path,
) -> bool {
    let Some(file_name) = relative_path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    match unity_target_platform_family(&plan.unity_target_platform) {
        Some("macos") => has_any_suffix_case_insensitive(
            file_name,
            UNITY_MACOS_OPTIONAL_ARCHIVE_PATH_SUFFIXES,
        ),
        Some("windows") => has_any_suffix_case_insensitive(
            file_name,
            UNITY_WINDOWS_OPTIONAL_ARCHIVE_FILE_SUFFIXES,
        ),
        Some("webgl") => has_any_suffix_case_insensitive(
            file_name,
            UNITY_WEBGL_OPTIONAL_ARCHIVE_FILE_SUFFIXES,
        ),
        _ => false,
    }
}

fn unity_target_platform_family(platform: &str) -> Option<&'static str> {
    let normalized = platform.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    if normalized == "windows"
        || normalized == "windows64"
        || normalized.contains("standalonewindows")
    {
        Some("windows")
    } else if normalized == "linux"
        || normalized == "linux64"
        || normalized.contains("standalonelinux")
        || normalized.contains("linuxheadless")
    {
        Some("linux")
    } else if normalized == "macos"
        || normalized == "osx"
        || normalized == "ios"
        || normalized.contains("standaloneosx")
        || normalized.contains("visionos")
        || normalized.contains("tvos")
    {
        Some("macos")
    } else if normalized.contains("webgl") {
        Some("webgl")
    } else if normalized.contains("android") {
        Some("android")
    } else {
        None
    }
}

fn has_any_suffix_case_insensitive(value: &str, suffixes: &[&str]) -> bool {
    let normalized = value.to_ascii_lowercase();
    suffixes
        .iter()
        .any(|suffix| normalized.ends_with(&suffix.to_ascii_lowercase()))
}

fn build_unity_log_stem(platform: &str) -> String {
    let platform = platform
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();

    if platform.is_empty() {
        String::from("unity-build")
    } else {
        format!("unity-build-{platform}")
    }
}

fn execute_host_native_unity(
    request: &UnityBuildExecutionRequest,
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

    let build_method = match require_non_empty(
        &request.plan.unity_build_method,
        "build method",
    ) {
        Ok(build_method) => build_method,
        Err(error) => return Err((Vec::new(), error)),
    };
    let output_path = request.output_path.display().to_string();
    let build_target = match platform_to_unity_build_target(
        &request.plan.unity_target_platform,
    ) {
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
    command.env(
        "HGB_TARGET_PLATFORM",
        request.plan.unity_target_platform.trim(),
    );
    command.env("HGB_UNITY_VERSION", request.plan.engine_version.trim());

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
        "host-native unity runner",
        timeout,
        &request.workspace.log_path,
        &log_preamble,
        editor_log_path.as_deref(),
        Some("Unity Editor.log"),
        reporter,
    ) {
        Ok(output) => output,
        Err(error) => {
            let classified =
                classify_execution_error(error.error, &error.output, error.exit_status);
            return Err((error.output, classified));
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
    plan: &UnityBuildExecutionPlan,
    profile: &HostCapabilityProfile,
) -> io::Result<String> {
    let requested_version = plan.engine_version.trim();
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
                "host capability discovery did not find any runnable Unity editor and the build plan does not declare an engine_version",
            )),
            _ => Err(io::Error::new(
                ErrorKind::InvalidInput,
                "host capability discovery found multiple Unity editors, but the build plan does not declare an engine_version to disambiguate them",
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

fn execution_log_preamble(
    request: &UnityBuildExecutionRequest,
    unity_executable_path: &str,
    build_target: &str,
    build_method: &str,
    output_path: &str,
    log_path: &str,
    additional_argument_count: usize,
    editor_log_path: Option<&Path>,
) -> Vec<u8> {
    let mut preamble = String::new();
    preamble.push_str("HGP host-native execution\n");
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

fn platform_to_unity_build_target(platform: &str) -> io::Result<String> {
    let platform = require_non_empty(platform, "Unity platform")?;
    Ok(platform)
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

pub(crate) fn classify_execution_error(
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

pub(crate) fn normalize_failure_summary_line(raw_line: &str) -> String {
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
    use super::*;

    #[test]
    fn platform_to_unity_build_target_requires_canonical_values() {
        let cases = [
            ("StandaloneLinux64", "StandaloneLinux64"),
            ("StandaloneWindows64", "StandaloneWindows64"),
            ("StandaloneOSX", "StandaloneOSX"),
            ("WebGL", "WebGL"),
            ("Android", "Android"),
            ("iOS", "iOS"),
            ("LinuxHeadlessSimulation", "LinuxHeadlessSimulation"),
            ("WSAPlayer", "WSAPlayer"),
            ("PS5", "PS5"),
        ];

        for (input, expected) in cases {
            assert_eq!(
                platform_to_unity_build_target(input)
                    .expect("Unity target platform should resolve"),
                expected,
            );
        }
    }

    #[test]
    fn platform_to_unity_build_target_rejects_blank_values() {
        let error = platform_to_unity_build_target("   ")
            .expect_err("blank Unity target platforms should fail");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(error.to_string().contains("Unity platform must not be empty"));
    }

    #[test]
    fn unity_archive_path_is_optional_debug_symbol_accepts_canonical_platform_names() {
        assert!(unity_archive_path_is_optional_debug_symbol(
            &test_execution_plan("StandaloneWindows64"),
            Path::new("Builds/Game.pdb"),
        ));
        assert!(unity_archive_path_is_optional_debug_symbol(
            &test_execution_plan("StandaloneOSX"),
            Path::new("Builds/Game.app.dSYM"),
        ));
        assert!(unity_archive_path_is_optional_debug_symbol(
            &test_execution_plan("WebGL"),
            Path::new("Builds/Game.symbols.json"),
        ));
        assert!(!unity_archive_path_is_optional_debug_symbol(
            &test_execution_plan("PS5"),
            Path::new("Builds/Game.pdb"),
        ));
    }

    fn test_execution_plan(platform: &str) -> UnityBuildExecutionPlan {
        UnityBuildExecutionPlan {
            build_run_id: 1,
            release_run_id: 1,
            build_target_id: 1,
            repository_name: String::from("revolutions"),
            repository_url: String::from("https://example.com/revolutions.git"),
            git_tag: String::from("v1.0.0"),
            target_name: String::from("player"),
            unity_target_platform: String::from(platform),
            runner_type: String::from("host-native"),
            unity_build_method: String::from("Builder.Perform"),
            output_kind: None,
            output_path_template: None,
            engine_version: String::from("6000.0.0f1"),
            config_json: String::from("{}"),
            timeout_seconds: 60,
        }
    }
}