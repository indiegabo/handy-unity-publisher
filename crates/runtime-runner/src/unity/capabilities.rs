use super::*;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CapabilityInspectionInput {
    pub(crate) architecture: String,
    pub(crate) packaging_mode: String,
    pub(crate) inside_wsl: bool,
    pub(crate) path_entries: Vec<PathBuf>,
    pub(crate) discovery_roots: Vec<PathBuf>,
    pub(crate) unity_license_paths: Vec<PathBuf>,
}

impl CapabilityInspectionInput {
    pub(crate) fn current(platform: HostPlatform) -> Self {
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
            inside_wsl: super::detect_wsl_from_environment(),
            path_entries,
            discovery_roots: default_unity_discovery_root_paths(platform),
            unity_license_paths: super::default_unity_license_paths(platform),
        }
    }
}

/// Inspects one host-native runner config without exposing environment values.
pub fn diagnose_host_native_runner_config(config_json: &str) -> HostNativeRunnerDiagnostics {
    match super::parse_host_native_runner_config(config_json) {
        Ok(config) => super::build_host_native_runner_diagnostics(&config),
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

pub(crate) fn inspect_host_capability_profile_with_input(
    platform: HostPlatform,
    input: CapabilityInspectionInput,
) -> HostCapabilityProfile {
    let git_tool = super::detect_git_tool(&input.path_entries);
    let platform_prerequisites =
        super::detect_platform_prerequisites(platform, &input.path_entries);
    let unity_license = super::detect_unity_license(&input.unity_license_paths);
    let discovered_editors = super::discover_unity_editors(platform, &input.discovery_roots);
    let runner_selection = super::select_runner_family(
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

/// Inspects the local host and summarizes the capability profile used by Unity runner selection.
pub fn inspect_host_capability_profile(platform: HostPlatform) -> HostCapabilityProfile {
    inspect_host_capability_profile_with_input(
        platform,
        CapabilityInspectionInput::current(platform),
    )
}
